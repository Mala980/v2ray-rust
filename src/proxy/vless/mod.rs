use crate::proxy::{Address, BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType};
use async_trait::async_trait;
use std::io;

mod vless_stream;
pub mod vless_option;

use vless_option::VlessOption;
use vless_stream::VlessStream;

#[derive(Clone)]
pub struct VlessBuilder {
    pub(crate) vless_option: VlessOption,
}

#[async_trait]
impl ChainableStreamBuilder for VlessBuilder {
    async fn build_tcp(&self, io: BoxProxyStream) -> io::Result<BoxProxyStream> {
        let opt = self.vless_option.clone();
        Ok(Box::new(VlessStream::new(opt, io)))
    }

    async fn build_udp(
        &self,
        io: BoxProxyUdpStream,
        build_tcp_inside: bool,
    ) -> io::Result<BoxProxyUdpStream> {
        let mut opt = self.vless_option.clone();
        opt.is_udp = !build_tcp_inside;
        Ok(Box::new(VlessStream::new(opt, io)))
    }

    fn into_box(self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self)
    }

    fn clone_box(&self) -> Box<dyn ChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::Vless
    }

    fn get_addr(&self) -> Option<Address> {
        Some(self.vless_option.addr.clone())
    }
}
