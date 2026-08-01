//! The one remediation sentence for "no usable Perl interpreter".
//!
//! Every user-facing message that reports an unresolvable Perl interpreter ends
//! with [`PERL_REMEDIATION`]. Three emitters use it today:
//!
//! - `runtime::lifecycle::workspace` — startup interpreter detection,
//! - `execute_command::provider` — run/test/critic commands,
//! - `runtime::language::misc::debug_launch` — `perl.debugFile`.
//!
//! Keeping the text in one place is the point. The same defect — remediation
//! naming a setting the user cannot set — was fixed three times in three files
//! (#969, #5034, #5373) before #5376 found the last two, precisely because each
//! site carried its own wording.

/// Remediation for "no usable Perl interpreter", naming only actions a user can
/// actually perform.
///
/// It deliberately does **not** name `perl.path` / `perl-lsp.perl.path`. That
/// field exists on `WorkspaceConfig`, but no user-facing channel writes it:
///
/// - it is absent from the extension's `contributes.configuration` — the
///   shipped settings are `perl-lsp.serverPath` (the *server binary*, not the
///   interpreter), `includePaths`, and `externalIncludePaths`;
/// - `.perl-lsp.toml`'s `[perl]` section has no interpreter field
///   (`ProjectPerlConfig` carries only include/discovery/PERL5LIB keys);
/// - workspace-scoped `perlPath` is *deliberately ignored*, because a hostile
///   project could otherwise redirect the interpreter used for the `@INC` probe
///   and get arbitrary code execution at config-load time (#3729).
///
/// It also does not name `launch.json`'s `perlPath`: that is a DAP-only key
/// selecting the debuggee's interpreter, with no effect on the language server.
/// The DAP side carries its own, correct `launch.json` guidance.
///
/// If an interpreter-path channel is ever intentionally wired, this constant and
/// the DAP-side guidance are updated together.
pub(crate) const PERL_REMEDIATION: &str = "Install Perl (https://strawberryperl.com on Windows, \
     `brew install perl` on macOS, or your system package manager) and make sure `perl` is on \
     PATH, then reload the window (Ctrl+Shift+P \u{2192} Developer: Reload Window).";

#[cfg(test)]
mod tests {
    use super::PERL_REMEDIATION;

    #[test]
    fn remediation_names_no_setting_the_user_cannot_write() {
        assert!(
            !PERL_REMEDIATION.contains("perl.path"),
            "remediation must not name an unsettable interpreter setting: {PERL_REMEDIATION}"
        );
        assert!(
            !PERL_REMEDIATION.contains("launch.json"),
            "language-server remediation must not send users to a DAP-only key: {PERL_REMEDIATION}"
        );
    }

    #[test]
    fn remediation_names_every_route_a_user_needs() {
        // Collapsing these into one canonical sentence would leave whichever
        // platform was dropped without a route.
        assert!(PERL_REMEDIATION.contains("strawberryperl.com"), "Windows route missing");
        assert!(PERL_REMEDIATION.contains("brew install perl"), "macOS route missing");
        assert!(PERL_REMEDIATION.contains("package manager"), "Linux route missing");
        assert!(PERL_REMEDIATION.contains("PATH"), "PATH is the resolution channel");
        assert!(PERL_REMEDIATION.contains("Reload Window"), "must say how to re-trigger detection");
    }
}
