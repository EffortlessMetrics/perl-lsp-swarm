use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json";
const TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/managed-asset-template.json";
const SCRIPT: &str = "scripts/zed_public_asset_receipts.py";

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

fn run(root: &Path, arguments: &[&str]) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(python()).arg(root.join(SCRIPT)).args(arguments).current_dir(root).output()?)
}

fn assert_success(output: &std::process::Output, context: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

#[test]
fn checked_projection_and_not_run_template_validate_offline() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    assert_success(
        &run(&root, &["validate-contract", "--contract", CONTRACT])?,
        "contract validation",
    )?;
    assert_success(
        &run(&root, &["validate-receipt", "--receipt", TEMPLATE])?,
        "template validation",
    )?;
    Ok(())
}

#[test]
fn mutation_controls_reject_wrong_identity_and_zed_overclaim() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/zed-receipt-contract-tests");
    fs::create_dir_all(&target)?;

    let contract_text = fs::read_to_string(root.join(CONTRACT))?;
    let mut contract: Value = serde_json::from_str(&contract_text)?;
    contract["identity"]["executable"] = Value::String("perl-lsp".to_string());
    let wrong_contract = target.join("wrong-product.json");
    fs::write(&wrong_contract, serde_json::to_vec_pretty(&contract)?)?;
    let wrong_contract_output = run(
        &root,
        &[
            "validate-contract",
            "--contract",
            wrong_contract
                .to_str()
                .ok_or_else(|| io::Error::other("wrong contract path is not UTF-8"))?,
        ],
    )?;
    assert!(!wrong_contract_output.status.success());

    let template_text = fs::read_to_string(root.join(TEMPLATE))?;
    let mut template: Value = serde_json::from_str(&template_text)?;
    template["claim_boundary"]["actual_zed"] = Value::String("proven".to_string());
    let overclaim = target.join("zed-overclaim.json");
    fs::write(&overclaim, serde_json::to_vec_pretty(&template)?)?;
    let overclaim_output = run(
        &root,
        &[
            "validate-receipt",
            "--receipt",
            overclaim.to_str().ok_or_else(|| io::Error::other("overclaim path is not UTF-8"))?,
        ],
    )?;
    assert!(!overclaim_output.status.success());
    Ok(())
}

#[test]
fn implementation_binds_bytes_archive_process_and_cross_build_boundaries()
-> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let read = |relative: &str| -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(root.join(relative))?)
    };

    let github = read("scripts/zed_assets/github_io.py")?;
    let archive = read("scripts/zed_assets/archive.py")?;
    let common = read("scripts/zed_assets/common.py")?;
    let framing = read("scripts/zed_assets/framing.py")?;
    let process = read("scripts/zed_assets/process.py")?;
    let producer = read("scripts/zed_assets/producer.py")?;
    let validation = read("scripts/zed_assets/validation.py")?;

    assert!(github.contains("application/octet-stream"));
    assert!(github.contains(".partial"));
    assert!(producer.contains("sha256_file(archive_path)"));
    assert!(producer.contains("contract_stale"));
    assert!(producer.contains("managed_extracted_not_executed"));

    assert!(archive.contains("validate_relative_member"));
    assert!(archive.contains("duplicate archive member"));
    assert!(archive.contains("archive links are not accepted"));
    assert!(!archive.contains("extractall("));
    assert!(archive.contains("unexpected code-intelligence executable"));
    assert!(common.contains("not path.parts"));

    assert!(framing.contains("Content-Length"));
    assert!(process.contains("[str(binary), \"--version\"]"));
    assert!(process.contains("[str(binary), \"--stdio\"]"));
    assert!(process.contains("process inventory grew after shutdown"));
    assert!(validation.contains("actual_zed"));
    assert!(validation.contains("public_registry"));
    assert!(validation.contains("proven_for_matching_host_only"));
    Ok(())
}

#[test]
fn template_cannot_be_mistaken_for_an_executed_receipt() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = fs::read_to_string(root.join(TEMPLATE))?;
    let receipt: Value = serde_json::from_str(&text)?;
    assert_eq!(receipt.pointer("/result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(
        receipt.pointer("/claim_boundary/asset_bytes").and_then(Value::as_str),
        Some("not_run")
    );
    assert_eq!(
        receipt.pointer("/claim_boundary/actual_zed").and_then(Value::as_str),
        Some("not_proven")
    );
    assert_eq!(
        receipt.pointer("/claim_boundary/public_registry").and_then(Value::as_str),
        Some("not_proven")
    );
    Ok(())
}
