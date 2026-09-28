use crate::common::new_error;
use crate::proxy::{BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType};
use async_trait::async_trait;
use std::io;

/// QUIC transport - for Go parity
/// Currently implemented as stub that returns error, as QUIC requires quiche/quinn
/// For full implementation, would use quinn crate with BoringSSL or ring
#[derive(Clone)]
pub struct QuicStreamBuilder {
    pub security: String,
    pub key: String,
    pub header_type: String,
}

impl QuicStreamBuilder {
    pub fn new(security: String, key: String, header_type: String) -> Self {
        Self {
            security,
            key,
            header_type,
        }
    }
}

#[async_trait]
impl ChainableStreamBuilder for QuicStreamBuilder {
    async fn build_tcp(&self, _io: BoxProxyStream) -> io::Result<BoxProxyStream> {
        // QUIC is UDP-based, not TCP, so for TCP we return error
        // Go's QUIC transport only works over UDP
        Err(new_error("QUIC transport not implemented for TCP - use UDP"))
    }

    async fn build_udp(
        &self,
        _io: BoxProxyUdpStream,
        _build_tcp_inside: bool,
    ) -> io::Result<BoxProxyUdpStream> {
        // For now, return error indicating QUIC not yet fully implemented
        // Full implementation would require quinn crate
        // This matches Go's behavior where QUIC needs special handling
        Err(new_error(
            "QUIC transport not yet implemented in v2ray-rust - use WS/H2/gRPC for now. \
            Track: https://github.com/Mala980/v2ray-rust/issues - QUIC planned",
        ))
    }

    fn into_box(self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self)
    }

    fn clone_box(&self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::Quic
    }
}
