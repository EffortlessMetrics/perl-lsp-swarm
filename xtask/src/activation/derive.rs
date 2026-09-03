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
    // Absent and present-but-wrong-type are different facts here too.
    // `features.toml` writes the literal string `missing` when it records no
    // value, so an absent key legitimately reads as `missing`; a present
    // non-string is malformed authority data and would otherwise be laundered
    // into the same sentinel, fabricating provenance text.
    let optional_string = |key: &str| -> Result<&str, ActivationError> {
        match feature.get(key) {
            None => Ok("missing"),
            Some(value) => value.as_str().ok_or_else(|| {
                ActivationError::new(format!(
                    "{FEATURES_TOML}: feature `{id}` has a non-string `{key}`"
                ))
            }),
        }
    };
    let implementation_owner = optional_string("implementation_owner")?;
    let capability_gate = optional_string("capability_gate")?;
    let registration_field = optional_string("registration")?;
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

    // `established` is the strongest claim a row carries, so it must rest on
    // content, not merely on a value that is not the `missing` sentinel.
    // `capability_gate = ""` is not a capability gate; treating it as one
    // would let a row assert it is wired into its consuming mechanism on the
    // strength of an empty string — the same blank-is-not-content hole the
    // override ledger closes with `is_blank`.
    let recorded = |value: &str| value != "missing" && !value.trim().is_empty();
    let established = recorded(capability_gate) && recorded(registration_field);
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
    if let Some(evidence) = feature.get("evidence") {
        // Absent and present-but-wrong-type are different facts. `and_then`
        // alone would collapse them, so a scalar or table `evidence` would
        // read as "records no proof" and silently drop every reference.
        let entries = evidence.as_array().ok_or_else(|| {
            ActivationError::new(format!(
                "{FEATURES_TOML}: feature `{id}` has an `evidence` value that is not an array"
            ))
        })?;
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
            // `GateDefinition` (xtask/src/tasks/gates.rs:145-151) declares
            // `tier: String` and `description: String` with no serde default,
            // so a gate missing either one fails the gate runner's own
            // deserialization. Substituting `""` here would be more permissive
            // than the authority being consumed: the inventory would record a
            // gate the runner rejects as a well-formed row.
            let required_str = |key: &str| -> Result<&str, ActivationError> {
                gate.get(key)
                    .and_then(serde_yaml_ng::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ActivationError::new(format!(
                            "{GATE_POLICY_YAML}: gate `{name}` has no non-empty string `{key}`"
                        ))
                    })
            };
            let tier = required_str("tier")?;
            let description = required_str("description")?;
            // `required` IS optional in gate-policy.yaml, and the gate runner
            // reads an absent value as TRUE (`#[serde(default = "default_true")]`
            // on `GateDefinition::required`). Defaulting to false here would
            // record a required gate as optional — the inventory would
            // understate enforcement, which is the exact dishonesty it exists
            // to prevent. So absence consumes the authority's own default,
            // while a present non-boolean is malformed data and fails: reading
            // `required: "yes"` as the absent-default `true` would report a
            // value the authority never stated.
            let required = match gate.get("required") {
                None => true,
                Some(value) => value.as_bool().ok_or_else(|| {
                    ActivationError::new(format!(
                        "{GATE_POLICY_YAML}: gate `{name}` has a non-boolean `required`"
                    ))
                })?,
            };
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

/// The package name a manifest declares.
///
/// Substituting the directory name would invent a fact: `crates/foo/` need
/// not contain a package called `foo`, and the name becomes both the row's
/// `owner` and half its `surface_id`. A manifest with no readable
/// `package.name` is malformed authority data, so it fails rather than
/// producing a plausible-looking ownership claim nothing in the repository
/// supports.
fn package_name(manifest_path: &str, manifest: &toml::Value) -> Result<String, ActivationError> {
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ActivationError::new(format!("{manifest_path}: has no non-empty string `package.name`"))
        })
}

