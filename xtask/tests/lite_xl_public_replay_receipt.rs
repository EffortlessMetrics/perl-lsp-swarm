//! Static falsifier surface for the Lite XL public-artifact replay journey
//! receipt (#9012, consuming the landed #11178 spec ledger as data).
//!
//! The public replay runs through a real released Lite XL host once the
//! external subjects exist; this suite binds the offline authority: the
//! committed receipt is a `blocked_external` observation whose gates are
//! live-bound to the committed upstream-acceptance manifest, the landed
//! #11178 ledger bytes, and the committed exact-source receipts directory.
//! Every identity-collapse, overclaim, substitution, stale-subject, and
//! orphan mutation on a synthetic pass fails closed with the exact defect
//! named, so the public journey cannot be rendered green by relabeling a
//! lower evidence stage.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const RECEIPT: &str = ".ci/fixtures/lite-xl-perl-upstream/receipts/public-replay.v1.json";
const ACCEPTANCE_MANIFEST: &str = ".ci/fixtures/lite-xl-perl-upstream/upstream-acceptance.toml";
const LEDGER: &str = ".spec/11178-lite-xl-bdd-journeys/acceptance.md";
const SCRIPT: &str = "scripts/lite_xl_public_replay.py";
const DEFAULT_RECEIPTS_DIR: &str = ".ci/fixtures/lite-xl-perl-upstream/receipts";

const PUBLISHED_MANIFEST: &str = r#"schema_version = "lite-xl-upstream-acceptance.v1"
issue = 9012
parent = 8950
ready = true

[hosts.lite_xl]
state = "released"
product = "Lite XL"
version = "2.1.0-rel-test"
released_build = "lite-xl-build-test"

[packages.lite_xl_lsp]
state = "released"
version = "0.2.3"
ref = "v0.2.3"
sha256 = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[packages.language_perl]
state = "released"
version = "1.5.0"
ref = "v1.5.0"
sha256 = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[packages.lsp_perl]
state = "released"
version = "1.1.0"
ref = "v1.1.0"
sha256 = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

[server.perllsp]
state = "released"
release_repository = "EffortlessMetrics/perl-lsp"
release_tag = "v9.9.9-test"
asset_name = "perllsp-9.9.9-test-x86_64-pc-windows-msvc.zip"
asset_sha256 = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
member_sha256 = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
release_version = "9.9.9-test"

[validation]
host_release_contains_changes = true
package_versions_match_refs = true
server_asset_digest_verified = true
"#;

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

fn run_with_receipts_dir(
    root: &Path,
    receipt: &Path,
    manifest: &Path,
    receipts_dir: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(python())
        .arg(root.join(SCRIPT))
        .arg("validate-public-replay-receipt")
        .arg("--receipt")
        .arg(receipt)
        .arg("--ledger")
        .arg(root.join(LEDGER))
        .arg("--acceptance-manifest")
        .arg(manifest)
        .arg("--receipts-dir")
        .arg(receipts_dir)
        .current_dir(root)
        .output()?)
}

