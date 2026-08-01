//! Debug launch command construction.

use serde_json::json;

use crate::perl_remediation::PERL_REMEDIATION;
use crate::protocol::JsonRpcError;

/// Message for "`perl.debugFile` needs Perl and none could be resolved".
///
/// This is a language-server `workspace/executeCommand` failure, not a DAP
/// launch failure: `perl.debugFile` spawns a detached `perl -d` and resolves
/// the interpreter through `WorkspaceConfig`, never through `launch.json`. So
/// the DAP-only `perlPath` key would be the wrong thing to name here even
/// though the word "debugger" appears in the message (#5376).
///
/// See [`crate::perl_remediation`] for why no interpreter-path setting is named.
pub(super) fn unresolved_debug_perl_error(resolved: &std::path::Path) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!(
            "Cannot start Perl debugger for '{}': no Perl interpreter was found on PATH, and \
             perl-lsp does not fall back to an ambient interpreter. {PERL_REMEDIATION}",
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
    use super::*;

    /// The `perl.debugFile` remediation must not name an interpreter-path
    /// setting, in either spelling the defect has appeared in.
    ///
    /// Matched on the bare `perl.path` substring so `perl-lsp.perl.path` is
    /// covered by the same assertion — #5373's guards matched only the
    /// prefixed form, which is exactly why this site survived that fix
    /// (#5376).
    #[test]
    fn debug_error_names_no_interpreter_path_setting() {
        let err = unresolved_debug_perl_error(std::path::Path::new("/w/s.pl"));

        assert!(
            !err.message.contains("perl.path"),
            "must not point at the nonexistent perl.path setting, got: {}",
            err.message
        );
        assert!(
            !err.message.contains("Configure"),
            "must not tell the user to configure anything; nothing is configurable here, got: {}",
            err.message
        );
    }

    /// `perl.debugFile` resolves through `WorkspaceConfig`, not `launch.json`,
    /// so the DAP-only `perlPath` key would be a different wrong answer —
    /// tempting here precisely because the message says "debugger".
    #[test]
    fn debug_error_does_not_send_users_to_dap_only_launch_json() {
        let err = unresolved_debug_perl_error(std::path::Path::new("/w/s.pl"));

        assert!(
            !err.message.contains("launch.json"),
            "perl.debugFile does not read launch.json, got: {}",
            err.message
        );
        assert!(
            !err.message.contains("perlPath"),
            "perlPath is a DAP-only key with no effect here, got: {}",
            err.message
        );
    }

    #[test]
    fn debug_error_keeps_the_file_and_the_actionable_remediation() {
        let err = unresolved_debug_perl_error(std::path::Path::new("/w/s.pl"));

        assert_eq!(err.code, -32603, "internal-error code is part of the wire contract");
        assert!(err.message.contains("/w/s.pl"), "must name the file, got: {}", err.message);
        assert!(
            err.message.contains("Install Perl"),
            "must give the one remediation that works, got: {}",
            err.message
        );
        assert!(
            err.message.contains("Reload Window"),
            "installing Perl alone does not refresh the server's inherited PATH, got: {}",
            err.message
        );
    }

    /// The deliberate refusal is behavior the user should understand, not a
    /// bug to be quietly dropped while rewording the remediation.
    #[test]
    fn debug_error_still_states_that_ambient_fallback_is_refused() {
        let err = unresolved_debug_perl_error(std::path::Path::new("/w/s.pl"));

        assert!(
            err.message.contains("does not fall back to an ambient interpreter"),
            "must explain why no interpreter was guessed, got: {}",
            err.message
        );
    }
}
