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
