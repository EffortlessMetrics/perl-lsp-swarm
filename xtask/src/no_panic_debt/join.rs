use super::model::{
    ClippyTargetStatus, DebtRow, DebtStatus, DerivedCounts, Discovered, Instrument,
    InstrumentStatus, Inventory, InventoryRequest, PRODUCER, Population, RegistryKey,
    RegistryRecord, RegistryState, SCHEMA, Topology,
};
use super::vocabulary;
use super::{normalize_path, read_to_string, sha256_hex};
use color_eyre::eyre::{Result, eyre};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub(crate) fn join(
    request: InventoryRequest<'_>,
    topology: Topology,
    discovered: Discovered,
) -> Result<Inventory> {
    let mut instruments = Vec::new();
    instruments.extend(topology.instruments);
    instruments.extend(discovered.instruments);

    let registry_path = request.registry_path();
    let mut registry = BTreeMap::new();
    if registry_path.is_file() {
        match load_registry(&registry_path) {
            Ok(loaded) => registry = loaded,
            Err(err) => instruments.push(Instrument {
                kind: "panic_registry".to_string(),
                subject: normalize_path(&registry_path, request.root),
                status: InstrumentStatus::NotProven,
                detail: err.to_string(),
            }),
        }
    } else {
        instruments.push(Instrument {
            kind: "panic_registry".to_string(),
            subject: normalize_path(&registry_path, request.root),
            status: InstrumentStatus::NotProven,
            detail: "panic registry file missing".to_string(),
        });
    }

    if let Some(observation) = request.clippy_observation {
        for target in &observation.targets {
            if matches!(target.status, ClippyTargetStatus::Aborted | ClippyTargetStatus::Missing) {
                instruments.push(Instrument {
                    kind: "clippy".to_string(),
                    subject: format!("{}:{}", target.package, target.target),
                    status: InstrumentStatus::NotProven,
                    detail: format!("clippy target {:?}", target.status),
                });
            }
        }
    }

    let mut rows = Vec::new();
    let mut seen_registry = BTreeSet::new();
    let mut selector_counts = BTreeMap::<(String, String, String, String), usize>::new();

    for site in &discovered.sites {
        let occurrence = selector_counts
            .entry((
                site.path.clone(),
                site.entrypoint.clone(),
                site.family.clone(),
                site.snippet.clone(),
            ))
            .or_default();
        *occurrence += 1;
        let selector = selector_identity(&site.snippet, *occurrence);
        let key = RegistryKey {
            path: site.path.clone(),
            enclosing_test_or_function: site.entrypoint.clone(),
            macro_family: site.family.clone(),
            normalized_snippet: site.snippet.clone(),
            selector_identity: selector.clone(),
        };
        let registry_hit = registry.get(&key);
        if let Some(record) = registry_hit {
            seen_registry.insert(record.key.clone());
        }
        let owner = site
            .covering_owner
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| registry_hit.map(|record| record.accepted_reason.clone()))
            .unwrap_or_default();
        let status = classify_site(registry_hit, request.owner_state, &owner);
        rows.push(DebtRow {
            kind: "site".to_string(),
            package: site.package.clone(),
            target_kind: site.target_kind,
            path: site.path.clone(),
            entrypoint: site.entrypoint.clone(),
            site_family: site.family.clone(),
            source_identity: site.snippet.clone(),
            selector_identity: selector,
            declaration_identity: site.covering_declaration.clone().unwrap_or_default(),
            declaration_scope: site.covering_scope.clone().unwrap_or_default(),
            registry_relation: registry_relation(registry_hit),
            owner,
            status,
            proof_requirement: "source-scan".to_string(),
            limitations: site_limitations(site),
        });
    }

    for declaration in &discovered.declarations {
        let owner = declaration.owner.clone();
        let status = classify_declaration(&owner, request.owner_state);
        rows.push(DebtRow {
            kind: "declaration".to_string(),
            package: declaration.package.clone(),
            target_kind: declaration.target_kind,
            path: declaration.path.clone(),
            entrypoint: declaration.entrypoint.clone(),
            site_family: declaration.lint.clone(),
            source_identity: declaration.snippet.clone(),
            selector_identity: format!(
                "declaration:{}:{}:{}",
                declaration.path, declaration.line, declaration.form
            ),
            declaration_identity: format!("{}:{}", declaration.form, declaration.lint),
            declaration_scope: declaration.scope.clone(),
            registry_relation: "not_applicable".to_string(),
            owner,
            status,
            proof_requirement: "source-declaration".to_string(),
            limitations: Vec::new(),
        });
    }

    let failed_paths: BTreeSet<String> = instruments
        .iter()
        .filter(|instrument| {
            instrument.kind == "source_parse" && instrument.status == InstrumentStatus::NotProven
        })
        .map(|instrument| instrument.subject.clone())
        .collect();

    for (key, record) in &registry {
        if seen_registry.contains(key) {
            continue;
        }
        let coverage =
            source_coverage(request.root, &key.path, &discovered.covered_paths, &failed_paths);
        let (status, relation, limitations) = match coverage {
            SourceCoverage::Covered | SourceCoverage::Absent => match record.state {
                RegistryState::Retired => (
                    DebtStatus::ConvertedAbsent,
                    "retired_absent_from_source".to_string(),
                    vec!["joined from ci/panic_test_identities.json".to_string()],
                ),
                RegistryState::Active => (
                    DebtStatus::StaleRegistry,
                    "active_absent_from_source".to_string(),
                    vec!["joined from ci/panic_test_identities.json".to_string()],
                ),
            },
            SourceCoverage::Failed => (
                DebtStatus::InstrumentNotProven,
                "source_not_proven".to_string(),
                vec![
                    "joined from ci/panic_test_identities.json".to_string(),
                    "source identity was not successfully inspected".to_string(),
                ],
            ),
            SourceCoverage::Unscanned => (
                DebtStatus::InstrumentNotProven,
                "source_uncovered".to_string(),
                vec![
                    "joined from ci/panic_test_identities.json".to_string(),
                    "source identity exists but was outside the scanned test population"
                        .to_string(),
                ],
            ),
        };
        rows.push(DebtRow {
            kind: "registry".to_string(),
            package: String::new(),
            target_kind: super::model::TargetKind::Unknown,
            path: key.path.clone(),
            entrypoint: key.enclosing_test_or_function.clone(),
            site_family: key.macro_family.clone(),
            source_identity: key.normalized_snippet.clone(),
            selector_identity: key.selector_identity.clone(),
            declaration_identity: String::new(),
            declaration_scope: String::new(),
            registry_relation: relation,
            owner: record.accepted_reason.clone(),
            status,
            proof_requirement: "registry-join".to_string(),
            limitations,
        });
    }

    rows.sort_by(|left, right| {
        (&left.kind, &left.path, &left.entrypoint, &left.site_family, &left.selector_identity).cmp(
            &(
                &right.kind,
                &right.path,
                &right.entrypoint,
                &right.site_family,
                &right.selector_identity,
            ),
        )
    });

    let commit = request.repository_commit.clone().unwrap_or_else(|| git_head(request.root));
    let (mut digests, digest_instruments) =
        vocabulary::digest_paths(request.root, request.lint_ledger_path, request.lint_catalog_dir);
    instruments.extend(digest_instruments);
    if registry_path.is_file()
        && let Ok(bytes) = std::fs::read(&registry_path)
    {
        digests.push(super::model::SourceDigest {
            path: normalize_path(&registry_path, request.root),
            sha256: sha256_hex(&bytes),
        });
    }
    if let Ok(bytes) = std::fs::read(request.root.join("Cargo.toml")) {
        digests.push(super::model::SourceDigest {
            path: "Cargo.toml".to_string(),
            sha256: sha256_hex(&bytes),
        });
    }
    digests.sort_by(|left, right| left.path.cmp(&right.path));

    let mut limitations = vec![
        "observation only; not a second allowlist".to_string(),
        "ordinary generation does not call GitHub".to_string(),
        "assert!/assert_eq! are not classified as panic-family debt".to_string(),
        "population is Cargo autodiscovery plus explicit target paths; cargo-metadata cfg/target tables are not a second interpreter".to_string(),
    ];
    if instruments.iter().any(|instrument| instrument.status == InstrumentStatus::NotProven)
        || rows.iter().any(|row| row.status == DebtStatus::InstrumentNotProven)
    {
        limitations
            .push("observation_complete is false; counts are not a complete zero".to_string());
    }

    let population = Population {
        packages: topology.packages,
        files: topology.files,
        entrypoints: discovered.entrypoints,
    };
    let counts = derive_counts(&population, &rows, &instruments);
    Ok(Inventory {
        schema: SCHEMA.to_string(),
        producer: PRODUCER.to_string(),
        repository_commit: commit,
        digests,
        instruments,
        population,
        rows,
        counts,
        limitations,
    })
}

