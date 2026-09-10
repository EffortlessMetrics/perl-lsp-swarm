use super::model::{DebtStatus, InstrumentStatus, Inventory};
use super::projection::{canonical_json, semantic_delta};
use super::read_to_string;
use color_eyre::eyre::{Result, eyre};
use std::path::Path;

pub struct CheckRequest<'a> {
    pub root: &'a Path,
    pub current: &'a Inventory,
    pub artifact: Option<&'a Path>,
    pub baseline: Option<&'a Path>,
}

#[derive(Clone, Debug, Default)]
pub struct CheckResult {
    pub ok: bool,
    pub findings: Vec<String>,
}

pub fn check_inventory(request: CheckRequest<'_>) -> Result<CheckResult> {
    let mut findings = Vec::new();
    findings.extend(integrity_findings(request.root, request.current));

    if let Some(path) = request.artifact {
        let artifact = load_inventory(path)?;
        let current_json = canonical_json(request.current)?;
        let artifact_json = canonical_json(&artifact)?;
        if current_json != artifact_json {
            findings.push(format!(
                "artifact {} does not match current source projection",
                path.display()
            ));
        }
        if artifact.counts.rows != artifact.rows.len()
            || artifact.counts.files != artifact.population.files.len()
        {
            findings.push("hand-edited counts do not match row identity arrays".to_string());
        }
    }

    if let Some(path) = request.baseline {
        let baseline = load_inventory(path)?;
        if baseline.schema != super::model::SCHEMA {
            return Err(eyre!(
                "baseline {} has unsupported schema {}",
                path.display(),
                baseline.schema
            ));
        }
        let delta = semantic_delta(&baseline, request.current);
        if !delta.added.is_empty() {
            findings.push(format!(
                "baseline {} is missing {} current identit(y/ies); count equality is not identity",
                path.display(),
                delta.added.len()
            ));
        }
        if baseline.counts.rows == request.current.counts.rows && !delta.added.is_empty() {
            findings.push(
                "row counts match but identities differ (moved or substituted site)".to_string(),
            );
        }
        for added in &delta.added {
            let unowned = request
                .current
                .rows
                .iter()
                .any(|row| row.identity_key() == *added && row.status == DebtStatus::Unowned);
            if unowned {
                findings.push(
                    "ordinary regeneration cannot absorb a new unowned site into an accepted baseline"
                        .to_string(),
                );
                break;
            }
        }
        let baseline_by_id: std::collections::BTreeMap<_, _> =
            baseline.rows.iter().map(|row| (row.identity_key(), row)).collect();
        for row in &request.current.rows {
            if row.status != DebtStatus::Unowned {
                continue;
            }
            if let Some(previous) = baseline_by_id.get(&row.identity_key())
                && previous.status != DebtStatus::Unowned
            {
                findings.push(
                    "existing baseline identity became unowned; ownership loss is not identity addition"
                        .to_string(),
                );
                break;
            }
        }
    }

    Ok(CheckResult { ok: findings.is_empty(), findings })
}

pub fn integrity_findings(root: &Path, inventory: &Inventory) -> Vec<String> {
    let mut findings = Vec::new();
    if inventory.schema != super::model::SCHEMA {
        findings.push(format!("unexpected schema {}", inventory.schema));
    }
    if inventory.counts.rows != inventory.rows.len() {
        findings.push("derived row count disagrees with rows array".to_string());
    }
    if inventory.counts.files != inventory.population.files.len() {
        findings.push("derived file count disagrees with population.files".to_string());
    }
    if inventory.counts.entrypoints != inventory.population.entrypoints.len() {
        findings.push("derived entrypoint count disagrees with population.entrypoints".to_string());
    }

    let not_proven = inventory
        .instruments
        .iter()
        .filter(|instrument| instrument.status == InstrumentStatus::NotProven)
        .count();
    if not_proven != inventory.counts.instrument_not_proven {
        findings.push("not_proven instruments were collapsed into a derived zero".to_string());
    }
    if inventory.instruments.iter().any(|instrument| {
        instrument.kind == "lint_vocabulary" && instrument.status == InstrumentStatus::NotProven
    }) {
        findings
            .push("lint vocabulary is not_proven; the denominator is not a clean zero".to_string());
    }
    if inventory.instruments.iter().any(|instrument| {
        instrument.kind == "panic_registry" && instrument.status == InstrumentStatus::NotProven
    }) {
        findings
            .push("panic registry is not_proven; exception identities were not loaded".to_string());
    }
    if inventory.instruments.iter().any(|instrument| {
        instrument.kind == "test_topology"
            && instrument.status == InstrumentStatus::NotProven
            && inventory.population.packages.is_empty()
    }) {
        findings
            .push("test topology is not_proven; the denominator is not a clean zero".to_string());
    }

    for row in &inventory.rows {
        if row.status == DebtStatus::ConvertedAbsent && row.kind != "registry" {
            findings.push(format!(
                "converted_absent is only valid with an accepted registry disposition: {}:{}",
                row.path, row.entrypoint
            ));
        }
        if row.status == DebtStatus::ConvertedAbsent
            && row.registry_relation != "retired_absent_from_source"
        {
            findings.push(format!(
                "source disappearance treated as converted without retired disposition: {}",
                row.path
            ));
        }
        let parse_failed = inventory.instruments.iter().any(|instrument| {
            instrument.kind == "source_parse"
                && instrument.status == InstrumentStatus::NotProven
                && instrument.subject == row.path
        });
        if row.status == DebtStatus::StaleRegistry && !parse_failed {
            findings.push(format!(
                "stale registry identity on successfully covered source: {}:{}",
                row.path, row.entrypoint
            ));
        }
        if matches!(row.status, DebtStatus::ConvertedAbsent | DebtStatus::StaleRegistry)
            && row.kind == "registry"
            && parse_failed
        {
            findings.push(format!(
                "absence claimed without successful coverage of {}:{}",
                row.path, row.entrypoint
            ));
        }
    }

    match super::topology::cargo_test_src_paths(root) {
        Ok(on_disk_tests) => {
            let known: std::collections::BTreeSet<_> =
                inventory.population.files.iter().map(|file| file.path.as_str()).collect();
            for path in on_disk_tests {
                if !known.contains(path.as_str()) {
                    findings.push(format!("test-bearing file missing from population: {path}"));
                }
            }
        }
        Err(err) => {
            let topology_failed = inventory.instruments.iter().any(|instrument| {
                instrument.kind == "test_topology"
                    && instrument.status == InstrumentStatus::NotProven
            });
            if !topology_failed {
                findings.push(format!("cargo metadata integrity read failed: {err}"));
            }
        }
    }

    let mut identities = std::collections::BTreeSet::new();
    for row in &inventory.rows {
        let id = row.identity_key();
        if !identities.insert(id.clone()) {
            findings.push(format!("duplicate row identity {id}"));
        }
    }

    findings
}

