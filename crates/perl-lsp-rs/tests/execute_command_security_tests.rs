//! Security regression tests for executeCommand.
//!
//! These tests verify that command injection vulnerabilities in run_test_sub,
//! run_tests, and run_file have been properly mitigated, and that the
//! PerlOracleEnv isolation applied via `with_workspace_config` prevents
//! ambient env vars from leaking into the Perl subprocess.
//!
//! Note: With secure path resolution, malicious/non-existent paths are rejected
//! early at the path validation stage (returning Err), preventing execution entirely.

use perl_lsp::execute_command::ExecuteCommandProvider;
use perl_lsp_rs_core::config::WorkspaceConfig;
use serde_json::Value;
use std::error::Error;
use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

/// Test that run_test_sub is protected against code injection via file_path.
///
/// The vulnerable code previously constructed:
/// `do '{}'; if (defined &{}) {{ {}() }} else {{ die 'Subroutine {} not found' }}`
/// which allowed injection through the file_path parameter.
///
/// With secure path resolution, non-existent files are rejected before execution.
/// The key security property is that the malicious code never reaches Perl.
#[test]
fn test_run_test_sub_file_path_injection() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    // Payload that would inject code if string interpolation is used
    let malicious_file_path = "nonexistent.pl'; print 'INJECTED_VIA_FILE'; '";

    let result = provider.execute_command(
        "perl.runTestSub",
        vec![Value::String(malicious_file_path.to_string()), Value::String("somesub".to_string())],
    );

    // With secure path resolution, non-existent files are rejected early
    // BEFORE any shell command or Perl code is executed
    assert!(result.is_err(), "Malicious path should be rejected during path resolution");
    let err = result.err().ok_or("Expected error but got Ok")?;

    // The error should be about path resolution (file not found/canonicalize failure)
    // NOT about Perl code execution or subroutine lookup
    assert!(
        err.contains("canonicalize") || err.contains("Failed to"),
        "Error should be about path validation, not code execution: {}",
        err
    );

    // The path may be echoed in the error (this is fine - it's just a filename),
    // but the key is that no Perl code was executed with this malicious string.
    // The secure path resolution catches it at the Rust layer.
    Ok(())
}

/// Test that run_test_sub is protected against code injection via sub_name.
///
/// Note: The sub_name is passed via @ARGV, so the malicious string is treated
/// as a literal subroutine name to look up, not as code to execute. The error
/// message will contain the literal name (safe behavior), but the injected
/// code will NOT be executed.
#[test]
fn test_run_test_sub_subname_injection() -> Result<(), Box<dyn Error>> {
    let temp_dir = TempDir::new()?;
    let config = match config_with_perl5lib(false) {
        Some(config) => config,
        None => return Ok(()),
    };
    let provider =
        ExecuteCommandProvider::with_workspace_roots(vec![temp_dir.path().to_path_buf()])
            .with_workspace_config(config);

    // Create a minimal test file with a marker subroutine
    let test_file = temp_dir.path().join("security_test_sub.pl");
    std::fs::write(&test_file, "sub safe_sub { print 'SAFE_SUB_EXECUTED'; }")?;

    // This payload would execute code if string interpolation was used.
    // With the fix (using @ARGV), it's treated as a literal subroutine name.
    let malicious_sub_name = "safe_sub(); print 'INJECTED_CODE_RAN'";

    let result = provider.execute_command(
        "perl.runTestSub",
        vec![
            Value::String(test_file.to_string_lossy().to_string()),
            Value::String(malicious_sub_name.to_string()),
        ],
    );

    let val = result
        .map_err(|err| format!("Command should not fail to spawn with Perl config: {err}"))?;
    let output = val["output"].as_str().ok_or("Missing 'output' field")?;

    // Key assertions:
    // 1. The injected print statement should NOT have executed
    assert!(
        !output.contains("INJECTED_CODE_RAN"),
        "Vulnerability: code injection via sub_name succeeded! Output: {}",
        output
    );

    // 2. The safe_sub should NOT have been called either (the malicious name
    //    includes "safe_sub()" but that should be treated literally, not executed)
    assert!(
        !output.contains("SAFE_SUB_EXECUTED"),
        "Unexpected: safe_sub was called despite malicious sub_name. Output: {}",
        output
    );

    // 3. The command should have failed because no subroutine with that literal name exists
    let success = val["success"].as_bool().ok_or("Missing 'success' field")?;
    assert!(!success, "Command should have failed (subroutine not found)");
    Ok(())
}

