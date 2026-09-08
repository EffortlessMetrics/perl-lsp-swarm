use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const OBSERVATIONS: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/exact-source-observations-template.json";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn read(root: &Path, relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(relative))?)
}

#[test]
fn observation_template_is_not_run_bound_and_cell_complete() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = read(&root, OBSERVATIONS)?;
    let template: Value = serde_json::from_str(&text)?;
    assert_eq!(
        template.get("schema_version").and_then(Value::as_str),
        Some("zed_exact_source_observations.v1")
    );
    assert_eq!(template.get("result").and_then(Value::as_str), Some("not_run"));
    assert!(
        template.get("prepared_manifest_sha256").is_some_and(Value::is_null),
        "template must declare an unbound prepared_manifest_sha256"
    );
    assert!(
        template
            .pointer("/language_server_log/prepared_manifest_sha256")
            .is_some_and(Value::is_null),
        "language_server_log must declare an unbound prepared_manifest_sha256"
    );
    assert!(
        template.pointer("/language_server_log/path").is_some_and(Value::is_null),
        "language_server_log.path must be null in the template"
    );
    assert!(
        template.pointer("/language_server_log/sha256").is_some_and(Value::is_null),
        "language_server_log.sha256 must be null in the template"
    );
    assert_eq!(
        template
            .pointer("/configuration/workspace_configuration_observed")
            .and_then(Value::as_bool),
        Some(false)
    );
    for cell in [
        "manifest_discovery",
        "perl_attachment",
        "initialize",
        "workspace_root",
        "diagnostics",
        "completion",
        "hover",
        "definition",
        "references",
        "document_symbols",
        "workspace_symbols",
        "safe_edit_or_refusal",
        "unicode_positions",
        "mixed_newlines",
        "semantic_tokens",
        "post_edit_freshness",
        "restart",
        "shutdown",
    ] {
        assert_eq!(
            template.pointer(&format!("/journey/{cell}/result")).and_then(Value::as_str),
            Some("not_proven"),
            "journey cell `{cell}` must fail closed"
        );
    }
    assert_eq!(
        template.pointer("/activation/pod/result").and_then(Value::as_str),
        Some("not_proven")
    );
    Ok(())
}

#[test]
fn driver_behaves_through_the_owned_python_contract() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let script = r#"
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path.cwd() / "scripts"))
from zed_host.prepare import HostReceiptError, _parse_perllsp_identity, _settings

assert _parse_perllsp_identity(
    "perllsp 0.18.0\nGit commit: abcdef1",
    "0.18.0",
    "abcdef1234567890abcdef1234567890abcdef12",
) == "abcdef1"

for output in ("", "perllsp 0.18.0\nGit commit: deadbee"):
    try:
        _parse_perllsp_identity(
            output,
            "0.18.0",
            "abcdef1234567890abcdef1234567890abcdef12",
        )
    except HostReceiptError:
        pass
    else:
        raise AssertionError("invalid perllsp identity was accepted")

settings = _settings({"trace": True}, Path("/tmp/perllsp"), "binary_override")
assert settings["languages"]["Perl"]["language_servers"] == [
    "perllsp",
    "!perlnavigator-server",
    "!perl-lsp",
    "...",
]
# The configured path is an opaque token, not a filesystem claim: _settings
# embeds `str(perllsp)`, so the expectation must go through the same Path->str
# conversion. On Windows str(Path("/tmp/perllsp")) is "\\tmp\\perllsp" while
# the raw environment token stays "/tmp/perllsp"; comparing one against the
# other makes this contract host-dependent.
expected_path = str(Path(os.environ["ZED_EXPECTED_PERLLSP_PATH"]))
assert settings["lsp"]["perllsp"]["binary"] == {
    "path": expected_path,
    "arguments": [],
}
assert settings["lsp"]["perllsp"]["settings"]["perl"] == {"trace": True}

path_settings = _settings({}, Path("/tmp/perllsp"), "worktree_path")
assert "binary" not in path_settings["lsp"]["perllsp"]
"#;
    let output = Command::new(python())
        .arg("-c")
        .arg(script)
        .env("ZED_EXPECTED_PERLLSP_PATH", PathBuf::from("/tmp/perllsp").to_string_lossy().as_ref())
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "Python contract test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn validator_cli_checks_schema_then_semantics() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template = root.join(".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json");
    let validator = env!("CARGO_BIN_EXE_validate-zed-host-receipt");

    let schema_only =
        Command::new(validator).arg("--schema-only").arg(&template).current_dir(&root).output()?;
    assert!(
        schema_only.status.success(),
        "schema-only validation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&schema_only.stdout),
        String::from_utf8_lossy(&schema_only.stderr)
    );

    let full = Command::new(validator).arg(&template).current_dir(&root).output()?;
    assert!(!full.status.success(), "not-run template must fail full semantic validation");
    Ok(())
}

#[test]
fn single_purpose_entry_points_parse_without_running_a_host() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    for script in [
        "scripts/zed_exact_source_prepare.py",
        "scripts/zed_exact_source_launch.py",
        "scripts/zed_exact_source_finalize.py",
    ] {
        let output = Command::new(python())
            .arg(root.join(script))
            .arg("--help")
            .current_dir(&root)
            .output()?;
        assert!(
            output.status.success(),
            "{script} --help failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output = Command::new(python())
        .arg("-m")
        .arg("py_compile")
        .args([
            "scripts/zed_exact_source_prepare.py",
            "scripts/zed_exact_source_launch.py",
            "scripts/zed_exact_source_finalize.py",
            "scripts/zed_host/common.py",
            "scripts/zed_host/prepare.py",
            "scripts/zed_host/process.py",
            "scripts/zed_host/finalize.py",
        ])
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "Python compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
