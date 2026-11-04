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
use xtask::editor_client_compat::EditorClientCompatReceipt;
use xtask::vim_lsp_cell_catalog::{registry, vim_vim_lsp_subject};

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
///
/// Keys are sorted explicitly rather than relying on `serde_json`'s map type:
/// the workspace lockfile resolves `serde_json` with `indexmap`, so a
/// `preserve_order` build would otherwise hash file order while the Python
/// side hashes sorted order.
fn canonical_digest(value: &serde_json::Value) -> Result<String, Box<dyn Error>> {
    let mut sorted = value.clone();
    sort_json_keys(&mut sorted);
    let encoded = serde_json::to_vec(&sorted)?;
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

fn sort_json_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(String, serde_json::Value)> =
                std::mem::take(map).into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, mut child) in entries {
                sort_json_keys(&mut child);
                map.insert(key, child);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(sort_json_keys),
        _ => {}
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    )
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// Every receipt the committed profile registers must satisfy the canonical
/// `editor_client_compat.v1` contract, not merely the fan-in's bounded checks.
/// The Python validator is standard-library only; this is where the shared
/// dialect authority is consumed.
#[test]
fn registered_receipts_satisfy_the_canonical_dialect() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let profile = load_profile(&root)?;
    let inputs = profile
        .get("inputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("profile inputs missing")?;
    for (family, spec) in inputs {
        let references = spec
            .get("receipt_references")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for reference in references {
            let relpath = reference
                .get("artifact")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("{family}: reference without artifact path"))?;
            let bytes = fs::read(root.join(relpath))?;
            let receipt: EditorClientCompatReceipt = serde_json::from_slice(&bytes)?;
            receipt.validate().map_err(|error| {
                format!("{family}: {relpath} fails the receipt contract: {error:#}")
            })?;
        }
    }
    Ok(())
}

/// The profile's `receipt_subject` is a projection of the compiled registry's
/// one admitted subject; the Python seam rejects receipts from any other host,
/// client, server, or integration mode against exactly these values.
#[test]
fn profile_receipt_subject_matches_compiled_registry() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let profile = load_profile(&root)?;
    let expected = serde_json::to_value(vim_vim_lsp_subject())?;
    assert_eq!(profile.get("receipt_subject"), Some(&expected));
    Ok(())
}

/// Every family's `catalog_id`/`cell_ids` denominator must equal the compiled
/// catalog it names, and that catalog must be one the first-class profile may
/// consume (the baseline through its core profile, specialized families
/// directly). The optional workspace family has no registered catalog yet.
#[test]
fn profile_family_denominators_match_compiled_catalogs() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let profile = load_profile(&root)?;
    let catalogs = registry();
    let inputs = profile
        .get("inputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("profile inputs missing")?;
    let mut seen = std::collections::BTreeSet::new();
    for (family, spec) in inputs {
        let cell_ids: Vec<&str> = spec
            .get("cell_ids")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| format!("{family}: cell_ids missing"))?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        let Some(catalog_id) = spec.get("catalog_id").and_then(serde_json::Value::as_str) else {
            assert_eq!(family, "workspace_folders", "{family} has no catalog");
            assert!(cell_ids.is_empty(), "{family} lists cells without a catalog");
            continue;
        };
        assert!(seen.insert(catalog_id), "catalog {catalog_id} bound twice");
        let catalog = catalogs
            .iter()
            .find(|catalog| catalog.catalog_id == catalog_id)
            .ok_or_else(|| format!("{family}: catalog {catalog_id} is not registered"))?;
        let registered: Vec<&str> =
            catalog.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
        assert_eq!(cell_ids, registered, "{family}: denominator drifted from {catalog_id}");
        let consumer = if family == "baseline_core" {
            assert_eq!(catalog.core_profile.as_deref(), Some("vim_actual_client_core"));
            "vim_actual_client_core"
        } else {
            "vim_first_class_exact_source"
        };
        for cell in &catalog.cells {
            assert!(
                cell.allowed_profiles.iter().any(|profile| profile == consumer),
                "{}: cell {} does not feed {consumer}",
                family,
                cell.cell_id
            );
        }
    }
    Ok(())
}

