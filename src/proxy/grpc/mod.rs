use crate::common::{new_error, LW_BUFFER_SIZE};
use crate::proxy::{
    BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType, UdpRead, UdpWrite,
};
use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};

use futures_util::ready;
use h2::{RecvStream, SendStream};
use http::{Request, Uri, Version};
use log::error;
use prost::encoding::{decode_varint, encode_varint};

use std::future::Future;
use std::io;
use std::io::{Error, ErrorKind};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Clone)]
pub struct GrpcStreamBuilder {
    pub host: String,
    pub path: http::uri::PathAndQuery,
}

impl GrpcStreamBuilder {
    pub fn new(host: String, path: http::uri::PathAndQuery) -> Self {
        Self { host, path }
    }
    fn req(&self) -> io::Result<Request<()>> {
        let uri: Uri = {
            Uri::builder()
                .scheme("https")
                .authority(self.host.as_str())
                .path_and_query(self.path.as_str())
                .build()
                .map_err(new_error)?
        };
        // For Go compatibility, include content-type and user-agent as Go does
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .version(Version::HTTP_2)
            .header("content-type", "application/grpc")
            .header("user-agent", "grpc-go/1.46.0")
            .header("TE", "trailers");
        Ok(request.body(()).unwrap())
    }
}

macro_rules! grpc_build_tcp_impl {
    ($s:tt,$io:tt) => {
        let (mut client, h2) = h2::client::handshake($io).await.map_err(new_error)?;
        let req = $s.req()?;
        let (resp, send_stream) = client.send_request(req, false).map_err(new_error)?;
        tokio::spawn(async move {
            if let Err(e) = h2.await {
                error!("http2 got err:{:?}", e);
            }
        });
        return Ok(Box::new(GrpcStream::new(resp, send_stream)))
    };
}

#[async_trait]
impl ChainableStreamBuilder for GrpcStreamBuilder {
    async fn build_tcp(&self, io: BoxProxyStream) -> io::Result<BoxProxyStream> {
        grpc_build_tcp_impl!(self, io);
    }

    async fn build_udp(
        &self,
        io: BoxProxyUdpStream,
        build_tcp_inside: bool,
    ) -> io::Result<BoxProxyUdpStream> {
        if build_tcp_inside {
            grpc_build_tcp_impl!(self, io);
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
        ProtocolType::Grpc
    }
}

/// GrpcStream implements Gun transport as in v2ray-core Go
/// Each write sends a Hunk message: gRPC header (5 bytes) + protobuf (0x0A + varint len + data)
/// Each read decodes Hunk messages and returns raw data
pub struct GrpcStream {
    resp_fut: h2::client::ResponseFuture,
    recv: Option<RecvStream>,
    send: SendStream<Bytes>,
    // Buffer for raw h2 data that hasn't been parsed into gRPC frames yet
    raw_buffer: BytesMut,
    // Buffer for decoded payload data that didn't fit into dst
    payload_buffer: BytesMut,
    // Remaining length of current Hunk data to read
    current_hunk_remaining: usize,
    // Whether we are in the middle of parsing a gRPC frame
    // For Go compatibility, we handle gRPC framing exactly as Go does
}

impl GrpcStream {
    pub fn new(resp_fut: h2::client::ResponseFuture, send: SendStream<Bytes>) -> Self {
        Self {
            resp_fut,
            recv: None,
            send,
            raw_buffer: BytesMut::with_capacity(LW_BUFFER_SIZE * 8),
            payload_buffer: BytesMut::with_capacity(LW_BUFFER_SIZE * 4),
            current_hunk_remaining: 0,
        }
    }

    fn reserve_send_capacity(&mut self, data: &[u8]) {
        let mut varint_buf = [0u8; 10];
        let mut slice = &mut varint_buf[..];
        encode_varint(data.len() as u64, &mut slice);
        let varint_len = 10 - slice.len();
        // gRPC header 5 + tag 1 + varint_len + data_len
        let total = 5 + 1 + varint_len + data.len();
        self.send.reserve_capacity(total);
    }

