//! Deterministic, offline, file-based derivation of activation rows from
//! existing repository authorities (#9204).
//!
//! Every function here reads exactly one committed authority file (or a
//! sorted directory listing) and turns it into activation rows plus one
//! `DerivationEntry` summary row. Nothing here reaches the network, the
//! process working directory, or wall-clock time — determinism holds
//! regardless of caller CWD or filesystem iteration order because every
//! collection is explicitly sorted before being returned.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::model::{
    ActivationClass, ActivationError, ActivationRow, ClassAuthority, ClassAuthorityKind,
    DerivationEntry, Promotion, PromotionState, ProofReference, Publication, PublicationState,
    Registration, RegistrationState,
};

const FEATURES_TOML: &str = "features.toml";
const GATE_POLICY_YAML: &str = ".ci/gate-policy.yaml";
const FUZZ_TARGETS_DIR: &str = "fuzz/fuzz_targets";
const FUZZ_CARGO_TOML: &str = "fuzz/Cargo.toml";

/// Closed token recorded when an authority declares no owner for a surface.
/// It is deliberately not a plausible team or crate name: a reader must not be
/// able to mistake an absent owner for a real one.
pub const UNOWNED: &str = "unowned";

/// Output of one derivation rule: the rows it emits plus its summary entry.
pub struct RuleOutput {
    pub rows: Vec<ActivationRow>,
    pub entry: DerivationEntry,
}

/// Run every real derivation rule (everything except `override`) and return
/// one output per rule, in rule-table order.
pub fn derive_all(root: &Path) -> Result<Vec<RuleOutput>, ActivationError> {
    let (product, preview) = derive_features(root)?;
    Ok(vec![
        product,
        preview,
        derive_gates(root)?,
        derive_benches(root)?,
        derive_test_features(root)?,
        derive_fuzz(root)?,
    ])
}

fn read_text(root: &Path, relative: &str) -> Result<String, ActivationError> {
    fs::read_to_string(root.join(relative))
        .map_err(|error| ActivationError::new(format!("{relative}: cannot read: {error}")))
}

fn not_applicable(authority: &str) -> Publication {
    Publication { state: PublicationState::NotApplicable, authority: authority.to_string() }
}

fn not_evaluated() -> Promotion {
    Promotion { state: PromotionState::NotEvaluated, blocker: None }
}

// ---------------------------------------------------------------------------
// features.toml -> product, preview
// ---------------------------------------------------------------------------

fn derive_features(root: &Path) -> Result<(RuleOutput, RuleOutput), ActivationError> {
    let text = read_text(root, FEATURES_TOML)?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| ActivationError::new(format!("{FEATURES_TOML}: invalid TOML: {error}")))?;
    let features = value.get("feature").and_then(toml::Value::as_array).ok_or_else(|| {
        ActivationError::new(format!("{FEATURES_TOML}: missing [[feature]] rows"))
    })?;

    let considered = features.len();
    let mut product_rows = Vec::new();
    let mut preview_rows = Vec::new();

    for feature in features {
        let id = feature.get("id").and_then(toml::Value::as_str).ok_or_else(|| {
            ActivationError::new(format!("{FEATURES_TOML}: feature row missing id"))
        })?;
        // Defaulting these would make a feature whose `maturity` is missing or
        // whose `advertised` is not a bool match neither branch and disappear
        // from the inventory with no violation — a real product surface lost
        // to a typo. The classification inputs are required, not optional.
        let maturity = feature.get("maturity").and_then(toml::Value::as_str).ok_or_else(|| {
            ActivationError::new(format!(
                "{FEATURES_TOML}: feature `{id}` has no string `maturity`"
            ))
        })?;
        let advertised =
            feature.get("advertised").and_then(toml::Value::as_bool).ok_or_else(|| {
                ActivationError::new(format!(
                    "{FEATURES_TOML}: feature `{id}` has no boolean `advertised`"
                ))
            })?;
        if maturity == "proven" && advertised {
            product_rows.push(feature_row(
                id,
                feature,
                ActivationClass::Product,
                "features-product",
            )?);
        } else if maturity == "preview" {
            preview_rows.push(feature_row(
                id,
                feature,
                ActivationClass::Preview,
                "features-preview",
            )?);
        }
    }
    product_rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));
    preview_rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));

    let product = RuleOutput {
        entry: DerivationEntry {
            rule: "features-product".to_string(),
            authority: FEATURES_TOML.to_string(),
            emits: ActivationClass::Product.as_str().to_string(),
            considered,
            emitted: product_rows.len(),
            not_seeded_reason: "rows with maturity != \"proven\" or advertised != true are not \
                product surfaces; earned-claim maturity stays owned by features.toml"
                .to_string(),
        },
        rows: product_rows,
    };
    let preview = RuleOutput {
        entry: DerivationEntry {
            rule: "features-preview".to_string(),
            authority: FEATURES_TOML.to_string(),
            emits: ActivationClass::Preview.as_str().to_string(),
            considered,
            emitted: preview_rows.len(),
            not_seeded_reason: "rows with maturity != \"preview\" are not preview surfaces"
                .to_string(),
        },
        rows: preview_rows,
    };
    Ok((product, preview))
}