fn run(
    root: &Path,
    receipt: &Path,
    manifest: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_with_receipts_dir(root, receipt, manifest, DEFAULT_RECEIPTS_DIR)
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
    // A bounded receipt failure exits 2 from the CLI's ReceiptError handler;
    // an unhandled traceback would exit 1 instead.
    assert_eq!(
        output.status.code(),
        Some(2),
        "{context} must fail with the bounded receipt-error status\nstdout:\n{stdout}\nstderr:\n{stderr}"
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

fn write_temp(target: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    // A bare-filename target would hand `create_dir_all` an empty parent
    // path; skip directory creation for that degenerate case.
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, text)?;
    Ok(())
}

/// A synthetic passing replay built on the exact committed bindings: the
/// ledger digest is quoted from the committed blocked receipt (so it matches
/// the real ledger bytes), every identity follows the synthetic published
/// upstream-acceptance manifest, and the public perllsp identity is the one
/// that manifest records.
fn pass_control(committed: &Value) -> Result<Value, Box<dyn Error>> {
    let mut receipt = committed.clone();
    receipt["result"] = json!("pass");
    for cell in [
        "exact_source_lite_xl_receipt",
        "released_lite_xl_build",
        "public_lite_xl_lsp_package_release",
        "public_language_perl_package_release",
        "public_lsp_perl_package_release",
        "public_perllsp_release_asset",
    ] {
        receipt["gates"][cell] = json!("current");
    }
    receipt["host"] = json!({
        "product": "Lite XL",
        "version": "2.1.0-rel-test",
        "build": "lite-xl-build-test",
    });
    receipt["platform"] = json!({"os": "windows", "architecture": "x86_64"});
    receipt["packages"] = json!({
        "lite_xl_lsp": {
            "ref": "v0.2.3",
            "sha256": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "version": "0.2.3",
        },
        "language_perl": {
            "ref": "v1.5.0",
            "sha256": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "version": "1.5.0",
        },
        "lsp_perl": {
            "ref": "v1.1.0",
            "sha256": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "version": "1.1.0",
        },
    });
    receipt["server"] = json!({
        "install_route": "manual_public_install",
        "installed_path": "user/packages/perllsp-9.9.9-test/perllsp.exe",
        "process_path": "C:/Users/t/data/lite-xl/user/packages/perllsp-9.9.9-test/perllsp.exe",
        "process_argv": ["perllsp", "--stdio"],
        "version_output": "perllsp 9.9.9-test",
        "binary_sha256": "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
    });
    let digest = committed.pointer("/ledger_sha256").cloned().unwrap_or_default();
    receipt["workspace"] = json!({
        "fixture_id": "fixture-test",
        "fixture_sha256": digest,
        "root_identity": "root-test",
    });
    receipt["configuration"] = json!({
        "config_sha256": digest,
        "driver_sha256": digest,
        "instrument_sha256": digest,
    });
    for cell in receipt["journey"]
        .as_object()
        .map(|it| it.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        receipt["journey"][&cell] =
            json!({"result": "pass", "evidence": format!("{cell}-evidence")});
    }
    receipt["discriminators"] = json!({
        "wrong_root_same_basename_rejected": true,
        "second_perl_server_absent": true,
        "ambient_path_satisfies_no_row": true,
        "managed_row_not_satisfied_by_ambient_path": true,
    });
    receipt["cleanup"] = json!({"adapter_orphans": [], "debuggee_orphans": []});
    Ok(receipt)
}

/// Materialize a synthetic receipts directory holding one `exact-source*.json`
/// receipt with the given evidence stage and result, for controls that need a
/// committed exact-source pass: the exact-source gate is live-bound to the
/// receipts directory the validator sees.
fn receipts_dir_with_exact_source(
    target: &Path,
    name: &str,
    evidence_stage: &str,
    result: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let dir = target.join(name);
    fs::create_dir_all(&dir)?;
    let receipt = json!({
        "schema_version": "lite_xl_host_compat.v1",
        "evidence_stage": evidence_stage,
        "result": result,
    });
    write_temp(&dir.join("exact-source-synthetic.json"), &serde_json::to_string_pretty(&receipt)?)?;
    Ok(dir)
}

#[test]
fn committed_receipt_is_an_honest_blocked_external_observation() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let committed = load_json(&root.join(RECEIPT))?;
    assert_eq!(
        committed.pointer("/schema_version").and_then(Value::as_str),
        Some("lite_xl_public_artifact_replay_receipt.v1")
    );
    assert_eq!(
        committed.pointer("/stage").and_then(Value::as_str),
        Some("released_public_artifact"),
        "the journey stage is released_public_artifact, never an exact-source or staged stage"
    );
    assert_eq!(
        committed.pointer("/result").and_then(Value::as_str),
        Some("blocked_external"),
        "the committed receipt must stay blocked while the external subjects are absent"
    );
    for cell in [
        "released_lite_xl_build",
        "public_lite_xl_lsp_package_release",
        "public_language_perl_package_release",
        "public_lsp_perl_package_release",
        "public_perllsp_release_asset",
        "exact_source_lite_xl_receipt",
    ] {
        assert_eq!(
            committed.pointer(&format!("/gates/{cell}")).and_then(Value::as_str),
            Some("absent"),
            "gate {cell} must record the absent external subject"
        );
    }
    let blockers = committed
        .pointer("/gates/blockers")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("committed receipt lacks gates.blockers"))?;
    assert!(blockers.len() >= 4, "blockers must name the exact-source and released-subject gaps");
    for cell in committed["journey"]
        .as_object()
        .map(|it| it.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        assert_eq!(
            committed.pointer(&format!("/journey/{cell}/result")).and_then(Value::as_str),
            Some("not_proven"),
            "blocked journey cell {cell} must fail closed"
        );
    }
    assert_eq!(
        committed.pointer("/claim_boundary/lsp_support_rows").and_then(Value::as_str),
        Some("unchanged"),
        "no LSP support row is changed by this stage"
    );
    // The committed receipt binds exactly the landed #11178 scenario set:
    // 73 baseline + 8 optional scenarios as of train head 61b689077.
    assert_eq!(
        committed["journey"].as_object().map(|it| it.len()),
        Some(81),
        "the blocked receipt must carry every landed ledger row"
    );
    assert_success(
        &run(&root, Path::new(RECEIPT), Path::new(ACCEPTANCE_MANIFEST))?,
        "committed blocked receipt validation",
    )?;
    Ok(())
}

