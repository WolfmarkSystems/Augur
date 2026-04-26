//! IP geolocation via MaxMind GeoLite2.
//!
//! Sprint 7 P1. Network artifacts pulled from evidence (URLs, log
//! lines, packet captures) routinely contain IP addresses; the
//! examiner needs to know where each address geolocates. MaxMind's
//! GeoLite2 City database is the de-facto standard offline GeoIP
//! source — free, accurate, no network access at runtime.
//!
//! # Offline invariant
//!
//! MaxMind's license terms bar VERIFY from auto-downloading the
//! GeoLite2 database. The examiner MUST place
//! `GeoLite2-City.mmdb` somewhere VERIFY can find it:
//! - explicit path via `VERIFY_GEOIP_PATH`
//! - or the XDG default `~/.cache/verify/GeoLite2-City.mmdb`
//!
//! When neither resolves, [`GeoIpEngine::with_xdg_cache`] returns
//! [`VerifyError::GeoIpNotConfigured`] with the download
//! instructions baked into the error message — never panics,
//! never silently falls back, never auto-downloads.

use crate::error::VerifyError;
use serde::Serialize;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// Standard filename of the GeoLite2 City database — matches the
/// archive MaxMind ships, so an examiner who follows the
/// instructions verbatim ends up with the right file.
pub const GEOIP_DB_FILENAME: &str = "GeoLite2-City.mmdb";

/// Examiner-facing setup blurb. Returned inside
/// [`VerifyError::GeoIpNotConfigured`] and printed by the CLI's
/// `verify geoip --setup` subcommand.
pub const GEOIP_DB_INSTRUCTIONS: &str =
    "Download GeoLite2-City.mmdb from \
     https://dev.maxmind.com/geoip/geolite2-free-geolocation-data \
     (a free MaxMind account is required) and either set \
     VERIFY_GEOIP_PATH=/path/to/GeoLite2-City.mmdb or place it at \
     ~/.cache/verify/GeoLite2-City.mmdb.";

/// Result of a single GeoIP lookup. `None` fields signal that the
/// MaxMind record didn't include that detail (some IPs lack city
/// or coordinate data).
#[derive(Debug, Clone, Serialize)]
pub struct GeoIpResult {
    pub ip: String,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// `true` for RFC 1918 (10/8, 172.16/12, 192.168/16),
    /// loopback (127/8, ::1), link-local, multicast, and
    /// unspecified addresses. We compute this ourselves rather
    /// than trusting MaxMind, because GeoLite2 records are sparse
    /// for private ranges (often returning a wrong "is this in
    /// the EU" flag based on whatever the DB happens to contain).
    pub is_private: bool,
    pub asn: Option<u32>,
    pub org: Option<String>,
}

/// GeoIP lookup engine. Wraps a memory-mapped MaxMind DB reader.
pub struct GeoIpEngine {
    reader: maxminddb::Reader<Vec<u8>>,
    db_path: PathBuf,
}

impl std::fmt::Debug for GeoIpEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoIpEngine")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl GeoIpEngine {
    /// Load a `GeoLite2-City.mmdb` from `db_path`. The file is
    /// read fully into memory (the MaxMind reader keeps it as a
    /// `Vec<u8>` — the DB is small enough that a full read is
    /// fine; mmap-style readers are a future optimisation).
    pub fn load(db_path: &Path) -> Result<Self, VerifyError> {
        if !db_path.exists() {
            return Err(VerifyError::GeoIpNotConfigured(format!(
                "{:?} not found. {GEOIP_DB_INSTRUCTIONS}",
                db_path
            )));
        }
        let reader = maxminddb::Reader::open_readfile(db_path).map_err(|e| {
            VerifyError::GeoIp(format!(
                "failed to open MaxMind DB at {db_path:?}: {e}"
            ))
        })?;
        Ok(Self {
            reader,
            db_path: db_path.to_path_buf(),
        })
    }

    /// XDG-style construction: prefer `$VERIFY_GEOIP_PATH`,
    /// otherwise look for `~/.cache/verify/GeoLite2-City.mmdb`.
    /// Returns [`VerifyError::GeoIpNotConfigured`] (with the
    /// download instructions) when neither resolves.
    pub fn with_xdg_cache() -> Result<Self, VerifyError> {
        if let Some(path) = configured_db_path() {
            return Self::load(&path);
        }
        Err(VerifyError::GeoIpNotConfigured(
            GEOIP_DB_INSTRUCTIONS.to_string(),
        ))
    }

    /// Look up a single IP. Returns `Err(VerifyError::GeoIp)` for
    /// malformed input or DB lookup failures; private IPs return
    /// `Ok` with `is_private = true` and country / city left
    /// `None` (we do NOT try to look them up — RFC 1918 means
    /// "no public geolocation").
    pub fn lookup(&self, ip: &str) -> Result<GeoIpResult, VerifyError> {
        let parsed: IpAddr = ip
            .parse()
            .map_err(|e| VerifyError::GeoIp(format!("not a valid IP {ip:?}: {e}")))?;
        let private = is_private_addr(parsed);
        if private {
            return Ok(GeoIpResult {
                ip: ip.to_string(),
                country_code: None,
                country_name: None,
                city: None,
                latitude: None,
                longitude: None,
                is_private: true,
                asn: None,
                org: None,
            });
        }
        let lookup_result = self
            .reader
            .lookup(parsed)
            .map_err(|e| VerifyError::GeoIp(format!("MaxMind lookup({ip}): {e}")))?;
        let decoded: Option<maxminddb::geoip2::City> = lookup_result
            .decode()
            .map_err(|e| VerifyError::GeoIp(format!("MaxMind decode({ip}): {e}")))?;
        let Some(city) = decoded else {
            return Ok(GeoIpResult {
                ip: ip.to_string(),
                country_code: None,
                country_name: None,
                city: None,
                latitude: None,
                longitude: None,
                is_private: false,
                asn: None,
                org: None,
            });
        };
        let country_code: Option<String> = city
            .country
            .iso_code
            .map(|s: &str| s.to_string());
        let country_name: Option<String> =
            city.country.names.english.map(|s: &str| s.to_string());
        let city_name: Option<String> =
            city.city.names.english.map(|s: &str| s.to_string());
        let lat: Option<f64> = city.location.latitude;
        let lon: Option<f64> = city.location.longitude;

        Ok(GeoIpResult {
            ip: ip.to_string(),
            country_code,
            country_name,
            city: city_name,
            latitude: lat,
            longitude: lon,
            is_private: false,
            // ASN data lives in a separate `GeoLite2-ASN.mmdb`
            // file; we don't ship it as a hard requirement. Future
            // sprint can add an optional second reader.
            asn: None,
            org: None,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// Free-function private-IP test. Used both by `GeoIpEngine` and
/// by tests / external callers that want the answer without
/// loading the whole DB.
pub fn is_private(ip: &str) -> bool {
    match ip.parse::<IpAddr>() {
        Ok(addr) => is_private_addr(addr),
        Err(_) => false,
    }
}

fn is_private_addr(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // Carrier-grade NAT 100.64/10 — RFC 6598. Not
                // covered by std's `is_private` as of 1.83.
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 0x40)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // Unique local addresses fc00::/7 — RFC 4193.
                || (v6.octets()[0] & 0xFE) == 0xFC
                // Link-local fe80::/10 — RFC 4291.
                || (v6.octets()[0] == 0xFE && (v6.octets()[1] & 0xC0) == 0x80)
        }
    }
}