fn feature_row(
    id: &str,
    feature: &toml::Value,
    class: ActivationClass,
    rule: &str,
) -> Result<ActivationRow, ActivationError> {
    let implementation_owner =
        feature.get("implementation_owner").and_then(toml::Value::as_str).unwrap_or("missing");
    let capability_gate =
        feature.get("capability_gate").and_then(toml::Value::as_str).unwrap_or("missing");
    let registration_field =
        feature.get("registration").and_then(toml::Value::as_str).unwrap_or("missing");
    let owning_crate = crate_dir_of(implementation_owner);

    let consumers = owning_crate.clone().map(|dir| vec![dir]).unwrap_or_default();
    // `features.toml` writes the literal `missing` when no implementation
    // crate is recorded. Passing that through as an owner would produce a
    // plausible-looking name that satisfies a non-blank owner check while
    // meaning the opposite, so the absence is recorded as the closed
    // `unowned` token instead. `validate` forbids `unowned` on a product row.
    let owner = owning_crate.unwrap_or_else(|| UNOWNED.to_string());
    let unowned_note = (owner == UNOWNED).then(|| {
        format!(
            "no implementation crate recorded: {FEATURES_TOML} sets \
             implementation_owner = \"{implementation_owner}\""
        )
    });

    let established = capability_gate != "missing" && registration_field != "missing";
    let registration = Registration {
        state: if established {
            RegistrationState::Established
        } else {
            RegistrationState::NotEstablished
        },
        authority: Some(FEATURES_TOML.to_string()),
        detail: Some(format!(
            "capability_gate = \"{capability_gate}\"; registration = \"{registration_field}\""
        )),
    };

    // A malformed evidence entry must not be filtered away: dropping it would
    // quietly shrink a product row's proof references while generation still
    // reported success. An absent `evidence` array is a different thing — the
    // authority simply records none — and stays an empty list.
    let mut proof_references = Vec::new();
    if let Some(entries) = feature.get("evidence").and_then(toml::Value::as_array) {
        for entry in entries {
            let class = entry.get("class").and_then(toml::Value::as_str).filter(|v| !v.is_empty());
            let evidence_id =
                entry.get("id").and_then(toml::Value::as_str).filter(|v| !v.is_empty());
            let (Some(class), Some(evidence_id)) = (class, evidence_id) else {
                return Err(ActivationError::new(format!(
                    "{FEATURES_TOML}: feature `{id}` has an evidence entry without a \
                     non-empty string `class` and `id`"
                )));
            };
            proof_references
                .push(ProofReference { class: class.to_string(), id: evidence_id.to_string() });
        }
    }

    Ok(ActivationRow {
        surface_id: format!("feature:{id}"),
        class,
        class_authority: ClassAuthority {
            kind: ClassAuthorityKind::Derived,
            authority: FEATURES_TOML.to_string(),
            rule: rule.to_string(),
        },
        semantic_authority: format!("{FEATURES_TOML}#{id}"),
        consumers,
        compile_profiles: Vec::new(),
        registration,
        data_authority: None,
        observable_contract: None,
        proof_references,
        publication: not_applicable(FEATURES_TOML),
        maturity_authority: Some(format!("{FEATURES_TOML}#{id}")),
        owner,
        promotion: not_evaluated(),
        retirement: None,
        notes: unowned_note,
    })
}

