//! Contract for the vim/vim-lsp first-class evidence-profile fan-in (#11408).
//!
//! The fan-in composes exact-subject `editor_client_compat.v1` receipts into one
//! `vim_first_class_exact_source` disposition. While every upstream producer is
//! still open (#10962 core, #11381/#11384/#11386/#11387/#11388 catalogs,
//! #11390/#11396/#11398/#11401/#11403 host proofs, #11405 optional folders),
//! every cell must stay honestly `not_proven`. These tests pin that honesty,
//! the deterministic composition, and the fail-closed negative controls by
//! driving the stdlib validator against mutated repository copies.

use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROFILE_RELPATH: &str = ".ci/editor-clients/vim-vim-lsp-first-class-profile.v1.json";
const SUBJECT_RELPATH: &str = ".ci/editor-clients/vim-vim-lsp-subject.v1.json";
const VALIDATOR_RELPATH: &str = "scripts/ux/validate_vim_first_class_profile.py";

const REQUIRED_FAMILIES: [&str; 6] =
    ["baseline_core", "freshness", "save", "recovery", "host_lifecycle", "expanded_activation"];

/// The #11369 pinned prabirshrestha/vim-lsp subject the fan-in consumes.
const PINNED_VIM_LSP_COMMIT: &str = "e10d186452743beb7b43d2b3427020832f930c2b";
const PINNED_VIM_LSP_TREE: &str = "dd24cb8e10096c82766143c9fd058105637d72dc";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn load_profile(root: &Path) -> Result<serde_json::Value, Box<dyn Error>> {
    let text = fs::read_to_string(root.join(PROFILE_RELPATH))?;
    Ok(serde_json::from_str(&text)?)
}

/// The canonical-json digest law shared with the validator; any drift between
/// the two computations fails this contract rather than silently diverging.
fn canonical_digest(value: &serde_json::Value) -> Result<String, Box<dyn Error>> {
    let encoded = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("sha256:{hex}"))
}

/// The profile binds the #11369 subject manifest by content digest, so any pin
/// movement without regeneration fails closed (NC12).
#[test]
fn profile_binds_current_subject_manifest_digest() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let subject_bytes = fs::read(root.join(SUBJECT_RELPATH))?;
    let subject: serde_json::Value = serde_json::from_slice(&subject_bytes)?;
    let profile = load_profile(&root)?;
    let recorded = profile
        .pointer("/generated_from/subject_content_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or("generated_from.subject_content_sha256 missing")?;
    assert_eq!(recorded, canonical_digest(&subject)?);
    Ok(())
}

struct ValidatorCopy {
    root: PathBuf,
}

impl ValidatorCopy {
    fn new(source_root: &Path, label: &str) -> Result<Self, Box<dyn Error>> {
        let root =
            std::env::temp_dir().join(format!("vim-fanin-contract-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".ci/editor-clients"))?;
        fs::create_dir_all(root.join("scripts/ux"))?;
        for name in [
            "vim-vim-lsp-first-class-profile.v1.json",
            "vim-vim-lsp-subject.v1.json",
            "vim-vim-lsp-configuration.v1.json",
            "vim-vim-lsp-public-surface.v1.json",
            "vim-vim-lsp-activation-root.v1.json",
        ] {
            fs::copy(
                source_root.join(".ci/editor-clients").join(name),
                root.join(".ci/editor-clients").join(name),
            )?;
        }
        fs::copy(source_root.join(VALIDATOR_RELPATH), root.join(VALIDATOR_RELPATH))?;
        Ok(Self { root })
    }

    fn edit_profile(
        &self,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> Result<(), Box<dyn Error>> {
        let path = self.root.join(PROFILE_RELPATH);
        let mut value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        mutate(&mut value);
        fs::write(&path, serde_json::to_vec_pretty(&value)?)?;
        Ok(())
    }

    fn validate(&self) -> Result<(bool, String), Box<dyn Error>> {
        let output = Command::new("python")
            .arg(self.root.join(VALIDATOR_RELPATH))
            .arg("--repo-root")
            .arg(&self.root)
            .output()?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok((output.status.success(), combined))
    }
}

impl Drop for ValidatorCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn validation_passes_and_is_deterministic() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root, "determinism")?;
    let first = copy.validate()?;
    let second = copy.validate()?;
    assert!(first.0, "validator failed on committed artifacts: {}", first.1);
    assert!(second.0, "validator failed on second run: {}", second.1);
    assert_eq!(first.1, second.1, "composition must be deterministic");
    Ok(())
}

/// While all producers are open, no cell may claim a pass and the aggregate
/// must be honestly not_proven with every open producer surfaced.
#[test]
fn aggregate_stays_not_proven_while_producers_are_open() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let profile = load_profile(&root)?;