struct ValidatorCopy {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl ValidatorCopy {
    fn new(source_root: &Path) -> Result<Self, Box<dyn Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().to_path_buf();
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
        Ok(Self { _tmp: tmp, root })
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
        let output = Command::new(python())
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

#[test]
fn validation_passes_and_is_deterministic() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
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
    let copy = ValidatorCopy::new(&root)?;
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
    let copy = ValidatorCopy::new(&root)?;
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
    let copy = ValidatorCopy::new(&root)?;
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
    let copy = ValidatorCopy::new(&root)?;
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

/// A synthetic exact-source receipt that satisfies the canonical
/// `editor_client_compat.v1` contract (the Rust validator is asserted on it),
/// so the fan-in's positive path is exercised with dialect-valid evidence
/// rather than a lookalike object.
fn synthetic_receipt(cell_ids: &[String]) -> serde_json::Value {
    let journey: Vec<serde_json::Value> = cell_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "capability_basis": "not_applicable",
                "observed": true,
                "result": "pass",
                "evidence": [format!("freshness/{id}.log")]
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": "editor_client_compat.v1",
        "observed_at": "2026-08-23T12:00:00Z",
        "stage": "exact_source_local",
        "repository": "EffortlessMetrics/perl-lsp-swarm",
        "candidate_sha": "a".repeat(40),
        "platform": {"os": "linux", "os_version": "6.1", "arch": "x86_64"},
        "host": {
            "client_id": "vim-lsp",
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
        "journey": journey,
        "process_cleanup": "pass",
        "result": "pass",
        "limitations": [],
        "artifacts": [
            {"kind": "client_log", "id": "client.log", "sha256": format!("sha256:{}", "8".repeat(64))},
            {"kind": "server_stderr", "id": "server.stderr", "sha256": format!("sha256:{}", "9".repeat(64))},
            {"kind": "capability_snapshot", "id": "initialize.json", "sha256": format!("sha256:{}", "a".repeat(64))},
            {"kind": "process_ledger", "id": "processes.json", "sha256": format!("sha256:{}", "b".repeat(64))}
        ],
        "claim_boundary": "synthetic probe receipt exercising fan-in controls only"
    })
}

/// The subject-equality block a well-formed reference declares for
/// [`synthetic_receipt`], keyed on the vim-lsp commit it claims.
fn equality_for(commit: &str) -> serde_json::Value {
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
}

const PROBE_ARTIFACT: &str = ".ci/editor-clients/vim-fanin-probe.v1.json";

/// The freshness family's registered denominator, read from the committed
/// profile (which the registry test above pins to the compiled catalog).
fn freshness_cell_ids(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let profile = load_profile(root)?;
    Ok(profile
        .pointer("/inputs/freshness/cell_ids")
        .and_then(serde_json::Value::as_array)
        .ok_or("freshness cell_ids missing")?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect())
}

/// Write a receipt into the copy and return its content digest. The receipt
/// is asserted dialect-valid first so every probe exercises the fan-in with
/// evidence the canonical contract accepts.
fn write_receipt(
    copy: &ValidatorCopy,
    receipt: &serde_json::Value,
) -> Result<String, Box<dyn Error>> {
    let typed: EditorClientCompatReceipt = serde_json::from_value(receipt.clone())?;
    typed.validate().map_err(|error| format!("probe receipt is not dialect-valid: {error:#}"))?;
    let path = copy.root.join(PROBE_ARTIFACT);
    fs::write(&path, serde_json::to_vec_pretty(receipt)?)?;
    Ok(sha256_hex(&fs::read(&path)?))
}

/// Write the full-denominator freshness probe receipt; returns its digest and
/// the cell ids it carries.
fn write_probe_receipt(copy: &ValidatorCopy) -> Result<(String, Vec<String>), Box<dyn Error>> {
    let cells = freshness_cell_ids(&copy.root)?;
    let digest = write_receipt(copy, &synthetic_receipt(&cells))?;
    Ok((digest, cells))
}

fn reference(
    digest: &str,
    fills: &str,
    cells: &[String],
    equality: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "artifact": PROBE_ARTIFACT,
        "artifact_sha256": digest,
        "fills": fills,
        "journey_cell_ids": cells,
        "subject_equality": equality
    })
}

fn pinned(digest: &str, cells: &[String]) -> serde_json::Value {
    reference(digest, "freshness", cells, equality_for(PINNED_VIM_LSP_COMMIT))
}

/// Register a dialect-valid receipt for `freshness` and store the cell it
/// composes to; this is the only positive fan-in path while every committed
/// producer stays open.
fn register_freshness(
    copy: &ValidatorCopy,
    freshness_reference: serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    copy.edit_profile(|profile| {
        profile["inputs"]["freshness"]["state"] = "receipt_registered".into();
        profile["inputs"]["freshness"]["receipt_references"] =
            serde_json::json!([freshness_reference.clone()]);
        profile["cells"]["freshness"] = serde_json::json!({
            "result": "pass",
            "observed": true,
            "limitation": "",
            "receipt_references": [freshness_reference]
        });
    })
}