fn classify_site(
    registry: Option<&RegistryRecord>,
    owner_state: Option<&super::model::OwnerState>,
    owner: &str,
) -> DebtStatus {
    if let Some(record) = registry {
        return match record.state {
            RegistryState::Active => DebtStatus::IntentionalExactException,
            RegistryState::Retired => DebtStatus::StaleRegistry,
        };
    }
    if owner_is_stale(owner, owner_state) {
        return DebtStatus::StaleOwner;
    }
    if owner.is_empty() {
        return DebtStatus::Unowned;
    }
    DebtStatus::DirectDebt
}

fn classify_declaration(owner: &str, owner_state: Option<&super::model::OwnerState>) -> DebtStatus {
    if owner_is_stale(owner, owner_state) {
        return DebtStatus::StaleOwner;
    }
    if owner.is_empty() { DebtStatus::Unowned } else { DebtStatus::DirectDebt }
}

fn owner_is_stale(owner: &str, owner_state: Option<&super::model::OwnerState>) -> bool {
    let Some(state) = owner_state else {
        return false;
    };
    if owner.is_empty() {
        return false;
    }
    state.closed_or_missing.contains(owner)
        || owner.strip_prefix('#').is_some_and(|id| state.closed_or_missing.contains(id))
}

fn registry_relation(registry: Option<&RegistryRecord>) -> String {
    match registry {
        None => "unregistered".to_string(),
        Some(record) if record.state == RegistryState::Active => "matched_active".to_string(),
        Some(_) => "matched_retired".to_string(),
    }
}

