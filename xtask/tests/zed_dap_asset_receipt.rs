//! Static falsifier surface for the Zed perl-dap public asset receipt matrix
//! (#9516, train sidecar phase `public_perl_dap_asset_receipts`).
//!
//! The executable matrix runs against live public bytes through
//! `scripts/zed_dap_asset_receipts.py execute-dap`; this test binds the
//! offline contract: the checked DAP projection validates against the live
//! tree, the not-run template stays not-run, every identity-collapse and
//! overclaim mutation fails closed with the exact field named, the offline
//! known-good cache recovery suite passes, and the implementation keeps the
//! product, member, cache, and proof boundaries that separation requires.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json";
const TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/dap-asset-template.json";
const EXECUTED_RECEIPT: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/dap-asset-windows-x86_64.v1.json";
const SCRIPT: &str = "scripts/zed_dap_asset_receipts.py";

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
        "{context} should have failed naming {expected_fragment:?}, but stderr was:\n{stderr}"
    );
    Ok(())
}

fn load_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn write_temp(target: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn contract_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn checked_projection_not_run_template_and_cache_recovery_validate_offline()
-> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    assert_success(
        &run(&root, &["validate-dap-contract", "--contract", CONTRACT, "--bind-repo-root"])?,
        "contract validation",
    )?;
    assert_success(
        &run(&root, &["validate-dap-receipt", "--receipt", TEMPLATE, "--contract", CONTRACT])?,
        "template validation",
    )?;

    // The committed matching-host receipt stays current against the checked
    // contract: any contract, release, or binding drift that invalidates it
    // fails here until a fresh matrix run replaces the fixture.
    assert_success(
        &run(
            &root,
            &["validate-dap-receipt", "--receipt", EXECUTED_RECEIPT, "--contract", CONTRACT],
        )?,
        "executed receipt validation",
    )?;

    let committed = load_json(&root.join(EXECUTED_RECEIPT))?;
    assert_eq!(
        committed.pointer("/result").and_then(Value::as_str),
        Some("pass"),
        "the committed matching-host receipt must be a passing observation"
    );
    assert_eq!(
        committed.pointer("/claim_boundary/dap_process").and_then(Value::as_str),
        Some("proven_for_matching_host_only")
    );
    let verifier_os = committed.pointer("/verifier/os").and_then(Value::as_str);
    let verifier_arch = committed.pointer("/verifier/architecture").and_then(Value::as_str);
    assert_eq!((verifier_os, verifier_arch), (Some("windows"), Some("x86_64")));
    let executed: Vec<&str> = committed["targets"]
        .as_array()
        .ok_or_else(|| io::Error::other("executed receipt lacks targets"))?
        .iter()
        .filter(|row| row.get("result").and_then(Value::as_str) == Some("managed_executed"))
        .filter_map(|row| row.get("target").and_then(Value::as_str))
        .collect();
    assert_eq!(
        executed,
        vec!["x86_64-pc-windows-msvc"],
        "exactly the verifier-matching row may appear executed"
    );
    let executed_row = committed["targets"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("result").and_then(Value::as_str) == Some("managed_executed"))
        })
        .cloned()
        .ok_or_else(|| io::Error::other("committed receipt lacks an executed row"))?;
    let smoke_version = executed_row
        .pointer("/stdio_smoke/version_output")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(
        smoke_version, "perl-dap 0.17.0",
        "the executed row must record the exact canonical perl-dap version line"
    );
    let work_dir = root.join("target/zed-dap-receipt-contract-tests/cache");
    let _ = fs::remove_dir_all(&work_dir);
    assert_success(
        &run(&root, &["dap-cache-recovery", "--work-dir", &work_dir.to_string_lossy()])?,
        "cache recovery suite",
    )?;
    Ok(())
}