/// A registered exact-subject receipt composes to a passing family cell while
/// the aggregate stays honestly not_proven for the other open producers.
#[test]
fn registered_exact_subject_receipt_composes_to_a_passing_cell() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let (digest, cells) = write_probe_receipt(&copy)?;
    register_freshness(&copy, pinned(&digest, &cells))?;
    let (ok, output) = copy.validate()?;
    assert!(ok, "a dialect-valid exact-subject receipt must compose: {output}");
    Ok(())
}

/// A receipt from another editor (host, client, integration mode) cannot fill
/// a Vim family even when it is dialect-valid and declares the pinned subject.
#[test]
fn foreign_host_receipt_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let cells = freshness_cell_ids(&copy.root)?;
    let mut receipt = synthetic_receipt(&cells);
    receipt["host"]["product"] = "emacs".into();
    receipt["host"]["client_id"] = "eglot".into();
    receipt["integration"]["mode"] = "native_editor_extension".into();
    let digest = write_receipt(&copy, &receipt)?;
    register_freshness(&copy, pinned(&digest, &cells))?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "a foreign editor receipt must fail closed");
    assert!(output.contains("foreign host/client/server subject"), "{output}");
    assert!(
        output.contains("host_product")
            && output.contains("client_id")
            && output.contains("integration_mode"),
        "{output}"
    );
    Ok(())
}

/// Binding only part of a family's catalog denominator earns no disposition.
#[test]
fn incomplete_denominator_cannot_earn_a_pass() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let (digest, cells) = write_probe_receipt(&copy)?;
    register_freshness(&copy, pinned(&digest, &cells[..1]))?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "an incomplete family must fail closed");
    assert!(output.contains("registered receipts cover 1 of 6 catalog cells"), "{output}");
    assert!(output.contains("stored 'pass' but inputs compose 'not_proven'"), "{output}");
    Ok(())
}

/// A journey cell outside the family's registered catalog cannot be bound,
/// even when the receipt carries it.
#[test]
fn unregistered_cell_id_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let mut cells = freshness_cell_ids(&copy.root)?;
    cells.push("vim.vim_lsp.freshness.invented".to_string());
    let digest = write_receipt(&copy, &synthetic_receipt(&cells))?;
    register_freshness(&copy, pinned(&digest, &cells))?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "an unregistered cell id must fail closed");
    assert!(output.contains("outside the family's registered catalog denominator"), "{output}");
    Ok(())
}

/// Family observation is a fact of the bound cells: a receipt whose cells are
/// all `unsupported` (unobserved by contract) composes to an unobserved cell.
#[test]
fn unsupported_cells_compose_to_an_unobserved_family() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let cells = freshness_cell_ids(&copy.root)?;
    let mut receipt = synthetic_receipt(&cells);
    for cell in receipt["journey"].as_array_mut().ok_or("journey")? {
        cell["observed"] = false.into();
        cell["result"] = "unsupported".into();
        cell["limitation"] = "host exposes no route for this cell".into();
    }
    receipt["result"] = "partial".into();
    receipt["limitations"] = serde_json::json!(["every freshness cell is unsupported"]);
    let digest = write_receipt(&copy, &receipt)?;
    register_freshness(&copy, pinned(&digest, &cells))?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "the stored cell claims observed=true and result=pass");
    assert!(
        output.contains("cells.freshness.observed disagrees with composed reality"),
        "{output}"
    );
    assert!(output.contains("inputs compose 'unsupported'"), "{output}");
    Ok(())
}

