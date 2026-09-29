#[cfg(all(feature = "boring-tls", not(feature = "rustls-tls")))]
mod boring_platform {
    use crate::common::new_error;
    use boring::x509::X509;
    use std::io;
    static PKIX_SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
    fn usable_for_rustls(uses: schannel::cert_context::ValidUses) -> bool {
        match uses {
            schannel::cert_context::ValidUses::All => true,
            schannel::cert_context::ValidUses::Oids(strs) => strs.iter().any(|x| x == PKIX_SERVER_AUTH),
        }
    }
    pub fn load_native_certs() -> io::Result<Vec<X509>> {
        let mut certs = Vec::new();
        let current_user_store = schannel::cert_store::CertStore::open_current_user("Root")?;
        for cert in current_user_store.certs() {
            if usable_for_rustls(cert.valid_uses().unwrap()) && cert.is_time_valid().unwrap() {
                certs.push(X509::from_der(cert.to_der()).map_err(new_error)?);
            }
        }
        Ok(certs)
    }
}
#[cfg(all(feature = "boring-tls", not(feature = "rustls-tls")))]
pub use boring_platform::load_native_certs;

#[cfg(feature = "rustls-tls")]
mod rustls_platform {
    use rustls::pki_types::CertificateDer;
    use std::io;
    static PKIX_SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
    fn usable_for_rustls(uses: schannel::cert_context::ValidUses) -> bool {
        match uses {
            schannel::cert_context::ValidUses::All => true,
            schannel::cert_context::ValidUses::Oids(strs) => strs.iter().any(|x| x == PKIX_SERVER_AUTH),
        }
    }
    pub fn load_native_certs() -> io::Result<Vec<CertificateDer<'static>>> {
        let mut certs = Vec::new();
        let current_user_store = schannel::cert_store::CertStore::open_current_user("Root")?;
        for cert in current_user_store.certs() {
            if usable_for_rustls(cert.valid_uses().unwrap()) && cert.is_time_valid().unwrap() {
                certs.push(CertificateDer::from(cert.to_der().to_vec()));
            }
        }
        Ok(certs)
    }
}
#[cfg(feature = "rustls-tls")]
pub use rustls_platform::load_native_certs;

#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
mod fallback_platform {
    use crate::common::new_error;
    use boring::x509::X509;
    use std::io;
    static PKIX_SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
    fn usable_for_rustls(uses: schannel::cert_context::ValidUses) -> bool {
        match uses {
            schannel::cert_context::ValidUses::All => true,
            schannel::cert_context::ValidUses::Oids(strs) => strs.iter().any(|x| x == PKIX_SERVER_AUTH),
        }
    }
    pub fn load_native_certs() -> io::Result<Vec<X509>> {
        let mut certs = Vec::new();
        let current_user_store = schannel::cert_store::CertStore::open_current_user("Root")?;
        for cert in current_user_store.certs() {
            if usable_for_rustls(cert.valid_uses().unwrap()) && cert.is_time_valid().unwrap() {
                certs.push(X509::from_der(cert.to_der()).map_err(new_error)?);
            }
        }
        Ok(certs)
    }
}
#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
pub use fallback_platform::load_native_certs;
