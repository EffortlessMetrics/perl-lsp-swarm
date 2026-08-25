// Integration test: assertion helpers (`expect`/`unwrap`/`panic!`) carry the
// failure message. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]
#[path = "support/zed_host_compat.rs"]
mod zed_host_compat;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zed_host_compat::{PUBLIC_SUBJECT_RELATIVE_PATH, content_sha256, exact_sha256, validate_pass};

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_json(root: &Path, relative: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(root.join(relative))?)?)
}

fn read_json_bytes(root: &Path, relative: &str) -> Result<(Value, Vec<u8>), Box<dyn Error>> {
    let bytes = fs::read(root.join(relative)).map_err(|error| -> Box<dyn Error> {
        format!("public subject read failed for {relative}: {error}").into()
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| -> Box<dyn Error> {
        format!("public subject parse failed for {relative}: {error}").into()
    })?;
    Ok((value, bytes))
}

fn sha256_fill(nibble: char) -> String {
    let mut value = String::with_capacity("sha256:".len() + 64);
    value.push_str("sha256:");
    for _ in 0..64 {
        value.push(nibble);
    }
    value
}

fn synthetic_published_subject(template: &Value) -> Value {
    let mut subject = template.clone();
    subject["status"] = Value::String("published".to_string());
    subject["registry"]["repository_commit"] =
        Value::String("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    subject["registry"]["extension_version"] = Value::String("0.5.0".to_string());
    subject["extension"]["manifest_version"] = Value::String("0.5.0".to_string());
    subject["extension"]["commit"] =
        Value::String("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string());
    subject["extension"]["package_identity"] = Value::String("perl@0.5.0".to_string());
    subject["zed_defaults"]["source_commit"] =
        Value::String("cccccccccccccccccccccccccccccccccccccccc".to_string());
    subject["zed_defaults"]["released_build"] = Value::String("zed-build-test".to_string());
    subject["perllsp_asset"]["release"] = Value::String("v0.0.0-test".to_string());
    subject["perllsp_asset"]["target"] = Value::String("x86_64-unknown-linux-musl".to_string());
    subject["perllsp_asset"]["asset_name"] = Value::String("perllsp".to_string());
    subject["perllsp_asset"]["asset_url"] =
        Value::String("https://example.test/perllsp".to_string());
    subject["perllsp_asset"]["asset_sha256"] = Value::String(sha256_fill('d'));
    subject["clean_profile"]["identity"] = Value::String("clean-profile-test".to_string());
    subject["clean_profile"]["prior_extension_absent"] = Value::Bool(true);
    subject["clean_profile"]["prior_managed_cache_absent"] = Value::Bool(true);
    subject["clean_profile"]["path_override_absent"] = Value::Bool(true);
    subject
}

fn synthetic_public_pass(template: &Value, subject: &Value, subject_sha256: &str) -> Value {
    let mut receipt = template.clone();
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-13T00:00:00Z".to_string());
    receipt["zed"]["version"] = Value::String("0.0.0-test".to_string());
    receipt["zed"]["channel"] = Value::String("stable".to_string());
    receipt["zed"]["build"] = subject["zed_defaults"]["released_build"].clone();
    receipt["extension"]["base_commit"] =
        Value::String("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string());
    receipt["extension"]["candidate_commit"] = subject["extension"]["commit"].clone();
    receipt["extension"]["manifest_version"] = subject["extension"]["manifest_version"].clone();
    receipt["extension"]["wasm_sha256"] = Value::String(sha256_fill('f'));
    receipt["perllsp"]["command"] = Value::String("/tmp/perllsp".to_string());
    receipt["perllsp"]["version"] = Value::String("0.0.0-test".to_string());
    receipt["perllsp"]["build_commit"] =
        Value::String("ffffffffffffffffffffffffffffffffffffffff".to_string());
    receipt["perllsp"]["binary_sha256"] = subject["perllsp_asset"]["asset_sha256"].clone();
    receipt["perllsp"]["resolution_route"] = Value::String("managed_download".to_string());
    receipt["platform"]["os"] = Value::String("linux".to_string());
    receipt["platform"]["version"] = Value::String("test".to_string());
    receipt["platform"]["architecture"] = Value::String("x86_64".to_string());
    receipt["profile"]["clean_profile"] = Value::Bool(true);
    receipt["profile"]["prior_extension_absent"] = Value::Bool(true);
    receipt["profile"]["prior_managed_cache_absent"] = Value::Bool(true);
    receipt["profile"]["other_perl_servers_disabled"] = Value::Bool(true);
    receipt["workspace"]["fixture_id"] = Value::String("fixture-test".to_string());
    receipt["workspace"]["fixture_sha256"] = Value::String(sha256_fill('1'));
    receipt["workspace"]["root_identity"] = Value::String("root-test".to_string());
    receipt["configuration"]["settings_sha256"] = Value::String(sha256_fill('2'));
    receipt["configuration"]["workspace_configuration_observed"] = Value::Bool(true);
    receipt["configuration"]["precedence_observed"] = Value::String("observed".to_string());
    receipt["configuration"]["live_update_observed"] = Value::String("observed".to_string());
    for cell in ["pl", "pm", "t", "PL", "psgi", "cgi", "fcgi", "shebang", "pod"] {
        receipt["activation"][cell] =
            serde_json::json!({"result": "pass", "evidence": format!("{cell}-evidence")});
    }
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
        receipt["journey"][cell] =
            serde_json::json!({"result": "pass", "evidence": format!("{cell}-evidence")});
    }
    receipt["artifacts"]["zed_log"] = Value::String("zed.log".to_string());
    receipt["artifacts"]["language_server_log"] = Value::String("perllsp.log".to_string());
    receipt["artifacts"]["process_inventory"] = Value::String("procs.txt".to_string());
    receipt["artifacts"]["redacted"] = Value::Bool(true);
    receipt["public_subject"] = serde_json::json!({
        "relative_path": PUBLIC_SUBJECT_RELATIVE_PATH,
        "sha256": subject_sha256,
    });
    receipt
}

#[test]
fn public_template_uses_the_official_registry_stage() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let receipt =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/public-registry-template.json")?;
    assert_eq!(
        receipt.get("evidence_stage").and_then(Value::as_str),
        Some("public_registry_install"),
        "public template must use the official-registry evidence stage"
    );
    assert_eq!(
        receipt.get("result").and_then(Value::as_str),
        Some("not_run"),
        "public template must remain not_run"
    );
    assert_eq!(
        receipt.pointer("/extension/install_route").and_then(Value::as_str),
        Some("official_registry"),
        "public template must use official_registry install route"
    );
    assert_eq!(
        receipt.pointer("/perllsp/server_id").and_then(Value::as_str),
        Some("perllsp"),
        "public template must bind the perllsp server id"
    );
    assert_eq!(
        receipt.pointer("/perllsp/arguments"),
        Some(&serde_json::json!(["--stdio"])),
        "public template must require stdio-only arguments"
    );
    assert_eq!(
        receipt.pointer("/public_subject/relative_path").and_then(Value::as_str),
        Some(PUBLIC_SUBJECT_RELATIVE_PATH),
        "public template must bind the canonical public subject path"
    );
    assert!(
        receipt.pointer("/public_subject/sha256").is_some(),
        "public template must declare the public_subject.sha256 field"
    );
    assert!(
        receipt.pointer("/public_subject/sha256").and_then(Value::as_str).is_none(),
        "public template digest must remain null until a published subject exists"
    );
    Ok(())
}

#[test]
fn public_subject_cannot_invent_publication_or_promotion() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let (subject, bytes) = match read_json_bytes(root.as_path(), PUBLIC_SUBJECT_RELATIVE_PATH) {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("public subject read failed")
                    || message.contains("public subject parse failed"),
                "unexpected subject load failure: {message}"
            );
            return Err(error);
        }
    };
    assert_eq!(subject.get("status").and_then(Value::as_str), Some("blocked_pending_publication"));
    assert_eq!(subject.pointer("/registry/extension_id").and_then(Value::as_str), Some("perl"));
    assert_eq!(
        subject.pointer("/registry/submodule_path").and_then(Value::as_str),
        Some("extensions/perl")
    );
    for cell in ["registry_row", "managed_download_row", "path_row", "documentation_projection"] {
        assert_eq!(
            subject.pointer(&format!("/promotion/{cell}")).and_then(Value::as_str),
            Some("not_proven")
        );
    }
    assert_eq!(content_sha256(&bytes).len(), "sha256:".len() + 64);
    Ok(())
}

