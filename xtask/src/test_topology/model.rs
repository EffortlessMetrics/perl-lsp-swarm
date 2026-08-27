//! Versioned topology row model and canonical register loading (#12411).
//!
//! The register is a hand-maintained source of truth under
//! `.ci/test-topology/<cohort>.v1.toml`. Each row names one exact proof
//! target with its package/test subject, route class, affected-subject
//! prefixes, expected nonzero work, budget, and receipt schema. Row identity
//! is the `target_id` slug; nothing depends on filesystem root, metadata
//! ordering, or display command text.
//!
//! Dormancy is explicit: a `declared_pending` row records the leaf identity
//! (with owner issue) before its target exists, carries no execution command,
//! and can never emit work or go green. Activation happens in the landing
//! leaf's own PR by flipping `status` to `active` with exact execution fields
//! plus `min_work_items > 0`; that diff is review-visible.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// Register wire format version.
pub const REGISTER_SCHEMA_VERSION: &str = "test_topology_register.v1";
/// Per-target receipt wire format version.
pub const RECEIPT_SCHEMA_VERSION: &str = "test_topology_receipt.v1";

/// Route disposition of one topology row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteClass {
    /// Executes on candidate changes that touch the row's subjects.
    RequiredAffected,
    /// Extra useful evidence that never satisfies a required row.
    Advisory,
    /// Scheduled pressure lane; never satisfies a required row.
    Scheduled,
    /// Explicit typed-input operation; never satisfies a required row.
    Manual,
}

impl RouteClass {
    /// Whether this class may discharge a required route obligation.
    pub fn satisfies_required(self) -> bool {
        matches!(self, Self::RequiredAffected)
    }

    /// Canonical machine tag used in receipts and fan-in reports.
    pub fn tag(self) -> &'static str {
        match self {
            Self::RequiredAffected => "required_affected",
            Self::Advisory => "advisory",
            Self::Scheduled => "scheduled",
            Self::Manual => "manual",
        }
    }
}

/// Landing state of a registered target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    /// Target exists on main and routes through [`ExecutionKind`].
    Active,
    /// Leaf is declared but unlanded: no execution, no work, never green.
    DeclaredPending,
}

/// Exact execution mechanism of an active row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExecutionKind {
    /// One cargo test invocation against an exact package/target/filter.
    #[serde(rename_all = "snake_case")]
    CargoTest {
        /// Cargo package that owns the test target.
        package: String,
        /// Integration-test binary name; absent means lib/bin suite filters.
        test_target: Option<String>,
        /// Positional libtest name filter applied after `--`. Empty = whole target.
        filter: String,
        /// Feature/build subject flags retained verbatim in rendered argv.
        feature_profile: String,
    },
}

/// One canonical proof-target row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyRow {
    /// Stable slug identity, unique across the register.
    pub target_id: String,
    /// Owning leaf/contract issue for claim-boundary navigation.
    pub owner_issue: u64,
    /// Cohort identifier; one file per cohort keeps rows bounded.
    pub cohort: String,
    /// Product-tree/profile/candidate/store subject in declared vocabulary.
    pub subject: String,
    /// What this proof must never allow, stated as a claim boundary.
    pub claim_boundary: String,
    /// Proof role from the issue's target population vocabulary.
    pub proof_role: String,
    /// Route disposition class.
    pub route_class: RouteClass,
    /// Dormancy state.
    pub status: TargetStatus,
    /// Candidate profiles (namespaces) the row participates in.
    #[serde(default)]
    pub candidate_profiles: Vec<String>,
    /// Path prefixes selecting this row during affected routing.
    #[serde(default)]
    pub subjects: Vec<String>,
    /// Exact execution mechanism; required exactly when status is active.
    #[serde(default)]
    pub execution: Option<ExecutionKind>,
    /// Minimum executed work items; zero is legal only while dormant.
    #[serde(default)]
    pub min_work_items: u32,
    /// Wall-clock budget in seconds for the routed run.
    #[serde(default)]
    pub budget_seconds: u64,
    /// Receipt schema this row emits; pinned to [`RECEIPT_SCHEMA_VERSION`].
    #[serde(default = "default_receipt_schema")]
    pub receipt_schema: String,
}

