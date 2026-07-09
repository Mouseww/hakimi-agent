//! URL safety checks for tool-managed HTTP fetches.

use hakimi_common::{HakimiError, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

const BLOCKED_HOSTNAMES: &[&str] = &["metadata.google.internal", "metadata.goog"];

const ALWAYS_BLOCKED_IPV4: &[Ipv4Addr] = &[
    Ipv4Addr::new(169, 254, 169, 254),
    Ipv4Addr::new(169, 254, 170, 2),
    Ipv4Addr::new(169, 254, 169, 253),
    Ipv4Addr::new(100, 100, 100, 200),
];

/// Validate an HTTP(S) URL before a tool fetches it.
pub(crate) fn assert_safe_http_url(raw_url: &str) -> Result<()> {
    assert_safe_http_url_with_private_override(raw_url, allow_private_urls())
}

fn assert_safe_http_url_with_private_override(raw_url: &str, allow_private: bool) -> Result<()> {
    let url = reqwest::Url::parse(raw_url)
        .map_err(|err| HakimiError::ToolSimple(format!("Invalid URL '{raw_url}': {err}")))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(HakimiError::ToolSimple(
            "URL must start with http:// or https://".into(),
        ));
    }

    let host = url
        .host_str()
        .ok_or_else(|| HakimiError::ToolSimple(format!("URL '{raw_url}' has no hostname")))?;
    assert_safe_host(
        host,
        url.port_or_known_default().unwrap_or(443),
        allow_private,
    )
}

pub(crate) fn safe_http_redirect_policy(limit: usize) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= limit {
            return attempt.stop();
        }
        if assert_safe_http_url(attempt.url().as_str()).is_err() {
            return attempt.stop();
        }
        attempt.follow()
    })
}

fn assert_safe_host(host: &str, port: u16, allow_private: bool) -> Result<()> {
    let mut normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.starts_with('[') && normalized.ends_with(']') {
        normalized = normalized[1..normalized.len() - 1].to_string();
    }
    if normalized.is_empty() {
        return Err(HakimiError::ToolSimple("URL hostname cannot be empty".into()));
    }
    if BLOCKED_HOSTNAMES.contains(&normalized.as_str()) {
        return Err(HakimiError::ToolSimple(format!(
            "Blocked internal metadata hostname: {host}"
        )));
    }

    if let Ok(ip) = normalized.parse::<IpAddr>() {
        return assert_safe_ip(ip, host, allow_private);
    }

    if normalized == "localhost" || normalized.ends_with(".localhost") {
        return private_url_error(host);
    }

    let addrs = (normalized.as_str(), port)
        .to_socket_addrs()
        .map_err(|err| {
            HakimiError::ToolSimple(format!(
                "Could not resolve URL hostname '{host}' for safety check: {err}"
            ))
        })?;

    for addr in addrs {
        assert_safe_ip(addr.ip(), host, allow_private)?;
    }

    Ok(())
}

fn assert_safe_ip(ip: IpAddr, host: &str, allow_private: bool) -> Result<()> {
    match ip {
        IpAddr::V4(ipv4) => assert_safe_ipv4(ipv4, host, allow_private),
        IpAddr::V6(ipv6) => {
            if let Some(mapped) = ipv4_mapped(ipv6) {
                assert_safe_ipv4(mapped, host, allow_private)
            } else if is_always_blocked_ipv6(ipv6) {
                Err(HakimiError::ToolSimple(format!(
                    "Blocked cloud metadata address: {host}"
                )))
            } else if allow_private {
                Ok(())
            } else if is_blocked_ipv6(ipv6) {
                private_url_error(host)
            } else {
                Ok(())
            }
        }
    }
}

fn assert_safe_ipv4(ip: Ipv4Addr, host: &str, allow_private: bool) -> Result<()> {
    if is_always_blocked_ipv4(ip) {
        return Err(HakimiError::ToolSimple(format!(
            "Blocked cloud metadata address: {host}"
        )));
    }
    if allow_private {
        return Ok(());
    }
    if is_blocked_ipv4(ip) {
        return private_url_error(host);
    }
    Ok(())
}

