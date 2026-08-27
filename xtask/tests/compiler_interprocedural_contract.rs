//! Contract test for the interprocedural composition contracts (#12672).
//!
//! Pins the cross-crate contract surface: the three versioned schemas, their
//! owner (perl-semantic-facts — no new public microcrate), the fail-closed
//! validation seam, and the falsifier coverage the issue's execution
//! contract requires.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn interprocedural_contracts_live_in_semantic_facts_with_versioned_schemas()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let module =
        fs::read_to_string(root.join("crates/perl-semantic-facts/src/interprocedural.rs"))?;
    let lib = fs::read_to_string(root.join("crates/perl-semantic-facts/src/lib.rs"))?;

    assert!(
        lib.contains("pub mod interprocedural;"),
        "perl-semantic-facts must own the interprocedural contracts (no new public microcrate)"
    );
    for (schema, version_const) in [
        ("call_application_subject.v1", "CALL_APPLICATION_SUBJECT_SCHEMA_VERSION"),
        ("callable_semantic_summary_ref.v1", "CALLABLE_SEMANTIC_SUMMARY_REF_SCHEMA_VERSION"),
        ("interprocedural_fact_result.v1", "INTERPROCEDURAL_FACT_RESULT_SCHEMA_VERSION"),
    ] {
        assert!(
            module.contains(version_const),
            "the module must declare {version_const} for {schema}"
        );
    }
    for ty in [
        "pub struct CallApplicationSubject",
        "pub struct CallableSemanticSummaryRef",
        "pub struct InterproceduralFactResult",
        "pub enum InterproceduralOutcome",
        "pub enum CompositionPolicy",
        "pub enum ClaimCeiling",
        "pub enum SummaryCurrentness",
    ] {
        assert!(module.contains(ty), "missing contract type: {ty}");
    }
    // References, never redefined fact vocabulary: the summary references
    // canonical facts by identity, and the contracts reuse the crate's
    // envelope types rather than copying their fields.
    assert!(
        module.contains("referenced_facts: Vec<FactId>")
            && module.contains("use crate::{")
            && module.contains("SourceAnchor")
            && module.contains("SourceGeneration"),
        "contracts must reference canonical fact identities, not redefine vocabulary"
    );
    // The fail-closed validation seam exists on every contract.
    assert!(
        module.matches("pub fn validate(&self) -> Result<(), Vec<String>>").count() >= 3,
        "every contract must carry its own fail-closed validation"
    );
    Ok(())
}

#[test]
fn interprocedural_falsifier_suite_covers_the_required_classes()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let tests =
        fs::read_to_string(root.join("crates/perl-semantic-facts/src/interprocedural/tests.rs"))?;

    for falsifier in [
        "duplicate_vocabulary",
        "name_path_output_identity",
        "incomplete_call_subject",
        "cross_facet_strengthening",
        "phase_collapse",
        "hidden_boundary",
        "missing_as_empty",
        "stale_reuse",
        "refusal_as_empty",
        "historical_as_current",
        "ordering",
        "privacy",
        "resource_ceiling",
    ] {
        assert!(
            tests.contains(&format!("falsifier_{falsifier}")),
            "missing falsifier test for class: {falsifier}"
        );
    }
    // Non-goals stay honest: no extraction, traversal, composition, or
    // provider behavior in the contract module.
    let module =
        fs::read_to_string(root.join("crates/perl-semantic-facts/src/interprocedural.rs"))?;
    for forbidden in ["fn extract_facts", "call_graph", "fn compose(", "fn traverse"] {
        assert!(
            !module.contains(forbidden),
            "the contract module must not implement behavior: found {forbidden}"
        );
    }
    Ok(())
}
