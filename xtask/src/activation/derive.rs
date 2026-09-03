//! Deterministic, offline, file-based derivation of activation rows from
//! existing repository authorities (#9204).
//!
//! Every function here reads exactly one committed authority file (or a
//! sorted directory listing) and turns it into activation rows plus one
//! `DerivationEntry` summary row. Nothing here reaches the network, the
//! process working directory, or wall-clock time — determinism holds
//! regardless of caller CWD or filesystem iteration order because every
//! collection is explicitly sorted before being returned.

use std::collections::{BTreeMap, BTreeSet};
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

/// `features.toml` -> product and preview rows.
///
/// One authority yields two classes, so both rule outputs are returned
/// together: a feature is `product` when it is `proven` AND advertised, and
/// `preview` when its maturity says so. Everything else is deliberately not
/// seeded — earned-claim maturity stays owned by `features.toml`, and the
/// rule records that rather than implying the remainder are ordinary build
/// features.
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

/// Build one row from a `[[feature]]` entry.
///
/// The classification inputs are required rather than defaulted, and an
/// absent optional value is distinguished from a present unreadable one:
/// `features.toml` writes the literal `missing` for a recorded absence, so
/// collapsing a wrong-typed value into that sentinel would fabricate
/// provenance the authority never stated.
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

/// `.ci/gate-policy.yaml` -> gate rows.
///
/// Mirrors `GateDefinition` (`xtask/src/tasks/gates.rs`) rather than being
/// more permissive than it: `tier` and `description` are required there, so
/// a gate missing either is malformed authority data, not a row to record
/// with empty strings. `required` is optional and absent means `true`,
/// matching the runner's own default — recording `false` would understate
/// enforcement.
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

/// Every workspace crate manifest, parsed, in sorted directory order.
///
/// Sorted so derivation does not depend on filesystem iteration order, and
/// entry errors are propagated rather than dropped: a directory that cannot
/// be read is a missing input, not an empty one.
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

