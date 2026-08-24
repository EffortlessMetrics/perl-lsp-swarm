//! Exact public contract for the `perl.runScript` compatibility alias.
//!
//! The alias is intentionally retained while command routing converges under
//! #8285/#10245. It must remain the same operation as `perl.runFile`: the same
//! arguments, validation, result, failures, and advertised command identity.

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout/print_stderr don't
// apply the way they do to production code.
#![allow(clippy::print_stderr)]

use perl_lsp::execute_command::{ExecuteCommandProvider, command_exists, get_supported_commands};
use perl_lsp_rs_core::config::WorkspaceConfig;
use serde_json::Value;
use std::error::Error;
use std::fs;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn Error>>;

fn provider_with_perl(root: &std::path::Path) -> ExecuteCommandProvider {
    let mut config = WorkspaceConfig::default();
    config.perl_path = Some("perl".to_string());

    ExecuteCommandProvider::with_workspace_roots(vec![root.to_path_buf()])
        .with_workspace_config(config)
}

#[test]
fn run_script_is_an_exact_run_file_alias() -> TestResult {
    // Requires a real Perl installation; skip when no `perl` is on PATH, matching
    // the established pattern in execute_command_security_tests.rs.
    if !command_exists("perl") {
        eprintln!("skipping run_script_is_an_exact_run_file_alias (no perl on PATH)");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("run_script_alias_contract.pl");
    fs::write(&script, "print \"run-script-alias-contract\\n\";\n")?;

    let provider = provider_with_perl(workspace.path());
    let arguments = vec![Value::String(script.to_string_lossy().into_owned())];

    let canonical = provider
        .execute_command("perl.runFile", arguments.clone())
        .map_err(|error| format!("perl.runFile failed: {error}"))?;
    let alias = provider
        .execute_command("perl.runScript", arguments)
        .map_err(|error| format!("perl.runScript failed: {error}"))?;

    assert_eq!(
        alias, canonical,
        "perl.runScript must preserve the complete perl.runFile result contract"
    );
    assert_eq!(alias.get("success"), Some(&Value::Bool(true)));
    assert!(
        alias
            .get("output")
            .and_then(Value::as_str)
            .is_some_and(|output| output.contains("run-script-alias-contract")),
        "the alias must execute the selected script, not merely return a shaped success"
    );

    Ok(())
}

#[test]
fn run_script_uses_run_file_argument_validation_and_failure_shape() {
    let provider = ExecuteCommandProvider::new();

    for arguments in [Vec::new(), vec![Value::Bool(true)]] {
        let canonical = provider.execute_command("perl.runFile", arguments.clone());
        let alias = provider.execute_command("perl.runScript", arguments);

        assert!(canonical.is_err(), "the malformed canonical request must fail");
        assert_eq!(
            alias, canonical,
            "the alias must not acquire a separate argument or failure contract"
        );
    }
}

#[test]
fn run_script_and_run_file_are_each_advertised_once() {
    let commands = get_supported_commands();

    assert_eq!(
        commands.iter().filter(|command| command.as_str() == "perl.runFile").count(),
        1,
        "the canonical command must be advertised exactly once"
    );
    assert_eq!(
        commands.iter().filter(|command| command.as_str() == "perl.runScript").count(),
        1,
        "the compatibility alias must be advertised exactly once"
    );
}
