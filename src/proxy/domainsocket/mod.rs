use crate::proxy::{BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType};
use async_trait::async_trait;
use std::io;

#[derive(Clone)]
pub struct DomainSocketStreamBuilder {
    pub path: String,
}

impl DomainSocketStreamBuilder {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

#[cfg(unix)]
mod imp_unix {
    use super::*;
    use crate::proxy::{UdpRead, UdpWrite};
    use tokio::net::UnixStream;

    #[async_trait]
    impl ChainableStreamBuilder for DomainSocketStreamBuilder {
        async fn build_tcp(&self, _io: BoxProxyStream) -> io::Result<BoxProxyStream> {
            let stream = UnixStream::connect(&self.path).await?;
            Ok(Box::new(stream))
        }

        async fn build_udp(
            &self,
            io: BoxProxyUdpStream,
            build_tcp_inside: bool,
        ) -> io::Result<BoxProxyUdpStream> {
            if build_tcp_inside {
                let stream = UnixStream::connect(&self.path).await?;
                Ok(Box::new(stream))
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
            ProtocolType::DomainSocket
        }
    }

    impl UdpRead for UnixStream {}
    impl UdpWrite for UnixStream {}
}

#[cfg(not(unix))]
mod imp_other {
    use super::*;
    #[async_trait]
    impl ChainableStreamBuilder for DomainSocketStreamBuilder {
        async fn build_tcp(&self, _io: BoxProxyStream) -> io::Result<BoxProxyStream> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "DomainSocket transport not supported on this platform (Go v2ray-core also only supports Unix domain sockets on Unix)",
            ))
        }

        async fn build_udp(
            &self,
            _io: BoxProxyUdpStream,
            _build_tcp_inside: bool,
        ) -> io::Result<BoxProxyUdpStream> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "DomainSocket transport not supported on this platform",
            ))
        }

        fn into_box(self) -> Box<dyn ChainableStreamBuilder> {
            Box::new(self)
        }

        fn clone_box(&self) -> Box<dyn ChainableStreamBuilder> {
            Box::new(self.clone())
        }

        fn protocol_type(&self) -> ProtocolType {
            ProtocolType::DomainSocket
        }
    }
}
