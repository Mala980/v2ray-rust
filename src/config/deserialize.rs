use crate::common::new_error;
use crate::proxy::shadowsocks::aead_helper::CipherKind;
use crate::proxy::{Address, AddressError};
use bytes::Buf;
use http::uri::PathAndQuery;
use http::Method;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use std::env;
use std::io::Cursor;
use std::path::PathBuf;
use tokio_tungstenite::tungstenite::http::Uri;
use uuid::Uuid;

pub(super) fn from_str_to_cipher_kind<'de, D>(deserializer: D) -> Result<CipherKind, D::Error>
where
    D: Deserializer<'de>,
{
    let method: &str = Deserialize::deserialize(deserializer)?;
    let method = match method {
        "none" | "plain" => CipherKind::None,
        "aes-128-gcm" => CipherKind::Aes128Gcm,
        "aes-256-gcm" => CipherKind::Aes256Gcm,
        "chacha20-ietf-poly1305" | "chacha20-poly1305" => CipherKind::ChaCha20Poly1305,
        _ => return Err(Error::custom("wrong ss encryption method")),
    };
    Ok(method)
}
pub(super) fn from_str_to_address<'de, D>(deserializer: D) -> Result<Address, D::Error>
where
    D: Deserializer<'de>,
{
    let addr: &str = Deserialize::deserialize(deserializer)?;
    addr.parse()
        .map_err(|e: AddressError| Error::custom(e.as_str()))
}
pub(super) fn from_str_to_option_address<'de, D>(
    deserializer: D,
) -> Result<Option<Address>, D::Error>
where
    D: Deserializer<'de>,
{
    from_str_to_address(deserializer).map(Some)
}

pub(super) fn from_str_to_security_num<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let security_num: u8;
    let security: &str = Deserialize::deserialize(deserializer)?;
    if security == "aes-128-gcm" {
        security_num = 0x03;
    } else if security == "chacha20-poly1305" || security == "chacha20-ietf-poly1305" {
        security_num = 0x04;
    } else if security == "none" || security == "zero" {
        // Go's v2ray-core supports none/zero as no encryption (0x05)
        security_num = 0x05;
    } else if security == "auto" {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            security_num = 0x03;
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            security_num = 0x04;
        }
    } else {
        let msg = format!("unknown vmess security type {}", security);
        return Err(Error::custom(msg.as_str()));
    };
    Ok(security_num)
}

pub(super) fn from_str_to_uuid<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
where
    D: Deserializer<'de>,
{
    let uuid_str: &str = Deserialize::deserialize(deserializer)?;
    Uuid::parse_str(uuid_str).map_err(Error::custom)
}

#[derive(Clone)]
pub struct EarlyDataUri {
    pub(super) uri: Uri,
    pub(super) early_data_header_name: String,
    pub(super) max_early_data: usize,
}

impl EarlyDataUri {
    fn new(uri: &str) -> std::io::Result<EarlyDataUri> {
        let ws_uri: Uri = uri.parse().map_err(new_error)?;
        if let Some(query) = ws_uri.query() {
            // Parse query string into key-value pairs, extract "ed" param
            let mut remaining_parts: Vec<String> = Vec::new();
            let mut max_early_data: Option<usize> = None;

            for part in query.split('&') {
                if part.is_empty() {
                    continue;
                }
                let mut kv = part.splitn(2, '=');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                if key == "ed" && max_early_data.is_none() {
                    // Parse ed value
                    let ed_val: usize = value.parse().map_err(new_error)?;
                    if ed_val == 0 {
                        return Err(new_error("read from query max early data is zero."));
                    }
                    max_early_data = Some(ed_val);
                    // Do not add this part to remaining_parts (remove ed from URI)
                } else {
                    remaining_parts.push(part.to_string());
                }
            }

            if let Some(ed) = max_early_data {
                // Rebuild URI without ed param, matching Go's behavior
                let path = ws_uri.path();
                let new_path_and_query = if remaining_parts.is_empty() {
                    path.to_string()
                } else {
                    format!("{}?{}", path, remaining_parts.join("&"))
                };

                let parts = ws_uri.into_parts();
                let mut new_uri_builder = Uri::builder();
                if let Some(s) = parts.scheme {
                    new_uri_builder = new_uri_builder.scheme(s);
                }
                if let Some(a) = parts.authority {
                    new_uri_builder = new_uri_builder.authority(a);
                }
                let uri = new_uri_builder
                    .path_and_query(new_path_and_query.as_str())
                    .build()
                    .map_err(new_error)?;

                return Ok(EarlyDataUri {
                    uri,
                    early_data_header_name: "Sec-WebSocket-Protocol".to_string(),
                    max_early_data: ed,
                });
            }
        }
        Ok(EarlyDataUri {
            uri: ws_uri,
            early_data_header_name: String::new(),
            max_early_data: 0,
        })
    }
}