#[test]
fn managed_and_path_routes_remain_distinct() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let (subject, bytes) = match read_json_bytes(root.as_path(), PUBLIC_SUBJECT_RELATIVE_PATH) {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("public subject read failed")
                    || message.contains("public subject parse failed"),
                "unexpected subject load failure: {message}"
            );
            return Err(error);
        }
    };
    assert_eq!(
        subject.pointer("/promotion/managed_download_row").and_then(Value::as_str),
        Some("not_proven"),
        "managed-download promotion must stay not_proven until a live receipt exists"
    );
    assert_eq!(
        subject.pointer("/promotion/path_row").and_then(Value::as_str),
        Some("not_proven"),
        "path-override promotion must stay not_proven until a live receipt exists"
    );
    assert!(
        subject.pointer("/perllsp_asset/asset_sha256").is_some(),
        "subject must reserve the managed-download asset digest field"
    );
    assert!(
        subject.pointer("/clean_profile/path_override_absent").is_some(),
        "subject must reserve the clean-profile path-override field"
    );
    assert!(
        exact_sha256(&serde_json::json!({"digest": content_sha256(&bytes)}), "/digest"),
        "subject bytes must hash to a content-addressed sha256 digest"
    );
    Ok(())
}

#[test]
fn public_subject_read_rejects_absent_path_with_exact_message() {
    let root = repo_root().expect("repo root");
    let error = read_json_bytes(
        root.as_path(),
        ".ci/fixtures/zed-perl-upstream/receipts/does-not-exist.v1.json",
    )
    .expect_err("missing public subject must fail closed");
    let message = error.to_string();
    assert!(
        message.starts_with("public subject read failed for "),
        "expected exact read failure prefix, got: {message}"
    );
    assert!(
        message.contains("does-not-exist.v1.json"),
        "expected missing path in error, got: {message}"
    );
}

