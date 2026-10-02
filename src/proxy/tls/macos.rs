#[cfg(feature = "boring-tls")]
mod boring_platform {
    use crate::common::new_error;
    use boring::x509::X509;
    use security_framework::trust_settings::{Domain, TrustSettings, TrustSettingsForCertificate};
    use std::collections::HashMap;
    use std::io::{self, Error, ErrorKind};
    pub fn load_native_certs() -> io::Result<Vec<X509>> {
        let mut all_certs = HashMap::new();
        for domain in &[Domain::User, Domain::Admin, Domain::System] {
            let ts = TrustSettings::new(*domain);
            let iter = ts.iter().map_err(|err| Error::new(ErrorKind::Other, err))?;
            for cert in iter {
                let der = cert.to_der();
                let trusted = ts
                    .tls_trust_settings_for_certificate(&cert)
                    .map_err(|err| Error::new(ErrorKind::Other, err))?
                    .unwrap_or(TrustSettingsForCertificate::TrustRoot);
                all_certs.entry(der).or_insert(trusted);
            }
        }
        let mut certs = Vec::new();
        for (der, trusted) in all_certs.drain() {
            use TrustSettingsForCertificate::*;
            if let TrustRoot | TrustAsRoot = trusted {
                certs.push(X509::from_der(&der).map_err(new_error)?);
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
    use security_framework::trust_settings::{Domain, TrustSettings, TrustSettingsForCertificate};
    use std::collections::HashMap;
    use std::io::{self, Error, ErrorKind};
    pub fn load_native_certs() -> io::Result<Vec<CertificateDer<'static>>> {
        let mut all_certs = HashMap::new();
        for domain in &[Domain::User, Domain::Admin, Domain::System] {
            let ts = TrustSettings::new(*domain);
            let iter = ts.iter().map_err(|err| Error::new(ErrorKind::Other, err))?;
            for cert in iter {
                let der = cert.to_der();
                let trusted = ts
                    .tls_trust_settings_for_certificate(&cert)
                    .map_err(|err| Error::new(ErrorKind::Other, err))?
                    .unwrap_or(TrustSettingsForCertificate::TrustRoot);
                all_certs.entry(der).or_insert(trusted);
            }
        }
        let mut certs = Vec::new();
        for (der, trusted) in all_certs.drain() {
            use TrustSettingsForCertificate::*;
            if let TrustRoot | TrustAsRoot = trusted {
                certs.push(CertificateDer::from(der));
            }
        }
        Ok(certs)
    }
}
#[cfg(feature = "rustls-tls")]
pub use rustls_platform::load_native_certs;

#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
mod fallback_platform {
    use rustls::pki_types::CertificateDer;
    use security_framework::trust_settings::{Domain, TrustSettings, TrustSettingsForCertificate};
    use std::collections::HashMap;
    use std::io::{self, Error, ErrorKind};
    pub fn load_native_certs() -> io::Result<Vec<CertificateDer<'static>>> {
        let mut all_certs = HashMap::new();
        for domain in &[Domain::User, Domain::Admin, Domain::System] {
            let ts = TrustSettings::new(*domain);
            let iter = ts.iter().map_err(|err| Error::new(ErrorKind::Other, err))?;
            for cert in iter {
                let der = cert.to_der();
                let trusted = ts
                    .tls_trust_settings_for_certificate(&cert)
                    .map_err(|err| Error::new(ErrorKind::Other, err))?
                    .unwrap_or(TrustSettingsForCertificate::TrustRoot);
                all_certs.entry(der).or_insert(trusted);
            }
        }
        let mut certs = Vec::new();
        for (der, trusted) in all_certs.drain() {
            use TrustSettingsForCertificate::*;
            if let TrustRoot | TrustAsRoot = trusted {
                certs.push(CertificateDer::from(der));
            }
        }
        Ok(certs)
    }
}
#[cfg(not(any(feature = "boring-tls", feature = "rustls-tls")))]
pub use fallback_platform::load_native_certs;
