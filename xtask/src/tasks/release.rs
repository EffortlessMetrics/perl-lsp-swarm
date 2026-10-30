//! Retired `release prepare` front door (#15392).
//!
//! The historical implementation could report a complete, tested release
//! preparation while running no declared proof (the `test_lsp_features.sh`
//! harness did not exist, so the test step silently succeeded), building
//! `perl-dap` from the wrong package, and assembling artifacts outside any
//! transactional receipt. The authoritative release-preparation route is the
//! `cargo xtask release-turnkey` orchestration (#13768 transaction).
//!
//! After successful CLI parsing, this surface refuses with a non-zero exit
//! and performs zero filesystem, git, tool, or network mutation. Clap handles
//! help and malformed arguments before dispatch. No input — including `--yes` — can make
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
    pub fn render(&self, version: &str) -> String {
        let route = if plausible_version(version) {
            format!("{} --version {version}", self.canonical_route)
        } else {
            format!("{} --version <VERSION> (supply a reviewed release version)", self.canonical_route)
        };
        format!(
            "REFUSED: {} is retired and fail-closed.\nReason: {}.\nUse {} instead (ruling: #{})",
            self.retired_command, self.reason, route, self.ruling_issue
        )
    }
}

fn plausible_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
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
/// `--yes` cannot bypass the refusal and no version string can make the legacy pipeline eligible. The function
/// performs no mutation and always returns an error so the process exits
/// non-zero.
pub fn run(version: String, yes: bool) -> Result<()> {
    let _ = yes;
    let refusal = refusal();
    // The rendered refusal travels as the error message so the top-level
    // handler surfaces it on stderr with a non-zero exit; this module prints
    // nothing and mutates nothing.
    bail!("{}", refusal.render(&version));
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, bail};

    #[test]
    fn refusal_is_stable_and_names_the_canonical_route() -> Result<()> {
        let refusal = refusal();
        if refusal.canonical_route != CANONICAL_ROUTE || refusal.ruling_issue != 15392
            || !refusal.retired_command.contains("release prepare") || refusal.reason.is_empty() {
            bail!("retirement refusal identity changed: {refusal:?}");
        }
        Ok(())
    }

    #[test]
    fn render_names_command_reason_route_and_ruling() -> Result<()> {
        let rendered = refusal().render("0.18.0");
        if !rendered.contains("REFUSED") || !rendered.contains("cargo xtask release prepare")
            || !rendered.contains("cargo xtask release-turnkey --version 0.18.0")
            || !rendered.contains("#15392") {
            bail!("incomplete refusal: {rendered}");
        }
        for unsafe_version in ["../../escape", "", "v1.2.3 with spaces", "1.2.3;echo"] {
            let rendered = refusal().render(unsafe_version);
            if rendered.contains(unsafe_version) && !unsafe_version.is_empty() {
                bail!("untrusted version forwarded: {rendered}");
            }
            if !rendered.contains("--version <VERSION>") {
                bail!("missing safe replacement guidance: {rendered}");
            }
        }
        Ok(())
    }

    #[test]
    fn no_input_makes_the_legacy_pipeline_eligible() -> Result<()> {
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
            let Err(error) = outcome else {
                bail!("legacy preparation became eligible for {version:?}");
            };
            let message = format!("{error:#}");
            if !message.contains("fail-closed") || !message.contains(CANONICAL_ROUTE) {
                bail!("unexpected error: {message}");
            }
        }
        Ok(())
    }
}