#[test]
fn blocked_gates_are_live_bound_to_the_current_surfaces() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/lite-xl-public-replay-tests/blocked-gates");
    fs::create_dir_all(&target)?;
    let committed = load_json(&root.join(RECEIPT))?;
    let real_manifest = root.join(ACCEPTANCE_MANIFEST);
    let published = target.join("manifest-published.toml");
    write_temp(&published, PUBLISHED_MANIFEST)?;

    // When the acceptance manifest records merged-and-released subjects, the
    // committed blocked receipt is stale and must be regenerated: the gate
    // accounting is a live binding, not prose.
    let output = run(&root, Path::new(RECEIPT), &published)?;
    assert_rejected(&output, "blocked receipt vs published subject", "cannot deny it")?;

    // A blocked receipt cannot claim subjects the real manifest denies.
    for cell in [
        "released_lite_xl_build",
        "public_perllsp_release_asset",
        "public_lsp_perl_package_release",
    ] {
        let mut lying = committed.clone();
        lying["gates"][cell] = json!("current");
        let path = target.join(format!("blocked-lying-{cell}.json"));
        write_temp(&path, &serde_json::to_string_pretty(&lying)?)?;
        let output = run(&root, &path, &real_manifest)?;
        assert_rejected(
            &output,
            &format!("lying gate {cell}"),
            "cannot claim a subject the acceptance manifest does not record",
        )?;
    }

    // A blocked receipt cannot claim an exact-source receipt that no
    // committed fixture records.
    let mut claiming = committed.clone();
    claiming["gates"]["exact_source_lite_xl_receipt"] = json!("current");
    let path = target.join("blocked-claiming-exact-source.json");
    write_temp(&path, &serde_json::to_string_pretty(&claiming)?)?;
    let output = run(&root, &path, &real_manifest)?;
    assert_rejected(&output, "claiming exact-source gate", "no committed fixture records")?;

    // The exact-source gate counts only genuine exact-source receipts: a
    // pass-shaped file caught by the glob but carrying another evidence stage
    // never satisfies the gate.
    let wrong_stage_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-wrong-stage",
        "staged_managed_package",
        "pass",
    )?;
    let output = run_with_receipts_dir(
        &root,
        &path,
        &real_manifest,
        wrong_stage_receipts.to_string_lossy().as_ref(),
    )?;
    assert_rejected(
        &output,
        "claiming exact-source gate vs wrong-stage pass file",
        "no committed fixture records",
    )?;

    // Conversely, once a genuine exact-source pass is committed, a blocked
    // receipt can no longer deny it.
    let current_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-current",
        "exact_source_dev_extension",
        "pass",
    )?;
    let output = run_with_receipts_dir(
        &root,
        Path::new(RECEIPT),
        &real_manifest,
        current_receipts.to_string_lossy().as_ref(),
    )?;
    assert_rejected(&output, "blocked receipt vs committed exact-source pass", "cannot deny it")?;

    // Blockers are load-bearing: an empty list hides the absent subjects.
    let mut unblocked = committed.clone();
    unblocked["gates"]["blockers"] = json!([]);
    let path = target.join("blocked-no-blockers.json");
    write_temp(&path, &serde_json::to_string_pretty(&unblocked)?)?;
    let output = run(&root, &path, &real_manifest)?;
    assert_rejected(&output, "empty blockers", "must name its absent external subjects")?;

    // A non-passing receipt cannot claim a proven journey cell.
    let first_cell = committed["journey"]
        .as_object()
        .and_then(|it| it.keys().next().cloned())
        .ok_or_else(|| io::Error::other("committed receipt has no journey cells"))?;
    let mut overclaim = committed.clone();
    overclaim["journey"][&first_cell] = json!({"result": "pass", "evidence": "relabeled"});
    let path = target.join("blocked-journey-overclaim.json");
    write_temp(&path, &serde_json::to_string_pretty(&overclaim)?)?;
    let output = run(&root, &path, &real_manifest)?;
    assert_rejected(&output, "blocked journey overclaim", "cannot claim a proven journey cell")?;

    // A non-passing result cannot escape the live gate binding: relabeling
    // the committed receipt to any other non-passing result keeps the
    // stale-subject check, so a released upstream still invalidates it.
    for relabeled_result in ["not_proven", "not_run", "fail", "instrument_failed", "contract_stale"]
    {
        let mut escaped = committed.clone();
        escaped["result"] = json!(relabeled_result);
        let path = target.join(format!("relabeled-{relabeled_result}.json"));
        write_temp(&path, &serde_json::to_string_pretty(&escaped)?)?;
        let output = run_with_receipts_dir(
            &root,
            &path,
            &published,
            current_receipts.to_string_lossy().as_ref(),
        )?;
        assert_rejected(
            &output,
            &format!("gate accounting after {relabeled_result} relabel"),
            "cannot deny it",
        )?;
    }
    Ok(())
}