/// `crates/<name>/...` -> `Some("crates/<name>")`; anything else (including
/// the literal `missing` sentinel) -> `None`.
fn crate_dir_of(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    if parts.next()? != "crates" {
        return None;
    }
    let name = parts.next()?;
    if name.is_empty() {
        return None;
    }
    Some(format!("crates/{name}"))
}

// ---------------------------------------------------------------------------
// .ci/gate-policy.yaml -> gate
// ---------------------------------------------------------------------------

fn derive_gates(root: &Path) -> Result<RuleOutput, ActivationError> {
    let text = read_text(root, GATE_POLICY_YAML)?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).map_err(|error| {
        ActivationError::new(format!("{GATE_POLICY_YAML}: invalid YAML: {error}"))
    })?;
    let gates = value
        .get("gates")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| ActivationError::new(format!("{GATE_POLICY_YAML}: missing gates: list")))?;

    let mut rows: Vec<ActivationRow> = Vec::with_capacity(gates.len());
    for gate in gates {
        // A gate row without a usable name is malformed authority data, not
        // an optional value. Skipping it would drop a real gate from the
        // inventory while still reporting a successful generation.
        let name = gate
            .get("name")
            .and_then(serde_yaml_ng::Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                ActivationError::new(format!(
                    "{GATE_POLICY_YAML}: a gates: entry has no non-empty string name"
                ))
            })?
            .to_string();
        {
            let tier = gate.get("tier").and_then(serde_yaml_ng::Value::as_str).unwrap_or("");
            // `required` is optional in gate-policy.yaml, and the gate runner
            // reads an absent value as TRUE (`#[serde(default = "default_true")]`
            // on `GateDefinition::required`, xtask/src/tasks/gates.rs:149).
            // Defaulting to false here would record a required gate as optional
            // — the inventory would understate enforcement, which is the exact
            // dishonesty it exists to prevent. Consume the authority's own
            // default rather than inventing a different one.
            let required =
                gate.get("required").and_then(serde_yaml_ng::Value::as_bool).unwrap_or(true);
            let description =
                gate.get("description").and_then(serde_yaml_ng::Value::as_str).unwrap_or("");
            rows.push(ActivationRow {
                surface_id: format!("gate:{name}"),
                class: ActivationClass::Gate,
                class_authority: ClassAuthority {
                    kind: ClassAuthorityKind::Derived,
                    authority: GATE_POLICY_YAML.to_string(),
                    rule: "gate-policy-gates".to_string(),
                },
                semantic_authority: format!("{GATE_POLICY_YAML}#{name}"),
                consumers: Vec::new(),
                compile_profiles: Vec::new(),
                registration: Registration {
                    state: RegistrationState::Established,
                    authority: Some(GATE_POLICY_YAML.to_string()),
                    detail: Some(format!("tier = \"{tier}\"; required = {required}")),
                },
                data_authority: None,
                observable_contract: None,
                proof_references: Vec::new(),
                publication: not_applicable(GATE_POLICY_YAML),
                maturity_authority: None,
                owner: "release/ci".to_string(),
                promotion: not_evaluated(),
                retirement: None,
                notes: if description.is_empty() { None } else { Some(description.to_string()) },
            });
        }
    }
    let considered = gates.len();
    rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));

    Ok(RuleOutput {
        entry: DerivationEntry {
            rule: "gate-policy-gates".to_string(),
            authority: GATE_POLICY_YAML.to_string(),
            emits: ActivationClass::Gate.as_str().to_string(),
            considered,
            emitted: rows.len(),
            not_seeded_reason: "every entry under gates: is emitted; nothing is filtered"
                .to_string(),
        },
        rows,
    })
}