    assert_eq!(
        profile.pointer("/aggregate_disposition").and_then(serde_json::Value::as_str),
        Some("not_proven"),
        "first-class promotion is impossible while producers are open"
    );
    assert_eq!(
        profile
            .pointer("/inputs/workspace_folders/consumption_policy")
            .and_then(serde_json::Value::as_str),
        Some("consumes_if_available"),
        "workspace folders stay optional per #11376"
    );

    for family in REQUIRED_FAMILIES {
        let issue = profile
            .pointer(&format!("/inputs/{family}/authority_issue"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        assert_ne!(issue, 0, "{family} must cite its owning producer issue");
        let result =
            profile.pointer(&format!("/cells/{family}/result")).and_then(serde_json::Value::as_str);
        assert_eq!(
            result,
            Some("not_proven"),
            "{family} cell missing or invented a stronger disposition"
        );
        assert_eq!(
            profile
                .pointer(&format!("/cells/{family}/observed"))
                .and_then(serde_json::Value::as_bool),
            Some(false),
            "{family} claims an observation that never happened"
        );
        let limitation = profile
            .pointer(&format!("/cells/{family}/limitation"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        assert!(
            limitation.contains(&issue.to_string()),
            "{family} limitation must name its open producer #{issue}"
        );
    }

    // Every open required producer stays visible in the aggregate limitations,
    // and the narrower bounded-core profile stays independently claimable.
    let joined = profile
        .pointer("/aggregate_limitations")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    for family in REQUIRED_FAMILIES {
        let issue = profile
            .pointer(&format!("/inputs/{family}/authority_issue"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        assert!(
            joined.contains(&issue.to_string()),
            "aggregate limitations must surface open {family} (#{issue})"
        );
    }
    assert!(joined.contains("10962"), "aggregate must keep the bounded-core separability visible");
    Ok(())
}

#[test]
fn manufactured_pass_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root, "manufactured-pass")?;
    copy.edit_profile(|profile| {
        profile["cells"]["baseline_core"]["result"] = "pass".into();
    })?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "a pass without a registered receipt must fail closed");
    assert!(output.contains("claims pass without any registered receipt"), "{output}");
    Ok(())
}

#[test]
fn pin_movement_without_regeneration_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root, "pin-movement")?;
    let path = copy.root.join(SUBJECT_RELPATH);
    let mut subject: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    subject["upstream"]["selected_commit"] = "a".repeat(40).into();
    fs::write(&path, serde_json::to_vec_pretty(&subject)?)?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "subject movement must invalidate the composed profile");
    assert!(output.contains("subject_content_sha256"), "{output}");
    Ok(())
}

#[test]
fn invented_cell_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root, "invented-cell")?;
    copy.edit_profile(|profile| {
        profile["inputs"]["invented_family"] = serde_json::json!({
            "required": true,
            "authority_issue": 1,
            "state": "producer_open",
            "stage": "exact_source_local"
        });
    })?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "the family denominator is fixed");
    assert!(output.contains("family denominator drifted"), "{output}");
    Ok(())
}

#[test]
fn dropped_required_cell_cannot_hide_a_not_proven_dimension() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root, "dropped-cell")?;
    copy.edit_profile(|profile| {
        if let Some(inputs) = profile.get_mut("inputs").and_then(serde_json::Value::as_object_mut) {
            inputs.remove("recovery");
        }
        if let Some(cells) = profile.get_mut("cells").and_then(serde_json::Value::as_object_mut) {
            cells.remove("recovery");
        }
    })?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "dropping a failing dimension must fail closed");
    assert!(output.contains("family denominator drifted"), "{output}");
    Ok(())
}