#[test]
fn contract_mutations_fail_closed_naming_the_exact_defect() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/zed-dap-receipt-contract-tests");
    fs::create_dir_all(&target)?;
    let original = load_json(&root.join(CONTRACT))?;

    let mutations: [(String, Value, &str, &str); 7] = [
        (
            "perllsp product substitution".into(),
            json!("perllsp"),
            "/identity/executable",
            "perl-dap executable identity",
        ),
        (
            "private perl-dap asset family".into(),
            json!("perl-dap-0.17.0-x86_64-unknown-linux-musl.tar.gz"),
            "/targets/0/asset_name",
            "shared perllsp- release family",
        ),
        (
            "perllsp archive member".into(),
            json!("perllsp-0.17.0-x86_64-unknown-linux-musl/perllsp"),
            "/targets/0/archive_member",
            "perllsp member can never satisfy",
        ),
        (
            "root-level windows member".into(),
            json!("perl-dap.exe"),
            "/targets/4/archive_member",
            "canonical nested perl-dap member",
        ),
        (
            "prerelease substitution".into(),
            json!(true),
            "/source/prerelease",
            "cannot target a prerelease",
        ),
        (
            "topology binding drift".into(),
            json!("sha256:1111111111111111111111111111111111111111111111111111111111111111"),
            "/bindings/topology/sha256",
            "drifted",
        ),
        (
            "silent projection divergence removal".into(),
            Value::Null,
            "/projection_divergence",
            "silent gap",
        ),
    ];

    for (label, value, pointer, fragment) in mutations {
        let mut mutated = original.clone();
        if value.is_null() {
            *mutated
                .pointer_mut(pointer)
                .ok_or_else(|| io::Error::other(format!("pointer {pointer} missing")))? =
                Value::Null;
        } else {
            *mutated
                .pointer_mut(pointer)
                .ok_or_else(|| io::Error::other(format!("pointer {pointer} missing")))? = value;
        }
        let path = target.join(format!("contract-{}.json", label.replace([' ', '-'], "_")));
        write_temp(&path, &mutated)?;
        let output = run(
            &root,
            &["validate-dap-contract", "--contract", &contract_arg(&path), "--bind-repo-root"],
        )?;
        assert_rejected(&output, &format!("contract mutation: {label}"), fragment)?;
    }

    // Dropping the explicitly-unsupported Windows ARM64 row entirely must
    // also fail closed: the matrix has no silent gaps.
    let mut dropped = original.clone();
    let targets = dropped
        .pointer_mut("/targets")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("contract lacks targets"))?;
    let had_unsupported = targets
        .iter()
        .any(|row| row.get("target").and_then(Value::as_str) == Some("aarch64-pc-windows-msvc"));
    assert!(had_unsupported, "checked contract must carry the Windows ARM64 row");
    targets
        .retain(|row| row.get("target").and_then(Value::as_str) != Some("aarch64-pc-windows-msvc"));
    let dropped_path = target.join("contract-dropped-arm64.json");
    write_temp(&dropped_path, &dropped)?;
    let output = run(
        &root,
        &["validate-dap-contract", "--contract", &contract_arg(&dropped_path), "--bind-repo-root"],
    )?;
    assert_rejected(
        &output,
        "dropped Windows ARM64 row",
        "Windows ARM64 must remain explicitly unsupported",
    )?;
    Ok(())
}

