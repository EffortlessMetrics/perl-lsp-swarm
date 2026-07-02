//! `cargo xtask check-native-product-surface` — native-stack product-surface guard.
//!
//! The product ships the native stack (`perllsp`, `perl-dap`, native formatter,
//! native critic). The first-mile product surfaces users actually read must not
//! tell them the product *requires* external Perl tooling (`perltidy`,
//! `perlcritic`) or a legacy `Perl::LanguageServer` bridge. This check greps
//! those surfaces for the misleading product-surface phrasings and fails if any
//! reappear.
//!
//! It bans the *requirement / legacy-as-product* framings, not benign
//! native-first negations — e.g. "the native path does not require
//! `Perl::LanguageServer`" is allowed and must stay allowed. Historical
//! design/architecture docs and reference/legacy/compatibility/conformance docs
//! are out of scope by design; legacy details are expected to live in
//! `docs/reference/DAP_LEGACY_BRIDGE_COMPAT.md`.

use color_eyre::eyre::{Result, bail};
use std::fs;
use std::path::Path;

use crate::utils::project_root;

/// First-mile product surfaces scanned for disallowed native-stack leaks.
const SURFACES: &[&str] = &[
    "vscode-extension/package.json",
    "vscode-extension/README.md",
    "crates/perl-dap/README.md",
    "docs/project/status/dap.md",
    "docs/tutorials/DAP_USER_GUIDE.md",
];

/// Misleading product-surface phrasings that must not appear on a first-mile
/// surface. Chosen to catch the "install external tools" / "legacy bridge is the
/// product" framings without flagging correct native-first negations such as
/// "does not require `Perl::LanguageServer`".
const DISALLOWED: &[&str] = &[
    "BridgeAdapter",
    "cpanm Perl::LanguageServer",
    "cpan Perl::LanguageServer",
    "Perl::LanguageServer requirement",
    "Bridge path documents",
    "requires perltidy",
    "requires perlcritic",
    "external Perl::Critic diagnostics",
    "Use bridge mode",
    "--bridge",
];

/// Entry point for `cargo xtask check-native-product-surface`.
pub fn run() -> Result<()> {
    let root = project_root()?;
    let violations = scan(&root)?;

    if violations.is_empty() {
        println!(
            "Native product-surface check passed: {} first-mile surface(s) are free of legacy/external-tool product framing.",
            SURFACES.len()
        );
        return Ok(());
    }

    eprintln!("NATIVE PRODUCT-SURFACE VIOLATIONS:");
    eprintln!("{}", "=".repeat(60));
    for v in &violations {
        eprintln!("  {v}");
    }
    eprintln!("{}", "=".repeat(60));
    eprintln!(
        "These phrasings belong only in reference/legacy/compatibility docs \
         (e.g. docs/reference/DAP_LEGACY_BRIDGE_COMPAT.md), not on first-mile \
         product surfaces."
    );
    bail!("native product-surface check failed with {} violation(s)", violations.len())
}

/// Scan every configured surface under `root`. A surface that does not exist is
/// skipped (not a violation) so the check stays robust to file moves.
fn scan(root: &Path) -> Result<Vec<String>> {
    let mut violations = Vec::new();
    for surface in SURFACES {
        let path = root.join(surface);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        collect_violations(surface, &text, &mut violations);
    }
    Ok(violations)
}

/// Pure per-surface scan, separated so it is unit-testable without touching the
/// repository.
fn collect_violations(surface: &str, text: &str, violations: &mut Vec<String>) {
    for (idx, line) in text.lines().enumerate() {
        for marker in DISALLOWED {
            if line.contains(marker) {
                violations.push(format!(
                    "{surface}:{}: disallowed native-stack marker `{marker}`",
                    idx + 1
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_each_disallowed_marker() {
        for marker in DISALLOWED {
            let text = format!("intro line\nsome text with {marker} in it\ntrailing\n");
            let mut violations = Vec::new();
            collect_violations("surface.md", &text, &mut violations);
            assert!(
                violations.iter().any(|v| v.contains(marker)),
                "marker `{marker}` should be flagged"
            );
        }
    }

    #[test]
    fn allows_native_first_negations() {
        // These are correct native-first statements that must NOT be flagged.
        let text = "\
The native path does **not** require `Perl::LanguageServer`.\n\
Native path avoids `Perl::LanguageServer` dependency.\n\
Enable native Perl document formatting.\n\
Enable external `perlcritic` diagnostics; native critic is always on by default.\n\
`perltidy` is not required unless you select an external compatibility mode.\n";
        let mut violations = Vec::new();
        collect_violations("surface.md", text, &mut violations);
        assert!(violations.is_empty(), "native-first negations must pass: {violations:?}");
    }

    #[test]
    fn reports_line_numbers() {
        let text = "clean\nclean\ncpanm Perl::LanguageServer\n";
        let mut violations = Vec::new();
        collect_violations("guide.md", text, &mut violations);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("guide.md:3:"), "got: {}", violations[0]);
    }

    /// The live repository's first-mile surfaces must be clean. This is the
    /// enforcement that makes the check meaningful: if a future edit reintroduces
    /// a legacy/external-tool product framing on a first-mile surface, this
    /// fails.
    #[test]
    fn live_product_surface_is_clean() -> Result<()> {
        let root = project_root()?;
        let violations = scan(&root)?;
        assert!(
            violations.is_empty(),
            "first-mile product surfaces must be free of legacy/external-tool framing: {violations:#?}"
        );
        Ok(())
    }
}
