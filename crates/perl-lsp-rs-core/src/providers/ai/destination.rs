//! Destination validation and credential binding for AI completion HTTP requests.
//!
//! Validates configured endpoints before outbound requests and ensures credentials
//! cannot be rebound to a different host/scheme/port via redirects or URL tampering.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
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
    /// Addresses returned by DNS at validation time (IPv4-mapped IPv6 normalized).
    pub resolved_ips: Vec<IpAddr>,
}

/// Errors raised when an endpoint fails destination policy.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DestinationError {
    /// URL failed to parse.
    #[error("invalid endpoint URL: {0}")]
    InvalidUrl(String),
    /// Scheme is not `http` or `https`.
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),
    /// Parsed URL has no host component.
    #[error("endpoint host is missing")]
    MissingHost,
    /// Non-loopback destination used plain HTTP.
    #[error("remote AI endpoints must use HTTPS")]
    HttpsRequired,
    /// Loopback HTTP without `local_model_mode`.
    #[error("plain HTTP to local models requires local_model_mode")]
    LocalHttpDisabled,
    /// DNS returned no addresses.
    #[error("hostname did not resolve to any address")]
    UnresolvedHost,
    /// Resolved address is private, link-local, CGNAT, metadata, or transition.
    #[error("destination resolves to a disallowed address")]
    DisallowedAddress,
    /// `localhost` resolved to a non-loopback address.
    #[error("localhost must resolve to loopback addresses only")]
    LocalhostNotLoopback,
}

impl perl_parser_core::ErrorClass for DestinationError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        match self {
            // URL parse failures and missing components are user config errors.
            Self::InvalidUrl(_)
            | Self::UnsupportedScheme(_)
            | Self::MissingHost
            | Self::HttpsRequired
            | Self::LocalHttpDisabled => perl_parser_core::ErrorCategory::UserError,
            // DNS resolution is an external dependency issue.
            Self::UnresolvedHost => perl_parser_core::ErrorCategory::Infra,
            // Security policy rejection — user must correct configuration.
            Self::DisallowedAddress | Self::LocalhostNotLoopback => {
                perl_parser_core::ErrorCategory::UserError
            }
        }
    }
}

type ResolveFn = dyn Fn(&str, u16) -> Result<Vec<IpAddr>, DestinationError>;

/// Validate `url` and resolve its host before any outbound AI HTTP request.
pub fn validate_endpoint(
    url: &str,
    allow_local_http: bool,
) -> Result<ApprovedDestination, DestinationError> {
    validate_endpoint_with_resolver(url, allow_local_http, &default_resolver)
}

/// Like [`validate_endpoint`], but with an injectable DNS resolver (tests).
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

    let host_raw = parsed.host_str().ok_or(DestinationError::MissingHost)?;
    let host = normalize_host(host_raw);

    let port = parsed.port().unwrap_or_else(|| default_port(&scheme));
    let resolved_ips: Vec<IpAddr> = resolve(&host, port)?.into_iter().map(normalize_ip).collect();

    if resolved_ips.is_empty() {
        return Err(DestinationError::UnresolvedHost);
    }

    let all_loopback = resolved_ips.iter().copied().all(|ip| ip.is_loopback());
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

/// Validate a raw `host:port` for a non-HTTP outbound connection (DAP attach).
///
/// Unlike [`validate_endpoint`], this does not enforce HTTPS because DAP
/// attach is a plain TCP connection to a user-specified debugger endpoint.
/// However, it applies the **same** SSRF protections: the host is resolved
/// via DNS and every resolved IP is checked against [`is_disallowed_address`].
/// This prevents a hostile `.vscode/launch.json` from opening a TCP connection
/// to cloud metadata (`169.254.169.254`) or other private/link-local
/// addresses (#5257).
///
/// Loopback addresses are allowed (the debugger typically runs locally).
pub fn validate_tcp_attach_host(host: &str, port: u16) -> Result<Vec<IpAddr>, DestinationError> {
    validate_tcp_attach_host_with_resolver(host, port, &default_resolver)
}