pub(super) fn from_str_to_path<'de, D>(deserializer: D) -> Result<PathAndQuery, D::Error>
where
    D: Deserializer<'de>,
{
    let path_and_query: &str = Deserialize::deserialize(deserializer)?;
    path_and_query.try_into().map_err(Error::custom)
}

pub(super) fn from_str_to_grpc_path<'de, D>(deserializer: D) -> Result<PathAndQuery, D::Error>
where
    D: Deserializer<'de>,
{
    let service_name: &str = Deserialize::deserialize(deserializer)?;
    let path_and_query = format!("/{}/Tun", service_name);
    path_and_query.try_into().map_err(Error::custom)
}

pub(super) fn from_str_to_http_method<'de, D>(deserializer: D) -> Result<Method, D::Error>
where
    D: Deserializer<'de>,
{
    let method: &str = Deserialize::deserialize(deserializer)?;
    method.try_into().map_err(Error::custom)
}

pub(super) fn from_str_to_ws_uri<'de, D>(deserializer: D) -> Result<EarlyDataUri, D::Error>
where
    D: Deserializer<'de>,
{
    let uri: &str = Deserialize::deserialize(deserializer)?;
    EarlyDataUri::new(uri).map_err(Error::custom)
}

// adapted from webpki::DnsNameRef
fn is_valid_dns_id(hostname: &[u8]) -> bool {
    // https://blogs.msdn.microsoft.com/oldnewthing/20120412-00/?p=7873/
    if hostname.len() > 253 {
        return false;
    }

    let mut input = Cursor::new(hostname);

    let mut label_length = 0;
    let mut label_is_all_numeric = false;
    let mut label_ends_with_hyphen = false;

    loop {
        const MAX_LABEL_LENGTH: usize = 63;
        let by = if input.has_remaining() {
            Ok(input.get_u8())
        } else {
            Err(new_error("eof of hostname"))
        };

        match by {
            Ok(b'-') => {
                if label_length == 0 {
                    return false; // Labels must not start with a hyphen.
                }
                label_is_all_numeric = false;
                label_ends_with_hyphen = true;
                label_length += 1;
                if label_length > MAX_LABEL_LENGTH {
                    return false;
                }
            }

            Ok(b'0'..=b'9') => {
                if label_length == 0 {
                    label_is_all_numeric = true;
                }
                label_ends_with_hyphen = false;
                label_length += 1;
                if label_length > MAX_LABEL_LENGTH {
                    return false;
                }
            }

            Ok(b'a'..=b'z') | Ok(b'A'..=b'Z') | Ok(b'_') => {
                label_is_all_numeric = false;
                label_ends_with_hyphen = false;
                label_length += 1;
                if label_length > MAX_LABEL_LENGTH {
                    return false;
                }
            }

            Ok(b'.') => {
                if label_length == 0 {
                    return false;
                }
                if label_ends_with_hyphen {
                    return false; // Labels must not end with a hyphen.
                }
                label_length = 0;
            }

            _ => {
                return false;
            }
        }

        if !input.has_remaining() {
            break;
        }
    }

    if label_ends_with_hyphen {
        return false; // Labels must not end with a hyphen.
    }

    if label_is_all_numeric {
        return false; // Last label must not be all numeric.
    }

    true
}

