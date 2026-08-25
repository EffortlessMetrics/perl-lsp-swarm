use super::model::{LintCatalogFragment, LintLedger};
use super::{LINT_CATALOG_DIR, LINT_LEDGER};
use color_eyre::eyre::{Result, bail, eyre};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;

pub(super) fn read_toml(path: PathBuf) -> Result<Value> {
    let content = fs::read_to_string(&path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

pub(super) fn read_toml_as<T>(path: PathBuf) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(&path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

pub(super) fn read_yaml_as<T>(path: PathBuf) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(&path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    serde_yaml_ng::from_str(&content)
        .map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

pub(super) fn load_lint_ledger(root: &Path) -> Result<LintLedger> {
    let mut ledger: LintLedger = read_toml_as(root.join(LINT_LEDGER))?;
    let catalog_dir = root.join(LINT_CATALOG_DIR);
    let entries = fs::read_dir(&catalog_dir)
        .map_err(|err| eyre!("failed to read {}: {err}", catalog_dir.display()))?;
    let mut paths = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|err| {
            eyre!("failed to inspect an entry in {}: {err}", catalog_dir.display())
        })?;
        let file_type = entry
            .file_type()
            .map_err(|err| eyre!("failed to inspect {}: {err}", entry.path().display()))?;
        if !file_type.is_file() {
            bail!("{} must contain only regular TOML files", catalog_dir.display());
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            bail!("{} must contain only .toml files", catalog_dir.display());
        }
        paths.push(path);
    }

    paths.sort();
    if paths.is_empty() {
        bail!("{} must contain at least one catalog fragment", catalog_dir.display());
    }

    for path in paths {
        let fragment: LintCatalogFragment = read_toml_as(path.clone())?;
        if fragment.schema != 1 {
            bail!("{} catalog fragment schema must be 1", path.display());
        }
        if fragment.lint.is_empty() {
            bail!("{} catalog fragment must contain lint entries", path.display());
        }
        ledger.lint.extend(fragment.lint);
    }

    Ok(ledger)
}

pub(super) fn collect_workspace_lints(cargo: &Value) -> Result<BTreeMap<String, String>> {
    let mut output = BTreeMap::new();
    let workspace_lints = value_at(cargo, &["workspace", "lints"])?
        .as_table()
        .ok_or_else(|| eyre!("workspace.lints must be a table"))?;

    for (tool, table) in workspace_lints {
        let lint_table =
            table.as_table().ok_or_else(|| eyre!("workspace.lints.{tool} must be a table"))?;
        for (name, value) in lint_table {
            let level = lint_level(value).ok_or_else(|| {
                eyre!("workspace.lints.{tool}.{name} must be a string or table with level")
            })?;
            let canonical = format!("{tool}::{}", name.replace('-', "_"));
            if output.insert(canonical.clone(), level).is_some() {
                bail!("Cargo.toml defines duplicate canonical lint {canonical}");
            }
        }
    }

    Ok(output)
}

fn lint_level(value: &Value) -> Option<String> {
    if let Some(level) = value.as_str() {
        return Some(level.to_owned());
    }
    value
        .as_table()
        .and_then(|table| table.get("level"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    let mut current = value;
    for segment in path {
        current =
            current.get(*segment).ok_or_else(|| eyre!("missing TOML key {}", path.join(".")))?;
    }
    Ok(current)
}