/// Like [`validate_tcp_attach_host`], but with an injectable DNS resolver
/// (tests).
pub fn validate_tcp_attach_host_with_resolver(
    host: &str,
    port: u16,
    resolve: &ResolveFn,
) -> Result<Vec<IpAddr>, DestinationError> {
    let normalized = normalize_host(host);
    if normalized.is_empty() {
        return Err(DestinationError::MissingHost);
    }
    let resolved_ips: Vec<IpAddr> =
        resolve(&normalized, port)?.into_iter().map(normalize_ip).collect();
    if resolved_ips.is_empty() {
        return Err(DestinationError::UnresolvedHost);
    }
    // localhost must resolve to loopback only (same contract as the AI path).
    if normalized == "localhost" && !resolved_ips.iter().copied().all(|ip| ip.is_loopback()) {
        return Err(DestinationError::LocalhostNotLoopback);
    }
    // Reject any resolved address in the private/link-local/metadata/CGNAT
    // ranges, EXCEPT loopback (debugger may run locally).
    if resolved_ips.iter().copied().any(is_disallowed_address) {
        return Err(DestinationError::DisallowedAddress);
    }
    Ok(resolved_ips)
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

/// Convert IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) to their IPv4 equivalents.
pub(crate) fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

/// Canonicalize host for policy identity: lowercase + strip IPv6 brackets.
fn normalize_host(host: &str) -> String {
    let lower = host.to_ascii_lowercase();
    strip_ipv6_brackets(&lower).to_string()
}

fn strip_ipv6_brackets(host: &str) -> &str {
    host.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')).unwrap_or(host)
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
    let host_raw = parsed.host_str().ok_or(DestinationError::MissingHost)?;
    let host = normalize_host(host_raw);
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
    let host = strip_ipv6_brackets(host);
    if host.is_empty() {
        return Err(DestinationError::UnresolvedHost);
    }
    // Formatting bare `::1:port` is invalid for to_socket_addrs — parse literals first.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![normalize_ip(ip)]);
    }
    let authority = format!("{host}:{port}");
    let addrs: Vec<IpAddr> = authority
        .to_socket_addrs()
        .map_err(|_e| DestinationError::UnresolvedHost)?
        .map(|addr| normalize_ip(addr.ip()))
        .collect();
    if addrs.is_empty() {
        return Err(DestinationError::UnresolvedHost);
    }
    Ok(addrs)
}

fn is_disallowed_address(ip: IpAddr) -> bool {
    let ip = normalize_ip(ip);
    if ip.is_loopback() {
        return false;
    }

    match ip {
        IpAddr::V4(v4) => is_disallowed_ipv4(v4),
        IpAddr::V6(v6) => {
            if v6.is_multicast()
                || v6.is_unspecified()
                || is_ipv6_unique_local(v6)
                || is_ipv6_link_local(v6)
                || is_ipv6_site_local(v6)
                || is_ipv6_6to4(v6)
                || is_ipv6_nat64(v6)
                || is_ipv6_ipv4_compatible(v6)
            {
                return true;
            }

            // Defense in depth: if any other embedding form appears, evaluate the
            // embedded IPv4 against the same private/link-local/CGNAT policy.
            if let Some(embedded) = embedded_ipv4_from_v6(v6) {
                return is_disallowed_ipv4(embedded);
            }

            false
        }
    }
}

fn is_disallowed_ipv4(v4: Ipv4Addr) -> bool {
    v4.is_private()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || is_cgnat(v4)
        || v4.octets() == [169, 254, 169, 254]
}

/// RFC 6598 Carrier-Grade NAT shared address space `100.64.0.0/10`.
fn is_cgnat(v4: Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 100 && (octets[1] & 0xc0) == 64
}

fn is_ipv6_unique_local(ip: Ipv6Addr) -> bool {
    (ip.octets()[0] & 0xfe) == 0xfc
}

fn is_ipv6_link_local(ip: Ipv6Addr) -> bool {
    ip.octets()[0] == 0xfe && (ip.octets()[1] & 0xc0) == 0x80
}

/// Deprecated site-local `fec0::/10` (RFC 3879).
fn is_ipv6_site_local(ip: Ipv6Addr) -> bool {
    ip.octets()[0] == 0xfe && (ip.octets()[1] & 0xc0) == 0xc0
}

/// 6to4 transition prefix `2002::/16` (RFC 3056).
fn is_ipv6_6to4(ip: Ipv6Addr) -> bool {
    let o = ip.octets();
    o[0] == 0x20 && o[1] == 0x02
}

/// NAT64 well-known prefix `64:ff9b::/96` (RFC 6052).
fn is_ipv6_nat64(ip: Ipv6Addr) -> bool {
    let o = ip.octets();
    o[0] == 0x00
        && o[1] == 0x64
        && o[2] == 0xff
        && o[3] == 0x9b
        && o[4] == 0
        && o[5] == 0
        && o[6] == 0
        && o[7] == 0
        && o[8] == 0
        && o[9] == 0
        && o[10] == 0
        && o[11] == 0
}

/// Deprecated IPv4-compatible IPv6 (`::a.b.c.d`), excluding `::` and `::1`.
fn is_ipv6_ipv4_compatible(ip: Ipv6Addr) -> bool {
    if ip.to_ipv4_mapped().is_some() {
        return false;
    }
    match ip.to_ipv4() {
        Some(v4) => !v4.is_unspecified() && !v4.is_loopback(),
        None => false,
    }
}

/// Extract an embedded IPv4 from known transition encodings (defense in depth).
fn embedded_ipv4_from_v6(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    let o = v6.octets();

    // 6to4: 2002:V4ADDR::/48 — IPv4 lives in bytes 2..6.
    if is_ipv6_6to4(v6) {
        return Some(Ipv4Addr::new(o[2], o[3], o[4], o[5]));
    }

    // NAT64 well-known prefix — IPv4 lives in the last 4 bytes.
    if is_ipv6_nat64(v6) {
        return Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
    }

    if is_ipv6_ipv4_compatible(v6) {
        return v6.to_ipv4();
    }

    None
}