// ---------------------------------------------------------------------------
// crates/*/Cargo.toml [[bench]] -> benchmark
// crates/*/Cargo.toml [features] test-*/expose_*/stress-tests/experimental-* -> test_api
// ---------------------------------------------------------------------------

fn sorted_crate_manifests(root: &Path) -> Result<Vec<(String, toml::Value)>, ActivationError> {
    let crates_dir = root.join("crates");
    // A discarded directory-entry error would silently shrink the input set:
    // generation would succeed against a partial view of `crates/`. Surface it.
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(&crates_dir)
        .map_err(|error| ActivationError::new(format!("crates: cannot list: {error}")))?
    {
        let entry = entry
            .map_err(|error| ActivationError::new(format!("crates: cannot read entry: {error}")))?;
        if !entry.path().join("Cargo.toml").is_file() {
            continue;
        }
        let name = entry.file_name().into_string().map_err(|name| {
            ActivationError::new(format!("crates: non-UTF-8 directory name {name:?}"))
        })?;
        names.push(name);
    }
    names.sort();

    let mut manifests = Vec::with_capacity(names.len());
    for name in names {
        let relative = format!("crates/{name}/Cargo.toml");
        let text = read_text(root, &relative)?;
        let value: toml::Value = toml::from_str(&text)
            .map_err(|error| ActivationError::new(format!("{relative}: invalid TOML: {error}")))?;
        manifests.push((name, value));
    }
    Ok(manifests)
}

fn package_name(crate_dir: &str, manifest: &toml::Value) -> String {
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| crate_dir.to_string())
}

fn derive_benches(root: &Path) -> Result<RuleOutput, ActivationError> {
    let manifests = sorted_crate_manifests(root)?;
    let mut rows = Vec::new();
    let mut considered = 0usize;

    for (crate_dir, manifest) in &manifests {
        let manifest_path = format!("crates/{crate_dir}/Cargo.toml");
        let name = package_name(crate_dir, manifest);
        let Some(benches) = manifest.get("bench").and_then(toml::Value::as_array) else {
            continue;
        };
        for bench in benches {
            considered += 1;
            // As with gates: a [[bench]] without a usable name is malformed
            // manifest data. Skipping it would silently omit a real target.
            let bench_name = bench
                .get("name")
                .and_then(toml::Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    ActivationError::new(format!(
                        "{manifest_path}: a [[bench]] target has no non-empty string name"
                    ))
                })?;
            rows.push(ActivationRow {
                surface_id: format!("bench:{name}/{bench_name}"),
                class: ActivationClass::Benchmark,
                class_authority: ClassAuthority {
                    kind: ClassAuthorityKind::Derived,
                    authority: manifest_path.clone(),
                    rule: "cargo-bench-targets".to_string(),
                },
                semantic_authority: format!("{manifest_path}#bench.{bench_name}"),
                consumers: vec![format!("crates/{crate_dir}")],
                compile_profiles: vec!["bench".to_string()],
                registration: Registration {
                    state: RegistrationState::Established,
                    authority: Some(manifest_path.clone()),
                    detail: Some(format!("[[bench]] name = \"{bench_name}\"")),
                },
                data_authority: None,
                observable_contract: None,
                proof_references: Vec::new(),
                publication: not_applicable(&manifest_path),
                maturity_authority: None,
                owner: name.clone(),
                promotion: not_evaluated(),
                retirement: None,
                notes: None,
            });
        }
    }
    rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));

    Ok(RuleOutput {
        entry: DerivationEntry {
            rule: "cargo-bench-targets".to_string(),
            authority: "crates/*/Cargo.toml".to_string(),
            emits: ActivationClass::Benchmark.as_str().to_string(),
            considered,
            emitted: rows.len(),
            not_seeded_reason: "every [[bench]] target is emitted; nothing is filtered".to_string(),
        },
        rows,
    })
}

