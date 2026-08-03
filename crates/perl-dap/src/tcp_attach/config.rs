use anyhow::Result;
use perl_lsp_rs_core::providers::ai::destination::{
    DestinationError, reject_ssrf_tcp_host, reject_ssrf_tcp_host_with_resolver,
};
use std::net::IpAddr;
use std::time::Duration;

/// Maximum connection timeout in milliseconds (5 minutes)
pub(crate) const MAX_TIMEOUT_MS: u32 = 300_000;

/// Default connection timeout in milliseconds
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 5000;

/// TCP attach configuration
#[derive(Debug, Clone)]
pub struct TcpAttachConfig {
    /// Hostname or IP address to connect to
    pub host: String,
    /// Port number to connect to
    pub port: u16,
    /// Connection timeout in milliseconds
    pub timeout_ms: Option<u32>,
}

impl TcpAttachConfig {
    /// Create a new TCP attach configuration
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port, timeout_ms: None }
    }

    /// Set the connection timeout
    pub fn with_timeout(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Validate the configuration.
    ///
    /// Checks basic syntax invariants and then applies the SSRF guard via
    /// [`reject_ssrf_tcp_host`]: private (RFC 1918), link-local (169.254/16),
    /// CGNAT (100.64/10), and cloud-metadata (169.254.169.254) addresses are
    /// rejected even when supplied as bare IP literals.  Loopback (`127.x`,
    /// `::1`, `localhost`) is always accepted so local debug sessions work
    /// without special configuration.
    ///
    /// For hostname targets the SSRF check resolves via DNS; if the name cannot
    /// be resolved the connection is refused.
    pub fn validate(&self) -> Result<()> {
        let host = self.check_basic_invariants()?;
        reject_ssrf_tcp_host(host, self.port)
            .map_err(|e| anyhow::anyhow!("Host rejected by SSRF policy: {e}"))?;
        Ok(())
    }

    /// Like [`validate`] but accepts an injectable DNS resolver.
    ///
    /// Intended for unit tests that need to exercise the hostname-based SSRF
    /// path without performing real DNS lookups.
    pub fn validate_with_resolver<'r>(
        &self,
        resolve: &'r (dyn Fn(&str, u16) -> Result<Vec<IpAddr>, DestinationError> + 'r),
    ) -> Result<()> {
        let host = self.check_basic_invariants()?;
        reject_ssrf_tcp_host_with_resolver(host, self.port, resolve)
            .map_err(|e| anyhow::anyhow!("Host rejected by SSRF policy: {e}"))?;
        Ok(())
    }

    /// Shared syntax-only checks: returns the trimmed host on success.
    fn check_basic_invariants(&self) -> Result<&str> {
        let host = self.host.trim_matches(' ');
        if host.is_empty() {
            anyhow::bail!("Host cannot be empty");
        }
        if host.chars().any(char::is_whitespace) {
            anyhow::bail!("Host cannot contain whitespace");
        }
        if host.chars().any(char::is_control) {
            anyhow::bail!("Host cannot contain control characters");
        }
        if self.port == 0 {
            anyhow::bail!("Port must be in range 1-65535");
        }
        if let Some(timeout) = self.timeout_ms {
            if timeout == 0 {
                anyhow::bail!("Timeout must be greater than 0 milliseconds");
            }
            if timeout > MAX_TIMEOUT_MS {
                anyhow::bail!(
                    "Timeout cannot exceed {} milliseconds (5 minutes)",
                    MAX_TIMEOUT_MS
                );
            }
        }
        Ok(host)
    }

    /// Get the connection timeout duration
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn loopback_resolver(_host: &str, _port: u16) -> Result<Vec<IpAddr>, DestinationError> {
        Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
    }

    fn private_resolver(_host: &str, _port: u16) -> Result<Vec<IpAddr>, DestinationError> {
        Ok(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))])
    }

    fn metadata_resolver(_host: &str, _port: u16) -> Result<Vec<IpAddr>, DestinationError> {
        Ok(vec![IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))])
    }

    fn unresolvable_resolver(_host: &str, _port: u16) -> Result<Vec<IpAddr>, DestinationError> {
        Err(DestinationError::UnresolvedHost)
    }

    #[test]
    fn validates_required_host_port_and_timeout_bounds() {
        let config = TcpAttachConfig::new("localhost".to_string(), 13603);
        assert!(config.validate().is_ok());

        let config = TcpAttachConfig::new("".to_string(), 13603);
        assert!(config.validate().is_err());

        let config = TcpAttachConfig::new(" localhost ".to_string(), 13603);
        assert!(config.validate().is_ok());

        let config = TcpAttachConfig::new("local host".to_string(), 13603);
        assert!(config.validate().is_err());

        let config = TcpAttachConfig::new("localhost\n".to_string(), 13603);
        assert!(config.validate().is_err());

        let config = TcpAttachConfig::new("localhost".to_string(), 0);
        assert!(config.validate().is_err());

        let config = TcpAttachConfig::new("localhost".to_string(), 13603).with_timeout(5000);
        assert!(config.validate().is_ok());

        let config = TcpAttachConfig::new("localhost".to_string(), 13603).with_timeout(0);
        assert!(config.validate().is_err());

        let config =
            TcpAttachConfig::new("localhost".to_string(), 13603).with_timeout(MAX_TIMEOUT_MS + 1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn timeout_duration_uses_default_or_configured_value() {
        let config = TcpAttachConfig::new("localhost".to_string(), 13603);
        assert_eq!(config.timeout_duration(), Duration::from_millis(DEFAULT_TIMEOUT_MS as u64));

        let config = TcpAttachConfig::new("localhost".to_string(), 13603).with_timeout(10000);
        assert_eq!(config.timeout_duration(), Duration::from_millis(10000));
    }

    // --- SSRF guard tests (IP literal path; no DNS involved) ---

    #[test]
    fn ssrf_guard_rejects_link_local_metadata_ip() {
        // 169.254.169.254 — AWS/GCP/Azure instance metadata endpoint.
        let config = TcpAttachConfig::new("169.254.169.254".to_string(), 80);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("SSRF"), "metadata IP: {err}");
    }

    #[test]
    fn ssrf_guard_rejects_rfc1918_private_ipv4_literals() {
        for addr in ["10.0.0.1", "172.16.0.1", "192.168.1.1"] {
            let config = TcpAttachConfig::new(addr.to_string(), 13603);
            let err = config.validate().unwrap_err();
            assert!(
                err.to_string().contains("SSRF"),
                "private {addr} should be rejected: {err}"
            );
        }
    }

    #[test]
    fn ssrf_guard_rejects_cgnat_literal() {
        let config = TcpAttachConfig::new("100.64.0.1".to_string(), 13603);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("SSRF"), "CGNAT 100.64.0.1: {err}");
    }

    #[test]
    fn ssrf_guard_allows_loopback_ip_literals() {
        let config = TcpAttachConfig::new("127.0.0.1".to_string(), 13603);
        assert!(config.validate().is_ok(), "127.0.0.1 must be allowed");

        let config = TcpAttachConfig::new("::1".to_string(), 13603);
        assert!(config.validate().is_ok(), "::1 must be allowed");
    }

    #[test]
    fn ssrf_guard_allows_public_ip_literal() {
        // 203.0.113.0/24 is TEST-NET-3 (RFC 5737): public-routable documentation range.
        let config = TcpAttachConfig::new("203.0.113.1".to_string(), 13603);
        assert!(config.validate().is_ok(), "public IP 203.0.113.1 must pass");
    }

    #[test]
    fn ssrf_guard_rejects_ipv6_ula_literal() {
        // fd00::/8 (unique local) is the IPv6 equivalent of RFC 1918.
        let config = TcpAttachConfig::new("fd12:3456:789a:1::1".to_string(), 13603);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("SSRF"), "IPv6 ULA: {err}");
    }

    #[test]
    fn ssrf_guard_rejects_ipv6_link_local_literal() {
        let config = TcpAttachConfig::new("fe80::1".to_string(), 13603);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("SSRF"), "IPv6 link-local: {err}");
    }

    // --- SSRF guard tests (hostname path via injectable resolver) ---

    #[test]
    fn ssrf_guard_allows_localhost_resolving_to_loopback() {
        let config = TcpAttachConfig::new("localhost".to_string(), 13603);
        assert!(config.validate_with_resolver(&loopback_resolver).is_ok());
    }

    #[test]
    fn ssrf_guard_rejects_hostname_resolving_to_private_address() {
        let config = TcpAttachConfig::new("debugger.internal".to_string(), 13603);
        let err = config.validate_with_resolver(&private_resolver).unwrap_err();
        assert!(err.to_string().contains("SSRF"), "hostname→private: {err}");
    }

    #[test]
    fn ssrf_guard_rejects_hostname_resolving_to_metadata_address() {
        let config = TcpAttachConfig::new("metadata.internal".to_string(), 80);
        let err = config.validate_with_resolver(&metadata_resolver).unwrap_err();
        assert!(err.to_string().contains("SSRF"), "hostname→metadata: {err}");
    }

    #[test]
    fn ssrf_guard_rejects_localhost_resolving_to_private_address() {
        // DNS spoofing: "localhost" resolves to a private IP.
        let config = TcpAttachConfig::new("localhost".to_string(), 13603);
        let err = config.validate_with_resolver(&private_resolver).unwrap_err();
        assert!(err.to_string().contains("SSRF"), "localhost→private: {err}");
    }

    #[test]
    fn ssrf_guard_rejects_unresolvable_hostname() {
        let config = TcpAttachConfig::new("cant-resolve.invalid".to_string(), 13603);
        let err = config.validate_with_resolver(&unresolvable_resolver).unwrap_err();
        assert!(err.to_string().contains("SSRF"), "unresolvable host: {err}");
    }
}
