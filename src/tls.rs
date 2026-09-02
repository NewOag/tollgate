//! Self-signed TLS certificate generation and caching for the gateway's
//! HTTPS listener. The cert is generated once per app-config directory and
//! reused across restarts (see [`load_or_generate`]) so a browser that has
//! already clicked through the self-signed warning doesn't have to re-trust
//! a new cert on every launch — only when the required hostnames change.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use rcgen::{CertificateParams, KeyPair, SanType};
use sha2::{Digest, Sha256};

use crate::config::Config;

const CERT_FILE: &str = "cert.pem";
const KEY_FILE: &str = "key.pem";

/// One entry the cert's Subject Alternative Names must cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SanEntry {
    Dns(String),
    Ip(IpAddr),
}

/// A loaded or freshly generated cert/key pair, PEM-encoded, ready to hand
/// to [`rustls_config`].
pub struct TlsCert {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// SHA-256 hex digest of the DER cert, for display/verification in the UI.
    pub fingerprint: String,
}

/// Builds the SAN list a cert must cover: always `localhost`/loopback, plus
/// `{mdns_hostname}.local` when mDNS advertising is enabled (non-empty).
pub fn build_san_list(mdns_hostname: &str) -> Vec<SanEntry> {
    let mut sans = vec![
        SanEntry::Dns("localhost".to_string()),
        SanEntry::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanEntry::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ];
    if !mdns_hostname.is_empty() {
        sans.push(SanEntry::Dns(format!("{mdns_hostname}.local")));
    }
    sans
}

/// `cfg.tls_cert_dir` if set, otherwise the same directory the config file
/// lives in (the per-app config dir the CLI and Tauri app already share).
pub fn resolve_cert_dir(cfg: &Config, config_path: &Path) -> PathBuf {
    if cfg.tls_cert_dir.is_empty() {
        config_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(&cfg.tls_cert_dir)
    }
}

/// Loads the cert/key cached in `cert_dir` if present and its SANs already
/// cover every entry in `required_sans`; otherwise generates a fresh
/// self-signed cert covering `required_sans`, writes it to `cert_dir`, and
/// returns that instead.
pub fn load_or_generate(cert_dir: &Path, required_sans: &[SanEntry]) -> anyhow::Result<TlsCert> {
    let cert_path = cert_dir.join(CERT_FILE);
    let key_path = cert_dir.join(KEY_FILE);

    if cert_path.exists() && key_path.exists() {
        let cert_pem = fs::read(&cert_path)?;
        let key_pem = fs::read(&key_path)?;
        if cert_covers_sans(&cert_pem, required_sans)? {
            let fingerprint = fingerprint_from_pem(&cert_pem)?;
            return Ok(TlsCert { cert_pem, key_pem, fingerprint });
        }
        tracing::info!("cached TLS cert doesn't cover the current hostnames; regenerating");
    }

    generate_cert(cert_dir, required_sans)
}

fn generate_cert(cert_dir: &Path, required_sans: &[SanEntry]) -> anyhow::Result<TlsCert> {
    fs::create_dir_all(cert_dir)?;

    let dns_names: Vec<String> = required_sans
        .iter()
        .filter_map(|s| match s {
            SanEntry::Dns(name) => Some(name.clone()),
            SanEntry::Ip(_) => None,
        })
        .collect();

    let mut params = CertificateParams::new(dns_names)?;
    for san in required_sans {
        if let SanEntry::Ip(ip) = san {
            params.subject_alt_names.push(SanType::IpAddress(*ip));
        }
    }
    // 10 years: long enough that a local dev cert never needs unprompted
    // renewal (which would force re-trusting it in the browser).
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2035, 1, 1);

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    fs::write(cert_dir.join(CERT_FILE), &cert_pem)?;
    fs::write(cert_dir.join(KEY_FILE), &key_pem)?;

    let fingerprint = hex_sha256(cert.der());
    tracing::info!(%fingerprint, "generated self-signed TLS cert");

    Ok(TlsCert { cert_pem: cert_pem.into_bytes(), key_pem: key_pem.into_bytes(), fingerprint })
}

