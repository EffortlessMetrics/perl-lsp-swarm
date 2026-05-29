use chrono::Utc;
use color_eyre::eyre::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use super::CATEGORIES;

pub(super) fn load(path: &Path) -> Result<HashMap<String, usize>> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    let mut values = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Ok(parsed) = value.trim().parse::<usize>() else {
            continue;
        };
        values.insert(key.trim().to_string(), parsed);
    }
    Ok(values)
}

pub(super) fn write(path: &Path, counts: &HashMap<String, usize>, total: usize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut lines = Vec::new();
    lines.push(format!("# Ignored test baseline - {}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ")));
    lines.push("# Updated by: ignored-test-count.sh --update".to_string());
    let mut ordered = BTreeMap::new();
    for key in CATEGORIES {
        ordered.insert(key, counts.get(key).copied().unwrap_or(0));
    }
    for (key, value) in &ordered {
        lines.push(format!("{key}={value}"));
    }
    lines.push(format!("total={total}"));
    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}