#[test]
fn pass_journey_requires_an_accepted_upstream_subject() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/lite-xl-public-replay-tests/pass-control");
    fs::create_dir_all(&target)?;
    let committed = load_json(&root.join(RECEIPT))?;
    let control = pass_control(&committed)?;
    let published = target.join("manifest-published.toml");
    write_temp(&published, PUBLISHED_MANIFEST)?;
    let real_manifest = root.join(ACCEPTANCE_MANIFEST);

    let control_path = target.join("pass-control.json");
    write_temp(&control_path, &serde_json::to_string_pretty(&control)?)?;
    let current_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-current",
        "exact_source_dev_extension",
        "pass",
    )?;

    // The control passes only against an accepted merged-and-released
    // upstream subject plus a committed exact-source pass (whose gate is
    // live-bound to the receipts directory).
    assert_success(
        &run_with_receipts_dir(
            &root,
            &control_path,
            &published,
            current_receipts.to_string_lossy().as_ref(),
        )?,
        "synthetic pass control validation",
    )?;

    // The same control without any committed exact-source pass fails closed.
    let output = run(&root, &control_path, &published)?;
    assert_rejected(
        &output,
        "pass control without a committed exact-source pass",
        "no committed fixture records one",
    )?;

    // The same receipt against the real (still blocked) manifest is a
    // fabricated public journey and must fail closed.
    let output = run_with_receipts_dir(
        &root,
        &control_path,
        &real_manifest,
        current_receipts.to_string_lossy().as_ref(),
    )?;
    assert_rejected(
        &output,
        "pass control vs blocked manifest",
        "requires an accepted merged-and-released upstream subject",
    )?;

    // Relabeling an unrelated family's receipt shape as this journey cannot
    // work either.
    let mut foreign = committed.clone();
    foreign["schema_version"] = json!("zed_perl_dap_public_registry_receipt.v1");
    let path = target.join("foreign-schema.json");
    write_temp(&path, &serde_json::to_string_pretty(&foreign)?)?;
    let output = run(&root, &path, &published)?;
    assert_rejected(
        &output,
        "foreign schema as public replay",
        "unexpected lite-xl public replay receipt schema",
    )?;
    Ok(())
}