/// Test that run_file is protected against argument injection via file_path.
///
/// A file path starting with `-` could be interpreted as a flag without `--`.
/// With secure path resolution, non-existent files are rejected before execution.
#[test]
fn test_run_file_argument_injection() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    // Payload that would be interpreted as a flag without `--` separator
    // `-e print 'INJECTED'` would execute arbitrary code
    let malicious_file_path = "-e";

    let result = provider
        .execute_command("perl.runFile", vec![Value::String(malicious_file_path.to_string())]);

    // With secure path resolution, non-existent files are rejected early
    assert!(result.is_err(), "Malicious path '-e' should be rejected during path resolution");
    let err = result.err().ok_or("Expected error but got Ok")?;

    // The error should be about path validation
    assert!(
        err.contains("canonicalize") || err.contains("not found"),
        "Error should be about path validation: {}",
        err
    );
    Ok(())
}

/// Test that run_tests is protected against argument injection via file_path.
/// With secure path resolution, non-existent files are rejected before execution.
#[test]
fn test_run_tests_argument_injection() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    // Similar test for run_tests
    let malicious_file_path = "-e";

    let result = provider
        .execute_command("perl.runTests", vec![Value::String(malicious_file_path.to_string())]);

    // With secure path resolution, non-existent files are rejected early
    assert!(result.is_err(), "Malicious path '-e' should be rejected during path resolution");
    let err = result.err().ok_or("Expected error but got Ok")?;

    // The error should be about path validation
    assert!(
        err.contains("canonicalize") || err.contains("not found"),
        "Error should be about path validation: {}",
        err
    );
    Ok(())
}

/// Test that file paths with shell metacharacters are safely rejected.
///
/// With secure path resolution, files that don't exist are rejected before
/// any shell command is executed, preventing shell metacharacter expansion.
#[test]
fn test_shell_metacharacter_safety() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    // File paths with shell metacharacters that could cause issues
    // if shell expansion occurred. These non-existent paths should be
    // rejected during path validation before any shell execution.
    let dangerous_paths = vec![
        "/tmp/test$(whoami).pl",
        "/tmp/test`id`.pl",
        "/tmp/test;rm -rf /.pl",
        "/tmp/test|cat /etc/passwd.pl",
        "/tmp/test&& echo pwned.pl",
    ];

    for path in dangerous_paths {
        let result =
            provider.execute_command("perl.runFile", vec![Value::String(path.to_string())]);

        // Non-existent paths should be rejected during path resolution
        assert!(result.is_err(), "Non-existent path should be rejected: {}", path);
        let err = result.err().ok_or("Expected error but got Ok")?;

        // Error should be about path validation, not shell execution
        assert!(
            err.contains("canonicalize") || err.contains("not found"),
            "Error should be about path validation for {}: {}",
            path,
            err
        );
    }
    Ok(())
}

/// Test that valid files with safe paths execute correctly.
#[test]
fn test_valid_file_execution() -> Result<(), Box<dyn Error>> {
    let temp_dir = TempDir::new()?;
    let file_path = temp_dir.path().join("test_valid.pl");
    fs::write(&file_path, "print 'VALID_OUTPUT';")?;

    let config = match config_with_perl5lib(false) {
        Some(config) => config,
        None => return Ok(()),
    };
    let provider =
        ExecuteCommandProvider::with_workspace_roots(vec![temp_dir.path().to_path_buf()])
            .with_workspace_config(config);

    let result = provider.execute_command(
        "perl.runFile",
        vec![Value::String(file_path.to_string_lossy().to_string())],
    );

    let val = result.map_err(|err| format!("Valid file should execute with Perl config: {err}"))?;
    let output = val["output"].as_str().ok_or("Missing 'output' field")?;

    assert!(output.contains("VALID_OUTPUT"), "Output should contain expected result: {}", output);
    Ok(())
}

// ============= Slice E: executeCommand Hardening Tests =============
// These tests verify the CWD boundary fallback, path traversal protection,
// argument length caps, and command injection prevention.

