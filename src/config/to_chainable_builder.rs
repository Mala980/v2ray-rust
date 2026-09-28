use crate::config::{
    BlackHoleConfig, DirectConfig, DomainSocketConfig, GrpcConfig, Http2Config, HttpConfig,
    KcpConfig, QuicConfig, ShadowsocksConfig, SimpleObfsConfig, TlsConfig, TrojanConfig,
    VlessConfig, VmessConfig, WebsocketConfig, SS_LOCAL_SHARED_CONTEXT,
};
use crate::proxy::blackhole::BlackHoleStreamBuilder;
use crate::proxy::direct::DirectStreamBuilder;
use crate::proxy::domainsocket::DomainSocketStreamBuilder;
use crate::proxy::grpc::GrpcStreamBuilder;
use crate::proxy::h2::Http2StreamBuilder;
use crate::proxy::http_transport::HttpTransportBuilder;
use crate::proxy::kcp::KcpStreamBuilder;
use crate::proxy::quic::QuicStreamBuilder;
use crate::proxy::shadowsocks::ShadowsocksBuilder;
use crate::proxy::simpleobfs::SimpleObfsStreamBuilder;
use crate::proxy::tls::TlsStreamBuilder;
use crate::proxy::trojan::TrojanStreamBuilder;
use crate::proxy::vless::vless_option::VlessOption;
use crate::proxy::vless::VlessBuilder;
use crate::proxy::vmess::vmess_option::VmessOption;
use crate::proxy::vmess::VmessBuilder;
use crate::proxy::websocket::BinaryWsStreamBuilder;
use crate::proxy::{Address, ChainableStreamBuilder, ProtocolType};

pub trait ToChainableStreamBuilder: Sync + Send {
    fn to_chainable_stream_builder(&self, addr: Option<Address>)
        -> Box<dyn ChainableStreamBuilder>;
    fn tag(&self) -> &str;
    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder>;
    fn get_protocol_type(&self) -> ProtocolType;
    fn get_addr(&self) -> Option<Address> {
        None
    }
}
impl Clone for Box<dyn ToChainableStreamBuilder> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

impl ToChainableStreamBuilder for VmessConfig {
    fn to_chainable_stream_builder(
        &self,
        addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(VmessBuilder {
            vmess_option: VmessOption {
                uuid: self.uuid,
                alter_id: 0,
                addr: addr.unwrap(),
                security_num: self.security_num,
                is_udp: false,
            },
        })
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Vmess
    }

    fn get_addr(&self) -> Option<Address> {
        Some(self.addr.clone())
    }
}

impl ToChainableStreamBuilder for VlessConfig {
    fn to_chainable_stream_builder(
        &self,
        addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(VlessBuilder {
            vless_option: VlessOption {
                uuid: self.uuid,
                addr: addr.unwrap(),
                is_udp: false,
                flow: self.flow.clone(),
            },
        })
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Vless
    }

    fn get_addr(&self) -> Option<Address> {
        Some(self.addr.clone())
    }
}

impl ToChainableStreamBuilder for TrojanConfig {
    fn to_chainable_stream_builder(
        &self,
        addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(TrojanStreamBuilder::new(
            addr.unwrap(),
            self.password.as_bytes(),
            false,
        ))
    }
    fn tag(&self) -> &str {
        self.tag.as_str()
    }
    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Trojan
    }

    fn get_addr(&self) -> Option<Address> {
        Some(self.addr.clone())
    }
}
impl ToChainableStreamBuilder for TlsConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(TlsStreamBuilder::new_from_config(
            self.sni.clone(),
            &self.cert_file,
            self.verify_hostname,
            self.verify_sni,
        ))
    }
    fn tag(&self) -> &str {
        self.tag.as_str()
    }
    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Tls
    }
}

impl ToChainableStreamBuilder for WebsocketConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        // we use early data config in uri query.
        if self.uri.max_early_data > 0 {
            Box::new(BinaryWsStreamBuilder::new_from_config(
                self.uri.uri.clone(),
                self.uri.max_early_data,
                self.uri.early_data_header_name.clone(),
                None,
                self.headers.clone(),
            ))
        } else if self.max_early_data > 0 && !self.early_data_header_name.is_empty() {
            Box::new(BinaryWsStreamBuilder::new_from_config(
                self.uri.uri.clone(),
                self.max_early_data,
                self.early_data_header_name.clone(),
                None,
                self.headers.clone(),
            ))
        } else {
            Box::new(BinaryWsStreamBuilder::new_from_config(
                self.uri.uri.clone(),
                0,
                String::new(),
                None,
                self.headers.clone(),
            ))
        }
    }
    fn tag(&self) -> &str {
        self.tag.as_str()
    }
    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::WS
    }
}
impl ToChainableStreamBuilder for BlackHoleConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(BlackHoleStreamBuilder)
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Blackhole
    }
}
impl ToChainableStreamBuilder for DirectConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(DirectStreamBuilder)
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Direct
    }
}
impl ToChainableStreamBuilder for ShadowsocksConfig {
    fn to_chainable_stream_builder(
        &self,
        addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(ShadowsocksBuilder::new_from_config(
            addr.unwrap(),
            self.password.as_str(),
            self.method,
            SS_LOCAL_SHARED_CONTEXT.clone(),
        ))
    }
    fn tag(&self) -> &str {
        self.tag.as_str()
    }
    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::SS
    }

    fn get_addr(&self) -> Option<Address> {
        Some(self.addr.clone())
    }
}

impl ToChainableStreamBuilder for GrpcConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(GrpcStreamBuilder::new(self.host.clone(), self.path.clone()))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Grpc
    }
}

impl ToChainableStreamBuilder for SimpleObfsConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(SimpleObfsStreamBuilder::new(self.host.clone()))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::SimpleObfs
    }
}

impl ToChainableStreamBuilder for Http2Config {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(Http2StreamBuilder::new(
            self.hosts.clone(),
            self.headers.clone(),
            self.method.clone(),
            self.path.clone(),
        ))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::H2
    }
}

impl ToChainableStreamBuilder for DomainSocketConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(DomainSocketStreamBuilder::new(self.path.clone()))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::DomainSocket
    }
}

impl ToChainableStreamBuilder for HttpConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(HttpTransportBuilder::new(
            self.hosts.clone(),
            self.headers.clone(),
            self.method.clone(),
            self.path.clone(),
        ))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Http
    }
}

impl ToChainableStreamBuilder for QuicConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(QuicStreamBuilder::new(
            self.security.clone(),
            self.key.clone(),
            self.header_type.clone(),
        ))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Quic
    }
}

impl ToChainableStreamBuilder for KcpConfig {
    fn to_chainable_stream_builder(
        &self,
        _addr: Option<Address>,
    ) -> Box<dyn ChainableStreamBuilder> {
        Box::new(KcpStreamBuilder::new(
            self.mtu,
            self.tti,
            self.uplink_capacity,
            self.downlink_capacity,
            self.congestion,
            self.read_buffer_size,
            self.write_buffer_size,
            self.header_type.clone(),
            self.seed.clone(),
        ))
    }

    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    fn clone_box(&self) -> Box<dyn ToChainableStreamBuilder> {
        Box::new(self.clone())
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::Kcp
    }
}