#[test]
fn pass_mutations_fail_closed_naming_the_exact_defect() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/lite-xl-public-replay-tests/pass-mutations");
    fs::create_dir_all(&target)?;
    let committed = load_json(&root.join(RECEIPT))?;
    let control = pass_control(&committed)?;
    let published = target.join("manifest-published.toml");
    write_temp(&published, PUBLISHED_MANIFEST)?;
    let current_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-current",
        "exact_source_dev_extension",
        "pass",
    )?;
    let receipts = current_receipts.to_string_lossy().into_owned();

    let mutations: Vec<(String, Value, &str, &str)> = vec![
        (
            "exact-source stage relabel".into(),
            json!("exact_source_dev_extension"),
            "/stage",
            "must name the released_public_artifact stage",
        ),
        (
            "staged-managed stage relabel".into(),
            json!("staged_managed_package"),
            "/stage",
            "must name the released_public_artifact stage",
        ),
        (
            "source checkout install route".into(),
            json!("source_checkout"),
            "/server/install_route",
            "developer shortcut install_route",
        ),
        (
            "worktree candidate install route".into(),
            json!("worktree_candidate"),
            "/server/install_route",
            "developer shortcut install_route",
        ),
        (
            "cargo target install route".into(),
            json!("cargo_target"),
            "/server/install_route",
            "developer shortcut install_route",
        ),
        (
            "PATH override install route".into(),
            json!("path_override"),
            "/server/install_route",
            "developer shortcut install_route",
        ),
        (
            "unknown install route".into(),
            json!("carrier_pigeon"),
            "/server/install_route",
            "unknown install_route",
        ),
        (
            "tampered binary digest".into(),
            json!("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            "/server/binary_sha256",
            "must equal the selected public asset member identity",
        ),
        (
            "ambient PATH binary substitution".into(),
            json!("C:/Windows/system32/perllsp.exe"),
            "/server/process_path",
            "is not the managed public artifact resolved by the host",
        ),
        (
            "decoy root without component boundary".into(),
            json!("C:/tmp/decoyuser/packages/perllsp-9.9.9-test/perllsp.exe"),
            "/server/process_path",
            "is not the managed public artifact resolved by the host",
        ),
        (
            "non-perllsp product argv".into(),
            json!(["perlnavigator", "--stdio"]),
            "/server/process_argv",
            "exact perllsp product",
        ),
        (
            "wrong version banner".into(),
            json!("perllsp 0.17.0"),
            "/server/version_output",
            "exact canonical perllsp product and version",
        ),
        (
            "lite-xl-lsp package substitution".into(),
            json!({
                "ref": "v0.9.9",
                "sha256": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "version": "0.9.9",
            }),
            "/packages/lite_xl_lsp",
            "disagrees with the accepted subject",
        ),
        (
            "private checksum override".into(),
            json!("sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"),
            "/packages/language_perl/sha256",
            "must equal the accepted subject identity",
        ),
        (
            "surviving second perl server".into(),
            json!(false),
            "/discriminators/second_perl_server_absent",
            "must hold on a public replay",
        ),
        (
            "wrong-root same-basename stop".into(),
            json!(false),
            "/discriminators/wrong_root_same_basename_rejected",
            "must hold on a public replay",
        ),
        (
            "ambient PATH satisfies a managed row".into(),
            json!(false),
            "/discriminators/managed_row_not_satisfied_by_ambient_path",
            "must hold on a public replay",
        ),
        (
            "hand-populated package cache".into(),
            json!(false),
            "/profile/hand_populated_package_cache_absent",
            "clean public-install profile precondition failed",
        ),
        (
            "candidate source checkout present".into(),
            json!(false),
            "/profile/candidate_source_checkout_absent",
            "clean public-install profile precondition failed",
        ),
        (
            "other perl server selected".into(),
            json!(false),
            "/profile/other_perl_server_selected_absent",
            "clean public-install profile precondition failed",
        ),
        (
            "surviving adapter orphan".into(),
            json!([4243]),
            "/cleanup/adapter_orphans",
            "must be empty on shutdown",
        ),
        (
            "unknown shutdown truth".into(),
            json!(null),
            "/cleanup/debuggee_orphans",
            "must be empty on shutdown",
        ),
        (
            "unproven activation cell".into(),
            json!({"result": "not_proven", "evidence": null}),
            "/journey/lite_xl.bdd.activate.01",
            "is not proven by this receipt",
        ),
        (
            "unsupported read cell".into(),
            json!({"result": "unsupported", "evidence": null}),
            "/journey/lite_xl.bdd.read.01",
            "is not proven by this receipt",
        ),
        (
            "unsupported optional cell without limitation".into(),
            json!({"result": "unsupported", "evidence": null}),
            "/journey/lite_xl.bdd.opt.01",
            "record an explicit unsupported limitation",
        ),
        (
            "invented journey cell".into(),
            json!({"result": "pass", "evidence": "fabricated"}),
            "/journey/lite_xl.bdd.invented.99",
            "is not in the landed",
        ),
        (
            "dropped journey cell".into(),
            Value::Null,
            "/journey/lite_xl.bdd.protocol.13",
            "from the landed ledger is missing",
        ),
        (
            "stale ledger binding".into(),
            json!("sha256:8888888888888888888888888888888888888888888888888888888888888888"),
            "/ledger_sha256",
            "binding is stale",
        ),
        (
            "LSP support row drift".into(),
            json!("promoted"),
            "/claim_boundary/lsp_support_rows",
            "unchanged",
        ),
        ("missing observation time".into(), Value::Null, "/observed_at", "observed_at"),
        (
            "non-timestamp observation time".into(),
            json!("not-a-timestamp"),
            "/observed_at",
            "RFC 3339",
        ),
        ("dropped limitations".into(), json!([]), "/limitations", "limitations"),
        (
            "dropped currentness invalidators".into(),
            json!([]),
            "/currentness/invalidators",
            "invalidators",
        ),
        (
            "absent exact-source entry gate".into(),
            json!("absent"),
            "/gates/exact_source_lite_xl_receipt",
            "cannot outrun an absent or stale entry gate",
        ),
    ];

    for (label, value, pointer, fragment) in mutations {
        let mut mutated = control.clone();
        // A null mutation removes the key outright: the dropped-key and
        // explicit-null shapes must both fail closed at the same seam.
        if value.is_null() {
            remove_pointer(&mut mutated, pointer);
        } else if let Some(slot) = mutated.pointer_mut(pointer) {
            *slot = value;
        } else {
            insert_pointer(&mut mutated, pointer, value);
        }
        let path = target.join(format!("pass-{}.json", label.replace([' ', '-'], "_")));
        write_temp(&path, &serde_json::to_string_pretty(&mutated)?)?;
        let output = run_with_receipts_dir(&root, &path, &published, receipts.as_str())?;
        assert_rejected(&output, &format!("pass mutation: {label}"), fragment)?;
    }
    Ok(())
}