fn site_limitations(site: &super::model::RawSite) -> Vec<String> {
    let mut limitations = Vec::new();
    if site.feature.is_some() {
        limitations.push("feature-gated; discovered without enabling the feature".to_string());
    }
    if site.platform.is_some() {
        limitations.push("platform-gated; discovered without host filtering".to_string());
    }
    limitations
}

fn derive_counts(
    population: &Population,
    rows: &[DebtRow],
    instruments: &[Instrument],
) -> DerivedCounts {
    let mut by_family = BTreeMap::new();
    let mut by_status = BTreeMap::new();
    for row in rows {
        *by_family.entry(row.site_family.clone()).or_insert(0) += 1;
        *by_status.entry(row.status.as_str().to_string()).or_insert(0) += 1;
    }
    DerivedCounts {
        files: population.files.len(),
        entrypoints: population.entrypoints.len(),
        rows: rows.len(),
        unowned: rows.iter().filter(|row| row.status == DebtStatus::Unowned).count(),
        stale_registry: rows.iter().filter(|row| row.status == DebtStatus::StaleRegistry).count(),
        instrument_not_proven: instruments
            .iter()
            .filter(|instrument| instrument.status == InstrumentStatus::NotProven)
            .count(),
        observation_complete: instruments
            .iter()
            .all(|instrument| instrument.status != InstrumentStatus::NotProven)
            && rows.iter().all(|row| row.status != DebtStatus::InstrumentNotProven),
        by_family: by_family.into_iter().collect(),
        by_status: by_status.into_iter().collect(),
    }
}

enum SourceCoverage {
    Covered,
    Absent,
    Failed,
    Unscanned,
}

fn source_coverage(
    root: &Path,
    path: &str,
    covered: &BTreeSet<String>,
    failed: &BTreeSet<String>,
) -> SourceCoverage {
    if failed.contains(path) {
        return SourceCoverage::Failed;
    }
    if covered.contains(path) {
        return SourceCoverage::Covered;
    }
    if root.join(path).is_file() { SourceCoverage::Unscanned } else { SourceCoverage::Absent }
}

fn load_registry(path: &Path) -> Result<BTreeMap<RegistryKey, RegistryRecord>> {
    let raw = read_to_string(path)?;
    let document: Value =
        serde_json::from_str(&raw).map_err(|err| eyre!("parsing {path:?}: {err}"))?;
    let schema = document
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| eyre!("registry schema_version missing"))?;
    if schema != 1 {
        return Err(eyre!("unsupported registry schema_version {schema}"));
    }
    let sites = document
        .get("sites")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("registry sites missing"))?;
    let mut records = BTreeMap::new();
    for (index, value) in sites.iter().enumerate() {
        let object =
            value.as_object().ok_or_else(|| eyre!("registry entry {} not an object", index + 1))?;
        let required = |field: &str| {
            object
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| eyre!("registry entry {} missing {field}", index + 1))
        };
        let state = match required("state")?.as_str() {
            "active" => RegistryState::Active,
            "retired" => RegistryState::Retired,
            other => return Err(eyre!("registry entry {} invalid state {other}", index + 1)),
        };
        let key = RegistryKey {
            path: required("path")?,
            enclosing_test_or_function: required("enclosing_test_or_function")?,
            macro_family: required("macro_family")?,
            normalized_snippet: required("normalized_snippet")?,
            selector_identity: required("selector_identity")?,
        };
        if records.contains_key(&key) {
            return Err(eyre!(
                "registry entry {} duplicates identity {}:{}:{}:{}",
                index + 1,
                key.path,
                key.enclosing_test_or_function,
                key.macro_family,
                key.selector_identity
            ));
        }
        records.insert(
            key.clone(),
            RegistryRecord { key, accepted_reason: required("accepted_reason")?, state },
        );
    }
    Ok(records)
}

fn selector_identity(snippet: &str, occurrence: usize) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in snippet.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("invocation:{hash:016x}:occurrence:{occurrence}")
}

fn git_head(root: &Path) -> String {
    Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "not_proven".to_string())
}
