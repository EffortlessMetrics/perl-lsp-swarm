//! Static falsifier surface for the Zed perl-dap support projection
//! (#9489, train stage D06).
//!
//! The projection consumes the landed #9487 official-registry journey
//! validator exactly as it is: the committed receipt is validated first, so
//! every stale gate, landed external subject, committed exact-source pass,
//! lying pass shape, or aliased adapter identity fails the projection closed
//! with the typed defect named before a single support cell is emitted, and
//! the committed registry/docs cannot drift from the current receipts. The
//! Zed LSP support policy is never written by any of these paths.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const SCRIPT: &str = "scripts/zed_dap_asset_receipts.py";
const COMMITTED_RECEIPT: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/dap-public-registry.v1.json";
const EXTENSION_MANIFEST: &str = ".ci/fixtures/zed-perl-upstream/zed-perl/extension.toml";
const ADAPTER_SCHEMA: &str =
    ".ci/fixtures/zed-perl-upstream/zed-perl/debug_adapter_schemas/perl-dap.json";
const SUPPORT_POLICY: &str = "policy/zed-dap-support.toml";
const SUPPORT_DOCS: &str = "docs/EDITORS/ZED_DAP_SUPPORT.md";
const LSP_POLICY: &str = "policy/lsp-client-support.toml";

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

const GENUINE_EXACT_SOURCE_PASS: &str = r#"{
  "schema_version": "zed_host_compat.v1",
  "evidence_stage": "exact_source_dev_extension",
  "result": "pass"
}"#;

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

fn temp_dir(tag: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir()
        .join(format!("zed-dap-support-projection-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Run the D06 projection check against a mutated surface.
///
/// `mutations` receive the base command so each falsifier overrides only the
/// input it mutates; everything else consumes the committed canonical
/// surfaces exactly as the default CLI does.
fn run_projection<F>(root: &Path, mutate: F) -> Result<std::process::Output, Box<dyn Error>>
where
    F: FnOnce(&mut Command),
{
    let mut command = Command::new(python());
    command.arg(root.join(SCRIPT)).arg("project-dap-support").arg("--check").current_dir(root);
    mutate(&mut command);
    Ok(command.output()?)
}

fn load_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn write_temp(target: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, text)?;
    Ok(())
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

fn committed_receipt(root: &Path) -> Result<Value, Box<dyn Error>> {
    load_json(&root.join(COMMITTED_RECEIPT))
}

#[test]
fn stale_or_lying_gates_fail_closed_with_typed_errors() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("stale-gates")?;

    let mut denied = committed_receipt(&root)?;
    denied["gates"]["matching_host_asset_receipt"] = json!("stale");
    let denied_path = dir.join("denied-asset-gate.json");
    write_temp(&denied_path, &serde_json::to_string_pretty(&denied)?)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--receipt").arg(&denied_path);
        })?,
        "denying the current #9516 asset gate",
        "cannot deny it",
    )?;

    let mut overclaimed = committed_receipt(&root)?;
    overclaimed["gates"]["released_zed_build"] = json!("current");
    let overclaimed_path = dir.join("overclaimed-build.json");
    write_temp(&overclaimed_path, &serde_json::to_string_pretty(&overclaimed)?)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--receipt").arg(&overclaimed_path);
        })?,
        "overclaiming a released Zed build",
        "cannot claim a released build the acceptance manifest does not record",
    )?;
    Ok(())
}

#[test]
fn landed_registry_subject_invalidates_the_projection() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("published-subject")?;
    let manifest_path = dir.join("published-manifest.toml");
    write_temp(&manifest_path, PUBLISHED_MANIFEST)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--registry-manifest").arg(&manifest_path);
        })?,
        "projecting against an accepted registry subject",
        "this blocked receipt is stale and the public journey must be re-attempted",
    )?;
    Ok(())
}