/// Remove a JSON pointer key from an object.
fn remove_pointer(value: &mut Value, pointer: &str) {
    if !pointer.starts_with('/') {
        return;
    }
    let tokens: Vec<&str> = pointer[1..].split('/').collect();
    let mut current = value;
    for token in &tokens[..tokens.len() - 1] {
        match current.get_mut(*token) {
            Some(next) => current = next,
            None => return,
        }
    }
    if let Some(object) = current.as_object_mut() {
        object.remove(tokens[tokens.len() - 1]);
    }
}

/// Insert a value at a JSON pointer whose parents exist but whose leaf does
/// not (for invented-key mutations).
fn insert_pointer(value: &mut Value, pointer: &str, inserted: Value) {
    if !pointer.starts_with('/') {
        return;
    }
    let tokens: Vec<&str> = pointer[1..].split('/').collect();
    let mut current = value;
    for token in &tokens[..tokens.len() - 1] {
        match current.get_mut(*token) {
            Some(next) => current = next,
            None => return,
        }
    }
    if let Some(object) = current.as_object_mut() {
        object.insert(tokens[tokens.len() - 1].to_string(), inserted);
    }
}

#[test]
fn implementation_keeps_the_stage_and_ledger_boundaries() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let read = |relative: &str| -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(root.join(relative))?)
    };

    let public_replay = read("scripts/lite_xl_replay/public_replay.py")?;
    let cli = read("scripts/lite_xl_replay/cli.py")?;
    let entry = read(SCRIPT)?;

    // Cross-family containment: the lite-xl validator never delegates to or
    // binds another editor family's validators, contract modules, or
    // fixtures. These are source-containment properties about the code, not
    // behavior claims; every behavior boundary is proven by the mutation and
    // live-binding suites above.
    assert!(
        !public_replay.contains("zed_assets"),
        "the lite-xl validator must not delegate to the zed validators"
    );
    assert!(
        !public_replay.contains(".dap_") && !cli.contains(".dap_") && !entry.contains("zed_assets"),
        "the lite-xl replay surface must not bind zed fixtures or perllsp DAP contracts"
    );

    assert!(
        cli.contains("validate-public-replay-receipt"),
        "the CLI must expose the public journey validator"
    );
    assert!(entry.contains("cli import main"), "the entry script must reach the CLI");

    // The consumed landed surfaces stay byte-present authorities.
    assert!(root.join(LEDGER).is_file());
    assert!(root.join(ACCEPTANCE_MANIFEST).is_file());
    Ok(())
}