/// Cargo features that gate a test-only surface.
///
/// This is a name-based rule over a repository whose test features do not
/// share one spelling, so it enumerates the real spellings in use
/// (`test-*`, `expose_*`, `experimental-*`, `stress-tests`, `slow_tests`,
/// `integration-test`) rather than pretending a single prefix covers them.
/// It cannot be complete by construction: a future test feature under a new
/// spelling would not be seeded until it is added here, which is why the
/// rule's `not_seeded_reason` says so instead of claiming everything
/// unmatched is an ordinary build feature.
fn is_test_api_feature(name: &str) -> bool {
    name.starts_with("test-")
        || name.starts_with("expose_")
        || name.starts_with("experimental-")
        || matches!(name, "stress-tests" | "slow_tests" | "integration-test")
}

fn derive_test_features(root: &Path) -> Result<RuleOutput, ActivationError> {
    let manifests = sorted_crate_manifests(root)?;
    let mut rows = Vec::new();
    let mut considered = 0usize;

    for (crate_dir, manifest) in &manifests {
        let manifest_path = format!("crates/{crate_dir}/Cargo.toml");
        let name = package_name(crate_dir, manifest);
        let Some(features) = manifest.get("features").and_then(toml::Value::as_table) else {
            continue;
        };
        let mut feature_names: Vec<&String> = features.keys().collect();
        feature_names.sort();
        considered += feature_names.len();
        for feature_name in feature_names {
            if !is_test_api_feature(feature_name) {
                continue;
            }
            let enables: Vec<String> = features
                .get(feature_name)
                .and_then(toml::Value::as_array)
                .map(|entries| {
                    entries.iter().filter_map(toml::Value::as_str).map(str::to_string).collect()
                })
                .unwrap_or_default();
            rows.push(ActivationRow {
                surface_id: format!("cargo-feature:{name}/{feature_name}"),
                class: ActivationClass::TestApi,
                class_authority: ClassAuthority {
                    kind: ClassAuthorityKind::Derived,
                    authority: manifest_path.clone(),
                    rule: "cargo-test-features".to_string(),
                },
                semantic_authority: format!("{manifest_path}#features.{feature_name}"),
                consumers: vec![format!("crates/{crate_dir}")],
                compile_profiles: vec![feature_name.clone()],
                registration: Registration {
                    state: RegistrationState::Established,
                    authority: Some(manifest_path.clone()),
                    detail: Some(format!("[features] {feature_name} = [{}]", enables.join(", "))),
                },
                data_authority: None,
                observable_contract: None,
                proof_references: Vec::new(),
                publication: not_applicable(&manifest_path),
                maturity_authority: None,
                owner: name.clone(),
                promotion: not_evaluated(),
                retirement: None,
                notes: None,
            });
        }
    }
    rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));

    Ok(RuleOutput {
        entry: DerivationEntry {
            rule: "cargo-test-features".to_string(),
            authority: "crates/*/Cargo.toml".to_string(),
            emits: ActivationClass::TestApi.as_str().to_string(),
            considered,
            emitted: rows.len(),
            not_seeded_reason: "a feature is seeded when its name matches a known \
                test-only spelling (test-*, expose_*, experimental-*, stress-tests, \
                slow_tests, integration-test); this is a name rule, not a proof, so a \
                test feature under a new spelling stays unseeded until it is added"
                .to_string(),
        },
        rows,
    })
}

// ---------------------------------------------------------------------------
// fuzz/fuzz_targets/*.rs -> lab
// ---------------------------------------------------------------------------