#[test]
fn exact_source_pass_prevents_stale_blocked_projection() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("exact-source-pass")?;
    write_temp(&dir.join("exact-source-dap.v1.json"), GENUINE_EXACT_SOURCE_PASS)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--receipts-dir").arg(&dir);
        })?,
        "projecting while a genuine exact-source pass is committed",
        "a committed exact-source receipt records a pass",
    )?;
    Ok(())
}

#[test]
fn lying_pass_shapes_cannot_promote_projection_cells() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("lying-pass")?;

    // A relabeled public pass cannot outrun the unaccepted registry subject.
    let mut relabeled = committed_receipt(&root)?;
    relabeled["result"] = json!("pass");
    let relabeled_path = dir.join("relabeled-pass.json");
    write_temp(&relabeled_path, &serde_json::to_string_pretty(&relabeled)?)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--receipt").arg(&relabeled_path);
        })?,
        "relabeled public pass",
        "public pass requires an accepted merged-and-released registry subject",
    )?;

    // Even with a published subject, a pass must record every gate current;
    // the committed absent gates block it before any cell promotes.
    let manifest_path = dir.join("published-manifest.toml");
    write_temp(&manifest_path, PUBLISHED_MANIFEST)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command
                .arg("--receipt")
                .arg(&relabeled_path)
                .arg("--registry-manifest")
                .arg(&manifest_path);
        })?,
        "public pass without current entry gates",
        "a public pass requires gates.released_zed_build to be current",
    )?;

    // Archive presence is not session proof: a blocked receipt cannot claim a
    // proven journey cell.
    let mut session_proof = committed_receipt(&root)?;
    session_proof["journey"]["breakpoint_verified"]["result"] = json!("pass");
    let session_proof_path = dir.join("session-proof.json");
    write_temp(&session_proof_path, &serde_json::to_string_pretty(&session_proof)?)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--receipt").arg(&session_proof_path);
        })?,
        "journey cell proven inside a blocked receipt",
        "non-passing receipt cannot claim a proven journey cell",
    )?;
    Ok(())
}