/// Test that commands are rejected when workspace_roots is empty and path is outside CWD
#[test]
fn test_empty_workspace_roots_enforces_cwd_boundary() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();
    // Provider has empty workspace_roots by default, which now falls back to CWD.
    // Use a real file in a temp directory so the path exists but is still outside the repo CWD.
    let temp_dir = TempDir::new()?;
    let outside_file = temp_dir.path().join("outside_cwd.pl");
    fs::write(&outside_file, "print 'outside cwd';")?;

    let result = provider.execute_command(
        "perl.runCritic",
        vec![Value::String(outside_file.to_string_lossy().to_string())],
    );

    assert!(result.is_err(), "Should reject paths outside CWD when workspace_roots is empty");
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_command_exists_does_not_execute_path_hijacked_which() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    if child_mode("test_command_exists_does_not_execute_path_hijacked_which") {
        assert!(
            perl_lsp::execute_command::command_exists("perlcritic"),
            "the PATH fixture must make perlcritic discoverable"
        );
        let marker = child_env("SECURITY_MARKER")?;
        assert!(!Path::new(&marker).exists(), "PATH probe executed a hijacked which");
        return Ok(());
    }

    let temp_dir = TempDir::new()?;
    let which_path = temp_dir.path().join("which");
    let perlcritic_path = temp_dir.path().join("perlcritic");
    let marker_path = temp_dir.path().join("which-executed.marker");

    fs::write(
        &which_path,
        format!(
            "#!/bin/sh
printf 'executed' > '{}'
exit 0
",
            marker_path.display()
        ),
    )?;
    fs::write(&perlcritic_path, "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(&which_path, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(&perlcritic_path, fs::Permissions::from_mode(0o755))?;

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![temp_dir.path().to_path_buf()];
    path_entries.extend(std::env::split_paths(&original_path));
    let joined_path = std::env::join_paths(path_entries)?;

    let output = run_child(
        "test_command_exists_does_not_execute_path_hijacked_which",
        &[("PATH", joined_path.as_os_str())],
        &[("SECURITY_MARKER", marker_path.as_os_str())],
    )?;
    assert!(output.status.success(), "child failed: {}", output_text(&output));

    Ok(())
}
#[cfg(unix)]
#[test]
fn test_command_exists_does_not_execute_candidate_binary() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    if child_mode("test_command_exists_does_not_execute_candidate_binary") {
        let exists = perl_lsp::execute_command::command_exists("fake-security-probe");
        let marker = child_env("SECURITY_MARKER")?;
        assert!(exists, "candidate should be discoverable in PATH");
        assert!(!Path::new(&marker).exists(), "command_exists executed the candidate");
        return Ok(());
    }

    let temp_dir = TempDir::new()?;
    let script_path = temp_dir.path().join("fake-security-probe");
    let marker_path = temp_dir.path().join("executed.marker");

    fs::write(
        &script_path,
        format!("#!/bin/sh\nprintf 'executed' > '{}'\nexit 0\n", marker_path.display()),
    )?;
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![temp_dir.path().to_path_buf()];
    path_entries.extend(std::env::split_paths(&original_path));
    let joined_path = std::env::join_paths(path_entries)?;

    let output = run_child(
        "test_command_exists_does_not_execute_candidate_binary",
        &[("PATH", joined_path.as_os_str())],
        &[("SECURITY_MARKER", marker_path.as_os_str())],
    )?;
    assert!(output.status.success(), "child failed: {}", output_text(&output));
    Ok(())
}

/// Test that path traversal via .. is rejected
#[test]
fn test_path_traversal_with_dot_dot() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    let result = provider
        .execute_command("perl.runCritic", vec![Value::String("../../../etc/passwd".to_string())]);

    assert!(result.is_err(), "Path traversal with .. should be rejected");
    let err = result.err().ok_or("Expected error")?;
    assert!(
        err.contains("traversal") || err.contains(".."),
        "Error should mention traversal: {}",
        err
    );
    Ok(())
}

/// Test that extremely long arguments are rejected
#[test]
fn test_argument_length_cap() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    let long_path = "a".repeat(5000);
    let result = provider.execute_command("perl.runCritic", vec![Value::String(long_path)]);

    assert!(result.is_err(), "Extremely long arguments should be rejected");
    let err = result.err().ok_or("Expected error")?;
    assert!(
        err.contains("too long") || err.contains("4096"),
        "Error should mention length limit: {}",
        err
    );
    Ok(())
}

/// Test command injection attempts in file paths
#[test]
fn test_command_injection_semicolon() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    let malicious = "; rm -rf /tmp/test";
    let result =
        provider.execute_command("perl.runCritic", vec![Value::String(malicious.to_string())]);

    // Should fail at path validation, not reach shell execution.
    // perl.runCritic returns graceful error responses for file-not-found,
    // so check either Err or Ok-with-error-status.
    match result {
        Err(_) => {} // Rejected outright - good
        Ok(val) => {
            assert_eq!(val["status"], "error", "Should report error status for malicious path");
        }
    }
    Ok(())
}