fn derive_fuzz(root: &Path) -> Result<RuleOutput, ActivationError> {
    let targets_dir = root.join(FUZZ_TARGETS_DIR);
    // Same reasoning as `sorted_crate_manifests`: a dropped entry error would
    // hide a fuzz target rather than report that the directory could not be read.
    let mut stems: Vec<String> = Vec::new();
    for entry in fs::read_dir(&targets_dir).map_err(|error| {
        ActivationError::new(format!("{FUZZ_TARGETS_DIR}: cannot list: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            ActivationError::new(format!("{FUZZ_TARGETS_DIR}: cannot read entry: {error}"))
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
            ActivationError::new(format!("{FUZZ_TARGETS_DIR}: non-UTF-8 target file name"))
        })?;
        stems.push(stem.to_string());
    }
    stems.sort();

    let fuzz_manifest_text = read_text(root, FUZZ_CARGO_TOML)?;
    let fuzz_manifest: toml::Value = toml::from_str(&fuzz_manifest_text).map_err(|error| {
        ActivationError::new(format!("{FUZZ_CARGO_TOML}: invalid TOML: {error}"))
    })?;
    let fuzz_package = package_name("fuzz", &fuzz_manifest);
    let bins =
        fuzz_manifest.get("bin").and_then(toml::Value::as_array).cloned().unwrap_or_default();

    let mut rows = Vec::with_capacity(stems.len());
    for stem in &stems {
        let source_path = format!("{FUZZ_TARGETS_DIR}/{stem}.rs");
        let bin_entry = bins.iter().find(|bin| {
            bin.get("path").and_then(toml::Value::as_str)
                == Some(format!("fuzz_targets/{stem}.rs").as_str())
        });
        let bin_name = bin_entry.and_then(|bin| bin.get("name")).and_then(toml::Value::as_str);
        let registration = match bin_name {
            Some(bin_name) => Registration {
                state: RegistrationState::Established,
                authority: Some(FUZZ_CARGO_TOML.to_string()),
                detail: Some(format!(
                    "[[bin]] name = \"{bin_name}\", path = \"fuzz_targets/{stem}.rs\""
                )),
            },
            None => Registration {
                state: RegistrationState::NotEstablished,
                authority: Some(FUZZ_CARGO_TOML.to_string()),
                detail: Some(
                    "no [[bin]] entry in fuzz/Cargo.toml references this file".to_string(),
                ),
            },
        };
        // Surface identity follows the registered `[[bin]] name`, not the
        // source file stem: the runnable target is what a consumer names
        // (`cargo fuzz run <name>`), and the two differ in this repository
        // (`fuzz_targets/fuzz_target_1.rs` is registered as
        // `parser_integration`). An unregistered source file has no target
        // name, so it falls back to its stem and says so in `registration`.
        let target_name = bin_name.unwrap_or(stem.as_str());
        rows.push(ActivationRow {
            surface_id: format!("fuzz:{target_name}"),
            class: ActivationClass::Lab,
            class_authority: ClassAuthority {
                kind: ClassAuthorityKind::Derived,
                authority: source_path.clone(),
                rule: "fuzz-targets".to_string(),
            },
            semantic_authority: source_path,
            consumers: vec![fuzz_package.clone()],
            compile_profiles: vec!["fuzz".to_string()],
            registration,
            data_authority: None,
            observable_contract: None,
            proof_references: Vec::new(),
            publication: not_applicable(FUZZ_CARGO_TOML),
            maturity_authority: None,
            owner: fuzz_package.clone(),
            promotion: not_evaluated(),
            retirement: None,
            notes: None,
        });
    }
    rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));

    Ok(RuleOutput {
        entry: DerivationEntry {
            rule: "fuzz-targets".to_string(),
            authority: format!("{FUZZ_TARGETS_DIR}/*.rs"),
            emits: ActivationClass::Lab.as_str().to_string(),
            considered: stems.len(),
            emitted: rows.len(),
            not_seeded_reason: "every fuzz target source file is emitted; nothing is filtered"
                .to_string(),
        },
        rows,
    })
}

