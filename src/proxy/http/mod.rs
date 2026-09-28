mod connector;
use http::{header, StatusCode};
use hyper::server::conn::Http;
use std::collections::HashMap;
use std::io;
use std::str::FromStr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::net::TcpStream;

use crate::common::net::{relay, relay_with_atomic_counter};
use crate::common::new_error;
use crate::config::{Router, COUNTER_MAP};
use crate::debug_log;
use crate::proxy::{Address, ChainStreamBuilder};
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Body, Client, Method, Request, Response};
use log::info;

use self::connector::Connector;

#[derive(Clone)]
pub struct HttpInbound {
    inner_map: Arc<HashMap<String, ChainStreamBuilder>>,
    router: Arc<Router>,
    client: Client<Connector>,
    enable_api_server: bool,
    in_counter_up: Option<&'static AtomicU64>,
    in_counter_down: Option<&'static AtomicU64>,
    relay_buffer_size: usize,
}
impl HttpInbound {
    pub fn new(
        inner_map: Arc<HashMap<String, ChainStreamBuilder>>,
        router: Arc<Router>,
        enable_api_server: bool,
        in_counter_up: Option<&'static AtomicU64>,
        in_counter_down: Option<&'static AtomicU64>,
        relay_buffer_size: usize,
    ) -> Self {
        let client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build(Connector::new(inner_map.clone(), router.clone()));
        Self {
            client,
            router,
            enable_api_server,
            in_counter_up,
            in_counter_down,
            relay_buffer_size,
            inner_map,
        }
    }
    pub async fn serve_http_conn(&self, io: TcpStream) -> io::Result<()> {
        let http_conn = Http::new();
        let inner_map = self.inner_map.clone();
        let router = self.router.clone();
        let enable_api_server = self.enable_api_server;
        let in_counter_up = self.in_counter_up;
        let in_counter_down = self.in_counter_down;
        let relay_buffer_size = self.relay_buffer_size;
        let client = self.client.clone();
        let conn = http_conn
            .serve_connection(
                io,
                service_fn(|req| {
                    let inner_map = inner_map.clone();
                    let router = router.clone();
                    let client = client.clone();
                    async move {
                        if Method::CONNECT == req.method() {
                            proxy_connect(
                                req,
                                inner_map,
                                router,
                                enable_api_server,
                                in_counter_up,
                                in_counter_down,
                                relay_buffer_size,
                            )
                            .await
                        } else {
                            proxy(req, client).await
                        }
                    }
                }),
            )
            .with_upgrades();
        if let Err(e) = conn.await {
            return Err(new_error(e));
        }
        Ok(())
    }
}

async fn proxy_connect(
    req: Request<Body>,
    inner_map: Arc<HashMap<String, ChainStreamBuilder>>,
    router: Arc<Router>,

    enable_api_server: bool,
    in_counter_up: Option<&'static AtomicU64>,
    in_counter_down: Option<&'static AtomicU64>,
    relay_buffer_size: usize,
) -> Result<Response<Body>, hyper::Error> {
    if let Some(addr) = host_addr(req.uri()) {
        tokio::task::spawn(async move {
            let inner_map = inner_map;
            let router = router;
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = tunnel(
                        upgraded,
                        addr,
                        inner_map,
                        router,
                        enable_api_server,
                        in_counter_up,
                        in_counter_down,
                        relay_buffer_size,
                    )
                    .await
                    {
                        log::error!("http tunnel error: {}", e);
                    };
                }
                Err(e) => log::error!("upgrade error: {}", e),
            }
        });

        Ok(Response::new(Body::empty()))
    } else {
        log::error!("CONNECT host is not socket addr: {:?}", req.uri());
        let mut resp = Response::new(Body::from("CONNECT must be to a socket address"));
        *resp.status_mut() = http::StatusCode::BAD_REQUEST;

        Ok(resp)
    }
}
async fn proxy(
    mut req: Request<Body>,
    client: Client<Connector>,
) -> Result<Response<Body>, hyper::Error> {
    remove_proxy_headers(&mut req);
    debug_log!("http proxy server req: {:?}", req);
    // For Go compatibility, preserve original request but remove proxy headers
    let response: Result<Response<Body>, hyper::Error> = client.request(req).await;
    if response.is_err() {
        log::debug!("http proxy error: {:?}", response.as_ref().err());
        Ok(Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Body::empty())
            .unwrap())
    } else {
        response
    }
}

fn host_addr(uri: &http::Uri) -> Option<Address> {
    // For Go compatibility, parse authority as address
    // Authority may be like "example.com:443" or "[::1]:443" or "192.168.1.1:80"
    if let Some(auth) = uri.authority() {
        // Try to parse authority directly
        if let Ok(addr) = Address::from_str(auth.as_str()) {
            return Some(addr);
        }
        // If authority parsing fails, try to extract host and use default port 443 for CONNECT
        // Go's CONNECT requires port, but we try to be permissive
        let host = auth.host();
        // For CONNECT, default to 443 if no port specified (Go would require port, but we default)
        let port = auth.port_u16().unwrap_or(443);
        // Try to parse host:port
        let addr_str = format!("{}:{}", host, port);
        if let Ok(addr) = Address::from_str(&addr_str) {
            return Some(addr);
        }
    }
    None
}

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
async fn tunnel(
    upgraded: Upgraded,
    addr: Address,
    inner_map: Arc<HashMap<String, ChainStreamBuilder>>,
    router: Arc<Router>,
    enable_api_server: bool,
    in_counter_up: Option<&'static AtomicU64>,
    in_counter_down: Option<&'static AtomicU64>,
    relay_buffer_size: usize,
) -> io::Result<()> {
    // Connect to remote server
    let ob = router.match_addr(&addr);
    let stream_builder = inner_map.get(ob).unwrap();
    info!("routing {} to outbound:{}", addr, ob);
    if stream_builder.is_blackhole() {
        return Ok(());
    }
    let server = stream_builder.build_tcp(addr).await?;
    if enable_api_server {
        let out_down = format!("outbound>>>{}>>>traffic>>>downlink", ob);
        let out_up = format!("outbound>>>{}>>>traffic>>>uplink", ob);
        let out_down = COUNTER_MAP.get().unwrap().get(out_down.as_str()).unwrap();
        let out_up = COUNTER_MAP.get().unwrap().get(out_up.as_str()).unwrap();
        relay_with_atomic_counter(
            upgraded,
            server,
            in_counter_up.unwrap(),
            in_counter_down.unwrap(),
            out_up,
            out_down,
            relay_buffer_size,
        )
        .await?;
    } else {
        relay(upgraded, server, relay_buffer_size).await?;
    }
    Ok(())
}

pub fn remove_proxy_headers(req: &mut Request<Body>) {
    // Remove headers that shouldn't be forwarded to upstream
    // For Go compatibility, match v2ray-core's http inbound behavior
    req.headers_mut().remove(header::ACCEPT_ENCODING);
    req.headers_mut().remove(header::CONNECTION);
    req.headers_mut().remove("proxy-connection");
    req.headers_mut().remove(header::PROXY_AUTHENTICATE);
    req.headers_mut().remove(header::PROXY_AUTHORIZATION);
    // Also remove Keep-Alive for Go compatibility
    req.headers_mut().remove("keep-alive");
}