/// One probe fires two independent negative controls: NC3 — the receipt's
/// declared vim-lsp commit diverges from the #11369 pin, so one Vim build
/// cannot combine with another; and NC7 — the same journey cell id is bound
/// by a second family, so families cannot cross-fill each other's
/// observations. Deterministic and offline: temp-copy files only.
#[test]
fn synthetic_registered_receipt_fires_subject_and_cross_fill_controls() -> Result<(), Box<dyn Error>>
{
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let (digest, cells) = write_probe_receipt(&copy)?;
    let wrong_commit = "d".repeat(40);
    register_freshness(
        &copy,
        reference(&digest, "freshness", &cells, equality_for(&wrong_commit)),
    )?;
    let save_reference = reference(&digest, "save", &cells, equality_for(PINNED_VIM_LSP_COMMIT));
    copy.edit_profile(|profile| {
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

/// A reference may not restate the pinned subject while pointing at a receipt
/// produced for another candidate: the receipt's own identity governs.
#[test]
fn foreign_receipt_cannot_satisfy_subject_equality_by_declaration() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let (digest, cells) = write_probe_receipt(&copy)?;
    let mut equality = equality_for(PINNED_VIM_LSP_COMMIT);
    equality["candidate_sha"] = "c".repeat(40).into();
    equality["platform_arch"] = "aarch64".into();
    register_freshness(&copy, reference(&digest, "freshness", &cells, equality))?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "a declaration disagreeing with the receipt must fail closed");
    assert!(output.contains("disagrees with the receipt's own identity"), "{output}");
    assert!(output.contains("candidate_sha") && output.contains("platform_arch"), "{output}");
    Ok(())
}

/// The stored cell must cite exactly the references its result was composed
/// from; equal-length provenance with different digests is drift.
#[test]
fn stored_provenance_must_match_validated_inputs() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let (digest, cells) = write_probe_receipt(&copy)?;
    register_freshness(&copy, pinned(&digest, &cells))?;
    copy.edit_profile(|profile| {
        profile["cells"]["freshness"]["receipt_references"][0]["artifact_sha256"] =
            format!("sha256:{}", "f".repeat(64)).into();
    })?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "stored provenance that differs from the inputs must fail closed");
    assert!(output.contains("receipt_references drifted from the validated inputs"), "{output}");
    Ok(())
}

/// A receipt whose journey cell claims a pass without an observation is
/// rejected at the fan-in seam even before the canonical validator sees it.
#[test]
fn unobserved_pass_cell_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let cells = freshness_cell_ids(&copy.root)?;
    let mut receipt = synthetic_receipt(&cells);
    receipt["journey"][0]["observed"] = false.into();
    // Deliberately dialect-invalid (pass without observation), so it bypasses
    // the canonical check and must be caught at the fan-in seam itself.
    let path = copy.root.join(PROBE_ARTIFACT);
    fs::write(&path, serde_json::to_vec_pretty(&receipt)?)?;
    let digest = sha256_hex(&fs::read(&path)?);
    register_freshness(&copy, pinned(&digest, &cells))?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "an unobserved pass must fail closed");
    assert!(output.contains("claims a pass without an observation"), "{output}");
    Ok(())
}

/// Receipt references are repository-relative evidence; a traversing path
/// must not be read or hashed.
#[test]
fn receipt_path_escaping_the_repository_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    let cells = freshness_cell_ids(&copy.root)?;
    let outside = tempfile::NamedTempFile::new()?;
    fs::write(outside.path(), serde_json::to_vec_pretty(&synthetic_receipt(&cells))?)?;
    let digest = sha256_hex(&fs::read(outside.path())?);
    let mut escaping = pinned(&digest, &cells);
    escaping["artifact"] = outside.path().to_string_lossy().into_owned().into();
    register_freshness(&copy, escaping)?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "an absolute receipt path must fail closed");
    assert!(output.contains("escapes the repository root"), "{output}");
    Ok(())
}

/// A non-object family cell is a structural failure, not something the
/// comparison may skip.
#[test]
fn malformed_cell_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    copy.edit_profile(|profile| {
        profile["cells"]["baseline_core"] = "garbage".into();
    })?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "a malformed cell must fail closed");
    assert!(output.contains("cells.baseline_core: expected an object"), "{output}");
    Ok(())
}

/// Optional workspace evidence never moves the aggregate (#10858
/// consumes_if_available), but its absence may not vanish from the visible
/// limitations either.
#[test]
fn optional_family_absence_stays_visible_without_blocking() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let copy = ValidatorCopy::new(&root)?;
    copy.edit_profile(|profile| {
        let limitations = profile["aggregate_limitations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|item| !item.as_str().unwrap_or_default().contains("10960"))
            .collect::<Vec<_>>();
        profile["aggregate_limitations"] = serde_json::Value::Array(limitations);
    })?;
    let (ok, output) = copy.validate()?;
    assert!(!ok, "hiding the optional family's absence must fail closed");
    assert!(output.contains("optional family workspace_folders"), "{output}");
    assert!(
        !output.contains("aggregate_disposition stored"),
        "optional absence must not move the aggregate: {output}"
    );
    Ok(())
}
