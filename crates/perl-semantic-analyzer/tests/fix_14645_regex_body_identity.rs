//! Callable body identity must not report `Exact` when regex-family edits are
//! invisible to the per-body PIR lowering (#14645).

use std::error::Error;

use perl_semantic_analyzer::analysis::callable_semantic_summary::{
    SummaryAssemblyContext, assemble_from_source,
};
use perl_semantic_facts::interprocedural::{
    BodyIdentity, CompositionPolicy, PrivacyClass, WorkBudget,
};
use perl_semantic_facts::{FileId, SourceGeneration};

type TestResult = Result<(), Box<dyn Error>>;

fn ctx_unknown() -> SummaryAssemblyContext {
    SummaryAssemblyContext {
        document: FileId(1),
        source_generation: SourceGeneration::known("gen-14645"),
        body: BodyIdentity::Unknown,
        composition_policy: CompositionPolicy::DirectOnly,
        work_budget: WorkBudget::new(10_000),
        privacy: PrivacyClass::PrivateSafe,
    }
}

fn body_identity(source: &str) -> Result<BodyIdentity, Box<dyn Error>> {
    let assembly = assemble_from_source(source, &ctx_unknown())?;
    let packet =
        assembly.summaries.first().ok_or_else(|| format!("expected one summary for {source:?}"))?;
    Ok(packet.body.clone())
}

#[test]
fn regex_family_bodies_use_non_exact_body_identity() -> TestResult {
    for source in [
        "sub f { my $x; $x =~ /foo/i; return $x; }",
        "sub f { my $x; $x =~ /foo/g; return $x; }",
        "sub f { my $x; $x =~ /bar/i; return $x; }",
    ] {
        assert_eq!(
            body_identity(source)?,
            BodyIdentity::Unknown,
            "regex-family body must not mint Exact identity for {source:?}"
        );
    }
    Ok(())
}

#[test]
fn identical_regex_callables_share_body_identity() -> TestResult {
    let source = "sub f { my $x; $x =~ /foo/i; return $x; }";
    assert_eq!(body_identity(source)?, body_identity(source)?);
    Ok(())
}

#[test]
fn plain_call_targets_remain_exact_and_distinct() -> TestResult {
    let foo = body_identity("sub f { my $x; foo($x); return $x; }")?;
    let bar = body_identity("sub f { my $x; bar($x); return $x; }")?;
    assert!(
        matches!(foo, BodyIdentity::Exact(_)),
        "modeled call body should stay Exact, got {foo:?}"
    );
    assert_ne!(foo, bar, "changed callee must change body identity");
    Ok(())
}
