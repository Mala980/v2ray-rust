use crate::common::new_error;
use crate::debug_log;
use crate::proxy::{
    BoxProxyStream, BoxProxyUdpStream, ChainableStreamBuilder, ProtocolType, ProxyUdpStream,
    UdpRead, UdpWrite,
};
use async_trait::async_trait;

#[cfg(all(feature = "boring-tls", not(feature = "rustls-tls")))]
mod boring_impl {
    use super::*;
    use boring::ssl::{SslConnector, SslSignatureAlgorithm, SslVerifyMode};
    use boring::ssl::{SslMethod, SslVersion};
    use foreign_types_shared::ForeignTypeRef;
    use std::io;
    use tokio_boring::{connect, SslStream};

    #[cfg(target_os = "macos")]
    use super::super::macos as platform;
    #[cfg(all(unix, not(target_os = "macos")))]
    use super::super::unix as platform;
    #[cfg(windows)]
    use super::super::windows as platform;

    #[derive(Clone)]
    pub struct TlsStreamBuilder {
        connector: SslConnector,
        sni: String,
        verify_hostname: bool,
        verify_sni: bool,
    }

    impl TlsStreamBuilder {
        pub fn new_from_config(
            sni: String,
            cert_file: &Option<String>,
            verify_hostname: bool,
            verify_sni: bool,
        ) -> Self {
            let mut configuration = SslConnector::builder(SslMethod::tls()).unwrap();
            {
                log::debug!("start add system cert");
                match platform::load_native_certs() {
                    Ok(certs) => {
                        log::debug!("certs len:{}", certs.len());
                        let mut count = 0;
                        for cert in certs.into_iter() {
                            if configuration.cert_store_mut().add_cert(cert).is_ok() {
                                count += 1;
                            }
                        }
                        log::debug!("add system cert done, count:{}", count);
                    }
                    Err(e) => {
                        log::warn!("load system certs failed: {}, continuing", e);
                    }
                }
            }
            if let Some(cert_file) = cert_file {
                crate::debug_log!("load custom ca file: {}", cert_file);
                if let Err(e) = configuration.set_ca_file(cert_file) {
                    log::warn!("set ca file {} failed: {}", cert_file, e);
                }
            }
            if let Err(e) = configuration.set_alpn_protos(b"\x02h2\x08http/1.1") {
                log::warn!("set alpn failed: {}", e);
            }
            let _ = configuration.set_cipher_list("ALL:!aPSK:!ECDSA+SHA1:!3DES");
            let _ = configuration.set_verify_algorithm_prefs(&[
                SslSignatureAlgorithm::ECDSA_SECP256R1_SHA256,
                SslSignatureAlgorithm::RSA_PSS_RSAE_SHA256,
                SslSignatureAlgorithm::RSA_PKCS1_SHA256,
                SslSignatureAlgorithm::ECDSA_SECP384R1_SHA384,
                SslSignatureAlgorithm::RSA_PSS_RSAE_SHA384,
                SslSignatureAlgorithm::RSA_PKCS1_SHA384,
                SslSignatureAlgorithm::RSA_PSS_RSAE_SHA512,
                SslSignatureAlgorithm::RSA_PKCS1_SHA512,
            ]);
            let _ = configuration.set_min_proto_version(Some(SslVersion::TLS1_2));
            configuration.enable_signed_cert_timestamps();
            configuration.enable_ocsp_stapling();
            configuration.set_grease_enabled(true);
            if !verify_hostname {
                configuration.set_verify(SslVerifyMode::NONE);
            }
            unsafe {
                boring_sys::SSL_CTX_add_cert_compression_alg(
                    configuration.as_ptr(),
                    boring_sys::TLSEXT_cert_compression_brotli as u16,
                    None,
                    Some(decompress_ssl_cert),
                );
            }
            Self {
                connector: configuration.build(),
                sni,
                verify_hostname,
                verify_sni,
            }
        }
    }

    impl<S: ProxyUdpStream> UdpRead for SslStream<S> {}
    impl<S: ProxyUdpStream> UdpWrite for SslStream<S> {}

    macro_rules! build_tcp_impl {
        ($name:tt,$io:tt) => {
            let mut configuration = $name.connector.configure().unwrap();
            configuration.set_use_server_name_indication($name.verify_sni);
            configuration.set_verify_hostname($name.verify_hostname);
            if !$name.verify_hostname {
                configuration.set_verify(SslVerifyMode::NONE);
            }
            unsafe {
                boring_sys::SSL_add_application_settings(
                    configuration.as_ptr(),
                    b"h2".as_ptr(),
                    2,
                    b"\x00\x03".as_ptr(),
                    2,
                );
            }
            let stream = connect(configuration, $name.sni.as_str(), $io).await;
            return match stream {
                Ok(stream) => Ok(Box::new(stream)),
                Err(e) => {
                    let res = e.to_string();
                    crate::debug_log!("tls connect failed: {}", res);
                    Err(new_error(res))
                }
            };
        };
    }

