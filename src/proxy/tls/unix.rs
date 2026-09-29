#[cfg(all(feature = "boring-tls", not(feature = "rustls-tls")))]
mod boring_platform {
    use crate::common::new_error;
    use boring::x509::X509;
    use std::io;
    pub fn load_native_certs() -> io::Result<Vec<X509>> {
        let likely_locations = openssl_probe::probe();
        match likely_locations.cert_file {
            Some(cert_file) => {
                let pem = std::fs::read(cert_file)?;
                X509::stack_from_pem(pem.as_ref()).map_err(new_error)
            }
            None => Ok(Vec::new()),
        }
    }
}
#[cfg(all(feature = "boring-tls", not(feature = "rustls-tls")))]
pub use boring_platform::load_native_certs;

#[cfg(feature = "rustls-tls")]
mod rustls_platform {
    use rustls::pki_types::CertificateDer;
    use std::io;
    pub fn load_native_certs() -> io::Result<Vec<CertificateDer<'static>>> {
        let likely_locations = openssl_probe::probe();
        match likely_locations.cert_file {
            Some(cert_file) => {
                let pem = std::fs::read(cert_file)?;
                let mut reader = std::io::BufReader::new(&pem[..]);
                let certs = rustls_pemfile::certs(&mut reader)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(certs)
            }
            None => Ok(Vec::new()),
        }
    }
}
#[cfg(feature = "rustls-tls")]
pub use rustls_platform::load_native_certs;

#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
mod fallback_platform {
    use crate::common::new_error;
    use boring::x509::X509;
    use std::io;
    pub fn load_native_certs() -> io::Result<Vec<X509>> {
        let likely_locations = openssl_probe::probe();
        match likely_locations.cert_file {
            Some(cert_file) => {
                let pem = std::fs::read(cert_file)?;
                X509::stack_from_pem(pem.as_ref()).map_err(new_error)
            }
            None => Ok(Vec::new()),
        }
    }
}
#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
pub use fallback_platform::load_native_certs;
