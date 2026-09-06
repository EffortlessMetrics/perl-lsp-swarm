use super::model::{DebtStatus, InstrumentStatus, Inventory};
use super::projection::{canonical_json, semantic_delta};
use super::read_to_string;
use color_eyre::eyre::{Result, eyre};
use std::path::{Path, PathBuf};

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
        if matches!(row.status, DebtStatus::ConvertedAbsent | DebtStatus::StaleRegistry)
            && row.kind == "registry"
            && inventory.instruments.iter().any(|instrument| {
                instrument.kind == "source_parse"
                    && instrument.status == InstrumentStatus::NotProven
                    && instrument.subject == row.path
            })
        {
            findings.push(format!(
                "absence claimed without successful coverage of {}:{}",
                row.path, row.entrypoint
            ));
        }
    }

    let on_disk_tests = collect_test_files(root, inventory);
    let known: std::collections::BTreeSet<_> =
        inventory.population.files.iter().map(|file| file.path.as_str()).collect();
    for path in on_disk_tests {
        if !known.contains(path.as_str()) {
            findings.push(format!("test-bearing file missing from population: {path}"));
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

fn collect_test_files(root: &Path, inventory: &Inventory) -> Vec<String> {
    let mut package_roots = std::collections::BTreeSet::new();
    package_roots.insert(root.join("crates"));
    package_roots.insert(root.join("xtask"));
    for package in &inventory.population.packages {
        if let Some(parent) = Path::new(&package.manifest).parent() {
            package_roots.insert(root.join(parent));
        }
    }
    let mut files = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for base in package_roots {
        collect_package_test_files(root, &base, &mut files, &mut seen);
    }
    files.sort();
    files.dedup();
    files
}

fn collect_package_test_files(
    root: &Path,
    base: &Path,
    files: &mut Vec<String>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
) {
    let manifest = base.join("Cargo.toml");
    if manifest.is_file() {
        if !seen.insert(manifest) {
            return;
        }
        files.extend(super::topology::test_bearing_files_for_package(root, base));
        return;
    }
    if !base.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let mut children: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    children.sort();
    for child in children {
        let name = child.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if name == "target" || name == ".git" {
            continue;
        }
        if child.is_dir() && child.join("Cargo.toml").is_file() {
            collect_package_test_files(root, &child, files, seen);
        }
    }
}