/// Whether the PEM-encoded cert already carries every SAN in `required`.
/// Re-derives the cert's own SAN list by round-tripping it through rcgen's
/// parser rather than maintaining a separate manifest file.
fn cert_covers_sans(cert_pem: &[u8], required: &[SanEntry]) -> anyhow::Result<bool> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem)?;
    let x509 = pem.parse_x509()?;
    let Some(ext) = x509.subject_alternative_name()? else {
        return Ok(required.is_empty());
    };

    let mut present_dns = Vec::new();
    let mut present_ip = Vec::new();
    for name in &ext.value.general_names {
        match name {
            x509_parser::extensions::GeneralName::DNSName(s) => present_dns.push(s.to_string()),
            x509_parser::extensions::GeneralName::IPAddress(bytes) => {
                if let Some(ip) = ip_from_bytes(bytes) {
                    present_ip.push(ip);
                }
            }
            _ => {}
        }
    }

    Ok(required.iter().all(|san| match san {
        SanEntry::Dns(name) => present_dns.iter().any(|d| d == name),
        SanEntry::Ip(ip) => present_ip.contains(ip),
    }))
}

fn ip_from_bytes(bytes: &[u8]) -> Option<IpAddr> {
    match bytes.len() {
        4 => Some(IpAddr::V4(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]))),
        16 => {
            let arr: [u8; 16] = bytes.try_into().ok()?;
            Some(IpAddr::V6(Ipv6Addr::from(arr)))
        }
        _ => None,
    }
}

fn fingerprint_from_pem(cert_pem: &[u8]) -> anyhow::Result<String> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem)?;
    Ok(hex_sha256(&pem.contents))
}

fn hex_sha256(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

/// Builds a rustls server config from PEM-encoded cert/key bytes, ready to
/// pass to `axum_server::from_tcp_rustls`.
pub async fn rustls_config(cert: &TlsCert) -> anyhow::Result<axum_server::tls_rustls::RustlsConfig> {
    Ok(axum_server::tls_rustls::RustlsConfig::from_pem(cert.cert_pem.clone(), cert.key_pem.clone()).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tollgate-tls-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn build_san_list_without_mdns() {
        let sans = build_san_list("");
        assert_eq!(sans.len(), 3);
        assert!(sans.contains(&SanEntry::Dns("localhost".to_string())));
        assert!(sans.contains(&SanEntry::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST))));
        assert!(sans.contains(&SanEntry::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST))));
    }

    #[test]
    fn build_san_list_with_mdns() {
        let sans = build_san_list("tollgate");
        assert!(sans.contains(&SanEntry::Dns("tollgate.local".to_string())));
        assert_eq!(sans.len(), 4);
    }

    #[test]
    fn load_or_generate_creates_then_reuses() {
        let dir = temp_dir("create-reuse");
        let sans = build_san_list("tollgate");

        let first = load_or_generate(&dir, &sans).unwrap();
        let second = load_or_generate(&dir, &sans).unwrap();

        assert_eq!(first.cert_pem, second.cert_pem);
        assert_eq!(first.fingerprint, second.fingerprint);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_or_generate_regenerates_on_new_hostname() {
        let dir = temp_dir("regenerate");
        let first = load_or_generate(&dir, &build_san_list("tollgate")).unwrap();
        let second = load_or_generate(&dir, &build_san_list("other-name")).unwrap();

        assert_ne!(first.cert_pem, second.cert_pem);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_cert_dir_defaults_next_to_config() {
        let cfg = Config::default();
        let config_path = Path::new("/some/dir/config.yaml");
        assert_eq!(resolve_cert_dir(&cfg, config_path), PathBuf::from("/some/dir"));
    }

    #[test]
    fn resolve_cert_dir_honors_override() {
        let cfg = Config { tls_cert_dir: "/custom/tls".to_string(), ..Default::default() };
        let config_path = Path::new("/some/dir/config.yaml");
        assert_eq!(resolve_cert_dir(&cfg, config_path), PathBuf::from("/custom/tls"));
    }
}
