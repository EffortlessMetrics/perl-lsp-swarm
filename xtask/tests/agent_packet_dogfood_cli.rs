//! Executable CLI coverage for the agent-packet dogfood report and validation paths.

use assert_cmd::Command;
use color_eyre::eyre::{Context, Result, eyre};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

const DISPOSITIONS: &[&str] = &["started", "completed", "refused", "transferred", "not_proven"];
const FIXTURE: &str = "fixtures/agent_packet_dogfood_core/parser_p05_synthetic.v1.json";

fn fixture_document() -> Result<Value> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| eyre!("xtask manifest has no workspace parent"))?
        .to_path_buf();
    let text = fs::read_to_string(root.join(FIXTURE)).context("reading dogfood fixture")?;
    serde_json::from_str(&text).context("parsing dogfood fixture")
}

fn write_and_stamp(temp: &TempDir, name: &str, disposition: &str) -> Result<PathBuf> {
    let mut document = fixture_document()?;
    document["disposition"] = Value::String(disposition.to_string());
    let path = temp.path().join(name);
    fs::write(&path, serde_json::to_vec_pretty(&document)?).context("writing test manifest")?;

    let output = Command::cargo_bin("xtask")?
        .args(["agent-dogfood", "stamp", "--manifest"])
        .arg(&path)
        .output()?;
    if !output.status.success() {
        return Err(eyre!(
            "stamping {name} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(path)
}

fn write_without_stamping(temp: &TempDir, name: &str, disposition: &str) -> Result<PathBuf> {
    let mut document = fixture_document()?;
    document["disposition"] = Value::String(disposition.to_string());
    write_document(temp, name, document)
}

fn write_document(temp: &TempDir, name: &str, document: Value) -> Result<PathBuf> {
    let path = temp.path().join(name);
    fs::write(&path, serde_json::to_vec_pretty(&document)?).context("writing test manifest")?;
    Ok(path)
}

fn report_args(format: &str, paths: &[PathBuf]) -> Vec<String> {
    let mut args = vec![
        "agent-dogfood".to_string(),
        "report".to_string(),
        "--format".to_string(),
        format.to_string(),
    ];
    for path in paths {
        args.push("--manifest".to_string());
        args.push(path.to_string_lossy().into_owned());
    }
    args
}

#[test]
fn report_cli_reaches_both_formats_for_all_closed_dispositions() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let mut paths = Vec::new();
    for disposition in DISPOSITIONS {
        paths.push(write_and_stamp(&temp, &format!("{disposition}.json"), disposition)?);
    }

    for format in ["markdown", "json"] {
        let output = Command::cargo_bin("xtask")?.args(report_args(format, &paths)).output()?;
        if !output.status.success() {
            return Err(eyre!(
                "{format} report failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let stdout = String::from_utf8(output.stdout)?;
        if format == "markdown" {
            for disposition in DISPOSITIONS {
                assert!(stdout.contains(&format!("| {disposition} | valid |")));
            }
        } else {
            let report: Value = serde_json::from_str(&stdout).context("parsing JSON report")?;
            assert_eq!(report["report"], "agent-packet-dogfood.core.report.v1");
            let dispositions: BTreeSet<&str> = report["runs"]
                .as_array()
                .ok_or_else(|| eyre!("JSON report runs is not an array"))?
                .iter()
                .filter_map(|run| run["disposition"].as_str())
                .collect();
            let expected: BTreeSet<&str> = DISPOSITIONS.iter().copied().collect();
            assert_eq!(dispositions, expected);
        }
    }
    Ok(())
}

#[test]
fn validate_cli_redacts_unknown_disposition_values() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let leaked = "api_key=hunter2";
    let path = write_without_stamping(&temp, "invalid.json", leaked)?;
    let output = Command::cargo_bin("xtask")?
        .args(["agent-dogfood", "validate", "--manifest"])
        .arg(path)
        .output()?;

    assert!(!output.status.success(), "unknown disposition must fail validation");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("unknown_disposition"), "missing reason code: {stderr}");
    assert!(!stderr.contains(leaked), "validation CLI leaked the unknown value: {stderr}");
    Ok(())
}

#[test]
fn validate_cli_redacts_untrusted_structural_diagnostic_values() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let leaked = "api_key=hunter2";

    let mut invalid_kind = fixture_document()?;
    invalid_kind["events"][0]["kind"] = json!(leaked);
    let kind_path = write_document(&temp, "invalid-kind.json", invalid_kind)?;

    let mut invalid_role = fixture_document()?;
    invalid_role["human_intervention"][0]["role"] = json!(leaked);
    let role_path = write_document(&temp, "invalid-role.json", invalid_role)?;

    let mut invalid_field = fixture_document()?;
    invalid_field[leaked] = json!(true);
    let field_path = write_document(&temp, "invalid-field.json", invalid_field)?;

    let leaked_path = "C:/Users/dev/api_key=hunter2";
    let mut invalid_path_key = fixture_document()?;
    invalid_path_key[leaked_path] = json!(true);
    let path_key_path = write_document(&temp, "invalid-path-key.json", invalid_path_key)?;

    for (path, code) in [
        (&kind_path, "unknown_record_kind"),
        (&role_path, "unknown_intervention_role"),
        (&field_path, "unknown_field"),
        (&path_key_path, "unknown_field"),
    ] {
        let output = Command::cargo_bin("xtask")?
            .args(["agent-dogfood", "validate", "--manifest"])
            .arg(path)
            .output()?;
        assert!(!output.status.success(), "{code} must fail validation");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(code), "missing reason code {code}: {stderr}");
        assert!(!stderr.contains(leaked), "validation CLI leaked {leaked}: {stderr}");
    }
    let output = Command::cargo_bin("xtask")?
        .args(["agent-dogfood", "validate", "--manifest"])
        .arg(&path_key_path)
        .output()?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        !stderr.contains(leaked_path),
        "validation CLI leaked diagnostic path {leaked_path}: {stderr}"
    );
    Ok(())
}

