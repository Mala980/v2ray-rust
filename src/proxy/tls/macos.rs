use rustls::pki_types::CertificateDer;
use std::io;

// For macOS, use rustls-native-certs which uses Security Framework internally
pub fn load_native_certs() -> io::Result<Vec<CertificateDer<'static>>> {
    match rustls_native_certs::load_native_certs() {
        Ok(certs) => Ok(certs),
        Err((_, err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}