/// A synthetic fully passing public journey over the exact committed bindings
/// (mirroring the #9487 suite's pass control), so the projection's promotion
/// path can be discriminated without a real Zed host.
fn pass_control(committed: &Value) -> Result<Value, Box<dyn Error>> {
    let mut receipt = committed.clone();
    receipt["result"] = json!("pass");
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
        "process_path": format!("C:/Users/t/AppSupport/Zed/debug_adapters/{installed_path}"),
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

#[test]
fn an_lsp_exact_source_receipt_cannot_promote_the_dap_stage_or_fabricate_a_tier()
-> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("lsp-exact-source")?;

    // An LSP exact-source journey: same schema family and evidence stage, a
    // perllsp server block, and no perl-dap adapter identity. The D05 entry
    // gate (wide predicate) counts it; the projection's DAP stage must not.
    let lsp_receipt = json!({
        "schema_version": "zed_host_compat.v1",
        "evidence_stage": "exact_source_dev_extension",
        "result": "pass",
        "perllsp": {"server_id": "perllsp", "command": "perllsp", "arguments": ["--stdio"]},
    });
    let receipts_dir = dir.join("receipts");
    write_temp(
        &receipts_dir.join("exact-source-perllsp.v1.json"),
        &serde_json::to_string_pretty(&lsp_receipt)?,
    )?;

    let pass = pass_control(&committed_receipt(&root)?)?;
    let pass_path = dir.join("pass.json");
    write_temp(&pass_path, &serde_json::to_string_pretty(&pass)?)?;
    let manifest_path = dir.join("published-manifest.toml");
    write_temp(&manifest_path, PUBLISHED_MANIFEST)?;

    let policy_output = dir.join("policy.toml");
    let docs_output = dir.join("docs.md");
    let output = Command::new(python())
        .arg(root.join(SCRIPT))
        .arg("project-dap-support")
        .arg("--receipt")
        .arg(&pass_path)
        .arg("--registry-manifest")
        .arg(&manifest_path)
        .arg("--receipts-dir")
        .arg(&receipts_dir)
        .arg("--policy-output")
        .arg(&policy_output)
        .arg("--docs-output")
        .arg(&docs_output)
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "the synthetic public pass must project\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let policy: toml::Value = toml::from_str(&fs::read_to_string(&policy_output)?)?;
    let stages: Vec<(String, String)> = policy
        .get("stage")
        .and_then(toml::Value::as_array)
        .map(|stages| {
            stages
                .iter()
                .map(|stage| {
                    (
                        stage
                            .get("id")
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        stage
                            .get("state")
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let stage_state = |id: &str| {
        stages
            .iter()
            .find(|(stage_id, _)| stage_id == id)
            .map(|(_, state)| state.as_str())
            .unwrap_or_default()
    };
    assert_eq!(
        stage_state("public_registry_install"),
        "pass",
        "a valid public pass must project its earned public stage"
    );
    assert_eq!(
        stage_state("exact_source_dev_extension"),
        "not_proven",
        "an LSP exact-source receipt sharing the zed_host_compat.v1 family must not promote the DAP stage"
    );
    assert_eq!(
        policy
            .get("claim_boundary")
            .and_then(|boundary| boundary.get("support_tier"))
            .and_then(toml::Value::as_str),
        Some("public_registry_proven"),
        "the support tier must follow the validated receipt instead of a hard-coded verdict"
    );
    for cell in policy.get("cell").and_then(toml::Value::as_array).unwrap_or(&vec![]) {
        assert_eq!(
            cell.get("public_registry_install").and_then(toml::Value::as_str),
            Some("pass"),
            "every journey cell the pass proves must project as pass"
        );
        assert_eq!(
            cell.get("exact_source_dev_extension").and_then(toml::Value::as_str),
            Some("not_proven"),
            "the exact-source column must stay independent of the public pass"
        );
    }
    let docs = fs::read_to_string(&docs_output)?;
    assert!(
        docs.contains("public-registry journey proven at the recorded subjects"),
        "the generated documentation status must follow the validated receipt"
    );
    Ok(())
}

#[test]
fn crlf_committed_artifacts_fail_the_exact_byte_check() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("crlf-drift")?;

    let policy_text = fs::read_to_string(root.join(SUPPORT_POLICY))?;
    let crlf_policy = policy_text.replace('\n', "\r\n");
    let crlf_policy_path = dir.join("zed-dap-support.toml");
    write_temp(&crlf_policy_path, &crlf_policy)?;
    let docs_text = fs::read_to_string(root.join(SUPPORT_DOCS))?;
    let docs_path = dir.join("ZED_DAP_SUPPORT.md");
    write_temp(&docs_path, &docs_text)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command
                .arg("--policy-output")
                .arg(&crlf_policy_path)
                .arg("--docs-output")
                .arg(&docs_path);
        })?,
        "CRLF-committed support registry",
        "drifted from the projection",
    )?;
    Ok(())
}

#[test]
fn a_retargeted_schema_path_cannot_hide_behind_the_static_authority() -> Result<(), Box<dyn Error>>
{
    let root = repo_root()?;
    let dir = temp_dir("schema-retarget")?;

    let extension_text = fs::read_to_string(root.join(EXTENSION_MANIFEST))?;
    let retargeted = extension_text
        .replace("debug_adapter_schemas/perl-dap.json", "debug_adapter_schemas/other.json");
    assert_ne!(
        retargeted, extension_text,
        "the staged manifest must bind the perl-dap schema for this mutation to discriminate"
    );
    let retargeted_path = dir.join("extension-retargeted.toml");
    write_temp(&retargeted_path, &retargeted)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--extension-manifest").arg(&retargeted_path);
        })?,
        "manifest schema_path retargeted away from the validated schema",
        "names a different adapter schema",
    )?;
    Ok(())
}