/// `crates/*/Cargo.toml` `[[bench]]` targets -> benchmark rows.
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
    index: &GateIndex,
    crate_dir: &str,
    feature: &str,
) -> Result<Option<TestApiSignal>, ActivationError> {
    if declared_test_api_name(feature) {
        return Ok(Some(TestApiSignal::DeclaredByName));
    }
    let sites = feature_usage_closure(root, index, crate_dir, feature)?;
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

/// Files Cargo actually compiles for this crate, in sorted order.
///
/// `src/**` plus the IMMEDIATE children of `tests/` and `benches/`, because
/// those are the paths Cargo turns into targets. Anything nested below
/// `tests/` — most importantly `tests/fixtures/**` — is data a test reads,
/// not code that is built, and must not count as a usage site.
fn compiled_target_files(
    root: &Path,
    crate_dir: &str,
) -> Result<Vec<std::path::PathBuf>, ActivationError> {
    let crate_root = root.join("crates").join(crate_dir);
    let mut files = Vec::new();

    let mut stack = vec![crate_root.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in read_source_dir(&dir, crate_dir)? {
            let path = entry;
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    for directory in ["tests", "benches"] {
        let target_dir = crate_root.join(directory);
        for path in read_source_dir(&target_dir, crate_dir)? {
            if path.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
                // The target root itself, plus the module tree it declares.
                // A nested file is evidence only when a target actually
                // compiles it: `tests/support/mod.rs` reached by `mod support;`
                // is code, while `tests/fixtures/**` is data a test reads.
                // Following `mod` is what separates them — a path heuristic
                // would have to guess, and guessing wrong in the permissive
                // direction is how an unused feature became a claimed test API.
                collect_declared_modules(&path, &mut files, crate_dir)?;
                files.push(path);
            }
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

/// Directory entries, distinguishing "not there" from "there but unreadable".
///
/// An absent `benches/` is a legitimate absence and yields nothing. A
/// directory that exists and cannot be read is a missing input: treating it
/// as empty would drop production gates from the population and let the
/// remaining test-side sites classify a feature test-only.
fn read_source_dir(
    dir: &Path,
    crate_dir: &str,
) -> Result<Vec<std::path::PathBuf>, ActivationError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ActivationError::new(format!(
                "crates/{crate_dir}: cannot read `{}`: {error}",
                dir.file_name().and_then(|name| name.to_str()).unwrap_or("<dir>")
            )));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            ActivationError::new(format!("crates/{crate_dir}: cannot read entry: {error}"))
        })?;
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

/// Files reached by `mod NAME;` declarations from a compiled target root.
///
/// Cargo compiles a test or bench target's whole module tree, so a feature
/// gated only in `tests/support/mod.rs` is a real usage site. Resolution
/// follows Rust's own lookup — `NAME.rs` then `NAME/mod.rs`, relative to the
/// declaring file's directory — and recurses, bounded by a visited set.
/// Inline `mod NAME { … }` blocks need no resolution: they are already in the
/// file being scanned.
fn collect_declared_modules(
    file: &Path,
    files: &mut Vec<std::path::PathBuf>,
    crate_dir: &str,
) -> Result<(), ActivationError> {
    // `is_root` marks a file whose submodules live in its OWN directory
    // rather than in a subdirectory named after it. That is true of a target
    // root (`tests/root.rs` resolves `mod support;` to `tests/support/…`) and
    // of any `mod.rs`; for an ordinary `foo.rs`, submodules live in `foo/`.
    let mut pending = vec![(file.to_path_buf(), true)];
    let mut seen: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    while let Some((current, is_root)) = pending.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }
        // A declared module that cannot be read is a missing input, not an
        // empty one: its gates vanish from the population and the remaining
        // test-side sites classify the feature test-only. Same rule as
        // `read_source_dir` and the gate-index file read — absence of
        // evidence must never be manufactured.
        let text = fs::read_to_string(&current).map_err(|error| {
            ActivationError::new(format!(
                "crates/{crate_dir}: cannot read declared module `{}`: {error}",
                current.display()
            ))
        })?;
        let Some(parent) = current.parent() else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim_start();
            let declaration = trimmed
                .strip_prefix("pub mod ")
                .or_else(|| trimmed.strip_prefix("mod "))
                .or_else(|| trimmed.strip_prefix("pub(crate) mod "));
            let Some(rest) = declaration else {
                continue;
            };
            let Some(name) = rest.strip_suffix(';').map(str::trim) else {
                continue;
            };
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let stem = current.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
            let base =
                if is_root || stem == "mod" { parent.to_path_buf() } else { parent.join(stem) };
            for candidate in [base.join(format!("{name}.rs")), base.join(name).join("mod.rs")] {
                if candidate.is_file() {
                    let nested_root = candidate
                        .file_name()
                        .and_then(|file_name| file_name.to_str())
                        .is_some_and(|file_name| file_name == "mod.rs");
                    files.push(candidate.clone());
                    pending.push((candidate, nested_root));
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Every feature name this file gates on.
///
/// Only forms that actually gate compilation count: an outer or inner
/// attribute (`#[cfg(…)]`, `#![cfg(…)]`, `#[cfg_attr(…)]`) or the `cfg!(…)`
/// macro. Requiring the attribute to START a line is what separates a real
/// gate from the same text quoted inside a string literal — the fixture
/// `assert!(f(r#"#[cfg(feature = "simd")]"#))` is test data, not a usage, and
/// reading it as one classified an unused no-op as a test API.
///
/// An attribute may span lines, so once one opens, continuation lines are
/// joined until its parentheses balance. Whitespace is normalised away,
/// because `feature="x"` and `feature = "x"` are the same gate. Missing a
/// gate is not a harmless under-count: an unseen production gate leaves an
/// all-tests population behind it and turns a product feature into a claimed
/// test API.
fn gated_features(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();

    for rest in text.split("cfg!(").skip(1) {
        if let Some(args) = rest.split(')').next() {
            extract_feature_names(&squeeze(args), &mut found);
        }
    }

    let opens = ["#[cfg(", "#![cfg(", "#[cfg_attr(", "#![cfg_attr("];
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if !opens.iter().any(|open| trimmed.starts_with(open)) {
            continue;
        }
        let mut attribute = trimmed.to_string();
        // Join continuations until the brackets balance, bounded so a
        // malformed file cannot consume the rest of the source.
        for _ in 0..64 {
            if attribute.matches('(').count() <= attribute.matches(')').count() {
                break;
            }
            match lines.next() {
                Some(next) => {
                    attribute.push(' ');
                    attribute.push_str(next.trim());
                }
                None => break,
            }
        }
        extract_feature_names(&squeeze(&attribute), &mut found);
    }
    found
}

fn squeeze(value: &str) -> String {
    value.chars().filter(|character| !character.is_whitespace()).collect()
}

/// Pull every `feature="NAME"` out of already-whitespace-squeezed text.
fn extract_feature_names(squeezed: &str, out: &mut BTreeSet<String>) {
    for rest in squeezed.split("feature=\"").skip(1) {
        if let Some(name) = rest.split('"').next()
            && !name.is_empty()
        {
            out.insert(name.to_string());
        }
    }
}

/// Which features each compiled file gates on, per crate.
///
/// `crate directory -> feature -> sorted site paths`. Built once and shared,
/// because the naive shape — rescanning a crate's sources for every feature,
/// and again for every crate its features forward into — took generation from
/// under a second to three and a half minutes on this repository.
type GateIndex = BTreeMap<String, BTreeMap<String, Vec<String>>>;

/// Read every compiled file of every workspace crate once and record the
/// features it gates on.
fn build_gate_index(root: &Path) -> Result<GateIndex, ActivationError> {
    let mut index = GateIndex::new();
    let crates_dir = root.join("crates");
    let Ok(entries) = fs::read_dir(&crates_dir) else {
        return Ok(index);
    };
    let mut crate_dirs = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| ActivationError::new(format!("crates: cannot read entry: {error}")))?;
        if entry.path().is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            crate_dirs.push(name.to_string());
        }
    }
    crate_dirs.sort();

    for crate_dir in crate_dirs {
        let mut per_feature: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in compiled_target_files(root, &crate_dir)? {
            // Skipping an unreadable file would silently remove evidence: a
            // production gate that cannot be read looks like no gate at all,
            // and the feature is then classified test-only on an incomplete
            // population. Absence of evidence must not be manufactured here.
            let text = fs::read_to_string(&path).map_err(|error| {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                ActivationError::new(format!("{}: cannot read: {error}", relative.display()))
            })?;
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let site = relative.to_string_lossy().replace('\\', "/");
            for feature in gated_features(&text) {
                per_feature.entry(feature).or_default().push(site.clone());
            }
        }
        for sites in per_feature.values_mut() {
            sites.sort();
            sites.dedup();
        }
        index.insert(crate_dir, per_feature);
    }
    Ok(index)
}