/// A fabricated registered receipt drives the otherwise-unreachable
/// reference-validation path (all real families are producer_open today).
/// One probe fires two independent negative controls: NC3 — the receipt's
/// declared vim-lsp commit diverges from the #11369 pin, so one Vim build
/// cannot combine with another; and NC7 — the same journey cell id is bound
/// by a second family, so families cannot cross-fill each other's
/// observations. Deterministic and offline: temp-copy files only.
#[test]
fn synthetic_registered_receipt_fires_subject_and_cross_fill_controls() -> Result<(), Box<dyn Error>>
{
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root, "synthetic-receipt")?;

    let receipt_path = copy.root.join(".ci/editor-clients/vim-fanin-probe.v1.json");
    let receipt = serde_json::json!({
        "schema_version": "editor_client_compat.v1",
        "observed_at": "2026-08-23T12:00:00Z",
        "stage": "exact_source_local",
        "repository": "EffortlessMetrics/perl-lsp-swarm",
        "candidate_sha": "a".repeat(40),
        "platform": {"os": "linux", "os_version": "6.1", "arch": "x86_64"},
        "host": {
            "client_id": "vim_vim_lsp",
            "product": "vim",
            "version": "9.1",
            "source_state": "released",
            "source_ref": "vim/vim v9.1.0",
            "executable_sha256": format!("sha256:{}", "1".repeat(64))
        },
        "integration": {
            "mode": "generic_lsp",
            "registration_state": "manual_client_registration",
            "configuration_sha256": format!("sha256:{}", "2".repeat(64)),
            "driver_sha256": format!("sha256:{}", "3".repeat(64))
        },
        "server": {
            "executable": "perllsp",
            "version": "0.13.0",
            "build_revision": "b".repeat(40),
            "artifact_sha256": format!("sha256:{}", "4".repeat(64)),
            "protocol_version": "3.17",
            "launch_command": ["perllsp", "--stdio"]
        },
        "workspace_fixture": {
            "id": "vim_first_class_fixture",
            "digest": format!("sha256:{}", "5".repeat(64)),
            "expectation_set_id": "canonical_expectation_set",
            "expectation_set_digest": format!("sha256:{}", "6".repeat(64))
        },
        "capabilities": {
            "initialize_snapshot_sha256": format!("sha256:{}", "7".repeat(64)),
            "position_encodings_offered": ["utf-16"],
            "position_encoding_basis": "offered",
            "position_encoding_selected": "utf-16"
        },
        "diagnostics": {"advertised_mode": "push", "observed_messages": ["publish_diagnostics"]},
        "journey": [{
            "id": "freshness_route_observed",
            "capability_basis": "not_applicable",
            "observed": true,
            "result": "pass",
            "evidence": ["freshness/route.log"]
        }],
        "process_cleanup": "pass",
        "result": "pass",
        "limitations": [],
        "artifacts": [],
        "claim_boundary": "synthetic probe receipt exercising fan-in controls only"
    });
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    let artifact = ".ci/editor-clients/vim-fanin-probe.v1.json";
    let digest = format!(
        "sha256:{}",
        Sha256::digest(fs::read(&receipt_path)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );

    let equality_for = |commit: &str| {
        serde_json::json!({
            "vim_lsp_selected_commit": commit,
            "vim_lsp_tree_digest": PINNED_VIM_LSP_TREE,
            "platform_os": "linux",
            "platform_arch": "x86_64",
            "perllsp_build_revision": "b".repeat(40),
            "perllsp_artifact_sha256": format!("sha256:{}", "4".repeat(64)),
            "candidate_sha": "a".repeat(40),
            "workspace_fixture_id": "vim_first_class_fixture",
            "workspace_fixture_digest": format!("sha256:{}", "5".repeat(64)),
            "expectation_set_id": "canonical_expectation_set",
            "expectation_set_digest": format!("sha256:{}", "6".repeat(64))
        })
    };

    copy.edit_profile(|profile| {
        // NC3: this reference declares a vim-lsp commit that is not the pin.
        let wrong_commit = "d".repeat(40);
        let freshness_reference = serde_json::json!({
            "artifact": artifact,
            "artifact_sha256": digest,
            "fills": "freshness",
            "journey_cell_ids": ["freshness_route_observed"],
            "subject_equality": equality_for(&wrong_commit)
        });
        profile["inputs"]["freshness"]["state"] = "receipt_registered".into();
        profile["inputs"]["freshness"]["receipt_references"] =
            serde_json::json!([freshness_reference]);
        // NC7: save binds the same journey cell id the freshness reference
        // already claimed.
        let save_reference = serde_json::json!({
            "artifact": artifact,
            "artifact_sha256": digest,
            "fills": "save",
            "journey_cell_ids": ["freshness_route_observed"],
            "subject_equality": equality_for(PINNED_VIM_LSP_COMMIT)
        });
        profile["inputs"]["save"]["state"] = "receipt_registered".into();
        profile["inputs"]["save"]["receipt_references"] = serde_json::json!([save_reference]);
    })?;

    let (ok, output) = copy.validate()?;
    assert!(!ok, "cross-build substitution and cross-fill must fail closed");
    assert!(
        output.contains("one Vim build cannot combine with another"),
        "NC3 did not fire: {output}"
    );
    assert!(
        output.contains("cannot cross-fill each other's observations"),
        "NC7 did not fire: {output}"
    );
    Ok(())
}