/// Flat surface_id -> class index over every real derivation rule, used to
/// validate override targets and detect no-op overrides.
pub fn derived_class_index(
    root: &Path,
) -> Result<BTreeMap<String, ActivationClass>, ActivationError> {
    let mut index = BTreeMap::new();
    for output in derive_all(root)? {
        for row in output.rows {
            index.insert(row.surface_id, row.class);
        }
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch root holding only the one authority a rule reads, so a
    /// malformed-authority control can be written without touching the real
    /// repository or the process working directory.
    fn scratch_root(label: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("activation-derive-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn write(root: &Path, relative: &str, contents: &str) -> bool {
        let path = root.join(relative);
        let Some(parent) = path.parent() else {
            return false;
        };
        fs::create_dir_all(parent).is_ok() && fs::write(&path, contents).is_ok()
    }

    #[test]
    fn gate_without_a_name_fails_instead_of_disappearing() {
        let root = scratch_root("gate-no-name");
        assert!(write(
            &root,
            GATE_POLICY_YAML,
            "gates:\n  - name: real_gate\n    tier: pr_fast\n  - tier: pr_fast\n"
        ));
        let message = match derive_gates(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no non-empty string name"), "{message}");
    }

    #[test]
    fn well_formed_gates_still_derive() {
        let root = scratch_root("gate-ok");
        assert!(write(
            &root,
            GATE_POLICY_YAML,
            "gates:\n  - name: real_gate\n    tier: pr_fast\n    required: true\n"
        ));
        let derived = derive_gates(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(derived, Ok(vec!["gate:real_gate".to_string()]));
    }

    #[test]
    fn feature_without_maturity_fails_instead_of_disappearing() {
        let root = scratch_root("feature-no-maturity");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nadvertised = true\n"
        ));
        let message = match derive_features(&root) {
            Ok(_) => "unexpectedly derived".to_string(),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no string `maturity`"), "{message}");
    }

    #[test]
    fn feature_without_advertised_fails_instead_of_disappearing() {
        let root = scratch_root("feature-no-advertised");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"proven\"\n"
        ));
        let message = match derive_features(&root) {
            Ok(_) => "unexpectedly derived".to_string(),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no boolean `advertised`"), "{message}");
    }

    #[test]
    fn gate_without_required_inherits_the_runner_default_of_true() {
        // gate-policy.yaml makes `required` optional and the gate runner reads
        // an absent value as true. Recording false here would understate
        // enforcement for any future gate that omits the field.
        let root = scratch_root("gate-required-default");
        assert!(write(&root, GATE_POLICY_YAML, "gates:\n  - name: implicit\n    tier: pr_fast\n"));
        let detail = derive_gates(&root).map(|output| {
            output.rows.first().and_then(|row| row.registration.detail.clone()).unwrap_or_default()
        });
        let _ = fs::remove_dir_all(&root);
        assert_eq!(detail, Ok("tier = \"pr_fast\"; required = true".to_string()));
    }

    #[test]
    fn malformed_evidence_entry_fails_instead_of_shrinking_proof() {
        let root = scratch_root("feature-bad-evidence");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"proven\"\nadvertised = true\n\
             implementation_owner = \"crates/demo/src/lib.rs\"\n\
             evidence = [{ class = \"integration_test\" }]\n"
        ));
        let message = match derive_features(&root) {
            Ok(_) => "unexpectedly derived".to_string(),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("evidence entry without a non-empty string"), "{message}");
    }

    #[test]
    fn well_formed_evidence_still_derives_proof_references() {
        let root = scratch_root("feature-good-evidence");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"proven\"\nadvertised = true\n\
             implementation_owner = \"crates/demo/src/lib.rs\"\n\
             evidence = [{ class = \"integration_test\", id = \"crates/demo/tests/a.rs\" }]\n"
        ));
        let count = derive_features(&root)
            .map(|(product, _)| product.rows.first().map(|row| row.proof_references.len()));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(count, Ok(Some(1)));
    }

    #[test]
    fn bench_target_without_a_name_fails_instead_of_disappearing() {
        let root = scratch_root("bench-no-name");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n\n[[bench]]\npath = \"benches/x.rs\"\n"
        ));
        let message = match derive_benches(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no non-empty string name"), "{message}");
    }
}
