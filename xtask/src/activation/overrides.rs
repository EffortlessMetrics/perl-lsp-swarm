//! Typed loader and structural checks for the hand-maintained activation
//! override ledger (`policy/activation-overrides.toml`, #9204).
//!
//! Overrides exist only for surfaces no derivation rule in [`super::derive`]
//! settles today (`oracle`, `compatibility_shim`). Every row is narrow,
//! owner-bound, and expiry-bound; nothing here is inherited or defaulted.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::model::{
    ActivationClass, ActivationError, ActivationRow, ClassAuthority, ClassAuthorityKind,
    DerivationEntry, OVERRIDES_PATH, Promotion, PromotionState, Publication, PublicationState,
    Registration, RegistrationState, Retirement,
};

/// Top-level shape of the override ledger file.
#[derive(Debug, Clone, Deserialize)]
pub struct OverridesFile {
    pub schema_version: u32,
    pub policy: String,
    pub owner: String,
    pub status: String,
    pub updated: String,
    #[serde(default, rename = "override")]
    pub overrides: Vec<OverrideRecord>,
}

/// The only ledger header this code knows how to interpret. A future
/// `schema_version = 2` must fail closed here rather than being read with v1
/// meaning.
const EXPECTED_SCHEMA_VERSION: u32 = 1;
const EXPECTED_POLICY: &str = "activation-overrides";

/// One hand-maintained override row.
#[derive(Debug, Clone, Deserialize)]
pub struct OverrideRecord {
    pub surface_id: String,
    pub class: String,
    pub semantic_authority: String,
    #[serde(default)]
    pub consumers: Vec<String>,
    #[serde(default)]
    pub compile_profiles: Vec<String>,
    pub publication_state: String,
    pub publication_authority: String,
    #[serde(default)]
    pub retirement_owner: Option<String>,
    #[serde(default)]
    pub retirement_boundary: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub created: Option<String>,
    #[serde(default)]
    pub review_after: Option<String>,
}

/// Load and parse the override ledger. A malformed file is a hard error;
/// the individual field-presence rules below are checked separately so
/// their violations carry the required distinct message substrings.
pub fn load(root: &Path) -> Result<OverridesFile, ActivationError> {
    let text = fs::read_to_string(root.join(OVERRIDES_PATH))
        .map_err(|error| ActivationError::new(format!("{OVERRIDES_PATH}: cannot read: {error}")))?;
    parse(&text)
}

pub(crate) fn parse(text: &str) -> Result<OverridesFile, ActivationError> {
    toml::from_str(text)
        .map_err(|error| ActivationError::new(format!("{OVERRIDES_PATH}: invalid TOML: {error}")))
}

fn publication_state(value: &str) -> Option<PublicationState> {
    Some(match value {
        "published" => PublicationState::Published,
        "private_workspace_member" => PublicationState::PrivateWorkspaceMember,
        "unpublished" => PublicationState::Unpublished,
        "not_applicable" => PublicationState::NotApplicable,
        _ => return None,
    })
}

/// `crate:<name>` is the one surface-id kind no derivation rule in
/// [`super::derive`] ever emits, so it is the only kind an override may
/// introduce rather than reclassify. The crate must still exist: an override
/// may name a surface no rule classifies, never a surface that is not there
/// at all.
fn override_only_target(root: &Path, surface_id: &str) -> Result<(), String> {
    let mut parts = surface_id.splitn(2, ':');
    let (Some("crate"), Some(name)) = (parts.next(), parts.next()) else {
        return Err(format!("override `{surface_id}` targets an unknown surface"));
    };
    if name.is_empty() || !root.join(format!("crates/{name}/Cargo.toml")).is_file() {
        return Err(format!(
            "override `{surface_id}` targets an unknown surface \
             (no crates/{name}/Cargo.toml in this repository)"
        ));
    }
    Ok(())
}

