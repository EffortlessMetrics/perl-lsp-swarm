//! Static falsifier surface for the Zed perl-dap official-registry journey
//! receipt (#9487, train sidecar phase `official_registry_managed_dap_receipt`).
//!
//! The public journey runs through a real Zed host once the external subjects
//! exist; this suite binds the offline authority: the committed receipt is a
//! `blocked_external` observation whose gates are live-bound to the DU01
//! acceptance manifest, the checked #9516 contract, the committed aggregate
//! asset receipt, and the committed exact-source receipts. Every
//! identity-collapse, overclaim, substitution, stale-subject, and orphan
//! mutation on a synthetic pass fails closed with the exact defect named, so
//! the journey cannot be rendered green by relabeling a lower evidence stage.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json";
const AGGREGATE_RECEIPT: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/dap-asset-windows-x86_64.v1.json";
const REGISTRY_MANIFEST: &str = ".ci/fixtures/zed-perl-upstream/registry/manifest.toml";
const RECEIPTS_DIR: &str = ".ci/fixtures/zed-perl-upstream/receipts";
const COMMITTED_RECEIPT: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/dap-public-registry.v1.json";
const SCRIPT: &str = "scripts/zed_dap_asset_receipts.py";

const PUBLISHED_MANIFEST: &str = r#"schema_version = "zed-perl-registry-update.v1"
status = "ready"
ready = true
issue = 7910
programme = 7759

[registry]
repository = "zed-industries/extensions"
branch = "main"
captured_base_commit = "3823ee669031bb22e2d1b8e1bdb1417823808e9a"

[extension]
id = "perl"
submodule_path = "extensions/perl"
submodule_remote = "https://github.com/tree-sitter-perl/zed-perl.git"
current_version = "0.4.0"
current_commit = "eb27a19e69fed8a041b706b23a1f42fbafb29fd8"
new_version = "0.5.0"
new_commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
upstream_branch_containing_commit = "main"

[zed_defaults]
state = "resolved"
released_build = "zed-build-test"

[validation]
submodule_commit_branch_reachable = true
manifest_version_matches = true
released_build_contains_commit = true
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

fn run(
    root: &Path,
    receipt: &str,
    manifest: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_with_receipts_dir(root, receipt, manifest, RECEIPTS_DIR)
}

fn run_with_receipts_dir(
    root: &Path,
    receipt: &str,
    manifest: &Path,
    receipts_dir: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(python())
        .arg(root.join(SCRIPT))
        .arg("validate-dap-public-receipt")
        .arg("--receipt")
        .arg(receipt)
        .arg("--contract")
        .arg(CONTRACT)
        .arg("--asset-receipt")
        .arg(AGGREGATE_RECEIPT)
        .arg("--registry-manifest")
        .arg(manifest)
        .arg("--receipts-dir")
        .arg(receipts_dir)
        .current_dir(root)
        .output()?)
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

fn write_temp(target: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    // A bare-filename target would hand `create_dir_all` an empty parent
    // path; skip directory creation for that degenerate case.
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, text)?;
    Ok(())
}