#[test]
fn adapter_identity_aliasing_is_rejected() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("adapter-alias")?;

    let extension_text = fs::read_to_string(root.join(EXTENSION_MANIFEST))?;
    let aliased = extension_text.replace("[debug_adapters.perl-dap]", "[debug_adapters.perllsp]");
    assert_ne!(
        aliased, extension_text,
        "the staged manifest must name the perl-dap adapter for this mutation to discriminate"
    );
    let aliased_path = dir.join("extension-aliased.toml");
    write_temp(&aliased_path, &aliased)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--extension-manifest").arg(&aliased_path);
        })?,
        "adapter identity aliased onto a language-server ID",
        "must declare exactly the 'perl-dap' debug adapter",
    )?;

    let schema = load_json(&root.join(ADAPTER_SCHEMA))?;
    let mut attach = schema.clone();
    attach["properties"]["request"]["enum"] = json!(["launch", "attach"]);
    // Keep the manifest-bound relative suffix so the path-binding check passes
    // and the session-kind discriminator fires for what it mutates.
    let attach_path = dir.join("debug_adapter_schemas").join("perl-dap.json");
    write_temp(&attach_path, &serde_json::to_string_pretty(&attach)?)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command.arg("--adapter-schema").arg(&attach_path);
        })?,
        "unobserved attach session kind",
        "must support exactly the launch session kind",
    )?;
    Ok(())
}

#[test]
fn committed_output_drift_is_rejected() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let dir = temp_dir("output-drift")?;

    let policy_text = fs::read_to_string(root.join(SUPPORT_POLICY))?;
    let drifted_policy = dir.join("zed-dap-support.toml");
    write_temp(&drifted_policy, &(policy_text.clone() + "# hand edit\n"))?;
    let docs_text = fs::read_to_string(root.join(SUPPORT_DOCS))?;
    let pristine_docs = dir.join("ZED_DAP_SUPPORT.md");
    write_temp(&pristine_docs, &docs_text)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command
                .arg("--policy-output")
                .arg(&drifted_policy)
                .arg("--docs-output")
                .arg(&pristine_docs);
        })?,
        "hand-edited support registry",
        "drifted from the projection",
    )?;

    let drifted_docs = dir.join("ZED_DAP_SUPPORT-drifted.md");
    write_temp(&drifted_docs, &(docs_text + "hand edit\n"))?;
    let pristine_policy = dir.join("zed-dap-support-pristine.toml");
    write_temp(&pristine_policy, &policy_text)?;
    assert_rejected(
        &run_projection(&root, |command| {
            command
                .arg("--policy-output")
                .arg(&pristine_policy)
                .arg("--docs-output")
                .arg(&drifted_docs);
        })?,
        "hand-edited generated documentation",
        "drifted from the projection",
    )?;
    Ok(())
}

#[test]
fn the_lsp_support_policy_is_never_touched() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let before = fs::read(root.join(LSP_POLICY))?;

    // A failing projection run and a successful regeneration into temporary
    // outputs both leave the LSP client-support registry byte-identical.
    let dir = temp_dir("lsp-immunity")?;
    let manifest_path = dir.join("published-manifest.toml");
    write_temp(&manifest_path, PUBLISHED_MANIFEST)?;
    let failing = run_projection(&root, |command| {
        command.arg("--registry-manifest").arg(&manifest_path);
    })?;
    assert!(!failing.status.success(), "the stale-subject projection must fail before any write");
    assert_eq!(
        before,
        fs::read(root.join(LSP_POLICY))?,
        "a failed projection run must not touch the LSP support policy"
    );

    let output = Command::new(python())
        .arg(root.join(SCRIPT))
        .arg("project-dap-support")
        .arg("--policy-output")
        .arg(dir.join("policy.toml"))
        .arg("--docs-output")
        .arg(dir.join("docs.md"))
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "regeneration into temporary outputs must succeed\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        before,
        fs::read(root.join(LSP_POLICY))?,
        "the projection must never write the LSP support policy"
    );
    Ok(())
}
