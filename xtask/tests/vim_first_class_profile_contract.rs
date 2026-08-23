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
