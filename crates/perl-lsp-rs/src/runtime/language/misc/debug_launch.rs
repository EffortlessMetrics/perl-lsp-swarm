//! Debug launch command construction.

use serde_json::json;

use crate::perl_remediation::PERL_REMEDIATION;
use crate::protocol::JsonRpcError;

/// Error for "`perl.debugFile` was asked to launch `perl -d` and no interpreter
/// was usable".
///
/// This runs `perl -d` from the language server, not through the debug adapter,
/// so `launch.json`'s `perlPath` would not affect it and naming that key here
/// would be a different wrong answer. The remediation is the shared
/// [`PERL_REMEDIATION`] sentence; the previous text named an interpreter-path
/// setting that no user-facing channel can write (#5376).
pub(super) fn unresolved_debug_perl_error(resolved: &std::path::Path) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!(
            "Cannot start Perl debugger for '{}': no usable Perl interpreter was found on PATH; \
             refusing ambient fallback. {PERL_REMEDIATION}",
            resolved.display()
        ),
        data: Some(json!({"file": resolved.display().to_string()})),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn debug_command_from_oracle(
    oracle: Option<perl_lsp_rs_core::config::PerlOracleEnv>,
    resolved: &std::path::Path,
) -> Result<std::process::Command, JsonRpcError> {
    oracle
        .as_ref()
        .map(perl_lsp_rs_core::config::PerlOracleEnv::into_command)
        .ok_or_else(|| unresolved_debug_perl_error(resolved))
}

#[cfg(test)]
mod tests {
    use super::unresolved_debug_perl_error;

    /// `perl.debugFile` runs `perl -d` from the language server, so the only
    /// remediation that works here is making `perl` resolvable. Naming an
    /// interpreter-path setting sends the reader nowhere (#5376), and naming
    /// `launch.json` would send them to a key this path never reads (#5373).
    #[test]
    fn debug_launch_error_gives_remediation_the_user_can_act_on() {
        let message = unresolved_debug_perl_error(std::path::Path::new("script.pl")).message;

        assert!(
            !message.contains("perl.path"),
            "must not name an unsettable interpreter setting, got: {message}"
        );
        assert!(
            !message.contains("launch.json"),
            "language-server message must not send users to launch.json, got: {message}"
        );
        assert!(message.contains("PATH"), "must name PATH, got: {message}");
        assert!(message.contains("Install Perl"), "must name installing Perl, got: {message}");
        // The fail-closed behavior is deliberate and stays stated.
        assert!(
            message.contains("refusing ambient fallback"),
            "must keep the fail-closed statement, got: {message}"
        );
    }
}