/// Every cfg site reached by a feature, including the ones in the crates its
/// own definition forwards to.
///
/// A Cargo feature may be a pure forwarder: `perl-lsp-rs`'s `lsp-ga-lock` is
/// declared as `["perl-lsp-rs-core/lsp-ga-lock"]`, and every cfg site in
/// `perl-lsp-rs` itself is under `tests/`. Judging it on local sites alone
/// classifies a PRODUCTION capability switch as a test API, because the
/// behaviour it actually enables lives in the dependency
/// (`perl-lsp-rs-core/src/protocol/capabilities.rs`). Usage evidence is only
/// sound over the whole enablement closure.
///
/// Cycles are possible in principle, so a visited set bounds the walk.
fn feature_usage_closure(
    root: &Path,
    index: &GateIndex,
    crate_dir: &str,
    feature: &str,
) -> Result<Vec<String>, ActivationError> {
    let mut visited = BTreeSet::new();
    let mut sites = Vec::new();
    collect_feature_usage(root, index, crate_dir, feature, &mut visited, &mut sites)?;
    sites.sort();
    sites.dedup();
    Ok(sites)
}

/// The package a dependency key refers to, following a `package = "…"` rename.
///
/// `alias = { package = "real-name" }` means `alias/feat` forwards into
/// `real-name`. Taking the key at face value would look up a package that
/// does not exist.
fn dependency_package_name(manifest: &toml::Value, dependency: &str) -> String {
    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(renamed) = manifest
            .get(table)
            .and_then(toml::Value::as_table)
            .and_then(|entries| entries.get(dependency))
            .and_then(|entry| entry.get("package"))
            .and_then(toml::Value::as_str)
        {
            return renamed.to_string();
        }
    }
    dependency.to_string()
}

/// The `crates/` directory declaring this package, or `None` if no workspace
/// crate does.
///
/// A package name need not equal its directory name, so the manifests are
/// consulted rather than the path guessed. The common case — they match — is
/// checked first so the scan is only paid when it does not.
fn package_directory(root: &Path, package: &str) -> Option<String> {
    let direct = root.join("crates").join(package).join("Cargo.toml");
    if direct.is_file()
        && fs::read_to_string(&direct)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|manifest| manifest.get("package")?.get("name")?.as_str().map(str::to_string))
            .is_some_and(|name| name == package)
    {
        return Some(package.to_string());
    }
    let entries = fs::read_dir(root.join("crates")).ok()?;
    let mut directories: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    directories.sort();
    directories.into_iter().find(|directory| {
        fs::read_to_string(root.join("crates").join(directory).join("Cargo.toml"))
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|manifest| manifest.get("package")?.get("name")?.as_str().map(str::to_string))
            .is_some_and(|name| name == package)
    })
}

