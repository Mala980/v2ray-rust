use crate::common::{new_error, LW_BUFFER_SIZE};
use crate::proxy::{
    BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType, UdpRead, UdpWrite,
};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures_util::ready;
use h2::{RecvStream, SendStream};
use http::{Request, Uri, Version};
use log::error;
use rand::random;
use std::collections::HashMap;
use std::io;
use std::io::{Error, ErrorKind};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Clone)]
pub struct Http2StreamBuilder {
    pub hosts: Vec<String>,
    pub headers: HashMap<String, String>,
    pub method: http::Method,
    pub path: http::uri::PathAndQuery,
}

impl Http2StreamBuilder {
    pub fn new(
        hosts: Vec<String>,
        headers: HashMap<String, String>,
        method: http::Method,
        path: http::uri::PathAndQuery,
    ) -> Self {
        Self {
            hosts,
            headers,
            method,
            path,
        }
    }

    fn req(&self) -> io::Result<Request<()>> {
        // For Go compatibility, pick a random host from list
        let host = if self.hosts.is_empty() {
            // Go would use empty host? But we need something
            "example.com"
        } else {
            let uri_idx = random::<usize>() % self.hosts.len();
            self.hosts[uri_idx].as_str()
        };
        let uri: Uri = {
            Uri::builder()
                .scheme("https")
                .authority(host)
                .path_and_query(self.path.as_str())
                .build()
                .map_err(new_error)?
        };
        let mut request = Request::builder()
            .uri(uri)
            .method(self.method.clone())
            .version(Version::HTTP_2);
        // Add custom headers, matching Go's behavior
        // Go's h2 transport allows setting arbitrary headers, but skips Host
        // as it's set via authority. We mimic that.
        for (k, v) in self.headers.iter() {
            // Skip Host header as it's handled via authority, to match Go
            if k.eq_ignore_ascii_case("Host") {
                continue;
            }
            request = request.header(k.as_str(), v.as_str());
        }
        // Go's h2 transport also sets some default headers? For compatibility, we don't add extra.
        Ok(request.body(()).unwrap())
    }
}

macro_rules! http2_build_tcp_impl {
    ($s:tt,$io:tt) => {
        let (mut client, h2) = h2::client::handshake($io).await.map_err(new_error)?;
        let req = $s.req()?;
        let (resp, send_stream) = client.send_request(req, false).map_err(new_error)?;
        tokio::spawn(async move {
            if let Err(e) = h2.await {
                error!("http2 got err:{:?}", e);
            }
        });
        let recv_stream = resp.await.map_err(new_error)?.into_body();
        return Ok(Box::new(Http2Stream::new(recv_stream, send_stream)))
    };
}

#[async_trait]
impl ChainableStreamBuilder for Http2StreamBuilder {
    async fn build_tcp(&self, io: BoxProxyStream) -> io::Result<BoxProxyStream> {
        http2_build_tcp_impl!(self, io);
    }

    async fn build_udp(
        &self,
        io: BoxProxyUdpStream,
        build_tcp_inside: bool,
    ) -> io::Result<BoxProxyUdpStream> {
        if build_tcp_inside {
            http2_build_tcp_impl!(self, io);
        } else {
            Ok(io)
        }
    }

    fn into_box(self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self)
    }

    fn clone_box(&self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::H2
    }
}

// Adapted from https://github.com/zephyrchien/midori/blob/master/src/transport/h2/stream.rs
// For Go compatibility, this implements h2 transport as in v2ray-core
pub struct Http2Stream {
    recv: RecvStream,
    send: SendStream<Bytes>,
    buffer: BytesMut,
}

impl Http2Stream {
    #[inline]
    pub fn new(recv: RecvStream, send: SendStream<Bytes>) -> Self {
        Self {
            recv,
            send,
            buffer: BytesMut::with_capacity(LW_BUFFER_SIZE * 4),
        }
    }
}

impl AsyncRead for Http2Stream {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.buffer.is_empty() {
            let to_read = std::cmp::min(buf.remaining(), self.buffer.len());
            let data = self.buffer.split_to(to_read);
            buf.put_slice(&data[..to_read]);
            return Poll::Ready(Ok(()));
        };
        Poll::Ready(match ready!(self.recv.poll_data(cx)) {
            Some(Ok(data)) => {
                let to_read = std::cmp::min(buf.remaining(), data.len());
                buf.put_slice(&data[..to_read]);
                // copy the left payload into buffer for next read
                if data.len() > to_read {
                    self.buffer.extend_from_slice(&data[to_read..]);
                };
                // increase recv window - critical for flow control as in Go
                self.recv
                    .flow_control()
                    .release_capacity(to_read)
                    .map_or_else(
                        |e| Err(Error::new(ErrorKind::ConnectionReset, e)),
                        |_| Ok(()),
                    )
            }
            Some(Err(e)) => Err(Error::new(ErrorKind::Other, e)),
            None => Ok(()), // EOF
        })
    }
}

impl AsyncWrite for Http2Stream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        self.send.reserve_capacity(buf.len());
        Poll::Ready(match ready!(self.send.poll_capacity(cx)) {
            Some(Ok(to_write)) => {
                let to_write = std::cmp::min(to_write, buf.len());
                self.send
                    .send_data(Bytes::copy_from_slice(&buf[..to_write]), false)
                    .map_or_else(
                        |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                        |_| Ok(to_write),
                    )
            }
            _ => Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
        })
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.send.reserve_capacity(0);
        Poll::Ready(ready!(self.send.poll_capacity(cx)).map_or(
            Err(Error::new(ErrorKind::BrokenPipe, "broken pipe")),
            |_| {
                self.send
                    .send_data(Bytes::new(), true)
                    .map_or_else(|e| Err(Error::new(ErrorKind::BrokenPipe, e)), |_| Ok(()))
            },
        ))
    }
}

impl UdpRead for Http2Stream {}
impl UdpWrite for Http2Stream {}