pub(super) fn from_str_to_sni<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let sni: &str = Deserialize::deserialize(deserializer)?;
    if is_valid_dns_id(sni.as_bytes()) {
        Ok(sni.to_owned())
    } else {
        Err(Error::custom("Not a valid sni string"))
    }
}
pub(super) fn default_grpc_path() -> PathAndQuery {
    "/GunService/Tun".try_into().unwrap()
}
pub(super) fn default_relay_buffer_size() -> usize {
    20
}
pub(super) fn default_backlog() -> u32 {
    4096
}
pub(super) fn default_true() -> bool {
    true
}
pub(super) fn default_random_string() -> String {
    let id = Uuid::new_v4();
    id.to_string()
}

#[inline]
pub(super) fn default_http2_method() -> Method {
    Method::PUT
}

#[inline]
pub(super) fn default_quic_security() -> String {
    "none".to_string()
}

#[inline]
pub(super) fn default_quic_header() -> String {
    "none".to_string()
}

#[inline]
pub(super) fn default_kcp_mtu() -> u32 {
    1350
}

#[inline]
pub(super) fn default_kcp_tti() -> u32 {
    20
}

#[inline]
pub(super) fn default_kcp_uplink() -> u32 {
    5
}

#[inline]
pub(super) fn default_kcp_downlink() -> u32 {
    20
}

#[inline]
pub(super) fn default_kcp_read_buffer() -> u32 {
    2
}

#[inline]
pub(super) fn default_kcp_write_buffer() -> u32 {
    2
}

#[inline]
pub(super) fn default_kcp_header() -> String {
    "none".to_string()
}

fn default_v2ray_asset_path(file_name: &str) -> PathBuf {
    let mut prefix = env::var("v2ray.location.asset")
        .or_else(|_| env::var("V2RAY_LOCATION_ASSET"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut path_buf = env::current_exe().unwrap_or_default();
            path_buf.pop();
            path_buf
        });
    prefix.push(file_name);
    log::trace!("v2ray asset location: {}", prefix.display());
    prefix
}

pub(super) fn default_v2ray_geosite_path() -> PathBuf {
    default_v2ray_asset_path("geosite.dat")
}

pub(super) fn default_v2ray_geoip_path() -> PathBuf {
    default_v2ray_asset_path("geoip.dat")
}

#[cfg(test)]
mod tests {
    use crate::config::deserialize::EarlyDataUri;

    use super::is_valid_dns_id;

    #[test]
    fn test_ws_uri() {
        let e = EarlyDataUri::new("ws://example.com/?ed=2048").unwrap();
        assert!(!e.early_data_header_name.is_empty());
        assert_eq!(e.max_early_data, 2048);
        let e = EarlyDataUri::new("ws://example.com/?key=xx&ed=20488").unwrap();
        assert!(!e.early_data_header_name.is_empty());
        assert_eq!(e.max_early_data, 20488);
        let e = EarlyDataUri::new("ws://example.com/?key=xx&ed=20488&n=q").unwrap();
        assert!(!e.early_data_header_name.is_empty());
        assert_eq!(e.max_early_data, 20488);
    }

    #[test]
    fn test_is_valid_dns_id() {
        assert!(!is_valid_dns_id(b"*.google.com"));
        assert!(!is_valid_dns_id(b".google.com"));
        assert!(is_valid_dns_id(b"google.com"));
        assert!(!is_valid_dns_id(b"google.*.com"));
        assert!(!is_valid_dns_id(b"google*.com"));
        assert!(!is_valid_dns_id(b"google*sd.com"));
        assert!(!is_valid_dns_id(b"*googlesd.com"));
    }
}