#[test]
fn validate_cli_redacts_credential_keys_from_hygiene_diagnostics() -> Result<()> {
    let temp = tempfile::tempdir()?;

    for key in ["APIKey", "TOKEN"] {
        let mut invalid = fixture_document()?;
        invalid[key] = json!({"lease": true});
        // Keep the caller-supplied path neutral: validation output includes the
        // path, while the assertion targets diagnostic redaction of the JSON key.
        let path = write_document(&temp, "invalid-hygiene.json", invalid)?;
        let output = Command::cargo_bin("xtask")?
            .args(["agent-dogfood", "validate", "--manifest"])
            .arg(path)
            .output()?;

        assert!(!output.status.success(), "{key} must fail validation");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("credential_in_payload"), "missing credential reason: {stderr}");
        assert!(
            stderr.contains("mutable_state_embedded"),
            "missing mutable-state reason: {stderr}"
        );
        assert!(!stderr.contains(key), "validation CLI leaked credential key {key}: {stderr}");
    }
    Ok(())
}

#[test]
fn validate_cli_does_not_echo_caller_controlled_manifest_paths() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let leaked = "api_key=hunter2";
    let mut invalid = fixture_document()?;
    invalid["metadata"] = json!({"api_key": "hunter2"});
    let path = write_document(&temp, "api_key=hunter2.json", invalid)?;
    let output = Command::cargo_bin("xtask")?
        .args(["agent-dogfood", "validate", "--manifest"])
        .arg(&path)
        .output()?;

    assert!(!output.status.success(), "credential manifest must fail validation");
    let stderr = String::from_utf8(output.stderr)?;
    let path_text = path.to_string_lossy().into_owned();
    assert!(stderr.contains("manifest[0]"), "missing bounded manifest label: {stderr}");
    assert!(!stderr.contains(leaked), "validation CLI leaked path content: {stderr}");
    assert!(!stderr.contains(&path_text), "validation CLI echoed manifest path: {stderr}");

    let missing = temp.path().join("api_key=hunter2-missing.json");
    let output = Command::cargo_bin("xtask")?
        .args(["agent-dogfood", "validate", "--manifest"])
        .arg(&missing)
        .output()?;
    assert!(!output.status.success(), "missing manifest must fail validation");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("failed to read caller-supplied manifest"),
        "missing generic read error: {stderr}"
    );
    let missing_text = missing.to_string_lossy().into_owned();
    assert!(!stderr.contains("api_key=hunter2"), "read error leaked manifest path: {stderr}");
    assert!(!stderr.contains(&missing_text), "read error echoed manifest path: {stderr}");
    Ok(())
}