#[test]
fn public_pass_requires_managed_download_and_clean_prior_state() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/public-registry-template.json")?;
    let subject_template = read_json(&root, PUBLIC_SUBJECT_RELATIVE_PATH)?;
    let subject = synthetic_published_subject(&subject_template);
    let subject_bytes = serde_json::to_vec_pretty(&subject)?;
    let subject_sha = content_sha256(subject_bytes.as_slice());
    let mut receipt = synthetic_public_pass(&template, &subject, &subject_sha);
    let bound = Some(subject_bytes.as_slice());

    receipt["perllsp"]["resolution_route"] = Value::String("binary_override".to_string());
    assert_eq!(
        validate_pass(&receipt, bound).expect_err("binary override"),
        "public registry pass requires perllsp resolution_route=managed_download"
    );

    receipt["perllsp"]["resolution_route"] = Value::String("worktree_path".to_string());
    assert_eq!(
        validate_pass(&receipt, bound).expect_err("worktree path"),
        "public registry pass requires perllsp resolution_route=managed_download"
    );

    receipt["perllsp"]["resolution_route"] = Value::String("managed_download".to_string());
    receipt["profile"]["prior_extension_absent"] = Value::Bool(false);
    assert_eq!(
        validate_pass(&receipt, bound).expect_err("prior extension present"),
        "public registry pass requires a clean profile without prior extension or managed cache"
    );

    receipt["profile"]["prior_extension_absent"] = Value::Bool(true);
    receipt["profile"]["prior_managed_cache_absent"] = Value::Null;
    assert_eq!(
        validate_pass(&receipt, bound).expect_err("prior cache null"),
        "public registry pass requires a clean profile without prior extension or managed cache"
    );
    Ok(())
}

#[test]
fn public_pass_binds_content_addressed_subject() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/public-registry-template.json")?;
    let subject_template = read_json(&root, PUBLIC_SUBJECT_RELATIVE_PATH)?;
    let subject = synthetic_published_subject(&subject_template);
    let subject_bytes = serde_json::to_vec_pretty(&subject)?;
    let subject_sha = content_sha256(subject_bytes.as_slice());
    let mut receipt = synthetic_public_pass(&template, &subject, &subject_sha);

    assert_eq!(
        validate_pass(&receipt, None).expect_err("missing subject"),
        "public registry pass requires the bound public subject"
    );

    let blocked = subject_template.clone();
    let blocked_bytes = serde_json::to_vec_pretty(&blocked)?;
    assert_eq!(
        validate_pass(&receipt, Some(blocked_bytes.as_slice())).expect_err("blocked subject"),
        "public subject is not published"
    );

    receipt["public_subject"]["sha256"] = Value::String(sha256_fill('0'));
    assert_eq!(
        validate_pass(&receipt, Some(subject_bytes.as_slice())).expect_err("mismatched digest"),
        "public_subject.sha256 does not match the bound subject bytes"
    );

    receipt["public_subject"]["sha256"] = Value::String(subject_sha.clone());
    receipt["extension"]["candidate_commit"] =
        Value::String("0123456789012345678901234567890123456789".to_string());
    assert_eq!(
        validate_pass(&receipt, Some(subject_bytes.as_slice())).expect_err("mismatched commit"),
        "public receipt identities disagree with the bound public subject"
    );

    receipt["extension"]["candidate_commit"] = subject["extension"]["commit"].clone();
    assert_eq!(
        validate_pass(&receipt, Some(subject_bytes.as_slice())),
        Ok(()),
        "matching published subject bytes must validate"
    );
    Ok(())
}

#[test]
fn public_subject_loader_fails_closed_on_missing_and_invalid_bytes() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let missing = read_json_bytes(
        root.as_path(),
        ".ci/fixtures/zed-perl-upstream/receipts/does-not-exist-public-subject.json",
    );
    let missing_message = missing.expect_err("missing subject path").to_string();
    assert!(
        missing_message.contains("public subject read failed"),
        "missing subject must fail on the read path: {missing_message}"
    );

    let relative = ".tmp-zed-public-subject-invalid.json";
    let in_repo = root.join(relative);
    fs::write(&in_repo, b"{not-json")?;
    let invalid = read_json_bytes(root.as_path(), relative);
    let _ = fs::remove_file(&in_repo);
    let invalid_message = invalid.expect_err("invalid subject json").to_string();
    assert!(
        invalid_message.contains("public subject parse failed"),
        "invalid subject must fail on the parse path: {invalid_message}"
    );
    Ok(())
}
