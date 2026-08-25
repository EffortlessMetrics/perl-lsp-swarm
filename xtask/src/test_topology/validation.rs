//! Cross-validation of committed inventories against live discovery.
//!
//! Scaffolding commit (#12125): the checker contract is fixed here —
//! [`validate_inventory`] returns every finding comparing committed rows to
//! discovery — while the comparison itself lands with the implementation
//! commit. The stub reports no findings rather than guessing.

use super::discovery::DiscoveredTarget;
use super::model::{CompileObligationV1, DefaultProfileStateV1, TestTopologyInventoryV1};

/// One checker finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Violation {
    /// Row the finding attaches to (`"<inventory>"` for inventory-level findings).
    pub target_id: String,
    /// Human-readable description with stable wording for tests and receipts.
    pub detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.target_id, self.detail)
    }
}

/// Validates a committed inventory against live discovery results.
///
/// Scaffolding stub: returns no findings; the comparison lands with the
/// #12125 implementation commit.
pub fn validate_inventory(
    _inventory: &TestTopologyInventoryV1,
    _discovered: &[DiscoveredTarget],
) -> Vec<Violation> {
    Vec::new()
}

/// Convenience wrapper returning `Err` listing every finding when the
/// committed inventory is not current.
///
/// Scaffolding stub: always current until the comparison lands.
pub fn ensure_current(
    inventory: &TestTopologyInventoryV1,
    discovered: &[DiscoveredTarget],
) -> Result<(), anyhow::Error> {
    let violations = validate_inventory(inventory, discovered);
    if violations.is_empty() {
        return Ok(());
    }
    let rendered: Vec<String> =
        violations.iter().map(std::string::ToString::to_string).collect();
    Err(anyhow::anyhow!(
        "committed test-topology inventory is stale ({} finding(s)):\n{}",
        violations.len(),
        rendered.join("\n")
    ))
}

/// Compile-obligation/state consistency rule exposed for schema-only paths.
pub fn compile_obligation_matches_state(
    obligation: CompileObligationV1,
    state: DefaultProfileStateV1,
) -> bool {
    matches!(
        (obligation, state),
        (CompileObligationV1::IncludedInCheckAllTargets, DefaultProfileStateV1::IncludedByDefault)
            | (CompileObligationV1::ExplicitFeatureBuildRequired, DefaultProfileStateV1::FeatureGated)
    )
}
