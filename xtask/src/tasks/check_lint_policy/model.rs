use color_eyre::eyre::{Result, bail, eyre};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LintLedger {
    pub(super) schema: u64,
    pub(super) msrv: String,
    pub(super) policy: LintPolicy,
    #[serde(default)]
    pub(super) lint: Vec<LintEntry>,
    #[serde(default)]
    pub(super) planned: Vec<PlannedLint>,
    #[serde(default)]
    pub(super) deferred_due: Vec<DeferredLint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LintCatalogFragment {
    pub(super) schema: u64,
    #[serde(default)]
    pub(super) lint: Vec<LintEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LintPolicy {
    pub(super) panic_free_tests: bool,
    pub(super) allow_test_carveouts: bool,
    pub(super) suppression_style: String,
    pub(super) blanket_categories: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LintEntry {
    pub(super) name: String,
    pub(super) level: String,
    pub(super) status: String,
    pub(super) class: String,
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlannedLint {
    pub(super) name: String,
    pub(super) level: String,
    pub(super) activate_when_msrv: String,
    pub(super) class: String,
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeferredLint {
    pub(super) name: String,
    pub(super) level: String,
    pub(super) activate_when_msrv: String,
    pub(super) class: String,
    pub(super) owner: String,
    pub(super) reason: String,
    pub(super) review_after: String,
    pub(super) next_status: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DebtLedger {
    pub(super) schema: u64,
    #[serde(default)]
    pub(super) debt: Vec<DebtEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DebtEntry {
    pub(super) lint: String,
    pub(super) level: String,
    pub(super) path: String,
    pub(super) owner: String,
    pub(super) reason: String,
    pub(super) review_after: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RustToolchainFile {
    pub(super) toolchain: RustToolchain,
}

#[derive(Debug, Deserialize)]
pub(super) struct RustToolchain {
    pub(super) channel: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GatePolicyFile {
    pub(super) global: GateGlobal,
}

#[derive(Debug, Deserialize)]
pub(super) struct GateGlobal {
    pub(super) toolchain: GateToolchain,
}

#[derive(Debug, Deserialize)]
pub(super) struct GateToolchain {
    pub(super) msrv: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RustVersion {
    pub(super) major: u64,
    pub(super) minor: u64,
    pub(super) patch: u64,
}

impl RustVersion {
    pub(super) fn from_text(version: &str) -> Result<Self> {
        let parts: Vec<_> = version.split('.').collect();
        if !(2..=3).contains(&parts.len()) {
            bail!("Rust version {version} must contain two or three numeric components");
        }

        let mut parsed = [0_u64; 3];
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty() {
                bail!("Rust version {version} contains an empty component");
            }
            parsed[index] = part.parse::<u64>().map_err(|err| {
                eyre!("invalid Rust version component {part} in {version}: {err}")
            })?;
        }

        Ok(Self { major: parsed[0], minor: parsed[1], patch: parsed[2] })
    }
}