fn load_inventory(path: &Path) -> Result<Inventory> {
    let raw = read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|err| eyre!("parsing {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        DebtRow, DebtStatus, DerivedCounts, Instrument, InstrumentStatus, Inventory, PRODUCER,
        Population, SCHEMA, TargetKind,
    };
    use super::super::projection::canonical_json;
    use super::{CheckRequest, check_inventory};
    use color_eyre::eyre::Result;
    use std::fs;

    fn inventory_with_topology_gap() -> Inventory {
        Inventory {
            schema: SCHEMA.to_string(),
            producer: PRODUCER.to_string(),
            repository_commit: "test".to_string(),
            digests: Vec::new(),
            instruments: vec![Instrument {
                kind: "test_topology".to_string(),
                subject: "Cargo.toml".to_string(),
                status: InstrumentStatus::NotProven,
                detail: "workspace or package Cargo.toml missing".to_string(),
            }],
            population: Population::default(),
            rows: Vec::new(),
            counts: DerivedCounts {
                files: 0,
                entrypoints: 0,
                rows: 0,
                unowned: 0,
                stale_registry: 0,
                instrument_not_proven: 1,
                observation_complete: false,
                by_family: Vec::new(),
                by_status: Vec::new(),
            },
            limitations: Vec::new(),
        }
    }

    #[test]
    fn check_request_literal_fails_closed_on_unproven_topology() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let current = inventory_with_topology_gap();
        let artifact = dir.path().join("artifact.json");
        fs::write(&artifact, canonical_json(&current)?)?;
        let request = CheckRequest {
            root: dir.path(),
            current: &current,
            artifact: Some(&artifact),
            baseline: None,
        };
        let result = check_inventory(request)?;
        assert!(!result.ok, "unproven topology must not pass: {:?}", result.findings);
        assert!(
            result.findings.iter().any(|finding| finding.contains("test topology is not_proven")),
            "topology finding omitted: {:?}",
            result.findings
        );
        Ok(())
    }

    #[test]
    fn unsupported_baseline_schema_errors() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let current = inventory_with_topology_gap();
        let mut baseline = current.clone();
        baseline.schema = "not_test_panic_family_debt.v1".to_string();
        let path = dir.path().join("baseline.json");
        fs::write(&path, canonical_json(&baseline)?)?;
        let err = check_inventory(CheckRequest {
            root: dir.path(),
            current: &current,
            artifact: None,
            baseline: Some(&path),
        })
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("unsupported schema must error"))?;
        assert!(err.to_string().contains("unsupported schema"), "schema error omitted: {err}");
        Ok(())
    }

    fn site_row(status: DebtStatus, owner: &str) -> DebtRow {
        DebtRow {
            kind: "site".to_string(),
            package: "demo".to_string(),
            target_kind: TargetKind::IntegrationTest,
            path: "tests/owned.rs".to_string(),
            entrypoint: "owned".to_string(),
            site_family: "unwrap".to_string(),
            source_identity: "unwrap()".to_string(),
            selector_identity: "invocation:same:occurrence:1".to_string(),
            declaration_identity: String::new(),
            declaration_scope: String::new(),
            registry_relation: "none".to_string(),
            owner: owner.to_string(),
            status,
            proof_requirement: "source-scan".to_string(),
            limitations: Vec::new(),
        }
    }

    #[test]
    fn owned_identity_becoming_unowned_fails_baseline() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut baseline = inventory_with_topology_gap();
        baseline.rows.push(site_row(DebtStatus::DirectDebt, "#13397"));
        baseline.counts.rows = 1;
        let mut current = baseline.clone();
        current.rows[0] = site_row(DebtStatus::Unowned, "");
        let path = dir.path().join("baseline.json");
        fs::write(&path, canonical_json(&baseline)?)?;
        let result = check_inventory(CheckRequest {
            root: dir.path(),
            current: &current,
            artifact: None,
            baseline: Some(&path),
        })?;
        assert!(!result.ok, "unowned transition passed: {:?}", result.findings);
        assert!(
            result.findings.iter().any(|finding| finding.contains("became unowned")),
            "ownership-loss finding omitted: {:?}",
            result.findings
        );
        Ok(())
    }
}
