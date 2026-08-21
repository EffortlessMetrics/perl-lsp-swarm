//! Clippy lint policy coherence checks.

mod model;
mod read;
mod summary;
mod validate;

#[cfg(test)]
mod tests;

use chrono::Local;
use color_eyre::eyre::Result;
use model::DebtLedger;
use std::path::Path;

pub(super) const ROOT_MANIFEST: &str = "Cargo.toml";
pub(super) const CLIPPY_CONFIG: &str = "clippy.toml";
pub(super) const RUST_TOOLCHAIN: &str = "rust-toolchain.toml";
pub(super) const GATE_POLICY: &str = ".ci/gate-policy.yaml";
pub(super) const LINT_LEDGER: &str = "policy/clippy-lints.toml";
pub(super) const LINT_CATALOG_DIR: &str = "policy/clippy-lints.d";
pub(super) const DEBT_LEDGER: &str = "policy/clippy-debt.toml";

pub fn run() -> Result<()> {
    let root = Path::new(".");
    let cargo = read_toml(root.join(ROOT_MANIFEST))?;
    let lint_ledger: LintLedger = read_toml_as(root.join(LINT_LEDGER))?;
    let debt_ledger: DebtLedger = read_toml_as(root.join(DEBT_LEDGER))?;

    validate_policy_header(&lint_ledger)?;
    validate_msrv(&cargo, &lint_ledger)?;
    validate_workspace_lints(&cargo, &lint_ledger)?;
    validate_workspace_members_inherit_lints(root, &cargo)?;
    validate_clippy_config(root.join(CLIPPY_CONFIG), &lint_ledger)?;
    validate_planned_lints(&cargo, &lint_ledger)?;
    validate_debt_ledger(&debt_ledger)?;

    println!(
        "lint policy ok: {} lint entries, {} planned flips, {} debt entries",
        lint_ledger.lint.len(),
        lint_ledger.planned.len(),
        debt_ledger.debt.len()
    );
    Ok(())
}