/// Test command injection via backticks
#[test]
fn test_command_injection_backticks() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    let malicious = "`echo pwned`";
    let result =
        provider.execute_command("perl.runCritic", vec![Value::String(malicious.to_string())]);

    match result {
        Err(_) => {} // Rejected outright - good
        Ok(val) => {
            assert_eq!(val["status"], "error", "Should report error status for backtick injection");
        }
    }
    Ok(())
}

/// Test command injection via $()
#[test]
fn test_command_injection_dollar_paren() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    let malicious = "$(cat /etc/shadow)";
    let result =
        provider.execute_command("perl.runCritic", vec![Value::String(malicious.to_string())]);

    match result {
        Err(_) => {} // Rejected outright - good
        Ok(val) => {
            assert_eq!(val["status"], "error", "Should report error status for $() injection");
        }
    }
    Ok(())
}

/// Test pipe injection
#[test]
fn test_command_injection_pipe() -> Result<(), Box<dyn Error>> {
    let provider = ExecuteCommandProvider::new();

    let malicious = "| cat /etc/passwd";
    let result =
        provider.execute_command("perl.runCritic", vec![Value::String(malicious.to_string())]);

    match result {
        Err(_) => {} // Rejected outright - good
        Ok(val) => {
            assert_eq!(val["status"], "error", "Should report error status for pipe injection");
        }
    }
    Ok(())
}

// ── PerlOracleEnv poisoned-env tests (#8684) ──────────────────────────────────
//
// Verify that `run_file` and `run_test_sub` do NOT propagate PERL5LIB or
// PERL5OPT to the Perl subprocess unless the workspace config explicitly
// allows them — regression guard for the #8493-class incident.
//
// These tests require a real Perl installation; they are skipped when no Perl
// binary can be resolved.

const CHILD_MODE: &str = "PERL_LSP_SECURITY_CHILD";

fn child_mode(name: &str) -> bool {
    std::env::var_os(CHILD_MODE).is_some_and(|value| value.to_string_lossy() == name)
}

fn child_env(name: &str) -> Result<std::ffi::OsString, Box<dyn Error>> {
    std::env::var_os(name).ok_or_else(|| format!("missing child environment key {name}").into())
}

