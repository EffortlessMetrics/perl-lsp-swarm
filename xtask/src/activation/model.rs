//! Typed activation inventory model (`activation_inventory.v1`, #9204).

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const SCHEMA_VERSION: &str = "activation_inventory.v1";
pub const POLICY_NAME: &str = "activation-inventory";
pub const OWNER: &str = "architecture/activation";
pub const CONTROLLING_ISSUE: &str = "#9204";
pub const SCHEMA_PATH: &str = "schemas/activation_inventory.v1.schema.json";
pub const INVENTORY_PATH: &str = "policy/activation-inventory.v1.json";
pub const OVERRIDES_PATH: &str = "policy/activation-overrides.toml";

/// One or more activation inventory violations, reported together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationError(String);

impl ActivationError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// Build one combined error from a non-empty violation list. Callers
    /// that already know the list is non-empty (e.g. after a typed-decode
    /// failure) use this directly instead of routing through a `Result`.
    pub(crate) fn many(violations: &[String]) -> Self {
        let mut message =
            format!("activation inventory check failed with {} violation(s):", violations.len());
        for violation in violations {
            message.push_str("\n  - ");
            message.push_str(violation);
        }
        Self(message)
    }

    pub(crate) fn from_violations(violations: Vec<String>) -> Result<(), Self> {
        if violations.is_empty() { Ok(()) } else { Err(Self::many(&violations)) }
    }
}

impl Display for ActivationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ActivationError {}

/// Closed activation class vocabulary (exactly eight classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationClass {
    Product,
    Preview,
    CompatibilityShim,
    TestApi,
    Lab,
    Oracle,
    Benchmark,
    Gate,
}

impl ActivationClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Preview => "preview",
            Self::CompatibilityShim => "compatibility_shim",
            Self::TestApi => "test_api",
            Self::Lab => "lab",
            Self::Oracle => "oracle",
            Self::Benchmark => "benchmark",
            Self::Gate => "gate",
        }
    }

    /// Every activation class the schema admits.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Product,
            Self::Preview,
            Self::CompatibilityShim,
            Self::TestApi,
            Self::Lab,
            Self::Oracle,
            Self::Benchmark,
            Self::Gate,
        ]
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        Self::all().iter().copied().find(|class| class.as_str() == value)
    }
}

/// Where a row's class came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassAuthorityKind {
    Derived,
    Override,
}

/// Class provenance: which authority and rule assigned the class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassAuthority {
    pub kind: ClassAuthorityKind,
    pub authority: String,
    pub rule: String,
}

/// Established vs not-established registration state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationState {
    Established,
    NotEstablished,
}

/// Whether and how a surface is wired into its consuming mechanism.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub state: RegistrationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One proof reference for a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofReference {
    pub class: String,
    pub id: String,
}

/// Publication state vocabulary.
///
/// `published` and `publish_allowed` are deliberately distinct. A repository
/// file can prove that publication is *permitted* — a `[workspace.metadata.publish]`
/// allow list, an absent `publish = false` — but only a registry lookup can
/// prove a version was actually published. Collapsing the two would let an
/// in-repository permission masquerade as an external fact, which is exactly
/// the kind of unearned claim this inventory exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    Published,
    PublishAllowed,
    PrivateWorkspaceMember,
    Unpublished,
    NotApplicable,
}

/// Publication disposition and its authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub state: PublicationState,
    pub authority: String,
}

/// Fixed pre-#9205 promotion state. #9205 owns activation-verdict evaluation;
/// the initial inventory never claims one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionState {
    NotEvaluated,
}

/// Promotion disposition. Always `not_evaluated` in this inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    pub state: PromotionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
}

/// Retirement plan. Required iff `class == compatibility_shim`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retirement {
    pub owner: String,
    pub boundary: String,
}

/// One classified activation surface row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationRow {
    pub surface_id: String,
    pub class: ActivationClass,
    pub class_authority: ClassAuthority,
    pub semantic_authority: String,
    pub consumers: Vec<String>,
    pub compile_profiles: Vec<String>,
    pub registration: Registration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observable_contract: Option<String>,
    pub proof_references: Vec<ProofReference>,
    pub publication: Publication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maturity_authority: Option<String>,
    pub owner: String,
    pub promotion: Promotion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<Retirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// One row in the derivation summary table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivationEntry {
    pub rule: String,
    pub authority: String,
    pub emits: String,
    pub considered: usize,
    pub emitted: usize,
    pub not_seeded_reason: String,
}

/// The full generated/committed activation inventory artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationInventory {
    pub schema: String,
    pub schema_version: String,
    pub policy: String,
    pub owner: String,
    pub controlling_issue: String,
    pub derivation: Vec<DerivationEntry>,
    pub rows: Vec<ActivationRow>,
}

impl ActivationInventory {
    /// Serialize deterministically: pretty-printed JSON plus a trailing newline.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ActivationError> {
        let mut text = serde_json::to_string_pretty(self).map_err(|error| {
            ActivationError::new(format!("cannot serialize inventory: {error}"))
        })?;
        text.push('\n');
        Ok(text.into_bytes())
    }
}