#[test]
fn receipt_mutations_fail_closed_on_overclaim_and_stale_subjects() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/zed-dap-receipt-contract-tests");
    fs::create_dir_all(&target)?;
    let template = load_json(&root.join(TEMPLATE))?;
    let contract = load_json(&root.join(CONTRACT))?;

    // A fabricated passing receipt that quotes the real contract digest and
    // every contract-exact asset/release/binding identity, so only the
    // cross-build defect distinguishes it from honest evidence: a windows
    // verifier can never satisfy an executed linux row.
    let linux_row = contract["targets"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("target").and_then(Value::as_str) == Some("x86_64-unknown-linux-musl")
            })
        })
        .cloned()
        .ok_or_else(|| io::Error::other("contract lacks the linux musl row"))?;
    let scenarios: Vec<Value> = std::iter::repeat_n(
        json!({"scenario": "x", "known_good_preserved": true, "detail": "x"}),
        18,
    )
    .collect();
    let mut fake_pass = template.clone();
    fake_pass["result"] = json!("pass");
    fake_pass["contract"]["sha256"] =
        json!("sha256:3333333333333333333333333333333333333333333333333333333333333333");
    fake_pass["release"] = json!({
        "repository": contract["source"]["repository"],
        "id": contract["source"]["release_id"],
        "tag": contract["source"]["tag"],
        "version": contract["source"]["version"],
        "prerelease": false,
        "draft": false,
        "published_at": "2026-06-28T21:22:46Z",
        "producer": contract["source"]["producer"],
    });
    fake_pass["topology"] = json!({
        "subject": contract["bindings"]["topology"]["path"],
        "sha256": contract["bindings"]["topology"]["sha256"],
    });
    fake_pass["projection"] = json!({
        "zed_adapter_subject": contract["bindings"]["zed_adapter_projection"]["path"],
        "sha256": contract["bindings"]["zed_adapter_projection"]["sha256"],
        "debug_adapter_id": contract["bindings"]["zed_adapter_projection"]["debug_adapter_id"],
    });
    fake_pass["verifier"] =
        json!({"os": "windows", "version": "10", "architecture": "x86_64", "python": "3"});
    fake_pass["claim_boundary"]["dap_process"] = json!("proven_for_matching_host_only");
    fake_pass["claim_boundary"]["cache_recovery"] = json!("proven_isolated_cache_model_only");
    fake_pass["cache_recovery"] = json!({
        "result": "pass",
        "known_good_before": {"version": "9.9.9"},
        "selected_after": {"version": "9.9.9"},
        "scenario_results": scenarios,
        "limitations": [],
    });
    fake_pass["targets"] = json!([{
        "target": "x86_64-unknown-linux-musl", "os": "linux", "architecture": "x86_64",
        "disposition": "managed", "result": "managed_executed",
        "asset": {
            "id": linux_row["asset_id"], "name": linux_row["asset_name"],
            "sha256": linux_row["asset_digest"], "archive_type": linux_row["archive_type"],
        },
        "archive": {
            "members_sha256": "sha256:5555555555555555555555555555555555555555555555555555555555555555",
            "required_member": linux_row["archive_member"],
            "installed_path": linux_row["installed_path"],
            "safe": true,
        },
        "binary": {"product": "perl-dap", "sha256": linux_row["member_sha256"]},
        "stdio_smoke": {"result": "pass", "stdout_pure": true, "orphan_result": "no_orphans"},
        "errors": [],
    }]);
    let fake_path = target.join("receipt-fake-pass.json");
    write_temp(&fake_path, &fake_pass)?;
    let output = run(
        &root,
        &["validate-dap-receipt", "--receipt", &contract_arg(&fake_path), "--contract", CONTRACT],
    )?;
    assert_rejected(
        &output,
        "fabricated passing receipt",
        "does not match the checked perl-dap contract",
    )?;

    // Same receipt, now quoting the real checked contract digest: the only
    // remaining defect is the cross-build execution claim.
    use sha2::{Digest, Sha256};
    let contract_bytes = fs::read(root.join(CONTRACT))?;
    let digest = format!(
        "sha256:{}",
        Sha256::digest(&contract_bytes).iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    fake_pass["contract"]["sha256"] = json!(digest);
    write_temp(&fake_path, &fake_pass)?;
    let output = run(
        &root,
        &["validate-dap-receipt", "--receipt", &contract_arg(&fake_path), "--contract", CONTRACT],
    )?;
    assert_rejected(&output, "cross-build executed row", "cross-built")?;

    // The committed honest receipt must reject a single tampered asset digest
    // even though every other field still matches: the row is bound to the
    // contract's exact bytes, not to its shape.
    let committed = load_json(&root.join(EXECUTED_RECEIPT))?;
    let mut tampered = committed.clone();
    let tamper_index = committed["targets"]
        .as_array()
        .and_then(|rows| {
            rows.iter().position(|row| {
                row.get("result").and_then(Value::as_str) == Some("managed_executed")
            })
        })
        .ok_or_else(|| io::Error::other("committed receipt lacks an executed row"))?;
    tampered["targets"][tamper_index]["asset"]["sha256"] =
        json!("sha256:7777777777777777777777777777777777777777777777777777777777777777");
    let tampered_path = target.join("receipt-tampered-asset.json");
    write_temp(&tampered_path, &tampered)?;
    let output = run(
        &root,
        &[
            "validate-dap-receipt",
            "--receipt",
            &contract_arg(&tampered_path),
            "--contract",
            CONTRACT,
        ],
    )?;
    assert_rejected(&output, "tampered asset digest", "asset identity")?;

    // A cache-recovery block that silently drops a scenario fails closed.
    let mut dropped = committed.clone();
    dropped["cache_recovery"]["scenario_results"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("committed receipt lacks scenarios"))?
        .pop();
    let dropped_path = target.join("receipt-dropped-scenario.json");
    write_temp(&dropped_path, &dropped)?;
    let output = run(
        &root,
        &[
            "validate-dap-receipt",
            "--receipt",
            &contract_arg(&dropped_path),
            "--contract",
            CONTRACT,
        ],
    )?;
    assert_rejected(&output, "dropped cache scenario", "scenario set")?;

    // A cache-recovery row that flips one preservation result fails closed.
    let mut flipped = committed.clone();
    flipped["cache_recovery"]["scenario_results"][0]["known_good_preserved"] = json!(false);
    let flipped_path = target.join("receipt-flipped-scenario.json");
    write_temp(&flipped_path, &flipped)?;
    let output = run(
        &root,
        &[
            "validate-dap-receipt",
            "--receipt",
            &contract_arg(&flipped_path),
            "--contract",
            CONTRACT,
        ],
    )?;
    assert_rejected(&output, "flipped cache scenario", "is not preserved")?;

    // A perllsp product identity inside the receipt binary block is rejected.
    fake_pass["targets"][0]["binary"]["product"] = json!("perllsp");
    write_temp(&fake_path, &fake_pass)?;
    let output = run(
        &root,
        &["validate-dap-receipt", "--receipt", &contract_arg(&fake_path), "--contract", CONTRACT],
    )?;
    assert_rejected(&output, "perllsp product receipt row", "exact perl-dap product")?;

    // Zed overclaim fails closed even on the not-run template.
    let mut overclaim = template.clone();
    overclaim["claim_boundary"]["actual_zed"] = json!("proven");
    let overclaim_path = target.join("receipt-zed-overclaim.json");
    write_temp(&overclaim_path, &overclaim)?;
    let output = run(
        &root,
        &[
            "validate-dap-receipt",
            "--receipt",
            &contract_arg(&overclaim_path),
            "--contract",
            CONTRACT,
        ],
    )?;
    assert_rejected(&output, "Zed-overclaim receipt", "actual_zed")?;
    Ok(())
}

