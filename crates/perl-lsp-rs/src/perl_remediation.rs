//! The one piece of advice perl-lsp gives a user whose Perl it cannot find.
//!
//! # Why this is a shared constant and not a sentence at each call site
//!
//! Every "we could not resolve a Perl interpreter" message needs the same
//! remediation, and the reason it is *this* remediation is subtle enough that
//! three independent copies reliably drift back into naming a setting that does
//! not exist (#5034, then #5373, then #5376 — the same defect three times, in
//! three files, in two spellings).
//!
//! The server has **no user-facing interpreter-path setting**. Naming one is
//! always wrong:
//!
//! - `perl.path` / `perl-lsp.perl.path` is absent from the extension's
//!   `contributes.configuration`. The nearest shipped setting is
//!   `perl-lsp.serverPath`, which selects the *server binary*, not the
//!   interpreter.
//! - `.perl-lsp.toml`'s `[perl]` section has no interpreter field;
//!   `ProjectPerlConfig` carries only include-path, discovery, and `PERL5LIB`
//!   keys.
//! - `WorkspaceConfig::update_from_value_with_context` — the single path used
//!   for both `.perl-lsp.toml` and `initializationOptions.perl.*` — explicitly
//!   refuses workspace-supplied `perlPath`, because a hostile project could
//!   otherwise redirect the interpreter used for the `@INC` probe and get
//!   arbitrary code execution at config-load time (#3729).
//! - No CLI flag sets it.
//!
//! `perlPath` in `launch.json` is a DAP-only key: it selects the *debuggee's*
//! interpreter and has no effect on language-server command execution. Naming
//! it from a language-server message sends users to the wrong place, so the
//! guards below assert its absence too.
//!
//! That leaves exactly one actionable instruction: install Perl, put it on
//! `PATH`, and restart the server so it inherits the new `PATH`.

/// How a user can actually make perl-lsp find Perl.
///
/// Deliberately names **no editor setting** — see the [module docs](self) for
/// why every candidate setting is either nonexistent or deliberately ignored.
///
/// The reload instruction is load-bearing rather than boilerplate: the server
/// inherits `PATH` from the editor process at launch, so a Perl installed after
/// the editor started is invisible until the window reloads.
pub(crate) const PERL_REMEDIATION: &str = "Install Perl (https://strawberryperl.com on Windows, \
     `brew install perl` on macOS, or your system package manager) and make sure `perl` is on \
     PATH, then reload the window (Ctrl+Shift+P \u{2192} Developer: Reload Window).";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remediation_names_no_interpreter_path_setting() {
        // The defect this constant exists to prevent, in both spellings seen in
        // the wild (#5034 used `perl-lsp.perl.path`, #5376 used bare
        // `perl.path`). Matching the bare form covers the prefixed one.
        assert!(
            !PERL_REMEDIATION.contains("perl.path"),
            "remediation must not name the nonexistent perl.path setting, got: {PERL_REMEDIATION}"
        );
        assert!(
            !PERL_REMEDIATION.contains("perlPath"),
            "remediation must not name the ignored workspace perlPath key, got: {PERL_REMEDIATION}"
        );
        assert!(
            !PERL_REMEDIATION.contains("launch.json"),
            "launch.json perlPath is DAP-only and does not affect the server, got: \
             {PERL_REMEDIATION}"
        );
    }

    #[test]
    fn remediation_names_every_platform_install_route() {
        // A Windows user needs the Strawberry Perl link and a macOS user needs
        // the brew line; neither is inferable from the other, so collapsing
        // this to one canonical sentence would strand two of three platforms.
        assert!(PERL_REMEDIATION.contains("strawberryperl.com"), "must name the Windows route");
        assert!(PERL_REMEDIATION.contains("brew install perl"), "must name the macOS route");
        assert!(PERL_REMEDIATION.contains("package manager"), "must name the Linux route");
    }

    #[test]
    fn remediation_says_how_to_re_trigger_detection() {
        // Installing Perl is not enough on its own: the server holds the PATH
        // it inherited at launch, so without the reload the user installs Perl,
        // sees the same error, and concludes the tool is broken.
        assert!(PERL_REMEDIATION.contains("PATH"), "must tell the user where Perl must be");
        assert!(PERL_REMEDIATION.contains("Reload Window"), "must say how to re-trigger detection");
    }
}
