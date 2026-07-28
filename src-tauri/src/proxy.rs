//! System proxy discovery for libgit2.
//!
//! libgit2 ships its own HTTP stack (Schannel TLS on Windows); unlike a browser
//! or any WinINET-based client it does NOT consult the OS proxy settings. A
//! Synced-mode client behind a system proxy (Clash/Mihomo on :7897, a corporate
//! web gateway) therefore silently times out on push/fetch, even though the
//! user's browser works fine. [`sync`](crate::sync) discovers the proxy here
//! and feeds it to libgit2 via `ProxyOptions`.
//!
//! NOT cached: every push / fetch / clone / connect re-reads the current value,
//! so a live change to the OS proxy (toggled off, port changed, a different
//! proxy app started) takes effect on the very next sync — no restart, no
//! Settings round-trip.

/// Discover a proxy URL at this instant, or `None` when none is configured.
/// Order: an explicit env var wins (power-user override), else the OS proxy.
pub fn discover_system_proxy() -> Option<String> {
    for k in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(normalize_proxy_url(v));
            }
        }
    }
    platform_system_proxy()
}

/// Windows: read the HKCU system proxy (`Internet Settings`). `None` when the
/// proxy is disabled, unset, or unreadable.
#[cfg(windows)]
fn platform_system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enable: u32 = settings.get_value("ProxyEnable").ok()?;
    if enable == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    parse_proxy_server(&server)
}

/// macOS/Linux: env vars are the only proxy source for now (Windows reads the
/// registry). macOS's System Configuration framework is a later addition.
#[cfg(not(windows))]
fn platform_system_proxy() -> Option<String> {
    None
}

/// Ensure a scheme prefix so libgit2 parses the URL (`host:port` ⇒ `http://...`).
/// `http://`, `https://`, `socks5://`, `socks5h://` are passed through unchanged.
fn normalize_proxy_url(raw: &str) -> String {
    let s = raw.trim();
    if s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("socks5://")
        || s.starts_with("socks5h://")
    {
        s.to_string()
    } else {
        format!("http://{s}")
    }
}

/// Parse the Windows `ProxyServer` registry value: either a bare `host:port` or
/// a `;`-separated list of `scheme=host:port` (`http=`, `https=`, `ftp=`,
/// `socks=`). Prefer the `https=` entry (GitHub is HTTPS), else the first
/// schemeless `host:port`, else `http=`. Returns a normalized URL.
fn parse_proxy_server(raw: &str) -> Option<String> {
    let mut generic: Option<String> = None;
    for part in raw.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower
            .strip_prefix("https=")
            .or_else(|| lower.strip_prefix("secure="))
        {
            return Some(normalize_proxy_url(rest));
        }
        if generic.is_none() {
            if let Some(rest) = lower.strip_prefix("http=") {
                generic = Some(normalize_proxy_url(rest));
            } else if !part.contains('=') {
                generic = Some(normalize_proxy_url(part));
            }
        }
    }
    generic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_http_scheme_when_missing() {
        assert_eq!(
            normalize_proxy_url("127.0.0.1:7897"),
            "http://127.0.0.1:7897"
        );
    }

    #[test]
    fn normalize_preserves_existing_scheme() {
        assert_eq!(
            normalize_proxy_url("http://1.2.3.4:8080"),
            "http://1.2.3.4:8080"
        );
        assert_eq!(
            normalize_proxy_url("socks5://1.2.3.4:1080"),
            "socks5://1.2.3.4:1080"
        );
    }

    #[test]
    fn parse_bare_host_port() {
        assert_eq!(
            parse_proxy_server("127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
    }

    #[test]
    fn parse_prefers_https_entry() {
        let raw = "http=127.0.0.1:8080;https=127.0.0.1:8443";
        assert_eq!(
            parse_proxy_server(raw).as_deref(),
            Some("http://127.0.0.1:8443")
        );
    }

    #[test]
    fn parse_falls_back_to_http_then_generic() {
        // no https= ⇒ use http=
        assert_eq!(
            parse_proxy_server("ftp=10.0.0.1:21;http=10.0.0.1:8080").as_deref(),
            Some("http://10.0.0.1:8080")
        );
        // no http=/https= ⇒ use the schemeless entry
        assert_eq!(
            parse_proxy_server("ftp=10.0.0.1:21;127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
    }

    #[test]
    fn parse_case_insensitive_scheme() {
        assert_eq!(
            parse_proxy_server("HTTPS=127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
    }

    #[test]
    fn parse_empty_returns_none() {
        assert_eq!(parse_proxy_server(""), None);
        assert_eq!(parse_proxy_server("   ;  "), None);
    }
}