/// A synthetic passing journey built on the exact committed bindings: the
/// #9516 contract and aggregate receipt digests are quoted from the committed
/// blocked receipt (so they match the real bytes), the registry identities
/// come from the synthetic published DU01 manifest, and the selected target
/// is the windows row the aggregate receipt actually executed.
fn pass_control(committed: &Value) -> Result<Value, Box<dyn Error>> {
    let mut receipt = committed.clone();
    receipt["result"] = json!("pass");
    // A pass records every entry gate as current: the registry gates follow
    // the synthetic accepted manifest, the asset gate follows the bound #9516
    // pass, and the D02 exact-source and C01 routing gates record their
    // prerequisites (the D02 gate is additionally live-bound by the
    // validator against the receipts directory used for the run).
    for cell in [
        "released_zed_build",
        "official_registry_entry",
        "extension_upstream_release",
        "matching_host_asset_receipt",
        "exact_source_zed_dap_receipt",
        "routing_final_check_authority",
    ] {
        receipt["gates"][cell] = json!("current");
    }
    receipt["gates"]["blockers"] = json!([]);
    receipt["registry"] = json!({
        "repository": "zed-industries/extensions",
        "entry": "perl",
        "submodule_path": "extensions/perl",
        "extension_commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "extension_version": "0.5.0",
        "upstream_branch": "main",
        "released_build": "zed-build-test",
    });
    receipt["zed"] = json!({
        "product": "Zed",
        "version": "0.0.0-test",
        "channel": "stable",
        "build": "zed-build-test",
    });
    receipt["platform"] = json!({"os": "windows", "architecture": "x86_64"});
    receipt["extension"] = json!({
        "install_route": "official_registry",
        "upstream_commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "manifest_version": "0.5.0",
        "package_identity": "perl@0.5.0",
    });
    for cell in receipt["profile"]
        .as_object()
        .map(|it| it.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        receipt["profile"][&cell] = json!(true);
    }
    let installed_path = receipt
        .pointer("/asset_evidence/selected_target/installed_path")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("committed receipt lacks installed_path"))?
        .to_string();
    let member_digest = receipt
        .pointer("/asset_evidence/selected_target/member_sha256")
        .cloned()
        .unwrap_or_default();
    let version_directory = installed_path.split('/').next().unwrap_or_default().to_string();
    receipt["adapter"] = json!({
        "adapter_id": "perl-dap",
        "binary_route": "managed_public_artifact",
        "process_path": format!(
            "C:/Users/t/AppSupport/Zed/debug_adapters/{installed_path}"
        ),
        "process_argv": ["perl-dap", "--stdio"],
        "version_output": "perl-dap 0.17.0",
        "binary_sha256": member_digest,
    });
    let digest =
        committed.pointer("/asset_evidence/aggregate_receipt/sha256").cloned().unwrap_or_default();
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
    receipt["discriminators"] = json!({"wrong_root_same_basename_rejected": true});
    receipt["managed_cache"] = json!({
        "before": [],
        "after": [version_directory],
        "restart": {"same_subject": true, "second_provider_absent": true},
    });
    receipt["cleanup"] = json!({"adapter_orphans": [], "debuggee_orphans": []});
    Ok(receipt)
}

/// Materialize a synthetic receipts directory holding one `exact-source*.json`
/// receipt with the given evidence stage and result, for controls that need a
/// committed exact-source pass: the D02 prerequisite is live-bound to the
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
        "schema_version": "zed_host_compat.v1",
        "evidence_stage": evidence_stage,
        "result": result,
    });
    write_temp(&dir.join("exact-source-synthetic.json"), &serde_json::to_string_pretty(&receipt)?)?;
    Ok(dir)
}

#[test]
fn committed_receipt_is_an_honest_blocked_external_observation() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let committed = load_json(&root.join(COMMITTED_RECEIPT))?;
    assert_eq!(
        committed.pointer("/schema_version").and_then(Value::as_str),
        Some("zed_perl_dap_public_registry_receipt.v1")
    );
    assert_eq!(
        committed.pointer("/stage").and_then(Value::as_str),
        Some("public_registry_install"),
        "the journey stage is public_registry_install, never an exact-source stage"
    );
    assert_eq!(
        committed.pointer("/result").and_then(Value::as_str),
        Some("blocked_external"),
        "the committed receipt must stay blocked while the external subjects are absent"
    );
    assert_eq!(
        committed.pointer("/gates/matching_host_asset_receipt").and_then(Value::as_str),
        Some("current"),
        "the bound #9516 aggregate receipt is a current pass and the gate must record it"
    );
    for cell in ["released_zed_build", "official_registry_entry", "extension_upstream_release"] {
        assert_eq!(
            committed.pointer(&format!("/gates/{cell}")).and_then(Value::as_str),
            Some("absent"),
            "gate {cell} must record the absent external subject"
        );
    }
    let blockers = committed
        .pointer("/gates/blockers")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("committed receipt lacks blockers"))?;
    assert!(
        blockers.len() >= 3,
        "blockers must name the exact-source, registry, and routing gates"
    );
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
    let manifest = root.join(REGISTRY_MANIFEST);
    assert_success(
        &run(&root, COMMITTED_RECEIPT, &manifest)?,
        "committed blocked receipt validation",
    )?;
    Ok(())
}

