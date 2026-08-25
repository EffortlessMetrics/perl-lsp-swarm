//! Cross-validation of committed inventories against live discovery.
//!
//! The checker is the falsifier surface: missing rows, stale rows, duplicate
//! canonical identities, ordering drift, fact/fingerprint drift under an
//! unchanged identity, proof-role misassignment, claimed execution evidence,
//! and invented feature authorities all fail loudly instead of being
//! repaired silently.

use std::collections::BTreeMap;

use super::discovery::{DiscoveredTarget, classify_candidate_profiles, classify_proof_role};
use super::model::{
    CompileObligationV1, DefaultProfileStateV1, ProofRoleV1, TestTopologyInventoryV1,
    TestTopologyRowV1,
};

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
/// Returns every finding; an empty vector means the committed inventory is
/// current for the cohort.
pub fn validate_inventory(
    inventory: &TestTopologyInventoryV1,
    discovered: &[DiscoveredTarget],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    if let Err(error) = inventory.validate() {
        violations
            .push(Violation { target_id: "<inventory>".to_string(), detail: format!("{error:#}") });
        return violations;
    }

    let committed_by_id: BTreeMap<&str, &TestTopologyRowV1> =
        inventory.rows.iter().map(|row| (row.target_id.as_str(), row)).collect();

    let discovered_by_id: BTreeMap<&str, &DiscoveredTarget> =
        discovered.iter().map(|target| (target.target_id.as_str(), target)).collect();

    for target in discovered {
        if !committed_by_id.contains_key(target.target_id.as_str()) {
            violations.push(Violation {
                target_id: target.target_id.clone(),
                detail: "missing topology row: a compiler-critical subject appeared without a \
                     governed identity"
                    .to_string(),
            });
        }
    }

    for row in &inventory.rows {
        let Some(target) = discovered_by_id.get(row.target_id.as_str()) else {
            violations.push(Violation {
                target_id: row.target_id.clone(),
                detail: "stale topology row: the governed subject no longer exists in cargo \
                     metadata"
                    .to_string(),
            });
            continue;
        };
        compare_facts(row, target, &mut violations);
    }
    violations.sort_by(|left, right| {
        (&left.target_id, &left.detail).cmp(&(&right.target_id, &right.detail))
    });
    violations.dedup();
    violations
}

/// Field-by-field fact comparison between a committed row and the live
/// discovery result. Each mismatch becomes one specific finding.
fn compare_facts(
    row: &TestTopologyRowV1,
    target: &DiscoveredTarget,
    violations: &mut Vec<Violation>,
) {
    let mut drift = |detail: String| {
        violations.push(Violation { target_id: row.target_id.clone(), detail });
    };
    if row.path != target.path {
        drift(format!("path drift: committed {} vs discovered {}", row.path, target.path));
    }
    if row.target_kind != target.kind {
        drift(format!(
            "kind confusion or drift: committed {} vs discovered {}",
            row.target_kind.as_token(),
            target.kind.as_token()
        ));
    }
    if row.harness != target.harness {
        drift(format!("harness drift: committed {} vs discovered {}", row.harness, target.harness));
    }
    if row.doctest != target.doctest {
        drift(format!(
            "doctest marker drift: committed {:?} vs discovered {:?}",
            row.doctest, target.doctest
        ));
    }
    if row.feature_subject.required != target.required_features {
        drift(format!(
            "required feature subject drift without an identity change: committed {:?} vs \
             discovered {:?}; the subject fingerprint must move with the subject",
            row.feature_subject.required, target.required_features
        ));
    }
    let expected_state = match target.required_features.is_empty() {
        true => DefaultProfileStateV1::IncludedByDefault,
        false => DefaultProfileStateV1::FeatureGated,
    };
    if row.feature_subject.default_profile_state != expected_state {
        drift(format!(
            "default-profile state drift: committed {:?} vs discovered {:?} \
             (feature-gated-zero subjects stay explicit)",
            row.feature_subject.default_profile_state, expected_state
        ));
    }
    if row.feature_subject.authority_refs.is_empty()
        && expected_state == DefaultProfileStateV1::FeatureGated
    {
        drift(
            "feature subject carries no #3790/#8121 authority reference; rows reference \
         the feature authorities, they never copy or redefine the matrix"
                .to_string(),
        );
    }
    let expected_role = classify_proof_role(&target.package_name, &target.cargo_target_name);
    if row.proof_role != expected_role {
        drift(format!(
            "proof role misassignment: committed {} vs classifier-determined {}",
            role_token(row.proof_role),
            role_token(expected_role)
        ));
    }
    let expected_profiles = classify_candidate_profiles(&target.cargo_target_name);
    if row.candidate_profiles != expected_profiles {
        drift(format!(
            "candidate profile drift: committed [{}] vs classifier-determined [{}]",
            row.candidate_profiles
                .iter()
                .map(|profile| profile.as_token())
                .collect::<Vec<_>>()
                .join(", "),
            expected_profiles
                .into_iter()
                .map(|profile| profile.as_token())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let expected_compile = match expected_state {
        DefaultProfileStateV1::IncludedByDefault => CompileObligationV1::IncludedInCheckAllTargets,
        DefaultProfileStateV1::FeatureGated => CompileObligationV1::ExplicitFeatureBuildRequired,
    };
    if row.compile_obligation != expected_compile {
        drift(format!(
            "compile obligation drift: committed {:?} vs expected {:?} \
             (compile and execution obligations are represented separately)",
            row.compile_obligation, expected_compile
        ));
    }
    let fingerprint_consistent = {
        let mut probe = row.clone();
        probe.subject_fingerprint = String::new();
        probe.compute_fingerprint() == row.subject_fingerprint
    };
    let fingerprint_matches_live = target
        .topology_row()
        .is_ok_and(|expected| expected.subject_fingerprint == row.subject_fingerprint);
    if !fingerprint_consistent || !fingerprint_matches_live {
        drift(
            "subject fingerprint drift: the Cargo-observable subject changed while the \
             canonical identity stayed fixed"
                .to_string(),
        );
    }
}

/// Convenience wrapper returning `Err` listing every finding when the
/// committed inventory is not current.
pub fn ensure_current(
    inventory: &TestTopologyInventoryV1,
    discovered: &[DiscoveredTarget],
) -> Result<(), anyhow::Error> {
    let violations = validate_inventory(inventory, discovered);
    if violations.is_empty() {
        return Ok(());
    }
    let rendered: Vec<String> = violations.iter().map(std::string::ToString::to_string).collect();
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
            | (
                CompileObligationV1::ExplicitFeatureBuildRequired,
                DefaultProfileStateV1::FeatureGated
            )
    )
}

fn role_token(role: ProofRoleV1) -> &'static str {
    role.as_token()
}