impl TopologyRow {
    fn validate(&self) -> Result<()> {
        if self.target_id.trim().is_empty() {
            bail!("topology row target_id must not be empty");
        }
        if self.subject.trim().is_empty() {
            bail!("topology row {} subject must not be empty", self.target_id);
        }
        if self.claim_boundary.trim().is_empty() {
            bail!("topology row {} claim_boundary must not be empty", self.target_id);
        }
        if self.proof_role.trim().is_empty() {
            bail!("topology row {} proof_role must not be empty", self.target_id);
        }
        if self.owner_issue == 0 {
            bail!("topology row {} owner_issue must be a real issue number", self.target_id);
        }
        if self.receipt_schema != RECEIPT_SCHEMA_VERSION {
            bail!(
                "topology row {} receipt_schema {:?} must be {:?}",
                self.target_id, self.receipt_schema, RECEIPT_SCHEMA_VERSION
            );
        }
        match self.status {
            TargetStatus::Active => {
                let Some(execution) = &self.execution else {
                    bail!("active row {} requires an exact execution command", self.target_id);
                };
                if execution.cargo_package().trim().is_empty() {
                    bail!("active row {} package must not be empty", self.target_id);
                }
                if self.min_work_items < 1 {
                    bail!(
                        "active row {} must expect nonzero work (min_work_items >= 1)",
                        self.target_id
                    );
                }
                if self.budget_seconds == 0 {
                    bail!("active row {} budget_seconds must be positive", self.target_id);
                }
            }
            TargetStatus::DeclaredPending => {
                if self.execution.is_some() {
                    bail!(
                        "declared_pending row {} cannot carry an execution command",
                        self.target_id
                    );
                }
                if self.min_work_items != 0 {
                    bail!(
                        "declared_pending row {} cannot declare nonzero work before landing",
                        self.target_id
                    );
                }
            }
        }
        Ok(())
    }
}

impl ExecutionKind {
    /// Package whose dependency closure gates selection compile checks.
    pub fn cargo_package(&self) -> &str {
        match self {
            Self::CargoTest { package, .. } => package,
        }
    }
}

/// Cohort header declaration inside a register file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterCohortDeclaration {
    /// Cohort selector name (`--cohort compiler-profile`).
    pub cohort: String,
    /// Stable register identity including schema version suffix.
    pub register_id: String,
    /// Human summary kept short and factual.
    pub description: String,
}

/// Parsed canonical register for one cohort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyRegister {
    /// Wire format version, pinned to [`REGISTER_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Cohort declaration header.
    #[serde(flatten)]
    pub cohort: RegisterCohortDeclaration,
    /// Workspace packages scanned by the omitted-new-target guard.
    #[serde(default)]
    pub watch_packages: Vec<String>,
    /// Test-target name markers triggering register-membership enforcement.
    #[serde(default)]
    pub namespace_markers: Vec<String>,
    /// Every registered row of this cohort (serialized as `[[row]]`).
    #[serde(rename = "row", default)]
    pub rows: Vec<TopologyRow>,
}

/// Canonical receipt schema applied when a row omits the explicit field.
fn default_receipt_schema() -> String {
    RECEIPT_SCHEMA_VERSION.to_owned()
}

impl TopologyRegister {
    /// Parse and validate a register from TOML text.
    pub fn from_str(source: &str) -> Result<Self> {
        let register: Self =
            toml::from_str(source).context("parse test topology register")?;
        register.validate()?;
        Ok(register)
    }

    /// Load and validate the canonical register at `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("read test topology register {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("validate test topology register {}", path.display()))
    }

    /// Structural validation laws for the whole register.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != REGISTER_SCHEMA_VERSION {
            bail!(
                "register schema_version {:?} must be {:?}",
                self.schema_version, REGISTER_SCHEMA_VERSION
            );
        }
        if self.cohort.cohort.trim().is_empty()
            || self.cohort.register_id.trim().is_empty()
            || self.cohort.description.trim().is_empty()
        {
            bail!("register cohort header fields must not be empty");
        }
        let mut seen_ids = BTreeSet::new();
        for row in &self.rows {
            if !seen_ids.insert(row.target_id.as_str()) {
                bail!("duplicate topology target_id {}", row.target_id);
            }
            row.validate()?;
        }
        if self.rows.is_empty() {
            bail!("register {} declares no rows", self.cohort.register_id);
        }
        Ok(())
    }

    /// Rows filtered by cohort-external consumers; order preserved.
    pub fn rows(&self) -> &[TopologyRow] {
        &self.rows
    }
}
