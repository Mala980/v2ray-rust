use crate::common::new_error;
use crate::debug_log;
use crate::proxy::{
    BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType, ProxyUdpStream,
    UdpRead, UdpWrite,
};
use async_trait::async_trait;
use std::io;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use tokio_rustls::{client::TlsStream, TlsConnector};

#[cfg(target_os = "macos")]
use super::macos as platform;
#[cfg(all(unix, not(target_os = "macos")))]
use super::unix as platform;
#[cfg(windows)]
use super::windows as platform;

#[derive(Clone)]
pub struct TlsStreamBuilder {
    config: Arc<ClientConfig>,
    sni: String,
    verify_sni: bool,
}

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

impl TlsStreamBuilder {
    pub fn new_from_config(
        sni: String,
        cert_file: &Option<String>,
        verify_hostname: bool,
        verify_sni: bool,
    ) -> Self {
        let mut root_store = rustls::RootCertStore::empty();

        // Load webpki roots
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        // Load native certs via platform
        log::debug!("start add system cert");
        match platform::load_native_certs() {
            Ok(certs) => {
                log::debug!("certs len:{}", certs.len());
                let mut count = 0;
                for cert in certs.into_iter() {
                    if root_store.add(cert).is_ok() {
                        count += 1;
                    }
                }
                log::debug!("add system cert done, count:{}", count);
            }
            Err(e) => {
                log::warn!("load system certs failed: {}, continuing", e);
            }
        }

        // Load custom CA file if provided
        if let Some(cert_file) = cert_file {
            debug_log!("load custom ca file: {}", cert_file);
            match std::fs::read(cert_file) {
                Ok(pem_data) => {
                    let mut reader = std::io::BufReader::new(&pem_data[..]);
                    for cert in rustls_pemfile::certs(&mut reader).flatten() {
                        let _ = root_store.add(cert);
                    }
                }
                Err(e) => {
                    log::warn!("read ca file {} failed: {}", cert_file, e);
                }
            }
        }

        // Also try rustls-native-certs for more system certs
        match rustls_native_certs::load_native_certs() {
            Ok(certs) => {
                for cert in certs {
                    let _ = root_store.add(cert);
                }
            }
            Err(err) => {
                log::debug!("rustls-native-certs failed: {}, continuing", err);
            }
        }

        let config_builder = ClientConfig::builder().with_root_certificates(root_store);

        let mut config = if !verify_hostname {
            // Allow insecure - custom verifier that accepts any cert
            config_builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
                .with_no_client_auth()
        } else {
            config_builder.with_no_client_auth()
        };

        // ALPN for Go compatibility: h2, http/1.1
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        // For rustls, we don't need to set cipher list manually - it uses secure defaults
        // Min version TLS1.2 is default in rustls 0.23

        Self {
            config: Arc::new(config),
            sni,
            verify_sni,
        }
    }
}

impl<S: ProxyUdpStream> UdpRead for TlsStream<S> {}
impl<S: ProxyUdpStream> UdpWrite for TlsStream<S> {}

#[async_trait]
impl ChainableStreamBuilder for TlsStreamBuilder {
    async fn build_tcp(&self, io: BoxProxyStream) -> io::Result<BoxProxyStream> {
        let sni_str = if self.verify_sni {
            self.sni.clone()
        } else {
            // If verify_sni is false, we still need SNI for connection but don't verify?
            // For rustls, we need to provide server name for SNI, but we can use sni even if verify_sni false
            self.sni.clone()
        };

        let server_name = ServerName::try_from(sni_str.clone())
            .unwrap_or_else(|_| ServerName::try_from("example.com").unwrap());

        let connector = TlsConnector::from(self.config.clone());
        match connector.connect(server_name, io).await {
            Ok(stream) => Ok(Box::new(stream)),
            Err(e) => {
                let res = e.to_string();
                debug_log!("tls connect failed: {}", res);
                Err(new_error(res))
            }
        }
    }

    async fn build_udp(
        &self,
        io: BoxProxyUdpStream,
        build_tcp_inside: bool,
    ) -> io::Result<BoxProxyUdpStream> {
        if build_tcp_inside {
            // For UDP, we need to build TCP inside if requested - but TLS over UDP is not standard
            // We'll just return io as before for UDP
            let sni_str = self.sni.clone();
            let server_name = ServerName::try_from(sni_str)
                .unwrap_or_else(|_| ServerName::try_from("example.com").unwrap());
            let connector = TlsConnector::from(self.config.clone());
            match connector.connect(server_name, io).await {
                Ok(stream) => Ok(Box::new(stream)),
                Err(e) => {
                    let res = e.to_string();
                    debug_log!("tls connect failed: {}", res);
                    Err(new_error(res))
                }
            }
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
        ProtocolType::Tls
    }
}

#[cfg(all(target_os = "linux", test))]
mod tests {
    use crate::proxy::tls::tls_stream::TlsStreamBuilder;
    use crate::proxy::ChainableStreamBuilder;
    use std::net::ToSocketAddrs;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_tls() {
        let b = "google.com:443";
        let addr = b.to_socket_addrs().unwrap().next().unwrap();
        let stream = TcpStream::connect(&addr).await.unwrap();
        println!("local:{}", stream.local_addr().unwrap());
        let b = TlsStreamBuilder::new_from_config("google.com".to_string(), &None, true, true);
        let mut stream = b.build_tcp(Box::new(stream)).await.unwrap();
        stream.write_all(b"GET / HTTP/1.1\r\nHost: google.com\r\nAccept: */*\r\nUser-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/98.0.4758.102 Safari/537.36\r\n\r\n").await.unwrap();
        let mut buf = vec![0u8; 1024];
        stream.read_buf(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf);
        let response = response.trim_end();
        println!("from google response:{}", response);
    }
}