fn derive_benches(root: &Path) -> Result<RuleOutput, ActivationError> {
    let manifests = sorted_crate_manifests(root)?;
    let mut rows = Vec::new();
    let mut considered = 0usize;

    for (crate_dir, manifest) in &manifests {
        let manifest_path = format!("crates/{crate_dir}/Cargo.toml");
        let name = package_name(&manifest_path, manifest)?;
        let benches = match manifest.get("bench") {
            None => continue,
            Some(value) => value.as_array().ok_or_else(|| {
                ActivationError::new(format!(
                    "{manifest_path}: `bench` must be an array of [[bench]] targets"
                ))
            })?,
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

/// Why a Cargo feature was classified `test_api`.
///
/// Name and usage are genuinely different signals and neither subsumes the
/// other, so the row records which one settled it rather than presenting a
/// single opaque verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestApiSignal {
    /// The feature's own name declares the intent (`test-*`, `expose_*`, ...).
    /// This is the only signal that can classify a feature gating production
    /// code — `expose_lsp_test_api` gates `src/runtime/` precisely because a
    /// test API is *exposed from* production, which is what the class means.
    DeclaredByName,
    /// Every `cfg(feature = "…")` site is under `tests/` or `benches/`. This
    /// catches features whose names declare nothing (`crash-repros`,
    /// `lsp-extras`, `simd`) but whose use proves they gate only tests.
    ProvenByUsage,
}

/// Cargo features that gate a test-only surface, by name or by evidence.
///
/// The name rule alone under-classified: this repository's test features do
/// not share a spelling, and two review rounds of "you missed these
/// spellings" showed that enumerating them is the wrong mechanism, not that
/// the list was one entry short. Usage evidence alone would be wrong in the
/// other direction, dropping the `expose_*`/`test-*` features that gate
/// production code to expose a test API.
///
/// So a feature is `test_api` when its name declares it OR its usage proves
/// it. A feature matching neither is left to its own authority; that is
/// still recorded in the rule's `not_seeded_reason` rather than presented as
/// a claim that everything unmatched is an ordinary build feature.
fn test_api_signal(
    root: &Path,
    crate_dir: &str,
    feature: &str,
) -> Result<Option<TestApiSignal>, ActivationError> {
    if declared_test_api_name(feature) {
        return Ok(Some(TestApiSignal::DeclaredByName));
    }
    let sites = feature_cfg_sites(root, crate_dir, feature)?;
    // No usage at all proves nothing: a declared-but-unused feature is
    // neither shown to gate tests nor shown not to. Only a non-empty,
    // wholly test-side population is evidence.
    if sites.is_empty() {
        return Ok(None);
    }
    let test_only = sites.iter().all(|site| site.contains("/tests/") || site.contains("/benches/"));
    Ok(test_only.then_some(TestApiSignal::ProvenByUsage))
}

fn declared_test_api_name(name: &str) -> bool {
    name.starts_with("test-")
        || name.starts_with("expose_")
        || name.starts_with("experimental-")
        || matches!(name, "stress-tests" | "slow_tests" | "integration-test")
}

/// Every tracked `.rs` file under the crate that names `cfg(feature = "…")`
/// for this feature, sorted so the result does not depend on filesystem
/// iteration order.
///
/// This is a textual scan, not a parse, and it is deliberately conservative:
/// a match inside a comment or string would count as a usage site. That can
/// only make a feature look *less* test-only than it is (an extra non-test
/// site suppresses the `ProvenByUsage` signal), so the failure direction is
/// under-classification, which the rule already reports, rather than a false
/// test-api claim.
fn feature_cfg_sites(
    root: &Path,
    crate_dir: &str,
    feature: &str,
) -> Result<Vec<String>, ActivationError> {
    let needle = format!("feature = \"{feature}\"");
    let crate_root = root.join("crates").join(crate_dir);
    let mut sites = Vec::new();
    let mut stack = vec![crate_root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|error| {
            ActivationError::new(format!("crates/{crate_dir}: cannot read directory: {error}"))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                ActivationError::new(format!("crates/{crate_dir}: cannot read entry: {error}"))
            })?;
            let path = entry.path();
            if path.is_dir() {
                // `target/` is build output, not source, and may not exist.
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && fs::read_to_string(&path).is_ok_and(|text| text.contains(&needle))
            {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                sites.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    sites.sort();
    Ok(sites)
}

fn derive_test_features(root: &Path) -> Result<RuleOutput, ActivationError> {
    let manifests = sorted_crate_manifests(root)?;
    let mut rows = Vec::new();
    let mut considered = 0usize;

    for (crate_dir, manifest) in &manifests {
        let manifest_path = format!("crates/{crate_dir}/Cargo.toml");
        let name = package_name(&manifest_path, manifest)?;
        let features = match manifest.get("features") {
            None => continue,
            Some(value) => value.as_table().ok_or_else(|| {
                ActivationError::new(format!(
                    "{manifest_path}: `features` must be a table of feature arrays"
                ))
            })?,
        };
        let mut feature_names: Vec<&String> = features.keys().collect();
        feature_names.sort();
        considered += feature_names.len();
        for feature_name in feature_names {
            let Some(signal) = test_api_signal(root, crate_dir, feature_name)? else {
                continue;
            };
            let entries =
                features.get(feature_name).and_then(toml::Value::as_array).ok_or_else(|| {
                    ActivationError::new(format!(
                        "{manifest_path}: feature `{feature_name}` must be an array"
                    ))
                })?;
            let mut enables = Vec::with_capacity(entries.len());
            for entry in entries {
                enables.push(entry.as_str().ok_or_else(|| {
                    ActivationError::new(format!(
                        "{manifest_path}: feature `{feature_name}` contains a non-string entry"
                    ))
                })?.to_string());
            }
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
                // Which signal settled the class, so a reader can tell a
                // declared test feature from one proved test-only by its use.
                notes: Some(
                    match signal {
                        TestApiSignal::DeclaredByName => {
                            "test_api by name: the feature's own spelling declares test-only intent"
                        }
                        TestApiSignal::ProvenByUsage => {
                            "test_api by usage: every cfg(feature = \"...\") site for this \
                             feature is under tests/ or benches/"
                        }
                    }
                    .to_string(),
                ),
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
            not_seeded_reason: "a feature is seeded when its NAME declares test-only \
                intent (test-*, expose_*, experimental-*, stress-tests, slow_tests, \
                integration-test) or its USAGE proves it (at least one \
                cfg(feature = \"...\") site, all under tests/ or benches/). Each row \
                records which signal settled it. A feature with no cfg sites at all is \
                not seeded by usage: an unused feature is neither shown to gate tests \
                nor shown not to. A feature gating both production and test code is \
                seeded only if its name declares the intent, since gating production \
                code is what an exposed test API does"
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
    let fuzz_package = package_name(FUZZ_CARGO_TOML, &fuzz_manifest)?;
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
            "gates:\n  - name: real_gate\n    tier: pr_fast\n    description: d\n  - tier: pr_fast\n    description: d\n"
        ));
        let message = match derive_gates(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no non-empty string name"), "{message}");
    }

    #[test]
    fn gate_without_a_tier_fails_instead_of_recording_an_empty_one() {
        // `GateDefinition` declares `tier: String` with no serde default, so
        // this gate is one the gate runner itself rejects. Recording it with
        // `tier = ""` would let the inventory legitimize policy that cannot
        // actually run.
        let root = scratch_root("gate-no-tier");
        assert!(write(
            &root,
            GATE_POLICY_YAML,
            "gates:\n  - name: real_gate\n    description: does a thing\n"
        ));
        let message = match derive_gates(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no non-empty string `tier`"), "{message}");
    }

    #[test]
    fn gate_without_a_description_fails_instead_of_recording_an_empty_one() {
        let root = scratch_root("gate-no-description");
        assert!(write(&root, GATE_POLICY_YAML, "gates:\n  - name: real_gate\n    tier: pr_fast\n"));
        let message = match derive_gates(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no non-empty string `description`"), "{message}");
    }

    #[test]
    fn non_boolean_required_fails_instead_of_reading_as_the_absent_default() {
        // Absent `required` legitimately means true. A present non-boolean is
        // malformed data, and collapsing it into that same default would
        // report an enforcement value the authority never stated.
        let root = scratch_root("gate-required-wrong-type");
        assert!(write(
            &root,
            GATE_POLICY_YAML,
            "gates:\n  - name: real_gate\n    tier: pr_fast\n    description: d\n    required: yes-please\n"
        ));
        let message = match derive_gates(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has a non-boolean `required`"), "{message}");
    }

    #[test]
    fn well_formed_gates_still_derive() {
        let root = scratch_root("gate-ok");
        assert!(write(
            &root,
            GATE_POLICY_YAML,
            "gates:\n  - name: real_gate\n    tier: pr_fast\n    description: d\n    required: true\n"
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
        assert!(write(
            &root,
            GATE_POLICY_YAML,
            "gates:\n  - name: implicit\n    tier: pr_fast\n    description: d\n"
        ));
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
    fn non_array_evidence_fails_instead_of_reading_as_absent() {
        let root = scratch_root("feature-scalar-evidence");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"proven\"\nadvertised = true\n\
             implementation_owner = \"crates/demo/src/lib.rs\"\nevidence = \"see the tests\"\n"
        ));
        let message = match derive_features(&root) {
            Ok(_) => "unexpectedly derived".to_string(),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("`evidence` value that is not an array"), "{message}");
    }

    #[test]
    fn absent_evidence_is_not_an_error() {
        // Absence is the authority recording no proof, which is a fact, not a
        // defect. Only a present-but-malformed value is an error.
        let root = scratch_root("feature-no-evidence");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"proven\"\nadvertised = true\n\
             implementation_owner = \"crates/demo/src/lib.rs\"\n"
        ));
        let count = derive_features(&root)
            .map(|(product, _)| product.rows.first().map(|row| row.proof_references.len()));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(count, Ok(Some(0)));
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

    #[test]
    fn manifest_without_a_package_name_fails_instead_of_borrowing_the_directory() {
        // The directory name is not the package name: `crates/foo/` need not
        // contain a package called `foo`, and the value becomes both the
        // row's owner and half its surface id. Substituting it would invent
        // an ownership claim nothing in the repository supports.
        let root = scratch_root("crate-no-package-name");
        assert!(write(&root, "crates/demo/Cargo.toml", "[package]\nversion = \"0.1.0\"\n"));
        let message = match derive_benches(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has no non-empty string `package.name`"), "{message}");
    }

    #[test]
    fn a_feature_with_no_cfg_sites_is_not_seeded_by_usage() {
        // Absence of usage proves nothing either way, so an unused feature
        // whose name declares nothing must not be classified test_api on the
        // strength of a vacuously-true "all sites are tests".
        let root = scratch_root("feature-unused");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_feature_used_only_under_tests_is_seeded_by_usage() {
        let root = scratch_root("feature-usage-tests");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/thing.rs",
            "#[cfg(feature = \"quiet-feature\")]\nfn t() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(vec!["cargo-feature:demo/quiet-feature".to_string()]));
    }

    #[test]
    fn a_feature_also_used_in_src_is_not_seeded_by_usage() {
        // One non-test site is enough to withdraw the usage claim: the
        // feature demonstrably gates production code, so only its name could
        // classify it, and this one's name declares nothing.
        let root = scratch_root("feature-usage-mixed");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/thing.rs",
            "#[cfg(feature = \"quiet-feature\")]\nfn t() {}\n"
        ));
        assert!(write(
            &root,
            "crates/demo/src/lib.rs",
            "#[cfg(feature = \"quiet-feature\")]\npub fn p() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_name_declared_feature_used_in_src_is_still_seeded() {
        // The same shape as above, but the name declares the intent — which
        // is the real `expose_lsp_test_api` case, where gating production
        // code is precisely what exposing a test API means.
        let root = scratch_root("feature-name-declared-src");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nexpose_demo_test_api = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/src/lib.rs",
            "#[cfg(feature = \"expose_demo_test_api\")]\npub fn p() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(vec!["cargo-feature:demo/expose_demo_test_api".to_string()]));
    }

    #[test]
    fn malformed_bench_section_fails_instead_of_disappearing() {
        let root = scratch_root("bench-wrong-shape");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "bench = \"not-an-array\"\n\n[package]\nname = \"demo\"\n"
        ));
        let message = match derive_benches(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("`bench` must be an array"), "{message}");
    }

    #[test]
    fn malformed_feature_section_fails_instead_of_disappearing() {
        let root = scratch_root("features-wrong-shape");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "features = [\"test-api\"]\n\n[package]\nname = \"demo\"\n"
        ));
        let message = match derive_test_features(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("`features` must be a table"), "{message}");
    }

    #[test]
    fn malformed_feature_entry_fails_instead_of_disappearing() {
        let root = scratch_root("feature-entry-wrong-shape");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\ntest-api = \"not-an-array\"\n"
        ));
        let message = match derive_test_features(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("feature `test-api` must be an array"), "{message}");
    }

    #[test]
    fn non_string_implementation_owner_fails_instead_of_reading_as_missing() {
        // `features.toml` writes the literal string `missing` for a recorded
        // absence, so a collapse of "any unreadable value" to that sentinel
        // would turn malformed authority data into a fabricated provenance
        // claim: the row would say the authority recorded no owner when in
        // fact the authority said something this code could not read.
        let root = scratch_root("feature-owner-wrong-type");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"preview\"\n\
             advertised = false\nimplementation_owner = 42\n"
        ));
        let message = match derive_features(&root) {
            Ok((_, preview)) => format!("unexpectedly derived {} row(s)", preview.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("has a non-string `implementation_owner`"), "{message}");
    }

    #[test]
    fn blank_capability_gate_does_not_establish_registration() {
        // `established` asserts the surface is wired into its consuming
        // mechanism. An empty string is not a capability gate, so a row must
        // not obtain the strongest claim it carries on the strength of one.
        let root = scratch_root("feature-blank-gate");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"preview\"\nadvertised = false\n\
             capability_gate = \"\"\nregistration = \"registered\"\n"
        ));
        let states = derive_features(&root).map(|(_, preview)| {
            preview.rows.iter().map(|row| row.registration.state).collect::<Vec<_>>()
        });
        let _ = fs::remove_dir_all(&root);
        assert_eq!(states, Ok(vec![RegistrationState::NotEstablished]));
    }

    #[test]
    fn whitespace_only_registration_does_not_establish_registration() {
        let root = scratch_root("feature-blank-registration");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"preview\"\nadvertised = false\n\
             capability_gate = \"gated\"\nregistration = \"   \"\n"
        ));
        let states = derive_features(&root).map(|(_, preview)| {
            preview.rows.iter().map(|row| row.registration.state).collect::<Vec<_>>()
        });
        let _ = fs::remove_dir_all(&root);
        assert_eq!(states, Ok(vec![RegistrationState::NotEstablished]));
    }

    #[test]
    fn real_capability_gate_and_registration_still_establish() {
        // The control that keeps the blankness rule from rejecting a row that
        // genuinely records both.
        let root = scratch_root("feature-established");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"preview\"\nadvertised = false\n\
             capability_gate = \"gated\"\nregistration = \"registered\"\n"
        ));
        let states = derive_features(&root).map(|(_, preview)| {
            preview.rows.iter().map(|row| row.registration.state).collect::<Vec<_>>()
        });
        let _ = fs::remove_dir_all(&root);
        assert_eq!(states, Ok(vec![RegistrationState::Established]));
    }

    #[test]
    fn absent_implementation_owner_still_reads_as_the_recorded_absence() {
        // The control that keeps the type check above from being over-strict:
        // an absent key is the authority recording no owner, which is a fact
        // the inventory reports as `unowned` rather than an error.
        let root = scratch_root("feature-owner-absent");
        assert!(write(
            &root,
            FEATURES_TOML,
            "[[feature]]\nid = \"lsp.example\"\nmaturity = \"preview\"\nadvertised = false\n"
        ));
        let owners = derive_features(&root).map(|(_, preview)| {
            preview.rows.iter().map(|row| row.owner.clone()).collect::<Vec<_>>()
        });
        let _ = fs::remove_dir_all(&root);
        assert_eq!(owners, Ok(vec![UNOWNED.to_string()]));
    }
}