fn read_toml(path: PathBuf) -> Result<Value> {
    let content = fs::read_to_string(&path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

fn read_toml_as<T>(path: PathBuf) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(&path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

fn validate_policy_header(ledger: &LintLedger) -> Result<()> {
    if ledger.schema != 1 {
        bail!("{} schema must be 1", LINT_LEDGER);
    }
    if !ledger.policy.panic_free_tests {
        bail!("{} must set policy.panic_free_tests = true", LINT_LEDGER);
    }
    if ledger.policy.suppression_style != "expect-with-reason" {
        bail!("{} must set policy.suppression_style = \"expect-with-reason\"", LINT_LEDGER);
    }
    if ledger.policy.blanket_categories {
        bail!("{} must set policy.blanket_categories = false", LINT_LEDGER);
    }
    Ok(())
}

fn validate_msrv(cargo: &Value, ledger: &LintLedger) -> Result<()> {
    let msrv = value_at(cargo, &["workspace", "package", "rust-version"])?
        .as_str()
        .ok_or_else(|| eyre!("workspace.package.rust-version must be a string"))?;
    if msrv != ledger.msrv {
        bail!(
            "workspace.package.rust-version ({msrv}) must match {LINT_LEDGER} msrv ({})",
            ledger.msrv
        );
    }
    Ok(())
}

fn validate_workspace_lints(cargo: &Value, ledger: &LintLedger) -> Result<()> {
    let cargo_lints = collect_workspace_lints(cargo)?;
    let mut seen = BTreeSet::new();
    for lint in &ledger.lint {
        validate_lint_entry(lint)?;
        if !seen.insert(lint.name.clone()) {
            bail!("duplicate lint ledger entry for {}", lint.name);
        }
        match lint.status.as_str() {
            "active" | "debt" => {
                let cargo_level = cargo_lints
                    .get(&lint.name)
                    .ok_or_else(|| eyre!("active lint {} is missing from Cargo.toml", lint.name))?;
                if cargo_level != &lint.level {
                    bail!(
                        "lint {} level mismatch: Cargo.toml has {cargo_level}, ledger has {}",
                        lint.name,
                        lint.level
                    );
                }
            }
            "tracked" => {
                if cargo_lints.contains_key(&lint.name) {
                    bail!(
                        "tracked lint {} is already active in Cargo.toml; mark it active or debt",
                        lint.name
                    );
                }
            }
            _ => bail!("lint {} has unsupported status {}", lint.name, lint.status),
        }
    }
    Ok(())
}

fn validate_lint_entry(lint: &LintEntry) -> Result<()> {
    if lint.name.trim().is_empty() {
        bail!("lint entry name cannot be empty");
    }
    if !matches!(lint.level.as_str(), "allow" | "warn" | "deny" | "forbid") {
        bail!("lint {} has unsupported level {}", lint.name, lint.level);
    }
    if !matches!(lint.status.as_str(), "active" | "debt" | "tracked") {
        bail!("lint {} must have status active, debt, or tracked", lint.name);
    }
    if lint.class.trim().is_empty() {
        bail!("lint {} must have a class", lint.name);
    }
    if lint.reason.trim().is_empty() {
        bail!("lint {} must have a reason", lint.name);
    }
    Ok(())
}

fn collect_workspace_lints(cargo: &Value) -> Result<BTreeMap<String, String>> {
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
            output.insert(format!("{tool}::{}", name.replace('-', "_")), level);
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

fn validate_workspace_members_inherit_lints(root: &Path, cargo: &Value) -> Result<()> {
    let members = value_at(cargo, &["workspace", "members"])?
        .as_array()
        .ok_or_else(|| eyre!("workspace.members must be an array"))?;
    let mut missing = Vec::new();
    for member in members {
        let Some(member_path) = member.as_str() else {
            bail!("workspace.members must contain only strings");
        };
        let manifest_path = root.join(member_path).join("Cargo.toml");
        let manifest = read_toml(manifest_path.clone())?;
        let inherits = value_at(&manifest, &["lints", "workspace"])
            .and_then(|value| {
                value.as_bool().ok_or_else(|| eyre!("[lints].workspace must be boolean"))
            })
            .unwrap_or(false);
        if !inherits {
            missing.push(manifest_path.display().to_string());
        }
    }
    if !missing.is_empty() {
        bail!("workspace members missing [lints] workspace = true: {}", missing.join(", "));
    }
    Ok(())
}

fn validate_clippy_config(path: PathBuf, ledger: &LintLedger) -> Result<()> {
    let content = fs::read_to_string(&path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    if !ledger.policy.allow_test_carveouts {
        for carveout in TEST_CARVEOUTS {
            if !content.contains(carveout) {
                continue;
            }
            bail!("{} must not contain test carveout {carveout}", path.display());
        }
    }
    let config: Value = toml::from_str(&content)
        .map_err(|err| eyre!("failed to parse {}: {err}", path.display()))?;
    if let Some(msrv) = config.get("msrv").and_then(Value::as_str)
        && msrv != ledger.msrv
    {
        bail!("{} msrv ({msrv}) must match {LINT_LEDGER} msrv ({})", path.display(), ledger.msrv);
    }
    Ok(())
}

fn validate_planned_lints(cargo: &Value, ledger: &LintLedger) -> Result<()> {
    let cargo_lints = collect_workspace_lints(cargo)?;
    let planned_names: BTreeSet<_> = ledger.planned.iter().map(|lint| lint.name.as_str()).collect();
    for required in REQUIRED_PLANNED {
        if !planned_names.contains(required) {
            bail!("{} is missing planned lint {required}", LINT_LEDGER);
        }
    }
    for planned in &ledger.planned {
        validate_planned_lint(planned)?;
        if compare_versions(&planned.activate_when_msrv, &ledger.msrv)? > 0
            && cargo_lints.contains_key(&planned.name)
        {
            bail!(
                "planned lint {} must not be active before MSRV {}",
                planned.name,
                planned.activate_when_msrv
            );
        }
    }
    Ok(())
}

fn validate_planned_lint(planned: &PlannedLint) -> Result<()> {
    if planned.name.trim().is_empty() {
        bail!("planned lint name cannot be empty");
    }
    if !matches!(planned.level.as_str(), "warn" | "deny" | "forbid") {
        bail!("planned lint {} has unsupported level {}", planned.name, planned.level);
    }
    if planned.activate_when_msrv.trim().is_empty() {
        bail!("planned lint {} must have activate_when_msrv", planned.name);
    }
    if planned.reason.trim().is_empty() {
        bail!("planned lint {} must have a reason", planned.name);
    }
    Ok(())
}

fn validate_debt_ledger(ledger: &DebtLedger) -> Result<()> {
    if ledger.schema != 1 {
        bail!("{} schema must be 1", DEBT_LEDGER);
    }
    let today = Local::now().date_naive();

    validate::validate_all(root, &cargo, &lint_ledger, &debt_ledger, today)?;

    print!("{}", summary::render_policy_summary(&lint_ledger, &debt_ledger));
    Ok(())
}