#[test]
fn template_cannot_be_mistaken_for_an_executed_receipt() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let receipt = load_json(&root.join(TEMPLATE))?;
    assert_eq!(receipt.pointer("/result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(receipt.pointer("/stage").and_then(Value::as_str), Some("public_perl_dap_asset"));
    assert_eq!(
        receipt.pointer("/claim_boundary/asset_bytes").and_then(Value::as_str),
        Some("not_run")
    );
    for cell in ["real_zed_debug_session", "actual_zed", "public_registry"] {
        assert_eq!(
            receipt.pointer(&format!("/claim_boundary/{cell}")).and_then(Value::as_str),
            Some("not_proven"),
            "template cell {cell} must stay not_proven"
        );
    }
    assert!(
        receipt.pointer("/cache_recovery").is_none_or(Value::is_null),
        "template must not carry cache recovery evidence"
    );
    Ok(())
}

#[test]
fn implementation_binds_the_dap_matrix_boundaries() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let read = |relative: &str| -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(root.join(relative))?)
    };

    let dap_process = read("scripts/zed_assets/dap_process.py")?;
    let dap_archive = read("scripts/zed_assets/dap_archive.py")?;
    let dap_cache = read("scripts/zed_assets/dap_cache.py")?;
    let dap_producer = read("scripts/zed_assets/dap_producer.py")?;
    let dap_validation = read("scripts/zed_assets/dap_validation.py")?;
    let dap_contract = read("scripts/zed_assets/dap_contract.py")?;

    // The DAP smoke executes the exact public binary twice: --version and
    // --stdio, proves the exact canonical version line, and asserts the full
    // ORDERED DAP lifecycle with protocol-only stdout.
    assert!(dap_process.contains("[str(binary), \"--version\"]"));
    assert!(dap_process.contains("[str(binary), \"--stdio\"]"));
    assert!(dap_process.contains("perl-dap --version timed out"));
    assert!(
        dap_process.contains("version output does not identify perl-dap"),
        "a perllsp-only version output must never satisfy the DAP row"
    );
    assert!(
        dap_process.contains("canonical = f\"perl-dap {expected_version}\""),
        "the version proof must require the exact canonical perl-dap version line"
    );
    assert!(
        dap_process.contains("is not the exact canonical line"),
        "a prefix or suffix version substitution must fail closed"
    );
    assert!(
        dap_process.contains("expected_frames"),
        "the lifecycle proof must assert the exact frame transcript"
    );
    assert!(
        dap_process.contains("partial order is violated"),
        "an out-of-order DAP transcript must fail closed"
    );
    assert!(
        dap_process.contains("does not end with the terminated event"),
        "the terminated event must close the transcript"
    );
    assert!(dap_process.contains("DAP initialize response did not echo request_seq 1"));
    assert!(dap_process.contains("perl-dap process inventory grew after disconnect"));
    assert!(dap_process.contains("did not observe the launched perl-dap process"));
    assert!(dap_process.contains("orphan_result"));
    assert!(dap_process.contains("configuration_boundary"));

    // The shared archive family scan keeps both products but rejects any
    // foreign or ambiguous executable — including mode-executable members
    // with unknown names — never extractall()s, and cross-checks the
    // in-archive checksum manifest.
    assert!(dap_archive.contains("SHARED_BINARY_NAMES"));
    assert!(dap_archive.contains("_reject_foreign_executable"));
    assert!(
        dap_archive.contains("unexpected executable member"),
        "a mode-executable foreign member must be rejected before safe=true"
    );
    assert!(dap_archive.contains("ambiguous perl-dap member"));
    assert!(dap_archive.contains("duplicate archive member"));
    assert!(dap_archive.contains("archive links are not accepted"));
    assert!(!dap_archive.contains("extractall("));
    assert!(dap_archive.contains("SHA256SUMS.txt"));
    assert!(
        dap_archive.contains("in-archive checksum manifest disagrees"),
        "the in-archive sums authority must be enforced"
    );

    // The managed cache boundary is debugger-specific, swaps through a
    // retire-and-rollback promote, and preserves known-good state through
    // every failure class.
    assert!(dap_cache.contains("perl-dap-managed-"));
    assert!(dap_cache.contains("current.json"));
    assert!(dap_cache.contains("EXPECTED_SCENARIOS"));
    assert!(dap_cache.contains("missing_asset"));
    assert!(dap_cache.contains("wrong_product_member"));
    assert!(dap_cache.contains("foreign_executable_member"));
    assert!(dap_cache.contains("protocol_impurity"));
    assert!(dap_cache.contains("promote_failure"));
    assert!(
        dap_cache.contains(".retired"),
        "the incumbent must be retired, not deleted, before promotion"
    );
    assert!(
        dap_cache.contains("os.rename(retired, version_dir)"),
        "a failed promote must roll the incumbent back into place"
    );
    assert!(dap_cache.contains("cleanup_stays_inside_the_perl_dap_family"));
    assert!(
        dap_cache.contains("perllsp-0.17.0-x86_64-unknown-linux-musl/perllsp"),
        "the cleanup fixture must plant a language-server cache that must survive"
    );

    // The producer cross-checks two independent digest authorities, keeps
    // cross-host extraction distinct from execution, and bounds the cache
    // proof to the isolated model.
    assert!(dap_producer.contains("_verify_consolidated_sums"));
    assert!(dap_producer.contains("contract_stale"));
    assert!(dap_producer.contains("managed_extracted_not_executed"));
    assert!(dap_producer.contains("sha256_file(archive_path)"));
    assert!(
        dap_producer.contains("proven_isolated_cache_model_only"),
        "the cache proof must not claim the production Zed downloader"
    );

    // The receipt validator binds every row to the contract's exact subjects,
    // rejects cross-build execution, recomputes the full scenario denominator,
    // and keeps higher stages unproven.
    assert!(dap_validation.contains("cross-built"));
    assert!(
        dap_validation.contains("does not match the checked contract"),
        "receipt asset and release identities must be bound to the contract"
    );
    assert!(
        dap_validation.contains("scenario set"),
        "the cache-recovery scenario denominator must be recomputed"
    );
    assert!(
        dap_validation.contains("is not preserved"),
        "a flipped cache scenario result must fail closed"
    );
    assert!(
        dap_validation.contains("isolated model"),
        "the cache proof boundary must stay bounded to the isolated model"
    );
    assert!(dap_validation.contains("real_zed_debug_session"));
    assert!(dap_validation.contains("perl-dap-managed-"));

    // The contract validator enforces the identity separation in both
    // directions: no perllsp member, no private asset family, no root-level
    // Windows member.
    assert!(dap_contract.contains("perllsp member can never satisfy"));
    assert!(dap_contract.contains("private perl-dap- asset family"));
    assert!(dap_contract.contains("canonical nested perl-dap member"));
    assert!(dap_contract.contains("silent gap"));
    Ok(())
}

#[test]
fn perllsp_authority_surface_stays_product_separated() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;

    // The perllsp discriminators this lane must not weaken stay byte-exact:
    // the checked perllsp contract and its validator still exist and the DAP
    // modules never import the perllsp producer/validator in reverse.
    assert!(root.join(".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json").is_file());
    assert!(root.join("scripts/zed_assets/contract.py").is_file());
    let dap_validation = fs::read_to_string(root.join("scripts/zed_assets/dap_validation.py"))?;
    assert!(
        !dap_validation.contains("from .validation import"),
        "the perl-dap validator must not delegate to the perllsp validator"
    );
    let dap_contract = fs::read_to_string(root.join("scripts/zed_assets/dap_contract.py"))?;
    assert!(
        !dap_contract.contains("from .contract import"),
        "the perl-dap contract must not delegate to the perllsp contract"
    );
    Ok(())
}