/// Accumulate cfg sites for one feature and everything it forwards into.
///
/// See [`feature_usage_closure`] for why the closure rather than the
/// declaring package is the sound unit of evidence.
fn collect_feature_usage(
    root: &Path,
    index: &GateIndex,
    crate_dir: &str,
    feature: &str,
    visited: &mut BTreeSet<(String, String)>,
    sites: &mut Vec<String>,
) -> Result<(), ActivationError> {
    if !visited.insert((crate_dir.to_string(), feature.to_string())) {
        return Ok(());
    }
    let manifest_path = root.join("crates").join(crate_dir).join("Cargo.toml");
    if !manifest_path.is_file() {
        // A forwarded target outside `crates/` (a registry dependency) has no
        // in-repository source to read, so it contributes no evidence either
        // way. That is a recorded absence, not a failure.
        return Ok(());
    }
    if let Some(found) = index.get(crate_dir).and_then(|features| features.get(feature)) {
        sites.extend(found.iter().cloned());
    }

    let text = fs::read_to_string(&manifest_path).map_err(|error| {
        ActivationError::new(format!("crates/{crate_dir}/Cargo.toml: cannot read: {error}"))
    })?;
    let manifest: toml::Value = toml::from_str(&text).map_err(|error| {
        ActivationError::new(format!("crates/{crate_dir}/Cargo.toml: invalid TOML: {error}"))
    })?;
    let Some(entries) = manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get(feature))
    else {
        return Ok(());
    };
    let entries = entries.as_array().ok_or_else(|| {
        ActivationError::new(format!(
            "crates/{crate_dir}/Cargo.toml: feature `{feature}` must be an array"
        ))
    })?;
    for entry in entries {
        let entry = entry.as_str().ok_or_else(|| {
            ActivationError::new(format!(
                "crates/{crate_dir}/Cargo.toml: feature `{feature}` contains a non-string entry"
            ))
        })?;
        // `dep:name` enables an optional dependency and gates nothing by
        // itself; `pkg/feat` and `pkg?/feat` forward into that package.
        let Some((dependency, forwarded)) = entry.split_once('/') else {
            continue;
        };
        let dependency = dependency.trim_end_matches('?').trim_start_matches("dep:");
        // The name before the slash is a DEPENDENCY key, which is neither a
        // directory name nor necessarily the package name: a renamed
        // dependency (`alias = { package = "real-name" }`) forwards under its
        // alias. Resolving it as a directory would find nothing, silently
        // truncate the evidence, and leave an all-tests population behind —
        // the same over-classification this closure exists to prevent. It is
        // also the assumption `package_name` refuses to make elsewhere in
        // this file.
        let package = dependency_package_name(&manifest, dependency);
        let Some(target_dir) = package_directory(root, &package) else {
            // Not a workspace crate (a registry dependency): no in-repository
            // source to read, so it contributes no evidence either way.
            continue;
        };
        collect_feature_usage(root, index, &target_dir, forwarded, visited, sites)?;
    }
    Ok(())
}