#[test]
fn blocked_gates_are_live_bound_to_the_current_surfaces() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/zed-dap-public-receipt-tests");
    fs::create_dir_all(&target)?;
    let committed = load_json(&root.join(COMMITTED_RECEIPT))?;
    let real_manifest = root.join(REGISTRY_MANIFEST);
    let published = target.join("manifest-published.toml");
    write_temp(&published, PUBLISHED_MANIFEST)?;

    // When the DU01 manifest records a merged-and-released subject, the
    // committed blocked receipt is stale and must be regenerated: the gate
    // accounting is a live binding, not prose.
    let output = run(&root, COMMITTED_RECEIPT, &published)?;
    assert_rejected(&output, "blocked receipt vs published subject", "stale")?;

    // A blocked receipt cannot claim registry gates the manifest denies.
    let mut lying = committed.clone();
    lying["gates"]["official_registry_entry"] = json!("current");
    let path = target.join("blocked-lying-registry.json");
    write_temp(&path, &serde_json::to_string_pretty(&lying)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(
        &output,
        "lying registry gate",
        "cannot claim a subject the acceptance manifest does not record",
    )?;

    // A blocked receipt cannot deny the current #9516 evidence it is bound to.
    let mut denying = committed.clone();
    denying["gates"]["matching_host_asset_receipt"] = json!("absent");
    let path = target.join("blocked-denying-asset.json");
    write_temp(&path, &serde_json::to_string_pretty(&denying)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(&output, "denying asset gate", "cannot deny it")?;

    // A blocked receipt cannot claim an exact-source receipt that no
    // committed fixture records.
    let mut claiming = committed.clone();
    claiming["gates"]["exact_source_zed_dap_receipt"] = json!("current");
    let path = target.join("blocked-claiming-exact-source.json");
    write_temp(&path, &serde_json::to_string_pretty(&claiming)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(&output, "claiming exact-source gate", "no committed fixture records")?;

    // The D02 gate counts only genuine exact-source receipts: a pass-shaped
    // file caught by the glob but carrying another evidence stage never
    // satisfies the gate.
    let wrong_stage_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-wrong-stage",
        "public_registry_install",
        "pass",
    )?;
    let output = run_with_receipts_dir(
        &root,
        path.to_string_lossy().as_ref(),
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
        COMMITTED_RECEIPT,
        &real_manifest,
        current_receipts.to_string_lossy().as_ref(),
    )?;
    assert_rejected(&output, "blocked receipt vs committed exact-source pass", "cannot deny it")?;

    // A blocked receipt cannot claim a released Zed build the acceptance
    // manifest does not record (the manifest's released_build is empty).
    let mut build_overclaim = committed.clone();
    build_overclaim["gates"]["released_zed_build"] = json!("current");
    let path = target.join("blocked-claiming-released-build.json");
    write_temp(&path, &serde_json::to_string_pretty(&build_overclaim)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(
        &output,
        "claiming released-build gate",
        "cannot claim a released build the acceptance manifest does not record",
    )?;

    // Blockers are load-bearing: an empty list hides the absent subjects.
    let mut unblocked = committed.clone();
    unblocked["gates"]["blockers"] = json!([]);
    let path = target.join("blocked-no-blockers.json");
    write_temp(&path, &serde_json::to_string_pretty(&unblocked)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(&output, "empty blockers", "must name its absent external subjects")?;

    // A non-passing receipt cannot claim a proven journey cell.
    let mut overclaim = committed.clone();
    overclaim["journey"]["breakpoint_verified"] =
        json!({"result": "pass", "evidence": "relabeled"});
    let path = target.join("blocked-journey-overclaim.json");
    write_temp(&path, &serde_json::to_string_pretty(&overclaim)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(&output, "blocked journey overclaim", "cannot claim a proven journey cell")?;
    Ok(())
}

#[test]
fn pass_journey_requires_an_accepted_registry_subject() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/zed-dap-public-receipt-tests");
    fs::create_dir_all(&target)?;
    let committed = load_json(&root.join(COMMITTED_RECEIPT))?;
    let control = pass_control(&committed)?;
    let published = target.join("manifest-published.toml");
    write_temp(&published, PUBLISHED_MANIFEST)?;
    let real_manifest = root.join(REGISTRY_MANIFEST);

    let control_path = target.join("pass-control.json");
    write_temp(&control_path, &serde_json::to_string_pretty(&control)?)?;
    let current_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-current",
        "exact_source_dev_extension",
        "pass",
    )?;

    // The control passes only against an accepted merged-and-released
    // registry subject and a committed exact-source pass (the D02 gate is
    // live-bound to the receipts directory).
    assert_success(
        &run_with_receipts_dir(
            &root,
            control_path.to_string_lossy().as_ref(),
            &published,
            current_receipts.to_string_lossy().as_ref(),
        )?,
        "synthetic pass control validation",
    )?;

    // The same control against the real receipts directory has no committed
    // exact-source pass to bind: the D02 prerequisite is not current, so the
    // pass must fail closed.
    let output = run(&root, control_path.to_string_lossy().as_ref(), &published)?;
    assert_rejected(
        &output,
        "pass control without a committed exact-source pass",
        "D02 prerequisite is not current",
    )?;

    // The same receipt against the real (still blocked) manifest is a
    // fabricated public journey and must fail closed.
    let output = run(&root, control_path.to_string_lossy().as_ref(), &real_manifest)?;
    assert_rejected(
        &output,
        "pass control vs blocked manifest",
        "public pass requires an accepted merged-and-released registry subject",
    )?;

    // Relabeling the #9516 aggregate asset receipt as this stage cannot work:
    // public adapter bytes/process evidence is not real Zed debugger
    // behavior, and the receipt shape itself is rejected.
    let output = run(&root, AGGREGATE_RECEIPT, &published)?;
    assert_rejected(
        &output,
        "asset receipt as public journey",
        "unexpected perl-dap public registry receipt schema",
    )?;

    // Relabeling the exact-source template cannot work either.
    let output = run(
        &root,
        ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json",
        &published,
    )?;
    assert_rejected(
        &output,
        "exact-source template as public journey",
        "unexpected perl-dap public registry receipt schema",
    )?;
    Ok(())
}

#[test]
fn pass_mutations_fail_closed_naming_the_exact_defect() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let target = root.join("target/zed-dap-public-receipt-tests");
    fs::create_dir_all(&target)?;
    let committed = load_json(&root.join(COMMITTED_RECEIPT))?;
    let control = pass_control(&committed)?;
    let published = target.join("manifest-published.toml");
    write_temp(&published, PUBLISHED_MANIFEST)?;
    let current_receipts = receipts_dir_with_exact_source(
        &target,
        "receipts-current",
        "exact_source_dev_extension",
        "pass",
    )?;

    let mutations: Vec<(String, Value, &str, &str)> = vec![
        (
            "exact-source stage relabel".into(),
            json!("exact_source_dev_extension"),
            "/stage",
            "must name the public_registry_install stage",
        ),
        (
            "asset stage relabel".into(),
            json!("public_perl_dap_asset"),
            "/stage",
            "must name the public_registry_install stage",
        ),
        (
            "development extension install route".into(),
            json!("dev_extension"),
            "/extension/install_route",
            "install_route=official_registry",
        ),
        (
            "forked registry repository".into(),
            json!("example/perl-fork"),
            "/registry/repository",
            "disagree with the accepted registry subject",
        ),
        (
            "registry commit mismatch".into(),
            json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            "/registry/extension_commit",
            "disagree with the accepted registry subject",
        ),
        (
            "registry released build mismatch".into(),
            json!("zed-build-other"),
            "/registry/released_build",
            "disagree with the accepted registry subject",
        ),
        (
            "prior managed cache".into(),
            json!(["perl-dap-managed-0.16.0-x86_64-pc-windows-msvc"]),
            "/managed_cache/before",
            "must be an empty inventory",
        ),
        (
            "prior cache profile unobserved".into(),
            json!(false),
            "/profile/prior_managed_perl_dap_cache_absent",
            "clean official-registry profile precondition failed",
        ),
        (
            "PATH binary substitution".into(),
            json!("/usr/bin/perl-dap"),
            "/adapter/process_path",
            "is not the managed public artifact",
        ),
        (
            "explicit binary override route".into(),
            json!("path_override"),
            "/adapter/binary_route",
            "managed_public_artifact",
        ),
        (
            "perllsp version line".into(),
            json!("perllsp 0.17.0"),
            "/adapter/version_output",
            "exact canonical",
        ),
        (
            "perllsp adapter product".into(),
            json!("perllsp"),
            "/adapter/adapter_id",
            "exact perl-dap product",
        ),
        (
            "perllsp process argv".into(),
            json!(["perllsp", "--stdio"]),
            "/adapter/process_argv",
            "perllsp product",
        ),
        (
            "tampered binary digest".into(),
            json!("sha256:7777777777777777777777777777777777777777777777777777777777777777"),
            "/adapter/binary_sha256",
            "must equal the selected #9516 member digest",
        ),
        (
            "wrong-root same-basename stop".into(),
            json!(false),
            "/discriminators/wrong_root_same_basename_rejected",
            "wrong-root same-basename source mapping",
        ),
        (
            "surviving debuggee orphan".into(),
            json!([4242]),
            "/cleanup/debuggee_orphans",
            "must be empty",
        ),
        (
            "surviving adapter orphan".into(),
            json!([4243]),
            "/cleanup/adapter_orphans",
            "must be empty",
        ),
        (
            "restart provider substitution".into(),
            json!(false),
            "/managed_cache/restart/same_subject",
            "known-good",
        ),
        (
            "unproven breakpoint cell".into(),
            json!({"result": "not_proven", "evidence": null}),
            "/journey/breakpoint_verified",
            "is not proven",
        ),
        (
            "unproven stopped event".into(),
            json!({"result": "unsupported", "evidence": null}),
            "/journey/stopped_event",
            "is not proven",
        ),
        (
            "stale aggregate receipt digest".into(),
            json!("sha256:8888888888888888888888888888888888888888888888888888888888888888"),
            "/asset_evidence/aggregate_receipt/sha256",
            "binding is stale",
        ),
        (
            "stale contract digest".into(),
            json!("sha256:9999999999999999999999999999999999999999999999999999999999999999"),
            "/asset_evidence/contract/sha256",
            "was not produced against this contract",
        ),
        (
            "re-derived member digest".into(),
            json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            "/asset_evidence/selected_target/member_sha256",
            "exact checked contract row",
        ),
        (
            "unsupported windows arm64 platform".into(),
            json!("aarch64"),
            "/platform/architecture",
            "no managed contract row",
        ),
        (
            "cross-target selection".into(),
            json!({"os": "linux", "architecture": "x86_64"}),
            "/platform",
            "matching the journey platform",
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
            "absent D02 exact-source gate".into(),
            json!("absent"),
            "/gates/exact_source_zed_dap_receipt",
            "cannot outrun an absent or stale entry gate",
        ),
        (
            "absent C01 routing authority gate".into(),
            json!("absent"),
            "/gates/routing_final_check_authority",
            "cannot outrun an absent or stale entry gate",
        ),
    ];

    for (label, value, pointer, fragment) in mutations {
        let mut mutated = control.clone();
        *mutated
            .pointer_mut(pointer)
            .ok_or_else(|| io::Error::other(format!("pointer {pointer} missing")))? = value;
        let path = target.join(format!("pass-{}.json", label.replace([' ', '-'], "_")));
        write_temp(&path, &serde_json::to_string_pretty(&mutated)?)?;
        let output = run_with_receipts_dir(
            &root,
            path.to_string_lossy().as_ref(),
            &published,
            current_receipts.to_string_lossy().as_ref(),
        )?;
        assert_rejected(&output, &format!("pass mutation: {label}"), fragment)?;
    }

    // An asset receipt relabeled with this stage's schema/stamp still lacks
    // the journey shape: bytes and process smoke are not a real Zed journey.
    let asset = load_json(&root.join(AGGREGATE_RECEIPT))?;
    let mut relabeled = asset.clone();
    relabeled["schema_version"] = json!("zed_perl_dap_public_registry_receipt.v1");
    relabeled["stage"] = json!("public_registry_install");
    let path = target.join("pass-asset-relabel.json");
    write_temp(&path, &serde_json::to_string_pretty(&relabeled)?)?;
    let output = run(&root, path.to_string_lossy().as_ref(), &published)?;
    assert_rejected(&output, "asset receipt relabeled public", "journey")?;
    Ok(())
}

#[test]
fn implementation_keeps_the_stage_and_product_boundaries() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let read = |relative: &str| -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(root.join(relative))?)
    };

    let dap_public = read("scripts/zed_assets/dap_public.py")?;
    let dap_cli = read("scripts/zed_assets/dap_cli.py")?;

    // The public validator consumes the #9516 validator rather than
    // reconstructing public asset selection, and never delegates to the
    // perllsp contract or validator in either direction.
    assert!(
        dap_public.contains("from .dap_validation import validate_dap_receipt"),
        "the public journey must bind #9516 through its own validator"
    );
    assert!(
        dap_public.contains("from .dap_contract import"),
        "the public journey must bind the checked #9516 contract"
    );
    assert!(
        !dap_public.contains("from .validation import"),
        "the perl-dap public validator must not delegate to the perllsp validator"
    );
    assert!(
        !dap_public.contains("from .contract import"),
        "the perl-dap public validator must not delegate to the perllsp contract"
    );

    // The stage separation is structural: exact-source and asset stages are
    // named as never-satisfying, and the exact-source gate accounting scans
    // the committed receipts rather than trusting prose.
    assert!(dap_public.contains("exact_source_dev_extension"));
    assert!(dap_public.contains("public_perl_dap_asset"));
    assert!(dap_public.contains("exact_source_receipt_current"));
    assert!(dap_public.contains("registry_subject"));
    assert!(dap_public.contains("wrong_root_same_basename_rejected"));
    assert!(dap_public.contains("perl-dap-managed-"));

    assert!(
        dap_cli.contains("validate-dap-public-receipt"),
        "the CLI must expose the public journey validator"
    );

    // The consumed #9516 surfaces stay byte-untouched authorities.
    assert!(root.join(CONTRACT).is_file());
    assert!(root.join(AGGREGATE_RECEIPT).is_file());
    let aggregate = load_json(&root.join(AGGREGATE_RECEIPT))?;
    assert_eq!(
        aggregate.pointer("/result").and_then(Value::as_str),
        Some("pass"),
        "the bound aggregate receipt must remain a current pass"
    );
    assert_eq!(aggregate.pointer("/stage").and_then(Value::as_str), Some("public_perl_dap_asset"));
    Ok(())
}
