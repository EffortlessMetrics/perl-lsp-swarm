use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

#[test]
fn defaults_packet_preserves_provider_identity_and_order() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let manifest_text =
        fs::read_to_string(root.join(".ci/fixtures/zed-perl-upstream/zed-core/manifest.toml"))?;
    let manifest: toml::Value = toml::from_str(&manifest_text)?;
    let ordering: Vec<&str> = manifest
        .get("ordering")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("missing reviewed ordering"))?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(ordering, vec!["perlnavigator-server", "!perl-lsp", "!perllsp", "..."]);
    assert_eq!(
        manifest.get("base_commit").and_then(toml::Value::as_str),
        Some("7733b9922665f103abda7c6a3fde6b9dfdc8eba9")
    );
    assert_eq!(
        manifest.get("target_blob").and_then(toml::Value::as_str),
        Some("a03ad8874243f167e86deba8f975268eb384d20f")
    );

    let patch = fs::read_to_string(
        root.join(".ci/fixtures/zed-perl-upstream/zed-core/perl-defaults.patch"),
    )?;
    let exact =
        "\"language_servers\": [\"perlnavigator-server\", \"!perl-lsp\", \"!perllsp\", \"...\"]";
    assert_eq!(patch.matches(exact).count(), 1);
    assert!(patch.contains("\"!perl-lsp\""));
    assert!(patch.contains("\"!perllsp\""));
    assert!(!patch.contains("!perlnavigator-server"));
    assert_eq!(patch.matches("\"perl-lsp\"").count(), 0);
    assert_eq!(patch.matches("\"perllsp\"").count(), 0);

    Ok(())
}

#[test]
fn compatibility_and_submission_order_remain_unproven() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = fs::read_to_string(
        root.join(".ci/fixtures/zed-perl-upstream/zed-core/compatibility-matrix.v1.json"),
    )?;
    let matrix: Value = serde_json::from_str(&text)?;
    let rows = matrix
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("missing compatibility rows"))?;
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter().all(|row| row.get("observed").and_then(Value::as_str) == Some("not_proven"))
    );
    assert_eq!(
        matrix.pointer("/submission_order/status").and_then(Value::as_str),
        Some("unresolved_pending_actual_host")
    );

    Ok(())
}

#[test]
fn apply_script_fails_closed_on_external_subject_drift() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let script = fs::read_to_string(root.join("scripts/apply-zed-core-perl-defaults.sh"))?;
    assert!(script.contains("7733b9922665f103abda7c6a3fde6b9dfdc8eba9"));
    assert!(script.contains("a03ad8874243f167e86deba8f975268eb384d20f"));
    assert!(script.contains("status --porcelain"));
    assert!(script.contains("apply --check"));
    assert!(script.contains("diff --check"));
    Ok(())
}