    #[async_trait]
    impl ChainableStreamBuilder for TlsStreamBuilder {
        async fn build_tcp(&self, io: BoxProxyStream) -> io::Result<BoxProxyStream> {
            build_tcp_impl!(self, io);
        }
        async fn build_udp(
            &self,
            io: BoxProxyUdpStream,
            build_tcp_inside: bool,
        ) -> io::Result<BoxProxyUdpStream> {
            if build_tcp_inside {
                build_tcp_impl!(self, io);
            }
            Ok(io)
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

    extern "C" fn decompress_ssl_cert(
        _ssl: *mut boring_sys::SSL,
        out: *mut *mut boring_sys::CRYPTO_BUFFER,
        mut uncompressed_len: usize,
        in_: *const u8,
        in_len: usize,
    ) -> libc::c_int {
        unsafe {
            let mut buf: *mut u8 = std::ptr::null_mut();
            let x: *mut *mut u8 = &mut buf;
            let allocated_buffer = boring_sys::CRYPTO_BUFFER_alloc(x, uncompressed_len);
            if buf.is_null() {
                return 0;
            }
            let uncompressed_len_ptr: *mut usize = &mut uncompressed_len;
            if brotli::ffi::decompressor::CBrotliDecoderDecompress(
                in_len,
                in_,
                uncompressed_len_ptr,
                buf,
            ) as i32
                == 1
            {
                *out = allocated_buffer;
                1
            } else {
                boring_sys::CRYPTO_BUFFER_free(allocated_buffer);
                0
            }
        }
    }
}

#[cfg(feature = "rustls-tls")]
mod rustls_impl {
    use super::*;
    use std::io;
    use std::sync::Arc;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
    use tokio_rustls::{client::TlsStream, TlsConnector};

    #[cfg(target_os = "macos")]
    use super::super::macos as platform;
    #[cfg(all(unix, not(target_os = "macos")))]
    use super::super::unix as platform;
    #[cfg(windows)]
    use super::super::windows as platform;

    #[derive(Clone)]
    pub struct TlsStreamBuilder {
        config: Arc<ClientConfig>,
        sni: String,
        _verify_sni: bool,
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
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            match platform::load_native_certs() {
                Ok(certs) => {
                    for cert in certs.into_iter() {
                        let _ = root_store.add(cert);
                    }
                }
                Err(e) => {
                    log::warn!("load system certs failed: {}, continuing", e);
                }
            }
            if let Some(cert_file) = cert_file {
                crate::debug_log!("load custom ca file: {}", cert_file);
                if let Ok(pem_data) = std::fs::read(cert_file) {
                    let mut reader = std::io::BufReader::new(&pem_data[..]);
                    for cert in rustls_pemfile::certs(&mut reader).flatten() {
                        let _ = root_store.add(cert);
                    }
                }
            }
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
            let mut config = if !verify_hostname {
                ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
                    .with_no_client_auth()
            } else {
                ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth()
            };
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Self {
                config: Arc::new(config),
                sni,
                _verify_sni: verify_sni,
            }
        }
    }

    impl<S: ProxyUdpStream> UdpRead for TlsStream<S> {}
    impl<S: ProxyUdpStream> UdpWrite for TlsStream<S> {}

    #[async_trait]
    impl ChainableStreamBuilder for TlsStreamBuilder {
        async fn build_tcp(&self, io: BoxProxyStream) -> io::Result<BoxProxyStream> {
            let server_name = ServerName::try_from(self.sni.clone())
                .unwrap_or_else(|_| ServerName::try_from("example.com").unwrap());
            let connector = TlsConnector::from(self.config.clone());
            match connector.connect(server_name, io).await {
                Ok(stream) => Ok(Box::new(stream)),
                Err(e) => {
                    let res = e.to_string();
                    crate::debug_log!("tls connect failed: {}", res);
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
                let server_name = ServerName::try_from(self.sni.clone())
                    .unwrap_or_else(|_| ServerName::try_from("example.com").unwrap());
                let connector = TlsConnector::from(self.config.clone());
                match connector.connect(server_name, io).await {
                    Ok(stream) => Ok(Box::new(stream)),
                    Err(e) => {
                        let res = e.to_string();
                        crate::debug_log!("tls connect failed: {}", res);
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
}

// Default: boring-tls if no rustls-tls feature, or if both features enabled, prefer rustls for Android static
#[cfg(all(feature = "boring-tls", not(feature = "rustls-tls")))]
pub use boring_impl::TlsStreamBuilder;

#[cfg(feature = "rustls-tls")]
pub use rustls_impl::TlsStreamBuilder;

// Fallback if no feature enabled (should not happen, default is boring-tls)
#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
pub use boring_impl::TlsStreamBuilder;

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
