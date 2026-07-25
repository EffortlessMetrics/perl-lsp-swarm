//! Destination validation and credential binding for AI completion HTTP requests.
//!
//! Validates configured endpoints before outbound requests and ensures credentials
//! cannot be rebound to a different host/scheme/port via redirects or URL tampering.

use std::net::{IpAddr, Ipv6Addr, ToSocketAddrs};
use thiserror::Error;
use url::Url;

/// A validated, approved HTTP destination for AI completion requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedDestination {
    /// Normalized scheme (`http` or `https`).
    pub scheme: String,
    /// Normalized host (lowercase; punycode for IDN).
    pub host: String,
    /// Explicit or default port for the scheme.
    pub port: u16,
    /// Addresses returned by DNS at validation time.
    pub resolved_ips: Vec<IpAddr>,
}

/// Errors raised when an endpoint fails destination policy.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DestinationError {
    #[error("invalid endpoint URL: {0}")]
    InvalidUrl(String),
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),
    #[error("endpoint host is missing")]
    MissingHost,
    #[error("remote AI endpoints must use HTTPS")]
    HttpsRequired,
    #[error("plain HTTP to local models requires local_model_mode")]
    LocalHttpDisabled,
    #[error("hostname did not resolve to any address")]
    UnresolvedHost,
    #[error("destination resolves to a disallowed address")]
    DisallowedAddress,
    #[error("localhost must resolve to loopback addresses only")]
    LocalhostNotLoopback,
}

type ResolveFn = dyn Fn(&str, u16) -> Result<Vec<IpAddr>, DestinationError>;

/// Validate `url` and resolve its host before any outbound AI HTTP request.
pub fn validate_endpoint(
    url: &str,
    allow_local_http: bool,
) -> Result<ApprovedDestination, DestinationError> {
    validate_endpoint_with_resolver(url, allow_local_http, &default_resolver)
}

pub fn validate_endpoint_with_resolver(
    url: &str,
    allow_local_http: bool,
    resolve: &ResolveFn,
) -> Result<ApprovedDestination, DestinationError> {
    let parsed = Url::parse(url).map_err(|e| DestinationError::InvalidUrl(e.to_string()))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(DestinationError::UnsupportedScheme(scheme));
    }

    let host = parsed.host_str().ok_or(DestinationError::MissingHost)?.to_ascii_lowercase();

    let port = parsed.port().unwrap_or_else(|| default_port(&scheme));
    let resolved_ips = resolve(&host, port)?;

    if resolved_ips.is_empty() {
        return Err(DestinationError::UnresolvedHost);
    }

    let all_loopback = resolved_ips.iter().copied().all(IpAddr::is_loopback);
    if host == "localhost" && !all_loopback {
        return Err(DestinationError::LocalhostNotLoopback);
    }

    if resolved_ips.iter().copied().any(is_disallowed_address) {
        return Err(DestinationError::DisallowedAddress);
    }

    if all_loopback {
        if scheme == "http" && !allow_local_http {
            return Err(DestinationError::LocalHttpDisabled);
        }
    } else if scheme != "https" {
        return Err(DestinationError::HttpsRequired);
    }

    Ok(ApprovedDestination { scheme, host, port, resolved_ips })
}

/// Returns `true` only when `request_url` matches the approved destination identity.
pub fn credential_may_attach(approved: &ApprovedDestination, request_url: &str) -> bool {
    match parse_destination_identity(request_url) {
        Ok(identity) => {
            identity.scheme == approved.scheme
                && identity.host == approved.host
                && identity.port == approved.port
        }
        Err(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DestinationIdentity {
    scheme: String,
    host: String,
    port: u16,
}

fn parse_destination_identity(url: &str) -> Result<DestinationIdentity, DestinationError> {
    let parsed = Url::parse(url).map_err(|e| DestinationError::InvalidUrl(e.to_string()))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(DestinationError::UnsupportedScheme(scheme));
    }
    let host = parsed.host_str().ok_or(DestinationError::MissingHost)?.to_ascii_lowercase();
    let port = parsed.port().unwrap_or_else(|| default_port(&scheme));
    Ok(DestinationIdentity { scheme, host, port })
}

fn default_port(scheme: &str) -> u16 {
    match scheme {
        "https" => 443,
        _ => 80,
    }
}

fn default_resolver(host: &str, port: u16) -> Result<Vec<IpAddr>, DestinationError> {
    let authority = format!("{host}:{port}");
    let addrs: Vec<IpAddr> = authority
        .to_socket_addrs()
        .map_err(|e| DestinationError::UnresolvedHost)?
        .map(|addr| addr.ip())
        .collect();
    if addrs.is_empty() {
        return Err(DestinationError::UnresolvedHost);
    }
    Ok(addrs)
}

fn is_disallowed_address(ip: IpAddr) -> bool {
    if ip.is_loopback() {
        return false;
    }

    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v6) => {
            v6.is_multicast()
                || v6.is_unspecified()
                || is_ipv6_unique_local(v6)
                || is_ipv6_link_local(v6)
        }
    }
}

