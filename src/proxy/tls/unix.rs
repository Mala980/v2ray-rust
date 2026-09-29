use rustls::pki_types::CertificateDer;
use std::io;

// For rustls, load native certs via openssl_probe + rustls-native-certs
pub fn load_native_certs() -> io::Result<Vec<CertificateDer<'static>>> {
    let mut certs = Vec::new();

    // Try openssl_probe locations
    let likely_locations = openssl_probe::probe();
    if let Some(cert_file) = likely_locations.cert_file {
        if let Ok(pem_data) = std::fs::read(&cert_file) {
            let mut reader = std::io::BufReader::new(&pem_data[..]);
            for cert in rustls_pemfile::certs(&mut reader).flatten() {
                certs.push(cert);
            }
        }
    }

    // Also try rustls-native-certs
    match rustls_native_certs::load_native_certs() {
        Ok(native_certs) => {
            certs.extend(native_certs);
        }
        Err(_) => {}
    }

    Ok(certs)
}