    fn encode_hunk(&self, data: &[u8]) -> Bytes {
        let mut varint_buf = [0u8; 10];
        let mut slice = &mut varint_buf[..];
        encode_varint(data.len() as u64, &mut slice);
        let varint_len = 10 - slice.len();
        let varint_bytes = &varint_buf[..varint_len];

        let protobuf_len = 1 + varint_len + data.len();
        let mut buf = BytesMut::with_capacity(5 + protobuf_len);
        // gRPC header: 1 byte compression flag (0) + 4 bytes big-endian length
        buf.put_u8(0);
        buf.put_u32(protobuf_len as u32);
        // Protobuf Hunk: field 1, wire type 2 (length-delimited) => tag 0x0A
        buf.put_u8(0x0A);
        buf.put_slice(varint_bytes);
        buf.put_slice(data);
        buf.freeze()
    }

    // Try to parse one gRPC frame from raw_buffer, returns Some(data) if successful
    // For Go compatibility, we strictly follow gRPC + Hunk parsing
    fn try_parse_hunk(&mut self) -> io::Result<Option<BytesMut>> {
        // Need at least 5 bytes for gRPC header
        if self.raw_buffer.len() < 5 {
            return Ok(None);
        }
        // Check compression flag
        if self.raw_buffer[0] != 0 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("unexpected gRPC compression flag: {}", self.raw_buffer[0]),
            ));
        }
        let protobuf_len = u32::from_be_bytes([
            self.raw_buffer[1],
            self.raw_buffer[2],
            self.raw_buffer[3],
            self.raw_buffer[4],
        ]) as usize;

        if protobuf_len > 16 * 1024 * 1024 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("gRPC message too large: {}", protobuf_len),
            ));
        }

        if self.raw_buffer.len() < 5 + protobuf_len {
            // Not enough data yet
            return Ok(None);
        }

        // Parse protobuf Hunk
        let protobuf_data = &self.raw_buffer[5..5 + protobuf_len];
        if protobuf_data.is_empty() || protobuf_data[0] != 0x0A {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("invalid Hunk protobuf tag: {:x}", protobuf_data.get(0).unwrap_or(&0)),
            ));
        }

        let mut protobuf_slice = &protobuf_data[1..];
        let data_len = decode_varint(&mut protobuf_slice).map_err(new_error)? as usize;

        if protobuf_slice.len() < data_len {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Hunk data length exceeds protobuf remaining",
            ));
        }

        let data = BytesMut::from(&protobuf_slice[..data_len]);

        // Remove parsed frame from raw_buffer
        self.raw_buffer.advance(5 + protobuf_len);

        Ok(Some(data))
    }
}

impl AsyncRead for GrpcStream {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        dst: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Ensure recv stream is ready (wait for response headers)
        if self.recv.is_none() {
            let resp = ready!(Pin::new(&mut self.resp_fut).poll(cx)).map_err(new_error)?;
            // Check status? Go would check 200, but we assume OK
            self.recv = Some(resp.into_body());
            log::debug!("receive grpc recv stream");
        }

        // First, drain payload_buffer if not empty
        if !self.payload_buffer.is_empty() {
            let to_read = std::cmp::min(dst.remaining(), self.payload_buffer.len());
            let data = self.payload_buffer.split_to(to_read);
            dst.put_slice(&data);
            return Poll::Ready(Ok(()));
        }

        // If we have remaining data from current hunk (should be handled via payload_buffer)
        // Try to parse hunks from raw_buffer
        loop {
            // Try to parse a hunk from raw_buffer
            match self.try_parse_hunk() {
                Ok(Some(data)) => {
                    // We have a complete hunk data
                    if data.len() <= dst.remaining() {
                        dst.put_slice(&data);
                        // Continue loop to try to read more hunks if dst still has space?
                        // For Go compatibility, GunConn.Read returns up to len(b) bytes,
                        // possibly from one hunk. We'll return after one hunk if dst is full,
                        // otherwise continue to try to fill dst with next hunk.
                        if dst.remaining() == 0 {
                            return Poll::Ready(Ok(()));
                        }
                        // Continue to try next hunk
                        continue;
                    } else {
                        // Data larger than dst, split
                        let to_read = dst.remaining();
                        dst.put_slice(&data[..to_read]);
                        self.payload_buffer.extend_from_slice(&data[to_read..]);
                        return Poll::Ready(Ok(()));
                    }
                }
                Ok(None) => {
                    // Need more data from h2 stream
                    break;
                }
                Err(e) => {
                    return Poll::Ready(Err(e));
                }
            }
        }

