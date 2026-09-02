//! Regex-family bodies stay unmodeled for the Place/Effect completeness law (#7136).
//!
//! `count_unmodeled` in the callable-summary assembler is the fail-closed
//! guard that stops a body whose expressions the per-body PIR lowering cannot
//! model from reporting Place or Effect facets `Complete`.
//!
//! Before regex families were typed in canonical body HIR, `qr//` lowered to
//! `HirExpr::Opaque` and was therefore already counted here. Giving the four
//! families their own `HirExpr` variants moved them out of the counted set and
//! would have silently relaxed that law — a body containing a regex operation
//! could have started reporting those facets `Complete` even though PIR-A
//! still records every regex construct as unsupported (canonical PIR-A regex
//! operations are #7137).
//!
//! These tests pin the law directly, so the guard cannot be dropped again by a
//! refactor that only looks at the HIR variant list.
//!
//! Tests return `Result` and use `ok_or`/`?` rather than `expect`/`panic`,
//! per the workspace lint policy.

use std::error::Error;

use perl_semantic_analyzer::analysis::callable_semantic_summary::{
    CallableSummaryAssembly, SummaryAssemblyContext, assemble_from_source,
};
use perl_semantic_facts::interprocedural::{
    BodyIdentity, CompositionPolicy, PrivacyClass, SummaryFacetKind, SummaryFacetStatus, WorkBudget,
};
use perl_semantic_facts::{FileId, SourceGeneration};

type TestResult = Result<(), Box<dyn Error>>;

fn ctx(generation: &str) -> SummaryAssemblyContext {
    SummaryAssemblyContext {
        document: FileId(1),
        source_generation: SourceGeneration::known(generation),
        body: BodyIdentity::Exact("regex-unmodeled-body-set".to_string()),
        composition_policy: CompositionPolicy::DirectOnly,
        work_budget: WorkBudget::new(10_000),
        privacy: PrivacyClass::PrivateSafe,
    }
}

fn assemble(source: &str, generation: &str) -> Result<CallableSummaryAssembly, Box<dyn Error>> {
    Ok(assemble_from_source(source, &ctx(generation))?)
}

fn facet_status(
    packet: &perl_semantic_facts::interprocedural::CallableSemanticSummary,
    facet: SummaryFacetKind,
) -> Result<SummaryFacetStatus, Box<dyn Error>> {
    packet
        .facets
        .iter()
        .find(|entry| entry.facet == facet)
        .map(|entry| entry.status)
        .ok_or_else(|| format!("facet {facet:?} missing from ledger").into())
}

/// Every regex family keeps Place and Effect out of `Complete`.
///
/// Each source is a callable whose only unmodeled content is one regex-family
/// construct, so a regression that stops counting that family shows up here as
/// a facet becoming `Complete`.
#[test]
fn regex_family_bodies_never_report_place_or_effect_complete() -> TestResult {
    let cases = [
        ("regex literal", "sub f { my $r = qr/foo/i; return $r }"),
        ("match", "sub f { my $x = shift; return $x =~ /foo/ }"),
        ("substitution", "sub f { my $x = shift; $x =~ s/a/b/g; return $x }"),
        ("transliteration", "sub f { my $x = shift; $x =~ tr/a-z/A-Z/; return $x }"),
    ];

    for (label, source) in cases {
        let assembly = assemble(source, "gen-regex")?;
        let packet = assembly
            .summaries
            .first()
            .ok_or_else(|| format!("{label}: expected a summary for {source:?}"))?;
        packet.validate().map_err(|v| format!("{label}: packet must validate: {v:?}"))?;

        for facet in [SummaryFacetKind::Place, SummaryFacetKind::Effect] {
            assert_ne!(
                facet_status(packet, facet)?,
                SummaryFacetStatus::Complete,
                "{label}: {facet:?} must not be Complete while PIR does not model {source:?}"
            );
        }
    }
    Ok(())
}
