//! Retired `release prepare` front door (#15392).
//!
//! The historical implementation could report a complete, tested release
//! preparation while running no declared proof (the `test_lsp_features.sh`
//! harness did not exist, so the test step silently succeeded), building
//! `perl-dap` from the wrong package, and assembling artifacts outside any
//! transactional receipt. The authoritative release-preparation route is the
//! `cargo xtask release-turnkey` orchestration (#13768 transaction).
//!
//! This surface is fail-closed: every invocation returns a typed refusal with
//! a non-zero exit and performs zero filesystem, git, tool, or network
//! mutation. No input — including `--yes` or a well-formed version — can make
//! the legacy pipeline eligible again; a supported adapter would have to be a
//! thin typed façade over the same exact release-candidate pipeline, not a
//! second implementation that can drift.

use color_eyre::eyre::{Result, bail};

/// Canonical release-preparation route that replaces the retired command.
pub const CANONICAL_ROUTE: &str = "cargo xtask release-turnkey";

/// Typed refusal produced by the retired `release prepare` front door.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRefusal {
    /// The retired command surface.
    pub retired_command: &'static str,
    /// Why the command refuses to run.
    pub reason: &'static str,
    /// The single supported replacement route.
    pub canonical_route: &'static str,
    /// Controlling issue recording the retirement ruling.
    pub ruling_issue: u32,
}

impl PrepareRefusal {
    /// Operator-facing rendering of the refusal.
    pub fn render(&self) -> String {
        format!(
            "REFUSED: {} is retired and fail-closed.\nReason: {}.\nUse {} instead (ruling: #{})",
            self.retired_command, self.reason, self.canonical_route, self.ruling_issue
        )
    }
}

/// The one refusal this surface can produce.
///
/// The refusal is deliberately independent of caller input: the version and
/// confirmation flag carry no eligibility authority, so no argument can
/// re-enable the retired pipeline.
pub fn refusal() -> PrepareRefusal {
    PrepareRefusal {
        retired_command: "cargo xtask release prepare",
        reason: "the legacy implementation ran no declared release proof, built perl-dap \
                 from the wrong package, and assembled artifacts without an exact \
                 source/version/artifact receipt",
        canonical_route: CANONICAL_ROUTE,
        ruling_issue: 15392,
    }
}

/// Entry point for `cargo xtask release prepare`.
///
/// Both arguments are intentionally unused: `--yes` cannot bypass the refusal
/// and no version string can make the legacy pipeline eligible. The function
/// performs no mutation and always returns an error so the process exits
/// non-zero.
pub fn run(version: String, yes: bool) -> Result<()> {
    let _ = (version, yes);
    let refusal = refusal();
    // The rendered refusal travels as the error message so the top-level
    // handler surfaces it on stderr with a non-zero exit; this module prints
    // nothing and mutates nothing.
    bail!("{}", refusal.render());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // scoped test-only reads of refusal errors

    use super::*;

    #[test]
    fn refusal_is_stable_and_names_the_canonical_route() {
        let refusal = refusal();
        assert_eq!(refusal.canonical_route, "cargo xtask release-turnkey");
        assert_eq!(refusal.ruling_issue, 15392);
        assert!(refusal.retired_command.contains("release prepare"));
        assert!(!refusal.reason.is_empty());
    }

    #[test]
    fn render_names_command_reason_route_and_ruling() {
        let rendered = refusal().render();
        assert!(rendered.contains("REFUSED"));
        assert!(rendered.contains("cargo xtask release prepare"));
        assert!(rendered.contains("cargo xtask release-turnkey"));
        assert!(rendered.contains("#15392"));
    }

    #[test]
    fn no_input_makes_the_legacy_pipeline_eligible() {
        // Mutation check on the eligibility boundary: the refusal must be
        // total across confirmation flags and version shapes, including a
        // plausible release version, a path-like injection attempt, and an
        // empty string. Any input reaching an Ok path would be a regression
        // to the unsafe front door.
        let cases = [
            ("0.13.0", true),
            ("0.13.0", false),
            ("../../escape", true),
            ("", false),
            ("v1.2.3 with spaces", true),
        ];
        for (version, yes) in cases {
            let outcome = run(version.to_string(), yes);
            let error = outcome.expect_err("refusal must be total: no input is eligible");
            let message = format!("{error:#}");
            assert!(message.contains("fail-closed"), "unexpected error: {message}");
            assert!(message.contains(CANONICAL_ROUTE), "unexpected error: {message}");
        }
    }
}
