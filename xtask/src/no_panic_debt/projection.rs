use super::model::Inventory;
use color_eyre::eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

pub fn canonical_json(inventory: &Inventory) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(inventory)
        .map_err(|err| eyre!("serializing inventory: {err}"))?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

pub fn render_human(inventory: &Inventory) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}", inventory.schema);
    let _ = writeln!(out);
    let _ = writeln!(out, "- producer: {}", inventory.producer);
    let _ = writeln!(out, "- commit: {}", inventory.repository_commit);
    let _ = writeln!(out, "- files: {}", inventory.counts.files);
    let _ = writeln!(out, "- entrypoints: {}", inventory.counts.entrypoints);
    let _ = writeln!(out, "- rows: {}", inventory.counts.rows);
    let _ = writeln!(out, "- unowned: {}", inventory.counts.unowned);
    let _ = writeln!(out, "- stale_registry: {}", inventory.counts.stale_registry);
    let _ = writeln!(out, "- instrument_not_proven: {}", inventory.counts.instrument_not_proven);
    let _ = writeln!(out, "- observation_complete: {}", inventory.counts.observation_complete);
    let _ = writeln!(out);
    let _ = writeln!(out, "## Status");
    for (status, count) in &inventory.counts.by_status {
        let _ = writeln!(out, "- {status}: {count}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## Family");
    for (family, count) in &inventory.counts.by_family {
        let _ = writeln!(out, "- {family}: {count}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## Rows");
    let mut grouped: BTreeMap<(String, String, String), Vec<&super::model::DebtRow>> =
        BTreeMap::new();
    for row in &inventory.rows {
        grouped
            .entry((row.package.clone(), row.status.as_str().to_string(), row.site_family.clone()))
            .or_default()
            .push(row);
    }
    for ((package, status, family), rows) in grouped {
        let package = if package.is_empty() { "<none>" } else { package.as_str() };
        let _ = writeln!(out, "### {package} / {status} / {family}");
        for row in rows {
            let _ = writeln!(
                out,
                "- `{}` `{}` {} ({})",
                row.path, row.entrypoint, row.selector_identity, row.owner
            );
        }
        let _ = writeln!(out);
    }
    if !inventory.limitations.is_empty() {
        let _ = writeln!(out, "## Limitations");
        for limitation in &inventory.limitations {
            let _ = writeln!(out, "- {limitation}");
        }
    }
    out
}

/// Identity-level delta. Count equality is not identity equality.
pub fn semantic_delta(baseline: &Inventory, current: &Inventory) -> Delta {
    let base_ids = identities(baseline);
    let current_ids = identities(current);
    Delta {
        added: current_ids.difference(&base_ids).cloned().collect(),
        removed: base_ids.difference(&current_ids).cloned().collect(),
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Delta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

fn identities(inventory: &Inventory) -> BTreeSet<String> {
    inventory.rows.iter().map(|row| row.identity_key()).collect()
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        DebtRow, DebtStatus, DerivedCounts, Inventory, PRODUCER, Population, SCHEMA, TargetKind,
    };
    use super::*;

    fn empty_inventory() -> Inventory {
        Inventory {
            schema: SCHEMA.to_string(),
            producer: PRODUCER.to_string(),
            repository_commit: "base".to_string(),
            digests: Vec::new(),
            instruments: Vec::new(),
            population: Population::default(),
            rows: Vec::new(),
            counts: DerivedCounts {
                files: 0,
                entrypoints: 0,
                rows: 0,
                unowned: 0,
                stale_registry: 0,
                instrument_not_proven: 0,
                observation_complete: true,
                by_family: Vec::new(),
                by_status: Vec::new(),
            },
            limitations: Vec::new(),
        }
    }

    #[test]
    fn semantic_delta_is_identity_not_count() {
        let mut current = empty_inventory();
        current.rows.push(DebtRow {
            kind: "site".to_string(),
            package: "demo".to_string(),
            target_kind: TargetKind::IntegrationTest,
            path: "tests/new.rs".to_string(),
            entrypoint: "added".to_string(),
            site_family: "unwrap".to_string(),
            source_identity: "unwrap()".to_string(),
            selector_identity: "invocation:1".to_string(),
            declaration_identity: String::new(),
            declaration_scope: String::new(),
            registry_relation: "none".to_string(),
            owner: String::new(),
            status: DebtStatus::Unowned,
            proof_requirement: "source-scan".to_string(),
            limitations: Vec::new(),
        });
        let delta = semantic_delta(&empty_inventory(), &current);
        assert_eq!(
            delta,
            Delta { added: vec![current.rows[0].identity_key()], removed: Vec::new() }
        );
        let human = render_human(&current);
        assert!(human.contains("test_panic_family_debt.v1"));
        assert!(canonical_json(&current).is_ok());
    }
}