fn is_always_blocked_ipv4(ip: Ipv4Addr) -> bool {
    ALWAYS_BLOCKED_IPV4.contains(&ip) || ip.octets()[0..2] == [169, 254]
}

fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_unspecified()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && (18..=19).contains(&b))
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 240
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.segments()[0] == 0xfe80
        || (ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8)
}

fn is_always_blocked_ipv6(ip: Ipv6Addr) -> bool {
    ip == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254)
}

fn ipv4_mapped(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ip.segments();
    if segments[..5] == [0, 0, 0, 0, 0] && segments[5] == 0xffff {
        let [a, b] = segments[6].to_be_bytes();
        let [c, d] = segments[7].to_be_bytes();
        Some(Ipv4Addr::new(a, b, c, d))
    } else {
        None
    }
}

fn allow_private_urls() -> bool {
    ["HAKIMI_ALLOW_PRIVATE_URLS", "HERMES_ALLOW_PRIVATE_URLS"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
}

fn private_url_error(host: &str) -> Result<()> {
    Err(HakimiError::ToolSimple(format!(
        "Blocked private/internal URL target: {host}. Set HAKIMI_ALLOW_PRIVATE_URLS=true only for trusted local deployments."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_metadata_ip_literals() {
        assert!(assert_safe_http_url("http://169.254.169.254/latest/meta-data").is_err());
        assert!(assert_safe_http_url("http://100.100.100.200/latest/meta-data").is_err());
    }

    #[test]
    fn blocks_private_loopback_and_cgnat_literals() {
        assert!(assert_safe_http_url("http://127.0.0.1:8080").is_err());
        assert!(assert_safe_http_url("http://10.0.0.1/status").is_err());
        assert!(assert_safe_http_url("http://172.16.0.1/status").is_err());
        assert!(assert_safe_http_url("http://192.168.1.1/status").is_err());
        assert!(assert_safe_http_url("http://100.64.0.1/status").is_err());
        assert!(assert_safe_http_url("http://198.18.0.1/status").is_err());
    }

    #[test]
    fn blocks_localhost_hostnames() {
        assert!(assert_safe_http_url("https://localhost/admin").is_err());
        assert!(assert_safe_http_url("https://service.localhost/admin").is_err());
        assert!(assert_safe_http_url("https://metadata.google.internal/").is_err());
    }

    #[test]
    fn allows_public_ip_literals() {
        assert!(assert_safe_http_url("https://93.184.216.34/index.html").is_ok());
        assert!(assert_safe_http_url("https://[2606:2800:220:1:248:1893:25c8:1946]/").is_ok());
    }

    #[test]
    fn blocks_ipv4_mapped_metadata_literals() {
        assert!(
            assert_safe_http_url_with_private_override(
                "http://[::ffff:169.254.169.254]/latest/meta-data",
                true,
            )
            .is_err()
        );
        assert!(
            assert_safe_http_url_with_private_override(
                "http://[::ffff:100.100.100.200]/latest/meta-data",
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(assert_safe_http_url("file:///etc/passwd").is_err());
        assert!(assert_safe_http_url("ftp://example.com/file").is_err());
    }

    #[test]
    fn private_allow_override_does_not_allow_metadata_floor() {
        assert!(assert_safe_http_url_with_private_override("http://10.0.0.1/status", true).is_ok());
        assert!(
            assert_safe_http_url_with_private_override("http://[fd00::1]/status", true).is_ok()
        );
        assert!(
            assert_safe_http_url_with_private_override(
                "http://169.254.169.254/latest/meta-data",
                true
            )
            .is_err()
        );
        assert!(
            assert_safe_http_url_with_private_override(
                "http://[fd00:ec2::254]/latest/meta-data",
                true
            )
            .is_err()
        );
    }
}
