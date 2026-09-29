use crate::common::new_error;
use crate::proxy::{BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType};
use async_trait::async_trait;
use std::io;

/// mKCP transport - for Go parity
/// KCP is a fast and reliable ARQ protocol
/// Currently stub - full implementation would require kcp crate
#[derive(Clone)]
pub struct KcpStreamBuilder {
    pub mtu: u32,
    pub tti: u32,
    pub uplink_capacity: u32,
    pub downlink_capacity: u32,
    pub congestion: bool,
    pub read_buffer_size: u32,
    pub write_buffer_size: u32,
    pub header_type: String,
    pub seed: Option<String>,
}

impl KcpStreamBuilder {
    pub fn new(
        mtu: u32,
        tti: u32,
        uplink_capacity: u32,
        downlink_capacity: u32,
        congestion: bool,
        read_buffer_size: u32,
        write_buffer_size: u32,
        header_type: String,
        seed: Option<String>,
    ) -> Self {
        Self {
            mtu,
            tti,
            uplink_capacity,
            downlink_capacity,
            congestion,
            read_buffer_size,
            write_buffer_size,
            header_type,
            seed,
        }
    }
}

#[async_trait]
impl ChainableStreamBuilder for KcpStreamBuilder {
    async fn build_tcp(&self, _io: BoxProxyStream) -> io::Result<BoxProxyStream> {
        Err(new_error(
            "mKCP transport not implemented - use WS/H2/gRPC for better performance. \
            mKCP is deprecated in favor of QUIC and is not planned for full implementation \
            due to complexity and better alternatives",
        ))
    }

    async fn build_udp(
        &self,
        _io: BoxProxyUdpStream,
        _build_tcp_inside: bool,
    ) -> io::Result<BoxProxyUdpStream> {
        Err(new_error(
            "mKCP transport not implemented - use WS/H2/gRPC",
        ))
    }

    fn into_box(self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self)
    }

    fn clone_box(&self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::Kcp
    }
}
