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
fn driver_uses_reviewed_zed_surfaces_and_bound_provider_identity() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let prepare = read(&root, "scripts/zed_host/prepare.py")?;
    let process = read(&root, "scripts/zed_host/process.py")?;
    let finalize = read(&root, "scripts/zed_host/finalize.py")?;
    let entry = read(&root, "scripts/zed_exact_source_prepare.py")?;

    for identity in ["perllsp", "!perlnavigator-server", "!perl-lsp", "..."] {
        assert!(prepare.contains(identity), "missing provider ordering `{identity}`");
    }
    assert!(prepare.contains("\"arguments\": []"));
    assert!(prepare.contains("zed::InstallDevExtension"));
    assert!(prepare.contains("require_clean_git_checkout"));
    assert!(prepare.contains("_parse_perllsp_identity"));
    assert!(prepare.contains("prepared_manifest_sha256"));
    assert!(entry.contains("binary_override"));
    assert!(!entry.contains("explicit_binary_path"));
    assert!(!prepare.contains("extensions/installed"));
    assert!(!prepare.contains("index.json"));

    for argument in ["--zed", "--foreground", "--wait", "--user-data-dir"] {
        assert!(process.contains(argument), "missing reviewed Zed argument `{argument}`");
    }
    assert!(process.contains("matching_processes(perllsp)"));
    assert!(process.contains("new_surviving_perllsp_pids"));
    assert!(process.contains("prepared_manifest_sha256"));
    assert!(process.contains("Zed exact-source host session exceeded the bounded timeout"));

    assert!(finalize.contains("exact-source-template.json"));
    assert!(finalize.contains("validate-zed-host-receipt"));
    assert!(finalize.contains("_require_run_binding"));
    assert!(finalize.contains("verify_artifact_reference"));
    assert!(finalize.contains("ignored=(\".git\",)"));
    assert!(finalize.contains("Exact-source development-extension evidence only"));
    assert!(!finalize.contains("public_registry_install"));
    assert!(!finalize.contains("managed_download"));
    Ok(())
}

#[test]
fn shared_rust_validator_checks_schema_then_semantics() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let validator = read(&root, "xtask/src/bin/validate-zed-host-receipt.rs")?;
    assert!(validator.contains("support/zed_host_compat.rs"));
    assert!(validator.contains("validate_schema(&receipt)"));
    assert!(validator.contains("validate_pass(&receipt, None)"));
    assert!(validator.contains("--schema-only"));
    assert!(!validator.contains("public_subject"));
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