/// Structural checks on the override ledger itself: header identity, required
/// fields, target validity, and no-op detection against the live derivation
/// index.
pub fn validate(
    root: &Path,
    file: &OverridesFile,
    derived: &BTreeMap<String, ActivationClass>,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut seen = BTreeSet::new();

    if file.schema_version != EXPECTED_SCHEMA_VERSION {
        violations.push(format!(
            "{OVERRIDES_PATH}: unsupported schema_version `{}` (expected {EXPECTED_SCHEMA_VERSION})",
            file.schema_version
        ));
    }
    if file.policy != EXPECTED_POLICY {
        violations.push(format!(
            "{OVERRIDES_PATH}: unexpected policy `{}` (expected `{EXPECTED_POLICY}`)",
            file.policy
        ));
    }
    if file.owner.trim().is_empty() {
        violations.push(format!("{OVERRIDES_PATH}: ledger requires an owner"));
    }
    if file.status.trim().is_empty() {
        violations.push(format!("{OVERRIDES_PATH}: ledger requires a status"));
    }
    if file.updated.trim().is_empty() {
        violations.push(format!("{OVERRIDES_PATH}: ledger requires an updated date"));
    }

    for record in &file.overrides {
        if !seen.insert(record.surface_id.clone()) {
            violations
                .push(format!("duplicate surface id `{}` in {OVERRIDES_PATH}", record.surface_id));
        }
        if record.owner.as_deref().unwrap_or("").trim().is_empty() {
            violations.push(format!("override `{}` requires an owner", record.surface_id));
        }
        if record.review_after.as_deref().unwrap_or("").trim().is_empty() {
            violations
                .push(format!("override `{}` requires a review_after date", record.surface_id));
        }
        if record.reason.as_deref().unwrap_or("").trim().is_empty() {
            violations.push(format!("override `{}` requires a reason", record.surface_id));
        }
        if publication_state(&record.publication_state).is_none() {
            // Without this check an unparseable state would make `build_rows`
            // silently drop the row: the override would vanish from the
            // inventory with no violation reported anywhere. Fail closed.
            violations.push(format!(
                "override `{}`: unknown publication_state `{}`",
                record.surface_id, record.publication_state
            ));
        }
        let Some(class) = ActivationClass::from_str(&record.class) else {
            violations.push(format!(
                "override `{}`: unknown activation class `{}`",
                record.surface_id, record.class
            ));
            continue;
        };
        if class == ActivationClass::CompatibilityShim
            && (record.retirement_owner.is_none() || record.retirement_boundary.is_none())
        {
            violations.push(format!(
                "override `{}`: compatibility shim requires a retirement owner and boundary",
                record.surface_id
            ));
        }
        match derived.get(&record.surface_id) {
            Some(derived_class) if *derived_class == class => {
                violations.push(format!(
                    "override `{}` does not change the derived class `{}`",
                    record.surface_id,
                    class.as_str()
                ));
            }
            Some(_) => {}
            None => {
                if let Err(violation) = override_only_target(root, &record.surface_id) {
                    violations.push(violation);
                }
            }
        }
    }
    violations
}

/// Build activation rows for every structurally sound override row. Callers
/// must run [`validate`] first; rows built from an invalid record (unknown
/// class, unknown publication_state) are simply skipped here rather than
/// silently miscoded, since [`validate`] already reports the exact defect.
pub fn build_rows(file: &OverridesFile) -> Vec<ActivationRow> {
    let mut rows: Vec<ActivationRow> = file
        .overrides
        .iter()
        .filter_map(|record| {
            let class = ActivationClass::from_str(&record.class)?;
            let publication_state = publication_state(&record.publication_state)?;
            let retirement = if class == ActivationClass::CompatibilityShim {
                match (&record.retirement_owner, &record.retirement_boundary) {
                    (Some(owner), Some(boundary)) => {
                        Some(Retirement { owner: owner.clone(), boundary: boundary.clone() })
                    }
                    _ => None,
                }
            } else {
                None
            };
            Some(ActivationRow {
                surface_id: record.surface_id.clone(),
                class,
                class_authority: ClassAuthority {
                    kind: ClassAuthorityKind::Override,
                    authority: OVERRIDES_PATH.to_string(),
                    rule: "override".to_string(),
                },
                semantic_authority: record.semantic_authority.clone(),
                consumers: record.consumers.clone(),
                compile_profiles: record.compile_profiles.clone(),
                registration: Registration {
                    state: RegistrationState::NotEstablished,
                    authority: None,
                    detail: Some(
                        "narrow override row; no runtime registration mechanism established"
                            .to_string(),
                    ),
                },
                data_authority: None,
                observable_contract: None,
                proof_references: Vec::new(),
                publication: Publication {
                    state: publication_state,
                    authority: record.publication_authority.clone(),
                },
                maturity_authority: None,
                owner: record.owner.clone().unwrap_or_default(),
                promotion: Promotion { state: PromotionState::NotEvaluated, blocker: None },
                retirement,
                notes: record.reason.clone(),
            })
        })
        .collect();
    rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));
    rows
}

/// The `override` rule's own derivation summary row.
#[must_use]
pub fn derivation_entry(file: &OverridesFile, emitted: usize) -> DerivationEntry {
    DerivationEntry {
        rule: "override".to_string(),
        authority: OVERRIDES_PATH.to_string(),
        emits: "any".to_string(),
        considered: file.overrides.len(),
        emitted,
        not_seeded_reason: "hand-maintained; every row is a narrow, owner- and \
            expiry-bound exception for a class no derivation rule settles today"
            .to_string(),
    }
}
