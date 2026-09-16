use anyhow::{Result, ensure};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const CLEAN_WORKFLOW: &str = "on: push\npermissions: read-all\njobs:\n  check:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: echo selected\n";

fn invoke(binary: &Path, cwd: &Path, root: &Path, receipt: &Path) -> Result<Output> {
    Ok(Command::new(binary)
        .current_dir(cwd)
        .arg("workflow-policy-lint")
        .arg("--root")
        .arg(root)
        .arg("--receipt")
        .arg(receipt)
        .output()?)
}

fn read_receipt(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

#[test]
fn copied_workflow_policy_binary_uses_explicit_subject_not_cwd_or_build_root() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let binary = temporary.path().join(format!("relocated-xtask{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_xtask"), &binary)?;
    let unrelated = temporary.path().join("unrelated-cwd");
    fs::create_dir(&unrelated)?;
    let root = temporary.path().join("selected");
    let workflows = root.join(".github/workflows");
    fs::create_dir_all(&workflows)?;
    let workflow = workflows.join("selected.yml");
    fs::write(&workflow, CLEAN_WORKFLOW.replace("read-all", "write-all"))?;
    let receipt = temporary.path().join("receipt.json");
    let output = invoke(&binary, &unrelated, &root, &receipt)?;
    ensure!(!output.status.success(), "explicit violating subject passed");
    let failed = read_receipt(&receipt)?;
    ensure!(failed["passed"] == false, "receipt cleared the selected violation");
    ensure!(failed["subject"]["workflow_file_count"] == 1, "wrong file inventory scanned");
    ensure!(failed["subject"]["selection"] == "explicit_root", "explicit root ignored");
    ensure!(failed["subject"]["scan_completed"] == true, "finding confused with input failure");
    ensure!(
        failed["issues"]
            .as_array()
            .is_some_and(|issues| issues.iter().any(|issue| issue["workflow"] == "selected.yml"
                && issue["code"] == "WRITE_ALL_PERMISSIONS")),
        "selected workflow finding missing"
    );
    fs::write(&workflow, CLEAN_WORKFLOW)?;
    let output = invoke(&binary, &unrelated, &root, &receipt)?;
    ensure!(
        output.status.success(),
        "clean selected subject failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let passed = read_receipt(&receipt)?;
    ensure!(passed["passed"] == true && passed["error_count"] == 0, "clean result missing");
    ensure!(
        passed["subject"]["path_identity_sha256"] == failed["subject"]["path_identity_sha256"],
        "same selected root changed identity"
    );
    ensure!(
        passed["subject"]["path_identity_sha256"].as_str().is_some_and(|value| value.len() == 64),
        "root identity absent"
    );
    let output = invoke(&binary, &unrelated, &temporary.path().join("missing"), &receipt)?;
    ensure!(!output.status.success(), "missing selected root passed");
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("resolving selected workflow-policy root"),
        "missing root diagnostic lost"
    );
    let unavailable = read_receipt(&receipt)?;
    ensure!(
        unavailable["passed"] == false && unavailable["subject"]["scan_completed"] == false,
        "stale success survived failed invocation"
    );
    Ok(())
}

#[test]
fn workflow_policy_fixture_receipt_and_cli_conflicts_are_explicit() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let fixture = temporary.path().join("fixture.yml");
    let receipt = temporary.path().join("fixture-receipt.json");
    fs::write(&fixture, CLEAN_WORKFLOW)?;
    let binary = Path::new(env!("CARGO_BIN_EXE_xtask"));
    let output = Command::new(binary)
        .current_dir(temporary.path())
        .arg("workflow-policy-lint")
        .arg("--fixture")
        .arg(&fixture)
        .arg("--receipt")
        .arg(&receipt)
        .output()?;
    ensure!(
        output.status.success(),
        "fixture invocation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = read_receipt(&receipt)?;
    ensure!(evidence["subject"]["mode"] == "fixture", "fixture receipt claims repository proof");
    ensure!(evidence["subject"]["workflow_file_count"] == 1, "fixture file count missing");
    ensure!(
        evidence["subject"]["isolation_registry_requested"] == false,
        "fixture claims repository isolation check"
    );
    for argument in ["--root", "--check-lane-whitelist"] {
        let mut command = Command::new(binary);
        command.arg("workflow-policy-lint").arg("--fixture").arg(&fixture).arg(argument);
        if argument == "--root" {
            command.arg(temporary.path());
        }
        let output = command.output()?;
        ensure!(!output.status.success(), "ambiguous fixture option {argument} accepted");
        ensure!(
            String::from_utf8_lossy(&output.stderr).contains("cannot be used with"),
            "CLI did not reject conflicting subjects"
        );
    }
    Ok(())
}

#[test]
fn workflow_policy_receipt_write_failure_is_not_success() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let receipt_directory = temporary.path().join("not-a-receipt-file");
    fs::create_dir(&receipt_directory)?;
    let output = invoke(
        Path::new(env!("CARGO_BIN_EXE_xtask")),
        temporary.path(),
        &temporary.path().join("missing-root"),
        &receipt_directory,
    )?;
    ensure!(!output.status.success(), "unwritable receipt accepted");
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("writing receipt"),
        "receipt write diagnostic missing"
    );
    Ok(())
}