fn is_ipv6_unique_local(ip: Ipv6Addr) -> bool {
    (ip.octets()[0] & 0xfe) == 0xfc
}

fn is_ipv6_link_local(ip: Ipv6Addr) -> bool {
    ip.octets()[0] == 0xfe && (ip.octets()[1] & 0xc0) == 0x80
}

#[cfg(test)]
mod unit_tests {
    use super::{
        credential_may_attach, default_resolver, validate_endpoint_with_resolver,
        ApprovedDestination, DestinationError,
    };
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn resolver_with(
        ips: Vec<IpAddr>,
    ) -> impl Fn(&str, u16) -> Result<Vec<IpAddr>, DestinationError> {
        move |_host, _port| Ok(ips.clone())
    }

    #[test]
    fn rejects_remote_http() {
        let err = validate_endpoint_with_resolver(
            "http://api.example.com/v1/chat/completions",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::HttpsRequired);
    }

    #[test]
    fn accepts_loopback_https_without_local_model_mode() {
        let approved = validate_endpoint_with_resolver(
            "https://127.0.0.1:11434/v1/chat/completions",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
        )
        .expect("loopback https should be accepted");
        assert_eq!(approved.host, "127.0.0.1");
        assert_eq!(approved.port, 11434);
    }

    #[test]
    fn accepts_loopback_http_with_local_model_mode() {
        let approved = validate_endpoint_with_resolver(
            "http://127.0.0.1:11434/v1/chat/completions",
            true,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
        )
        .expect("local http loopback should be accepted");
        assert_eq!(approved.scheme, "http");
    }

    #[test]
    fn rejects_loopback_http_without_local_model_mode() {
        let err = validate_endpoint_with_resolver(
            "http://127.0.0.1:11434/v1/chat/completions",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::LocalHttpDisabled);
    }

    #[test]
    fn rejects_localhost_resolving_to_private_address() {
        let err = validate_endpoint_with_resolver(
            "http://localhost:8080/v1",
            true,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::LocalhostNotLoopback);
    }

    #[test]
    fn rejects_private_ipv4_targets() {
        let err = validate_endpoint_with_resolver(
            "https://10.0.0.5/v1",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn rejects_metadata_address() {
        let err = validate_endpoint_with_resolver(
            "https://metadata.internal/v1",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn rejects_non_loopback_ipv6_ula() {
        let err = validate_endpoint_with_resolver(
            "https://[fd12:3456:789a:1::1]/v1",
            false,
            &resolver_with(vec![IpAddr::V6(Ipv6Addr::new(
                0xfd12, 0x3456, 0x789a, 0x0001, 0, 0, 0, 1,
            ))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn accepts_bracketed_ipv6_loopback_with_explicit_port() {
        let approved = validate_endpoint_with_resolver(
            "https://[::1]:11434/v1",
            false,
            &resolver_with(vec![IpAddr::V6(Ipv6Addr::LOCALHOST)]),
        )
        .expect("ipv6 loopback should be accepted");
        assert_eq!(approved.host, "::1");
        assert_eq!(approved.port, 11434);
    }

    #[test]
    fn accepts_punycode_hostname_for_public_https() {
        let approved = validate_endpoint_with_resolver(
            "https://xn--bcher-kva.example/v1",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]),
        )
        .expect("punycode host should be accepted");
        assert_eq!(approved.host, "xn--bcher-kva.example");
    }

    #[test]
    fn credential_binding_rejects_host_change() {
        let approved = ApprovedDestination {
            scheme: "https".to_string(),
            host: "api.example.com".to_string(),
            port: 443,
            resolved_ips: vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
        };
        assert!(!credential_may_attach(&approved, "https://evil.example.com/v1/chat/completions"));
        assert!(credential_may_attach(&approved, "https://api.example.com/v1/chat/completions"));
    }

    #[test]
    fn credential_binding_rejects_scheme_or_port_change() {
        let approved = ApprovedDestination {
            scheme: "https".to_string(),
            host: "api.example.com".to_string(),
            port: 443,
            resolved_ips: vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
        };
        assert!(!credential_may_attach(&approved, "http://api.example.com/v1/chat/completions"));
        assert!(!credential_may_attach(
            &approved,
            "https://api.example.com:8443/v1/chat/completions"
        ));
    }

    #[test]
    fn default_resolver_rejects_empty_host() {
        let err = default_resolver("", 443).unwrap_err();
        assert_eq!(err, DestinationError::UnresolvedHost);
    }
}