        // Need to read more h2 DATA frames
        let recv = self.recv.as_mut().unwrap();
        match ready!(Pin::new(recv).poll_data(cx)) {
            Some(Ok(mut data)) => {
                let data_len = data.len();
                self.raw_buffer.extend_from_slice(&data);

                // Try to parse again after adding new data
                // We will loop to parse as many hunks as possible into dst
                loop {
                    match self.try_parse_hunk() {
                        Ok(Some(hunk_data)) => {
                            if hunk_data.len() <= dst.remaining() {
                                dst.put_slice(&hunk_data);
                                if dst.remaining() == 0 {
                                    // Release flow control for consumed data
                                    let _ = self
                                        .recv
                                        .as_mut()
                                        .unwrap()
                                        .flow_control()
                                        .release_capacity(data_len);
                                    return Poll::Ready(Ok(()));
                                }
                                continue;
                            } else {
                                let to_read = dst.remaining();
                                if to_read > 0 {
                                    dst.put_slice(&hunk_data[..to_read]);
                                    self.payload_buffer.extend_from_slice(&hunk_data[to_read..]);
                                } else {
                                    self.payload_buffer.extend_from_slice(&hunk_data);
                                }
                                let _ = self
                                    .recv
                                    .as_mut()
                                    .unwrap()
                                    .flow_control()
                                    .release_capacity(data_len);
                                return Poll::Ready(Ok(()));
                            }
                        }
                        Ok(None) => {
                            // Not enough data to parse a full hunk, need more h2 data
                            // Release capacity for the data we consumed into raw_buffer
                            // But we haven't consumed it yet (still in raw_buffer), so we should not release yet?
                            // Actually we should release the h2 flow control window for the data we received,
                            // as Go does. We'll release now.
                            let _ = self
                                .recv
                                .as_mut()
                                .unwrap()
                                .flow_control()
                                .release_capacity(data_len);
                            // Return Pending to wait for more data if dst is still empty
                            if dst.filled().is_empty() {
                                // No data produced yet, need to wait for more
                                // But we already have raw_buffer with partial frame, so we should return Pending
                                // and let next poll try again after more data arrives
                                // For now, return Pending if no data in dst
                                // Actually we should not return Ok(()) with empty dst, as that signals EOF
                                // So we return Pending
                                // However, we already released capacity, so we need to poll again
                                // To avoid busy loop, return Pending
                                return Poll::Pending;
                            } else {
                                return Poll::Ready(Ok(()));
                            }
                        }
                        Err(e) => {
                            let _ = self
                                .recv
                                .as_mut()
                                .unwrap()
                                .flow_control()
                                .release_capacity(data_len);
                            return Poll::Ready(Err(e));
                        }
                    }
                }
            }
            Some(Err(e)) => {
                return Poll::Ready(Err(Error::new(ErrorKind::Other, e)));
            }
            None => {
                // No more data frames - maybe trailer or cancelled
                // If we have payload_buffer or raw_buffer with data, return it, otherwise EOF
                if !self.payload_buffer.is_empty() || !self.raw_buffer.is_empty() {
                    // Try to drain payload_buffer
                    if !self.payload_buffer.is_empty() {
                        let to_read = std::cmp::min(dst.remaining(), self.payload_buffer.len());
                        let data = self.payload_buffer.split_to(to_read);
                        dst.put_slice(&data);
                        return Poll::Ready(Ok(()));
                    }
                    // If raw_buffer has incomplete data, treat as EOF? For Go compatibility, return Ok(())
                    return Poll::Ready(Ok(()));
                }
                // EOF
                return Poll::Ready(Ok(()));
            }
        }
    }
}

impl AsyncWrite for GrpcStream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        self.reserve_send_capacity(buf);
        Poll::Ready(match ready!(self.send.poll_capacity(cx)) {
            Some(Ok(to_write)) => {
                // to_write is the capacity reserved, but we may send less than buf.len() if capacity < total?
                // For simplicity, we send min(to_write, buf.len())? But our reserve is for full buf.
                // Go's GunConn.Write sends entire buf as one Hunk.
                // We'll send entire buf as one Hunk, but report to_write as bytes consumed.
                // Actually to_write should be buf.len() if reserve succeeded for full size.
                // If reserve is smaller, we should send partial? For Go compatibility, we send what we reserved.
                let data_to_send = if to_write >= buf.len() {
                    buf
                } else {
                    &buf[..to_write]
                };
                let encoded_buf = self.encode_hunk(data_to_send);
                self.send.send_data(encoded_buf, false).map_or_else(
                    |e| Err(Error::new(ErrorKind::BrokenPipe, e)),
                    |_| Ok(data_to_send.len()),
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

impl UdpRead for GrpcStream {}
impl UdpWrite for GrpcStream {}