#[cfg(test)]
mod unit_tests {
    use super::{
        ApprovedDestination, DestinationError, credential_may_attach, default_resolver,
        embedded_ipv4_from_v6, is_disallowed_address, normalize_ip,
        validate_endpoint_with_resolver, validate_tcp_attach_host_with_resolver,
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

    #[test]
    fn default_resolver_accepts_ipv6_literal_without_brackets() {
        let addrs = default_resolver("::1", 11434).expect("ipv6 literal must resolve");
        assert_eq!(addrs, vec![IpAddr::V6(Ipv6Addr::LOCALHOST)]);
    }

    #[test]
    fn normalizes_ipv4_mapped_ipv6() {
        let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001));
        assert_eq!(normalize_ip(mapped), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn rejects_ipv4_mapped_private_via_normalization() {
        let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0a00, 0x0005));
        let err = validate_endpoint_with_resolver(
            "https://evil.example/v1",
            false,
            &resolver_with(vec![mapped]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn rejects_cgnat_shared_address_space() {
        let err = validate_endpoint_with_resolver(
            "https://cgnat.example/v1",
            false,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(100, 64, 1, 2))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn rejects_6to4_embedding_private_ipv4() {
        // 2002:0a00:0001:: embeds 10.0.0.1
        let sixto4 = IpAddr::V6(Ipv6Addr::new(0x2002, 0x0a00, 0x0001, 0, 0, 0, 0, 0));
        assert!(is_disallowed_address(sixto4));
        let err = validate_endpoint_with_resolver(
            "https://sixto4.example/v1",
            false,
            &resolver_with(vec![sixto4]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn rejects_nat64_embedding_private_ipv4() -> Result<(), Box<dyn std::error::Error>> {
        // 64:ff9b::0a00:0001 embeds 10.0.0.1
        let nat64 = IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0x0a00, 0x0001));
        let IpAddr::V6(nat64_v6) = nat64 else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "test fixture is not v6",
            )
            .into());
        };
        assert_eq!(embedded_ipv4_from_v6(nat64_v6), Some(Ipv4Addr::new(10, 0, 0, 1)));
        let err = validate_endpoint_with_resolver(
            "https://nat64.example/v1",
            false,
            &resolver_with(vec![nat64]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
        Ok(())
    }

    #[test]
    fn rejects_ipv4_compatible_embedding_private() {
        // ::10.0.0.1
        let compatible = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x0a00, 0x0001));
        assert!(is_disallowed_address(compatible));
    }

    #[test]
    fn rejects_site_local_ipv6() {
        let site_local = IpAddr::V6(Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 1));
        assert!(is_disallowed_address(site_local));
    }

    #[test]
    fn rejects_6to4_even_when_embedded_ipv4_is_public() {
        // Entire 2002::/16 is disallowed — transition tunneling is an SSRF bypass class.
        let sixto4 = IpAddr::V6(Ipv6Addr::new(0x2002, 0x5db8, 0xd822, 0, 0, 0, 0, 1));
        assert!(is_disallowed_address(sixto4));
    }

    // ── validate_tcp_attach_host (DAP SSRF defense, #5257) ──────────────────

    #[test]
    fn tcp_attach_accepts_loopback() {
        let ips = validate_tcp_attach_host_with_resolver(
            "127.0.0.1",
            13603,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
        )
        .expect("loopback must be allowed for DAP attach");
        assert_eq!(ips, vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    }

    #[test]
    fn tcp_attach_accepts_localhost_resolving_to_loopback() {
        validate_tcp_attach_host_with_resolver(
            "localhost",
            13603,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
        )
        .expect("localhost → loopback must be accepted");
    }

    #[test]
    fn tcp_attach_rejects_cloud_metadata_ip() {
        // 169.254.169.254 is the AWS/GCP/Azure cloud metadata endpoint — the
        // canonical SSRF target. Must be rejected (#5257).
        let err = validate_tcp_attach_host_with_resolver(
            "169.254.169.254",
            80,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn tcp_attach_rejects_private_ip() {
        let err = validate_tcp_attach_host_with_resolver(
            "10.0.0.1",
            80,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::DisallowedAddress);
    }

    #[test]
    fn tcp_attach_rejects_localhost_resolving_to_non_loopback() {
        let err = validate_tcp_attach_host_with_resolver(
            "localhost",
            80,
            &resolver_with(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]),
        )
        .unwrap_err();
        assert_eq!(err, DestinationError::LocalhostNotLoopback);
    }

    #[test]
    fn tcp_attach_rejects_empty_host() {
        let err =
            validate_tcp_attach_host_with_resolver("", 80, &resolver_with(vec![])).unwrap_err();
        assert_eq!(err, DestinationError::MissingHost);
    }
}
