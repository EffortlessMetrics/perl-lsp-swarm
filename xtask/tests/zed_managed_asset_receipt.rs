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

fn assert_rejected(
    output: &std::process::Output,
    context: &str,
    expected_fragment: &str,
) -> Result<(), Box<dyn Error>> {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !output.status.success(),
        "{context} should have been rejected, but the command succeeded\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains(expected_fragment),
        "{context} should have failed with an error containing {expected_fragment:?}, but stderr was:\n{stderr}"
    );
    Ok(())
}

#[test]
fn checked_projection_and_not_run_template_validate_offline() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    assert_success(
        &run(&root, &["validate-contract", "--contract", CONTRACT])?,
        "contract validation",
    )?;
    assert_success(
        &run(&root, &["validate-receipt", "--receipt", TEMPLATE, "--contract", CONTRACT])?,
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
    assert_rejected(
        &wrong_contract_output,
        "wrong-product contract mutation control",
        "perllsp identity",
    )?;

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
            "--contract",
            CONTRACT,
        ],
    )?;
    assert_rejected(
        &overclaim_output,
        "Zed-overclaim receipt mutation control",
        "Zed host or registry proof",
    )?;
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
    let contract = read("scripts/zed_assets/contract.py")?;

    assert!(
        github.contains("application/octet-stream"),
        "asset downloads must request raw octet-stream bytes"
    );
    assert!(
        github.contains(".partial"),
        "downloads must land in a .partial file before the atomic replace"
    );
    assert!(
        producer.contains("sha256_file(archive_path)"),
        "producer must hash the downloaded archive bytes independently"
    );
    assert!(
        producer.contains("contract_stale"),
        "producer must fail closed when the live release no longer matches the contract"
    );
    assert!(
        producer.contains("managed_extracted_not_executed"),
        "producer must keep cross-host extraction distinct from execution"
    );

    assert!(
        archive.contains("validate_relative_member"),
        "every archive member name must be validated before use"
    );
    assert!(archive.contains("duplicate archive member"), "duplicate members must be rejected");
    assert!(
        archive.contains("archive links are not accepted"),
        "symlink and hardlink members must be rejected"
    );
    assert!(
        !archive.contains("extractall("),
        "archive extraction must never use extractall; only the exact member is copied"
    );
    assert!(
        archive.contains("unexpected code-intelligence executable"),
        "a foreign code-intelligence executable must be rejected"
    );
    assert!(
        archive.contains("noncanonical archive name"),
        "a selected member whose raw name differs from the expected member must be rejected"
    );
    assert!(
        archive.contains("malformed tar.gz archive"),
        "a corrupt tar.gz must become a ReceiptError, not a traceback"
    );
    assert!(
        archive.contains("malformed zip archive"),
        "a corrupt zip must become a ReceiptError, not a traceback"
    );
    assert!(
        common.contains("not path.parts"),
        "relative member validation must reject empty, absolute, and traversing paths"
    );
    assert!(
        common.contains("single relative path component"),
        "contract fields used as filesystem segments must be single relative components"
    );

    assert!(
        framing.contains("Content-Length"),
        "stdio framing must be parsed through exact Content-Length headers"
    );
    assert!(
        framing.contains("not strict ASCII"),
        "a non-ASCII protocol header must fail as a ReceiptError"
    );
    assert!(
        process.contains("[str(binary), \"--version\"]"),
        "the smoke must run the extracted binary with --version"
    );
    assert!(
        process.contains("[str(binary), \"--stdio\"]"),
        "the smoke must run the extracted binary with --stdio"
    );
    assert!(
        process.contains("expected_version not in version_output"),
        "the reported binary version must match the expected release version"
    );
    assert!(
        process.contains("perllsp --version timed out"),
        "a --version timeout must fail as a ReceiptError"
    );
    assert!(
        process.contains("process inventory grew after shutdown"),
        "new surviving perllsp processes must fail the smoke"
    );
    assert!(
        process.contains("did not observe the launched perllsp process"),
        "cleanup must fail closed when the launched process cannot be observed"
    );
    assert!(validation.contains("actual_zed"), "receipt validation must keep actual Zed unproven");
    assert!(
        validation.contains("public_registry"),
        "receipt validation must keep the public registry unproven"
    );
    assert!(
        validation.contains("proven_for_matching_host_only"),
        "host proof must stay bound to the matching host only"
    );
    assert!(
        validation.contains("sha256_file(contract_path)"),
        "receipt validation must recompute the checked contract digest"
    );
    assert!(
        validation.contains("no managed target evidence"),
        "a passing receipt without managed rows must be rejected"
    );
    assert!(
        contract.contains("validate_single_component"),
        "contract targets and asset names must be constrained to single path components"
    );
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