fn run_child(
    selector: &str,
    env: &[(&str, &std::ffi::OsStr)],
    extra_env: &[(&str, &std::ffi::OsStr)],
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--exact", selector, "--nocapture"]);
    command.env(CHILD_MODE, selector);
    for (key, value) in env.iter().chain(extra_env.iter()) {
        command.env(key, value);
    }
    Ok(command.output()?)
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Helper: resolve the system Perl binary via the toolchain resolver.
fn find_perl() -> Option<std::path::PathBuf> {
    perl_lsp_rs_core::platform::resolve_perl_path_with_toolchain().ok()
}

/// Helper: build a minimal `WorkspaceConfig` with a resolved Perl binary and
/// `use_perl5lib` set as requested.
fn config_with_perl5lib(use_perl5lib: bool) -> Option<WorkspaceConfig> {
    let perl = find_perl()?;
    let mut config = WorkspaceConfig::default();
    config.perl_path = Some(perl.to_string_lossy().into_owned());
    config.use_perl5lib = use_perl5lib;
    Some(config)
}

/// `run_file` with `use_perl5lib=false` must strip PERL5LIB from the subprocess.
///
/// Regression guard for the #8493-class incident: ambient PERL5LIB must not
/// reach the subprocess when the workspace config opts out.
#[test]
fn run_file_strips_perl5lib_when_use_perl5lib_false() -> Result<(), Box<dyn Error>> {
    if child_mode("run_file_strips_perl5lib_when_use_perl5lib_false") {
        let script = child_env("SECURITY_SCRIPT")?;
        let config = config_with_perl5lib(false).ok_or("no Perl binary available")?;
        let root =
            std::path::PathBuf::from(&script).parent().ok_or("script has no parent")?.to_path_buf();
        let provider =
            ExecuteCommandProvider::with_workspace_roots(vec![root]).with_workspace_config(config);
        let result = provider.execute_command(
            "perl.runFile",
            vec![Value::String(script.to_string_lossy().into_owned())],
        )?;
        let output = result["output"].as_str().ok_or("missing output")?;
        assert!(output.contains("UNSET"), "child inherited PERL5LIB: {output:?}");
        return Ok(());
    }
    if config_with_perl5lib(false).is_none() {
        return Ok(()); // no Perl — skip
    }

    let temp_dir = TempDir::new()?;
    // Script prints the PERL5LIB env var or "UNSET" if absent.
    let script = temp_dir.path().join("check_env.pl");
    std::fs::write(&script, "print $ENV{PERL5LIB} // 'UNSET';\n")?;

    let poison_dir = TempDir::new()?;
    let poison_path = poison_dir.path().to_path_buf();

    let before = std::env::var_os("PERL5LIB");
    let output = run_child(
        "run_file_strips_perl5lib_when_use_perl5lib_false",
        &[("PERL5LIB", poison_path.as_os_str())],
        &[("SECURITY_SCRIPT", script.as_os_str())],
    )?;
    assert!(output.status.success(), "child failed: {}", output_text(&output));
    assert_eq!(std::env::var_os("PERL5LIB"), before, "parent environment changed");
    Ok(())
}

/// `run_file` with `use_perl5lib=true` must pass PERL5LIB through to the subprocess.
#[test]
fn run_file_passes_perl5lib_when_use_perl5lib_true() -> Result<(), Box<dyn Error>> {
    if child_mode("run_file_passes_perl5lib_when_use_perl5lib_true") {
        let script = child_env("SECURITY_SCRIPT")?;
        let config = config_with_perl5lib(true).ok_or("no Perl binary available")?;
        let root =
            std::path::PathBuf::from(&script).parent().ok_or("script has no parent")?.to_path_buf();
        let provider =
            ExecuteCommandProvider::with_workspace_roots(vec![root]).with_workspace_config(config);
        let result = provider.execute_command(
            "perl.runFile",
            vec![Value::String(script.to_string_lossy().into_owned())],
        )?;
        let output = result["output"].as_str().ok_or("missing output")?;
        let marker = child_env("SECURITY_MARKER")?;
        assert!(
            output.contains(marker.to_string_lossy().as_ref()),
            "PERL5LIB was not passed: {output:?}"
        );
        return Ok(());
    }
    if config_with_perl5lib(true).is_none() {
        return Ok(());
    }

    let temp_dir = TempDir::new()?;
    let script = temp_dir.path().join("check_env.pl");
    std::fs::write(&script, "print $ENV{PERL5LIB} // 'UNSET';\n")?;

    let marker_dir = TempDir::new()?;
    let marker_path = marker_dir.path().to_path_buf();

    let before = std::env::var_os("PERL5LIB");
    let output = run_child(
        "run_file_passes_perl5lib_when_use_perl5lib_true",
        &[("PERL5LIB", marker_path.as_os_str())],
        &[("SECURITY_SCRIPT", script.as_os_str()), ("SECURITY_MARKER", marker_path.as_os_str())],
    )?;
    assert!(output.status.success(), "child failed: {}", output_text(&output));
    assert_eq!(std::env::var_os("PERL5LIB"), before, "parent environment changed");
    Ok(())
}

/// `run_test_sub` with `use_perl5lib=false` must strip PERL5LIB from the subprocess.
#[test]
fn run_test_sub_strips_perl5lib_when_use_perl5lib_false() -> Result<(), Box<dyn Error>> {
    if child_mode("run_test_sub_strips_perl5lib_when_use_perl5lib_false") {
        let script = child_env("SECURITY_SCRIPT")?;
        let config = config_with_perl5lib(false).ok_or("no Perl binary available")?;
        let root =
            std::path::PathBuf::from(&script).parent().ok_or("script has no parent")?.to_path_buf();
        let provider =
            ExecuteCommandProvider::with_workspace_roots(vec![root]).with_workspace_config(config);
        let result = provider.execute_command(
            "perl.runTestSub",
            vec![
                Value::String(script.to_string_lossy().into_owned()),
                Value::String("check_env".to_string()),
            ],
        )?;
        let output = result["output"].as_str().ok_or("missing output")?;
        assert!(output.contains("UNSET"), "child inherited PERL5LIB: {output:?}");
        return Ok(());
    }
    if config_with_perl5lib(false).is_none() {
        return Ok(());
    }

    let temp_dir = TempDir::new()?;
    // A Perl file with a sub that prints the env var.
    let script = temp_dir.path().join("check_env_sub.pl");
    std::fs::write(&script, "sub check_env { print $ENV{PERL5LIB} // 'UNSET'; }\n")?;

    let poison_dir = TempDir::new()?;
    let poison_path = poison_dir.path().to_path_buf();

    let before = std::env::var_os("PERL5LIB");
    let output = run_child(
        "run_test_sub_strips_perl5lib_when_use_perl5lib_false",
        &[("PERL5LIB", poison_path.as_os_str())],
        &[("SECURITY_SCRIPT", script.as_os_str())],
    )?;
    assert!(output.status.success(), "child failed: {}", output_text(&output));
    assert_eq!(std::env::var_os("PERL5LIB"), before, "parent environment changed");
    Ok(())
}