/// `crates/*/Cargo.toml` -> test_api rows, by declared name or proven usage.
///
/// The gate index is built once up front: gathering sites per feature meant
/// rescanning every crate's sources once per feature and again per forwarded
/// crate, which took generation from under a second to three and a half
/// minutes.
fn derive_test_features(root: &Path) -> Result<RuleOutput, ActivationError> {
    let manifests = sorted_crate_manifests(root)?;
    let gate_index = build_gate_index(root)?;
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
            let Some(signal) = test_api_signal(root, &gate_index, crate_dir, feature_name)? else {
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

/// `fuzz/fuzz_targets/*.rs` -> lab rows, identified by registered bin name.
///
/// A target's surface id follows what `cargo fuzz` actually runs, not the
/// source file stem: `fuzz_target_1.rs` is registered as `parser_integration`,
/// and naming the row after the file would invent an identity no consumer
/// uses.
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
    // An absent `[[bin]]` section is a real state (no target is registered);
    // a present one of the wrong shape is malformed authority data. Collapsing
    // both to an empty list would let every fuzz row silently fall back to its
    // source-file stem — reintroducing exactly the misnamed-surface defect an
    // earlier round fixed — while generation still reported success.
    let bins = match fuzz_manifest.get("bin") {
        None => Vec::new(),
        Some(value) => value.as_array().cloned().ok_or_else(|| {
            ActivationError::new(format!(
                "{FUZZ_CARGO_TOML}: `bin` must be an array of [[bin]] targets"
            ))
        })?,
    };

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
    fn quoted_cfg_text_is_not_a_usage_site() {
        // The exact shape review caught: a test asserting on cfg text held in
        // a string literal. The line does not START with the attribute, so it
        // is data, not a gate.
        let root = scratch_root("feature-quoted-cfg");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/thing.rs",
            "fn t() { assert!(selects(r##\"#[cfg(feature = \\\"quiet-feature\\\")]\"##)); }\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_fixture_under_tests_is_not_a_usage_site() {
        // `tests/fixtures/**` is data a test reads, not a Cargo target. A real
        // cfg attribute there gates nothing.
        let root = scratch_root("feature-fixture-cfg");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/fixtures/sample/input.rs",
            "#[cfg(feature = \"quiet-feature\")]\nfn f() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_forwarding_feature_is_judged_on_the_crate_it_enables() {
        // The real `perl-lsp-rs/lsp-ga-lock` shape: every local cfg site is a
        // test, but the feature forwards to a dependency where it gates
        // production code. Judged locally it looks test-only; judged over the
        // enablement closure it is a production capability switch.
        let root = scratch_root("feature-forwarding");
        assert!(write(
            &root,
            "crates/wrapper/Cargo.toml",
            "[package]\nname = \"wrapper\"\n[features]\nga-lock = [\"core-dep/ga-lock\"]\n"
        ));
        assert!(write(
            &root,
            "crates/wrapper/tests/only.rs",
            "#[cfg(feature = \"ga-lock\")]\nfn t() {}\n"
        ));
        assert!(write(
            &root,
            "crates/core-dep/Cargo.toml",
            "[package]\nname = \"core-dep\"\n[features]\nga-lock = []\n"
        ));
        assert!(write(
            &root,
            "crates/core-dep/src/lib.rs",
            "#[cfg(feature = \"ga-lock\")]\npub fn production() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn forwarding_resolves_a_renamed_dependency_to_its_real_package() {
        // The name before the slash is a dependency KEY. Under
        // `alias = { package = "real-name" }` it is neither the package name
        // nor the directory, so resolving it as a directory finds nothing and
        // silently truncates the evidence — leaving an all-tests population
        // and a production feature claimed as a test API.
        let root = scratch_root("feature-renamed-dep");
        assert!(write(
            &root,
            "crates/wrapper/Cargo.toml",
            "[package]\nname = \"wrapper\"\n\
             [dependencies]\ncore-alias = { package = \"real-core\", path = \"../real-core-dir\" }\n\
             [features]\nga-lock = [\"core-alias/ga-lock\"]\n"
        ));
        assert!(write(
            &root,
            "crates/wrapper/tests/only.rs",
            "#[cfg(feature = \"ga-lock\")]\nfn t() {}\n"
        ));
        // Package name differs from BOTH the alias and its directory.
        assert!(write(
            &root,
            "crates/real-core-dir/Cargo.toml",
            "[package]\nname = \"real-core\"\n[features]\nga-lock = []\n"
        ));
        assert!(write(
            &root,
            "crates/real-core-dir/src/lib.rs",
            "#[cfg(feature = \"ga-lock\")]\npub fn production() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_forwarding_feature_whose_closure_is_all_tests_is_still_seeded() {
        // The control: following forwarding edges must not reject a feature
        // that really is test-only everywhere it reaches.
        let root = scratch_root("feature-forwarding-tests");
        assert!(write(
            &root,
            "crates/wrapper/Cargo.toml",
            "[package]\nname = \"wrapper\"\n[features]\nquiet = [\"core-dep/quiet\"]\n"
        ));
        assert!(write(
            &root,
            "crates/wrapper/tests/only.rs",
            "#[cfg(feature = \"quiet\")]\nfn t() {}\n"
        ));
        assert!(write(
            &root,
            "crates/core-dep/Cargo.toml",
            "[package]\nname = \"core-dep\"\n[features]\nquiet = []\n"
        ));
        assert!(write(
            &root,
            "crates/core-dep/tests/also.rs",
            "#[cfg(feature = \"quiet\")]\nfn t() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        // `core-dep/quiet` is seeded on its own merits too: its only cfg site
        // is in that crate's own tests. Both rows are correct.
        assert_eq!(
            ids,
            Ok(vec![
                "cargo-feature:core-dep/quiet".to_string(),
                "cargo-feature:wrapper/quiet".to_string(),
            ])
        );
    }

    #[test]
    fn a_multiline_production_gate_is_not_missed() {
        // A line-anchored matcher that did not join continuations would miss
        // this `src/` gate, leaving an all-tests population behind it and
        // turning a production feature into a claimed test API.
        let root = scratch_root("feature-multiline-gate");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/only.rs",
            "#[cfg(feature = \"quiet-feature\")]\nfn t() {}\n"
        ));
        assert!(write(
            &root,
            "crates/demo/src/lib.rs",
            "#[cfg(all(\n    unix,\n    feature = \"quiet-feature\"\n))]\npub fn p() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_whitespace_variant_gate_is_not_missed() {
        // `feature="x"` and `feature = "x"` are the same gate.
        let root = scratch_root("feature-tight-gate");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/only.rs",
            "#[cfg(feature = \"quiet-feature\")]\nfn t() {}\n"
        ));
        assert!(write(
            &root,
            "crates/demo/src/lib.rs",
            "#[cfg(feature=\"quiet-feature\")]\npub fn p() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
    }

    #[test]
    fn a_cfg_macro_call_in_a_compiled_test_is_a_usage_site() {
        // The control that keeps the syntax rule from being too strict:
        // `cfg!(feature = "...")` gates at runtime and is a real usage.
        let root = scratch_root("feature-cfg-macro");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(
            &root,
            "crates/demo/tests/thing.rs",
            "fn t() { if cfg!(feature = \"quiet-feature\") { } }\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(vec!["cargo-feature:demo/quiet-feature".to_string()]));
    }

    #[test]
    fn malformed_fuzz_bin_section_fails_instead_of_falling_back_to_stems() {
        // A wrong-shaped `bin` collapsing to an empty list would make every
        // fuzz row fall back to its source-file stem, reintroducing the
        // misnamed-surface defect an earlier round fixed, while generation
        // still reported success.
        let root = scratch_root("fuzz-bin-wrong-shape");
        // `bin` must precede the [package] header, or TOML nests it inside
        // that table and the manifest simply has no top-level `bin`.
        assert!(write(&root, "fuzz/Cargo.toml", "bin = \"nope\"\n[package]\nname = \"fuzz\"\n"));
        assert!(write(&root, "fuzz/fuzz_targets/demo.rs", "fn main() {}\n"));
        let message = match derive_fuzz(&root) {
            Ok(output) => format!("unexpectedly derived {} row(s)", output.rows.len()),
            Err(error) => error.to_string(),
        };
        let _ = fs::remove_dir_all(&root);
        assert!(message.contains("`bin` must be an array of [[bin]] targets"), "{message}");
    }

    #[test]
    fn a_gate_in_a_declared_test_module_is_a_usage_site() {
        // Cargo compiles a test target's whole module tree, so a feature
        // gated only in `tests/support/mod.rs` — reached by `mod support;`
        // from the target root — is a real usage site. The repository has
        // exactly this shape in perl-parser.
        let root = scratch_root("nested-module-gate");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(&root, "crates/demo/tests/root.rs", "mod support;\nfn t() {}\n"));
        assert!(write(
            &root,
            "crates/demo/tests/support/mod.rs",
            "#[cfg(feature = \"quiet-feature\")]\npub fn helper() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(vec!["cargo-feature:demo/quiet-feature".to_string()]));
    }

    #[test]
    fn a_gate_in_an_undeclared_fixture_file_is_still_not_a_usage_site() {
        // The control that keeps module-tree following from reopening the
        // hole it replaced: `tests/fixtures/**` is data a test reads, and no
        // `mod` declaration reaches it, so it must not count. This is the
        // `perl-lexer/simd` shape that a path heuristic got wrong.
        let root = scratch_root("nested-fixture-gate");
        assert!(write(
            &root,
            "crates/demo/Cargo.toml",
            "[package]\nname = \"demo\"\n[features]\nquiet-feature = []\n"
        ));
        assert!(write(&root, "crates/demo/tests/root.rs", "fn t() {}\n"));
        assert!(write(
            &root,
            "crates/demo/tests/fixtures/sample.rs",
            "#[cfg(feature = \"quiet-feature\")]\npub fn data() {}\n"
        ));
        let ids = derive_test_features(&root)
            .map(|output| output.rows.iter().map(|row| row.surface_id.clone()).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(ids, Ok(Vec::new()));
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
