use http::uri::Scheme;
use http::Uri;
use hyper::client::connect::{Connected, Connection};

use std::collections::HashMap;
use std::future::Future;
use std::io::{self, Error, ErrorKind};
use std::pin::Pin;
use std::str::FromStr;

use std::sync::Arc;
use std::task::Poll;

use crate::config::Router;

use crate::proxy::{Address, BoxProxyStream, ChainStreamBuilder};

#[derive(Clone)]
pub struct Connector {
    inner_map: Arc<HashMap<String, ChainStreamBuilder>>,
    router: Arc<Router>,
}

impl Connector {
    pub fn new(inner_map: Arc<HashMap<String, ChainStreamBuilder>>, router: Arc<Router>) -> Self {
        Self { inner_map, router }
    }
}

impl tower::Service<Uri> for Connector {
    type Response = BoxProxyStream;

    type Error = io::Error;

    type Future = Pin<Box<dyn Future<Output = io::Result<BoxProxyStream>> + Send>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let is_tls_scheme = uri
            .scheme()
            .map(|s| s == &Scheme::HTTPS || s.as_str() == "wss")
            .unwrap_or(false);

        // For Go compatibility, parse authority handling IPv6 and userinfo
        let authority_str = uri.authority().map(|a| a.as_str()).unwrap_or("").to_string();
        let inner_map = self.inner_map.clone();
        let router = self.router.clone();
        let uri_clone = uri.clone();
        let f = async move {
            // Extract host:port from authority, stripping userinfo if present
            let addr_str = {
                let auth = authority_str.as_str();
                if auth.is_empty() {
                    // For Go compatibility, if no authority, try to use Host header? But we don't have it here
                    // Return error as Go would
                    ""
                } else {
                    // Strip userinfo: take part after last '@'
                    auth.rsplit('@').next().unwrap_or(auth)
                }
            };

            if addr_str.is_empty() {
                log::error!(
                    "HTTP inbound target URI must have authority, but found: {}",
                    uri_clone
                );
                return Err(Error::new(ErrorKind::Other, "URI must have authority"));
            }

            let addr = Address::from_str(addr_str);

            match addr {
                Ok(mut addr) => {
                    // For http proxy, if scheme is http and port is 0 (no port), default to 80
                    // For https via CONNECT, port should be present, but we already handled CONNECT separately
                    if is_tls_scheme {
                        let err = Error::new(
                            ErrorKind::Other,
                            "HTTP inbound target URI is tls and the client is not using CONNECT method.",
                        );
                        log::error!(
                            "HTTP inbound target URI is tls and the client is not using CONNECT method. URI is: {}",
                            uri_clone
                        );
                        return Err(err);
                    }
                    // If address is domain without explicit port and we got default 80 from parsing,
                    // but uri has explicit port, use that port
                    if let Some(port) = uri_clone.authority().and_then(|a| a.port_u16()) {
                        // Override port if address is domain and port was default
                        match &mut addr {
                            Address::DomainNameAddress(_, p) => {
                                *p = port;
                            }
                            Address::SocketAddress(sa) => {
                                sa.set_port(port);
                            }
                        }
                    }
                    let ob = router.match_addr(&addr);
                    let stream_builder = inner_map.get(ob).unwrap();
                    log::info!("routing {} to outbound:{}", addr, ob);
                    if stream_builder.is_blackhole() {
                        let err = Error::new(
                            ErrorKind::Other,
                            "HTTP inbound target URI is in blackhole",
                        );
                        return Err(err);
                    }
                    let server = stream_builder.build_tcp(addr).await?;
                    Ok(server)
                }
                Err(_) => {
                    log::error!(
                        "HTTP inbound target URI must be a valid address, but found: {} (authority: {})",
                        uri_clone,
                        addr_str
                    );
                    let err = Error::new(ErrorKind::Other, "URI must be a valid Address");
                    Err(err)
                }
            }
        };
        Box::pin(f)
    }
}

// To proxy tls scheme, the client must use CONNECT method. So here we are always using HTTP1.1.
impl Connection for BoxProxyStream {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}