/// Resolve the configured MaxMind DB path: env-var override,
/// then XDG default. Returns `None` if neither resolves to an
/// existing file.
pub fn configured_db_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VERIFY_GEOIP_PATH") {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home)
            .join(".cache/verify")
            .join(GEOIP_DB_FILENAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Status string for `verify self-test`. `Ok(path)` on success,
/// `Err(blurb)` (the instructions) when nothing's configured.
/// Pure — never reads the env var more than once, never touches
/// the network.
pub fn check_status() -> Result<PathBuf, String> {
    configured_db_path().ok_or_else(|| GEOIP_DB_INSTRUCTIONS.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ip_detected_as_private() {
        // RFC 1918 + loopback + link-local + IPv6 ::1.
        for ip in &[
            "10.0.0.1",
            "172.16.0.5",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fc00::1",
            "fd12:3456:789a::1",
            "100.64.0.1",
        ] {
            assert!(is_private(ip), "{ip} should be private");
        }
    }

    #[test]
    fn public_ip_not_detected_as_private() {
        for ip in &[
            "8.8.8.8",
            "1.1.1.1",
            "208.67.222.222",
            "2606:4700:4700::1111",
        ] {
            assert!(!is_private(ip), "{ip} should not be private");
        }
    }

    #[test]
    fn malformed_ip_is_not_private() {
        assert!(!is_private("not-an-ip"));
        assert!(!is_private(""));
    }

    #[test]
    fn geoip_not_configured_returns_clear_error() {
        // Force both env var and home to point at non-existent
        // paths so configured_db_path returns None. Use a Mutex
        // to avoid racing other tests.
        let _g = env_lock();
        let prev_home = std::env::var("HOME").ok();
        let prev_geoip = std::env::var("VERIFY_GEOIP_PATH").ok();
        // SAFETY: serialized via env_lock(); restored at end.
        unsafe {
            std::env::set_var("HOME", "/tmp/verify-geoip-no-such-dir-xyz");
            std::env::remove_var("VERIFY_GEOIP_PATH");
        }
        let r = GeoIpEngine::with_xdg_cache();
        match &r {
            Err(VerifyError::GeoIpNotConfigured(msg)) => {
                assert!(
                    msg.contains("VERIFY_GEOIP_PATH"),
                    "instructions missing env-var hint: {msg}"
                );
                assert!(
                    msg.contains("MaxMind"),
                    "instructions missing MaxMind reference: {msg}"
                );
            }
            other => panic!("expected GeoIpNotConfigured, got {other:?}"),
        }
        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match prev_geoip {
                Some(v) => std::env::set_var("VERIFY_GEOIP_PATH", v),
                None => std::env::remove_var("VERIFY_GEOIP_PATH"),
            }
        }
    }

    #[test]
    fn geoip_result_has_all_fields() {
        let r = GeoIpResult {
            ip: "8.8.8.8".into(),
            country_code: Some("US".into()),
            country_name: Some("United States".into()),
            city: Some("Mountain View".into()),
            latitude: Some(37.386),
            longitude: Some(-122.0838),
            is_private: false,
            asn: Some(15_169),
            org: Some("Google LLC".into()),
        };
        assert_eq!(r.ip, "8.8.8.8");
        assert!(!r.is_private);
        assert!(r.country_code.is_some());
        assert!(r.latitude.is_some());
    }

    #[test]
    fn load_missing_db_returns_not_configured_error() {
        let bogus = std::path::Path::new("/nonexistent/strata/verify/GeoLite2-City.mmdb");
        match GeoIpEngine::load(bogus) {
            Err(VerifyError::GeoIpNotConfigured(_)) => {}
            other => panic!("expected GeoIpNotConfigured, got {other:?}"),
        }
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
