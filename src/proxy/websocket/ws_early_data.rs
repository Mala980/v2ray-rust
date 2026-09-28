use crate::common::new_error;
use crate::debug_log;
use crate::proxy::websocket::BinaryWsStream;
use crate::proxy::{BoxProxyStream, UdpRead, UdpWrite};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::ready;
use std::future::Future;
use std::io::Error;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::{cmp, io};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::client_async_with_config;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue, Request, StatusCode};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

/// BinaryWsStreamWithEarlyData implements WebSocket 0-RTT as in v2ray-core Go
/// The first payload is sent as base64 in Sec-WebSocket-Protocol header
pub(super) struct BinaryWsStreamWithEarlyData {
    stream: Option<BoxProxyStream>,
    req: Option<Request<()>>,
    ws_stream_future: Option<Pin<Box<dyn Future<Output = io::Result<BoxProxyStream>> + Send>>>,
    early_waker: Option<Waker>,
    flush_waker: Option<Waker>,
    ws_config: Option<WebSocketConfig>,
    early_data_header_name: String,
    early_data_len: usize,
    max_early_data: usize,
    is_write_early_data: bool,
}

impl BinaryWsStreamWithEarlyData {
    pub fn new(
        io: BoxProxyStream,
        req: Request<()>,
        ws_config: Option<WebSocketConfig>,
        early_data_header_name: String,
        max_early_data: usize,
    ) -> BinaryWsStreamWithEarlyData {
        Self {
            stream: Some(io),
            req: Some(req),
            ws_stream_future: None,
            early_waker: None,
            flush_waker: None,
            ws_config,
            early_data_header_name,
            early_data_len: 0,
            max_early_data,
            is_write_early_data: false,
        }
    }

    fn build_stream_impl(
        io: BoxProxyStream,
        req: Request<()>,
        config: Option<WebSocketConfig>,
    ) -> Pin<Box<dyn Future<Output = io::Result<BoxProxyStream>> + Send>> {
        async fn run(
            io: BoxProxyStream,
            req: Request<()>,
            config: Option<WebSocketConfig>,
        ) -> io::Result<BoxProxyStream> {
            let (stream, resp) = client_async_with_config(req, io, config)
                .await
                .map_err(new_error)?;
            if resp.status() != StatusCode::SWITCHING_PROTOCOLS {
                return Err(new_error(format!("bad status: {}", resp.status())));
            }
            debug_log!("build ws stream success with 0-rtt");
            let ret: BoxProxyStream = Box::new(BinaryWsStream::new(stream));
            Ok(ret)
        }

        Box::pin(run(io, req, config))
    }
}

impl AsyncRead for BinaryWsStreamWithEarlyData {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        debug_log!("ws-0-rtt poll r");
        if !self.is_write_early_data {
            if self.early_waker.is_none() {
                self.early_waker = Some(cx.waker().clone());
            }
            return Poll::Pending;
        }
        let this = self.get_mut();
        match &mut this.stream {
            None => unreachable!(),
            Some(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for BinaryWsStreamWithEarlyData {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        debug_log!("ws-0-rtt poll w, buf len: {}", buf.len());
        if !self.is_write_early_data {
            loop {
                if let Some(f) = &mut self.ws_stream_future {
                    let stream = ready!(Pin::new(f).poll(cx))?;
                    self.stream = Some(stream);
                    self.is_write_early_data = true;
                    if let Some(w) = self.early_waker.take() {
                        w.wake();
                    }
                    if let Some(w) = self.flush_waker.take() {
                        w.wake();
                    }
                    // Return the length of early data that was sent as header
                    // For Go compatibility, this is considered as written
                    return Poll::Ready(Ok(self.early_data_len));
                } else {
                    let mut req = self.req.take().unwrap();
                    // Encode early data as base64 URL_SAFE_NO_PAD as Go does
                    let early_data_len = cmp::min(self.max_early_data, buf.len());
                    self.early_data_len = early_data_len;
                    
                    if early_data_len > 0 {
                        let header_value = URL_SAFE_NO_PAD.encode(&buf[..early_data_len]);
                        debug_log!("ws-0-rtt early data len: {}, base64 len: {}", early_data_len, header_value.len());
                        // Replace header value
                        if let Some(v) = req.headers_mut().get_mut(&self.early_data_header_name) {
                            *v = HeaderValue::from_str(&header_value)
                                .unwrap_or_else(|_| HeaderValue::from_static(""));
                        } else {
                            // If header not present, insert it
                            let header_name: HeaderName = self
                                .early_data_header_name
                                .parse()
                                .unwrap_or_else(|_| HeaderName::from_static("sec-websocket-protocol"));
                            req.headers_mut().insert(
                                header_name,
                                HeaderValue::from_str(&header_value).unwrap_or_else(|_| HeaderValue::from_static("")),
                            );
                        }
                        debug_log!("ws-0-rtt header {}: base64 len {}", self.early_data_header_name, header_value.len());
                    } else {
                        // No early data to send, remove header if empty?
                        // Go would not send header if no early data, but we keep empty
                    }
                    let io = self.stream.take().unwrap();
                    let config = self.ws_config.take();
                    self.ws_stream_future = Some(
                        BinaryWsStreamWithEarlyData::build_stream_impl(io, req, config),
                    );
                }
            }
        }
        // After handshake, write as normal websocket
        match &mut self.stream {
            None => unreachable!(),
            Some(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        debug_log!("ws-0-rtt poll f");
        if !self.is_write_early_data {
            if self.flush_waker.is_none() {
                self.flush_waker = Some(cx.waker().clone());
            }
            return Poll::Pending;
        }
        let this = self.get_mut();
        match &mut this.stream {
            None => unreachable!(),
            Some(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        debug_log!("ws-0-rtt poll s");
        if !self.is_write_early_data {
            // For Go compatibility, ensure handshake is done before shutdown
            // If flush is pending, wait for it
            ready!(self.as_mut().poll_flush(cx))?;
        }
        let this = self.get_mut();
        match &mut this.stream {
            None => unreachable!(),
            Some(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

impl UdpRead for BinaryWsStreamWithEarlyData {}
impl UdpWrite for BinaryWsStreamWithEarlyData {}
