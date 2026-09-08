//! Contract test for the callable semantic summary packet and assembler
//! (#12674, I02), mirroring the I01 contract test
//! (`compiler_interprocedural_contract.rs`).
//!
//! Pins: the packet contract lives in perl-semantic-facts (versioned schema,
//! fail-closed validation, no behavior), the assembler lives in
//! perl-semantic-analyzer, the assembler's non-goals stay honest (no
//! composition, call graph, traversal, extraction, or call resolution), and
//! the analyzer test file names the required falsifier classes.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn compiler_interprocedural_summary_contract_lives_in_semantic_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let module =
        fs::read_to_string(root.join("crates/perl-semantic-facts/src/interprocedural.rs"))?;

    assert!(
        module.contains("CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION"),
        "the module must declare CALLABLE_SEMANTIC_SUMMARY_SCHEMA_VERSION for \
         callable_semantic_summary.v1"
    );
    for ty in [
        "pub struct CallableSemanticSummary",
        "pub enum SummaryFacetKind",
        "pub enum SummaryFacetStatus",
        "pub struct FacetCompleteness",
        "pub enum CallableFactRef",
        "pub enum OutboundCallee",
        "pub enum CallResolution",
        "pub struct OutboundCallDependency",
        "pub enum ResultExitKind",
        "pub struct ResultExitRef",
        "pub enum PlaceRole",
        "pub struct BindingPlaceRef",
        "pub enum EffectKind",
        "pub struct EffectRef",
        "pub struct SummaryWorkLedger",
        "pub struct BoundarySiteRef",
    ] {
        assert!(module.contains(ty), "missing packet contract type: {ty}");
    }
    // The packet retains per-site boundary provenance, distinct from the
    // envelope's deduped referenced boundary identity set.
    assert!(
        module.contains("boundary_sites: Vec<BoundarySiteRef>"),
        "the packet must retain per-site boundary provenance"
    );
    // The packet carries its own fail-closed validation seam, anchored to
    // the packet's own impl — a count alone would pass if the packet's
    // validate were deleted and an unrelated one existed.
    let packet_impl = module
        .find("impl CallableSemanticSummary {")
        .ok_or("the packet contract must have its own impl block")?;
    let packet_section =
        module.get(packet_impl..).ok_or("the packet impl anchor must be a valid slice boundary")?;
    assert!(
        packet_section.contains("pub fn validate(&self) -> Result<(), Vec<String>>"),
        "CallableSemanticSummary must carry its own fail-closed validate()"
    );
    // And the module still validates every contract (subject, ref, result,
    // packet).
    assert!(
        module.matches("pub fn validate(&self) -> Result<(), Vec<String>>").count() >= 4,
        "every contract must carry its own fail-closed validation"
    );
    // PlaceRole is a documented passthrough of the interim #2660 lexical-role
    // vocabulary owned by perl-parser-core, not a new authority.
    assert!(
        module.contains("pir::extractor::LexicalRole")
            && module.contains("pir::lexical_contribution::OccurrenceRole")
            && module.contains("#2660"),
        "PlaceRole must document its interim #2660 passthrough authority"
    );
    Ok(())
}

#[test]
fn compiler_interprocedural_summary_assembler_keeps_its_non_goals()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let assembler = fs::read_to_string(
        root.join("crates/perl-semantic-analyzer/src/analysis/callable_semantic_summary.rs"),
    )?;
    let analysis_mod =
        fs::read_to_string(root.join("crates/perl-semantic-analyzer/src/analysis/mod.rs"))?;

    assert!(
        analysis_mod.contains("pub mod callable_semantic_summary;"),
        "the assembler module must be registered in the analysis tree"
    );
    for surface in [
        "pub struct SummaryAssemblyContext",
        "pub struct AssemblyBlocker",
        "pub struct CallableSummaryAssembly",
        "pub fn assemble_callable_summaries",
        "pub fn assemble_from_source",
    ] {
        assert!(assembler.contains(surface), "missing assembler surface: {surface}");
    }
    // Non-goals stay honest: no composing callee facts, no call graph/SCC,
    // no project traversal, no fact extraction, no call resolution. Comment
    // lines are stripped before scanning so prose cannot false-positive.
    let code_only = assembler
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in
        ["fn compose(", "call_graph", "fn traverse", "fn extract_facts", "resolve_call"]
    {
        assert!(
            !code_only.contains(forbidden),
            "the assembler must not implement non-goals: found {forbidden}"
        );
    }
    // The assembler consumes HIR/PIR objects (no source rescanning inside
    // the HIR-file entry point) and references the canonical identity
    // vocabulary.
    assert!(
        assembler.contains("lower_single_body") && assembler.contains("lower_ast"),
        "the assembler must consume the canonical HIR/PIR lowering"
    );
    Ok(())
}

#[test]
fn compiler_interprocedural_summary_falsifier_coverage() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    // Contract-level falsifiers (#12674 required names).
    let contract_tests =
        fs::read_to_string(root.join("crates/perl-semantic-facts/src/interprocedural/tests.rs"))?;
    for falsifier in [
        "falsifier_unknown_call_as_pure",
        "falsifier_missing_as_empty_summary",
        "falsifier_cross_facet_completeness_summary",
        "falsifier_zero_work_summary",
        "falsifier_summary_ordering",
        "falsifier_summary_stale_reuse",
        "falsifier_boundary_site_ledger_mismatch",
    ] {
        assert!(contract_tests.contains(falsifier), "missing contract falsifier test: {falsifier}");
    }
    // Assembler falsifiers over real parsed source.
    let analyzer_tests = fs::read_to_string(
        root.join("crates/perl-semantic-analyzer/tests/callable_semantic_summary.rs"),
    )?;
    for class in [
        "unresolved_call",
        "missing",
        "source_order",
        "cross_facet",
        "stale_reuse",
        "zero_work",
        "deterministic",
        "privacy",
        "accounted",
        "goto",
        "poison_pairing",
        "boundary_sites",
        "content_sensitive",
        "unmodeled_evidence",
        "payloads",
        "maximal_range",
        "false_precision",
        "blocker_not_a_summary",
    ] {
        assert!(analyzer_tests.contains(class), "missing assembler falsifier class: {class}");
    }
    // Input-side identity proof in the parser substrate.
    let parser_tests = fs::read_to_string(
        root.join("crates/perl-parser-core/tests/callable_summary_input_identity.rs"),
    )?;
    assert!(
        parser_tests.contains("callable_summary_input_identity_is_deterministic"),
        "missing input-side identity proof"
    );
    Ok(())
}
