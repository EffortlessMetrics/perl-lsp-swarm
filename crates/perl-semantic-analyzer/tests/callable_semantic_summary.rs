//! Assembler falsifiers for callable-local semantic summaries (#12674, I02).
//!
//! Every test assembles from real parsed source through
//! [`assemble_from_source`] and names the wrong-shape it kills: unresolved
//! calls treated as pure, missing evidence as exact empty sets, source-order
//! loss, cross-facet strengthening, stale reuse, zero-work summaries,
//! nondeterminism, privacy leaks, and silently dropped callables.
//!
//! Tests return `Result` and use `ok_or`/`?` rather than `expect`/`panic`,
//! per the workspace lint policy.

use std::error::Error;

use perl_semantic_analyzer::analysis::callable_semantic_summary::{
    AssemblyError, CallableSummaryAssembly, SummaryAssemblyContext, assemble_from_source,
};
use perl_semantic_facts::interprocedural::{
    BodyIdentity, CallResolution, CallableFactRef, ClaimCeiling, CompositionPolicy, OutboundCallee,
    PrivacyClass, ResultExitKind, SummaryCurrentness, SummaryFacetKind, SummaryFacetStatus,
    WorkBudget,
};
use perl_semantic_facts::{FileId, SourceGeneration};

type TestResult = Result<(), Box<dyn Error>>;

fn ctx(generation: &str) -> SummaryAssemblyContext {
    SummaryAssemblyContext {
        document: FileId(1),
        source_generation: SourceGeneration::known(generation),
        body: BodyIdentity::Exact("test-file-body-set".to_string()),
        composition_policy: CompositionPolicy::DirectOnly,
        work_budget: WorkBudget::new(10_000),
        privacy: PrivacyClass::PrivateSafe,
    }
}

fn assemble(source: &str, generation: &str) -> Result<CallableSummaryAssembly, Box<dyn Error>> {
    Ok(assemble_from_source(source, &ctx(generation))?)
}

fn only_summary(
    assembly: &CallableSummaryAssembly,
) -> Result<&perl_semantic_facts::interprocedural::CallableSemanticSummary, Box<dyn Error>> {
    if assembly.summaries.len() != 1 {
        return Err(format!(
            "expected exactly one summary, got {} summaries and {} blockers",
            assembly.summaries.len(),
            assembly.blockers.len()
        )
        .into());
    }
    Ok(&assembly.summaries[0])
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

/// Unresolved call: `sub f { g(1) }` records one unresolved transitive
/// dependency; Result and Effect are NOT Complete; no purity is inferred.
#[test]
fn callable_semantic_summary_unresolved_call_is_never_pure() -> TestResult {
    let assembly = assemble("sub f { g(1) }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    if packet.outbound_calls.len() != 1 {
        return Err(format!(
            "expected one outbound dependency, got {}",
            packet.outbound_calls.len()
        )
        .into());
    }
    let dependency = &packet.outbound_calls[0];
    assert_eq!(dependency.callee, OutboundCallee::Named("g".to_string()));
    assert_eq!(dependency.resolution, CallResolution::UnresolvedTransitive);
    // The dependency names exactly the facets it blocks — never empty.
    assert!(
        dependency.blocked_facets.contains(&SummaryFacetKind::Result)
            && dependency.blocked_facets.contains(&SummaryFacetKind::Effect),
        "an unresolved call must block Result and Effect: {:?}",
        dependency.blocked_facets
    );
    assert_ne!(
        facet_status(packet, SummaryFacetKind::Result)?,
        SummaryFacetStatus::Complete,
        "Result must not be Complete while an unresolved call blocks it"
    );
    assert_ne!(
        facet_status(packet, SummaryFacetKind::Effect)?,
        SummaryFacetStatus::Complete,
        "Effect must not be Complete while an unresolved call blocks it"
    );
    // The call is referenced by canonical identity (a HIR item), not a name.
    assert!(matches!(dependency.call, CallableFactRef::HirItem(_)));
    Ok(())
}

/// Missing-as-empty: AliasEscape is declared NotProven with an unsupported
/// count, never Complete-with-zero.
#[test]
fn callable_semantic_summary_missing_is_never_exact_empty() -> TestResult {
    let assembly = assemble("sub f { my $x = 1; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    let alias = packet
        .facets
        .iter()
        .find(|entry| entry.facet == SummaryFacetKind::AliasEscape)
        .ok_or("AliasEscape facet missing")?;
    assert_eq!(alias.status, SummaryFacetStatus::NotProven);
    assert!(alias.unsupported > 0, "the unsupported family must be declared, not zeroed");
    for facet in [SummaryFacetKind::Diagnostic, SummaryFacetKind::Exception] {
        assert_eq!(
            facet_status(packet, facet)?,
            SummaryFacetStatus::NotProven,
            "{facet:?} must be declared, not fabricated"
        );
    }
    Ok(())
}

/// Source order is preserved for outbound calls, effects, and exits.
#[test]
fn callable_semantic_summary_source_order_is_preserved() -> TestResult {
    let assembly = assemble("sub f { a(); b(); my $x = 1; $x += 2; $z = 3; return $x; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    let callees: Vec<&OutboundCallee> =
        packet.outbound_calls.iter().map(|dependency| &dependency.callee).collect();
    assert_eq!(
        callees,
        vec![&OutboundCallee::Named("a".to_string()), &OutboundCallee::Named("b".to_string())],
        "outbound calls must keep source order [a, b]"
    );

    let effect_kinds: Vec<_> = packet.effects.iter().map(|effect| effect.kind).collect();
    assert_eq!(
        effect_kinds,
        vec![
            perl_semantic_facts::interprocedural::EffectKind::Modify,
            perl_semantic_facts::interprocedural::EffectKind::StashWrite,
            perl_semantic_facts::interprocedural::EffectKind::Assign,
        ],
        "effects must keep lowered source order"
    );

    let exit_kinds: Vec<_> = packet.result_exits.iter().map(|exit| exit.kind).collect();
    assert_eq!(
        exit_kinds,
        vec![ResultExitKind::ExplicitReturn, ResultExitKind::ImplicitFallthrough],
        "the explicit return precedes the implicit fallthrough"
    );
    Ok(())
}

/// Bare `return;` is distinguished from `return EXPR;` by HIR evidence.
#[test]
fn callable_semantic_summary_bare_return_is_evidence_backed() -> TestResult {
    let assembly = assemble("sub f { return; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
    let exit_kinds: Vec<_> = packet.result_exits.iter().map(|exit| exit.kind).collect();
    assert_eq!(exit_kinds, vec![ResultExitKind::BareReturn, ResultExitKind::ImplicitFallthrough]);
    Ok(())
}

/// Cross-facet: a boundary limits only the facets it can invalidate — the
/// Boundary facet stays Complete while Result stays Limited.
#[test]
fn callable_semantic_summary_cross_facet_completeness_stays_specific() -> TestResult {
    let assembly = assemble("sub f { my $x = 1; eval \" $x \"; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    assert_eq!(
        facet_status(packet, SummaryFacetKind::Boundary)?,
        SummaryFacetStatus::Complete,
        "every observed boundary is represented"
    );
    assert!(!packet.summary_ref.referenced_boundaries.is_empty());
    assert_eq!(
        facet_status(packet, SummaryFacetKind::Result)?,
        SummaryFacetStatus::Limited,
        "a dynamic boundary limits Result"
    );
    assert_eq!(
        facet_status(packet, SummaryFacetKind::Control)?,
        SummaryFacetStatus::Limited,
        "no CFG exists; Control stays Limited with the gap declared"
    );
    // The claim ceiling reflects the weakest facet, never the strongest.
    assert_eq!(packet.summary_ref.claim_ceiling, ClaimCeiling::Provisional);
    Ok(())
}

/// Stale reuse: a packet is current only for its named generation; the I01
/// currentness laws hold through the packet/envelope join.
#[test]
fn callable_semantic_summary_stale_reuse_is_explicit() -> TestResult {
    let assembly = assemble("sub f { my $x = 1; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    assert_eq!(
        packet.summary_ref.currentness,
        SummaryCurrentness::Fresh(SourceGeneration::known("gen-1"))
    );
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    // A packet whose envelope claims freshness for a different generation
    // fails the one-freshness-identity join.
    let mut mismatched = packet.clone();
    mismatched.summary_ref.currentness =
        SummaryCurrentness::Fresh(SourceGeneration::known("gen-2"));
    assert!(
        mismatched.validate().is_err(),
        "Fresh(gen-2) over a gen-1 packet must fail the freshness join"
    );

    // A packet re-labeled to a new top-level generation disagrees with its
    // envelope and fails.
    let mut relabeled = packet.clone();
    relabeled.source_generation = SourceGeneration::known("gen-2");
    assert!(relabeled.validate().is_err(), "generation relabeling must fail the join");

    // Explicit staleness with a provisional ceiling is the honest form and
    // validates.
    let mut stale = packet.clone();
    stale.summary_ref.currentness = SummaryCurrentness::Stale;
    stale.validate().map_err(|v| format!("honest stale packet must validate: {v:?}"))?;

    // A fresh assembly under gen-2 is a different subject: the canonical
    // bytes differ.
    let other = assemble("sub f { my $x = 1; }", "gen-2")?;
    let other_packet = only_summary(&other)?;
    assert_ne!(
        packet.canonical_bytes()?,
        other_packet.canonical_bytes()?,
        "a gen-2 packet is a different subject than a gen-1 packet"
    );
    Ok(())
}

/// Zero-work law: a packet claiming success with visited_ops == 0 fails
/// validation, and an empty body yields a blocker instead of a summary.
#[test]
fn callable_semantic_summary_zero_work_never_satisfies() -> TestResult {
    let assembly = assemble("sub f { my $x = 1; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    assert!(packet.work.visited_ops > 0);
    let mut zeroed = packet.clone();
    zeroed.work.visited_ops = 0;
    let violations = match zeroed.validate() {
        Err(violations) => violations,
        Ok(()) => return Err("visited_ops == 0 must fail validation".into()),
    };
    assert!(
        violations.iter().any(|v| v.contains("work law")),
        "visited_ops == 0 must fail: {violations:?}"
    );

    // An empty-bodied sub lowers to zero nodes: blocker, never a summary.
    let empty = assemble("sub g { }", "gen-1")?;
    assert!(empty.summaries.is_empty(), "a zero-node body must not produce a summary");
    assert_eq!(empty.blockers.len(), 1);
    assert!(
        empty.blockers[0].reason.contains("zero"),
        "the blocker names the work law: {}",
        empty.blockers[0].reason
    );
    Ok(())
}

/// Determinism: two assemblies of the same input produce byte-identical
/// canonical JSON, and referenced identity sets are canonically ordered.
#[test]
fn callable_semantic_summary_canonical_bytes_are_deterministic() -> TestResult {
    let source = "sub f { a(); my $x = 1; $x += 2; eval \"$x\"; return $x; }";
    let first = assemble(source, "gen-1")?;
    let second = assemble(source, "gen-1")?;
    let first_packet = only_summary(&first)?;
    let second_packet = only_summary(&second)?;
    assert_eq!(
        first_packet.canonical_bytes()?,
        second_packet.canonical_bytes()?,
        "two assemblies of the same input must be byte-identical"
    );
    // Identity sets are normalized (strictly sorted) regardless of the
    // order evidence was encountered in.
    let boundaries = &first_packet.summary_ref.referenced_boundaries;
    assert!(
        boundaries.windows(2).all(|pair| pair[0] < pair[1]),
        "referenced boundaries must be strictly sorted and deduplicated"
    );
    // The entity identity is stable across assemblies of the same input.
    assert_eq!(first_packet.callable, second_packet.callable);
    Ok(())
}

/// Privacy: serialized packets carry identities, relative anchors, and
/// counts — no absolute paths, no environment values, no source text.
#[test]
fn callable_semantic_summary_serialized_bytes_are_privacy_safe() -> TestResult {
    // A distinctive, escape-free marker embedded in a string literal: if
    // source TEXT ever leaked into the packet, this exact token would appear
    // verbatim in the canonical JSON (identifier names like `$x` are
    // legitimate passthrough identity; literal content is not).
    let source = "sub f { a(); my $x = \"zqx-leak-marker-7f3a\"; return $x; }";
    let assembly = assemble(source, "gen-1")?;
    let packet = only_summary(&assembly)?;
    let json = String::from_utf8(packet.canonical_bytes()?)?;

    assert!(
        !json.contains("zqx-leak-marker-7f3a"),
        "no source literal content may leak into the packet: {json}"
    );
    // No absolute host paths: neither path-shaped substrings nor this
    // crate's own root path may appear in serialized identity fields.
    assert!(!json.contains("/home/"), "no absolute-path-shaped substring may leak: {json}");
    assert!(
        !json.contains(env!("CARGO_MANIFEST_DIR")),
        "the crate root path must never leak into the packet"
    );
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        assert!(!json.contains(&home), "no environment value may leak into the packet");
    }
    // Identities and counts only: the anchor names the document FileId, not
    // a path.
    assert!(json.contains("\"file_id\":1"));
    Ok(())
}

/// Every admitted callable is accounted: named subs and anonymous subs each
/// get exactly one summary or one explicit blocker.
#[test]
fn callable_semantic_summary_every_admitted_callable_is_accounted() -> TestResult {
    let source = "sub a { my $x = 1; } sub b { return 2; } my $cb = sub { return; }; sub empty { }";
    let assembly = assemble(source, "gen-1")?;
    let accounted = assembly.summaries.len() + assembly.blockers.len();
    assert_eq!(accounted, 4, "2 named subs + 1 anonymous + 1 empty-body blocker");
    assert_eq!(assembly.summaries.len(), 3);
    assert_eq!(assembly.blockers.len(), 1);
    assert_eq!(assembly.blockers[0].callable_name.as_deref(), Some("empty"));

    let mut names: Vec<Option<&str>> =
        assembly.summaries.iter().map(|packet| packet.callable_name.as_deref()).collect();
    names.sort();
    assert_eq!(names, vec![None, Some("a"), Some("b")], "no callable silently dropped");

    // Every packet validates and is anchored to the same document.
    for packet in &assembly.summaries {
        packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
        assert_eq!(packet.anchor.file_id, FileId(1));
    }
    // Distinct callables have distinct entity identities.
    let mut ids: Vec<u64> = assembly.summaries.iter().map(|packet| packet.callable.0).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "entity identities must be distinct per callable");
    Ok(())
}

/// A nested callable's outbound calls belong to the nested packet, never to
/// the parent's (scope-identity attribution).
#[test]
fn callable_semantic_summary_nested_callables_keep_their_own_calls() -> TestResult {
    let source = "sub outer { my $cb = sub { inner_call(); }; }";
    let assembly = assemble(source, "gen-1")?;
    assert_eq!(assembly.summaries.len(), 2, "outer + nested anonymous: {assembly:?}");
    for packet in &assembly.summaries {
        packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
        match packet.callable_name.as_deref() {
            Some("outer") => assert!(
                packet.outbound_calls.is_empty(),
                "the nested call must not leak into the parent packet"
            ),
            None => assert_eq!(
                packet.outbound_calls.len(),
                1,
                "the nested callable owns its outbound call"
            ),
            other => return Err(format!("unexpected callable {other:?}").into()),
        }
    }
    Ok(())
}

/// A parse failure is an explicit error, never an empty success.
#[test]
fn callable_semantic_summary_parse_failure_is_an_explicit_error() -> TestResult {
    // Nesting far beyond the parser's recursion budget is non-recoverable.
    let mut source = String::from("sub f { my $x = ");
    source.push_str(&"(".repeat(100_000));
    let result = assemble_from_source(&source, &ctx("gen-1"));
    match result {
        Err(AssemblyError::ParseFailed(message)) => {
            assert!(!message.is_empty(), "the error must say why");
        }
        Ok(assembly) => {
            // If the parser's budget admits this depth, the assembly must
            // still be honest — never an empty success with files claimed.
            return Err(format!(
                "expected a parse failure, got {} summaries",
                assembly.summaries.len()
            )
            .into());
        }
    }
    Ok(())
}

/// `goto &sub` replaces the frame: it is an outbound dependency, never a
/// pure/complete callable (R1).
#[test]
fn callable_semantic_summary_goto_is_an_outbound_dependency() -> TestResult {
    let assembly = assemble("sub f { my $x = 1; goto &g; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    assert!(
        !packet.outbound_calls.is_empty(),
        "goto &g must be recorded as an outbound dependency"
    );
    let goto = &packet.outbound_calls[0];
    // The ControlTransfer item does not name the `&sub` target — Unknown,
    // never dropped, never guessed.
    assert_eq!(goto.callee, OutboundCallee::Unknown);
    assert_eq!(goto.resolution, CallResolution::UnresolvedTransitive);
    for facet in [SummaryFacetKind::Result, SummaryFacetKind::Effect, SummaryFacetKind::Control] {
        assert!(
            goto.blocked_facets.contains(&facet),
            "a frame-replacing goto must block {facet:?}: {:?}",
            goto.blocked_facets
        );
        assert_ne!(
            facet_status(packet, facet)?,
            SummaryFacetStatus::Complete,
            "{facet:?} must not be Complete while a goto blocks it"
        );
    }
    let outbound_facet = packet
        .facets
        .iter()
        .find(|entry| entry.facet == SummaryFacetKind::OutboundCall)
        .ok_or("OutboundCall facet missing")?;
    assert!(outbound_facet.selected >= 1, "the goto is counted in the OutboundCall facet");
    Ok(())
}

/// Loop-control transfers are declared Control evidence, never silently
/// control-complete (R1, second half).
#[test]
fn callable_semantic_summary_loop_control_is_declared_control_evidence() -> TestResult {
    let assembly = assemble("sub f { for (1) { last; } }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
    let control = packet
        .facets
        .iter()
        .find(|entry| entry.facet == SummaryFacetKind::Control)
        .ok_or("Control facet missing")?;
    assert_eq!(control.status, SummaryFacetStatus::Limited);
    assert!(control.missing >= 1, "the `last` transfer must be declared missing evidence");
    Ok(())
}

/// A signature-default anonymous sub has no body of its own; it must not
/// shift the pairing of a later anonymous sub onto a false blocker (R2).
#[test]
fn callable_semantic_summary_signature_default_does_not_poison_pairing() -> TestResult {
    let assembly = assemble("sub f ($g = sub { 1 }) { 2 } my $h = sub { 3 };", "gen-1")?;
    assert_eq!(assembly.summaries.len(), 2, "f and the $h anonymous sub: {assembly:?}");
    assert_eq!(assembly.blockers.len(), 1, "the signature-default anon gets an honest blocker");
    assert!(
        assembly.blockers[0].reason.contains("no lowerable body"),
        "blocker names the missing body: {}",
        assembly.blockers[0].reason
    );
    assert_eq!(assembly.blockers[0].callable_name, None);

    let f = assembly
        .summaries
        .iter()
        .find(|packet| packet.callable_name.as_deref() == Some("f"))
        .ok_or("f must be summarized")?;
    f.validate().map_err(|v| format!("f packet must validate: {v:?}"))?;
    // f has a signature: parameter binding is Limited (declared), not
    // NotProven.
    assert_eq!(facet_status(f, SummaryFacetKind::ParameterBinding)?, SummaryFacetStatus::Limited);

    let anon = assembly
        .summaries
        .iter()
        .find(|packet| packet.callable_name.is_none())
        .ok_or("the $h anonymous sub must be summarized")?;
    anon.validate().map_err(|v| format!("anon packet must validate: {v:?}"))?;
    Ok(())
}

/// Two `eval` sites keep two provenance records with distinct anchors even
/// though the envelope dedups their boundary identity to one link (R3).
#[test]
fn callable_semantic_summary_boundary_sites_keep_provenance() -> TestResult {
    let assembly = assemble("sub f { my $x = 1; eval \"a\"; eval \"b\"; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    assert_eq!(packet.boundary_sites.len(), 2, "each eval site keeps its provenance edge");
    let first = &packet.boundary_sites[0];
    let second = &packet.boundary_sites[1];
    assert_ne!(
        first.anchor, second.anchor,
        "distinct sites carry distinct anchors: {first:?} vs {second:?}"
    );
    assert!(matches!(first.source, CallableFactRef::HirItem(_)));
    // The Boundary facet counts sites, not deduped links.
    let boundary = packet
        .facets
        .iter()
        .find(|entry| entry.facet == SummaryFacetKind::Boundary)
        .ok_or("Boundary facet missing")?;
    assert_eq!(boundary.selected, 2);
    // The envelope still dedups by semantic boundary identity — correct,
    // and now visibly distinct from the site record.
    assert_eq!(packet.summary_ref.referenced_boundaries.len(), 1);
    Ok(())
}

/// Body identity is content-sensitive: a changed call target or a changed
/// boundary-site set is a different body (R4).
#[test]
fn callable_semantic_summary_body_identity_is_content_sensitive() -> TestResult {
    let g = only_summary(&assemble("sub f { g(1) }", "gen-1")?)?.body.clone();
    let h = only_summary(&assemble("sub f { h(1) }", "gen-1")?)?.body.clone();
    assert_ne!(g, h, "a changed call target is a different body");

    let one_eval =
        only_summary(&assemble("sub f { my $x = 1; eval \"a\"; }", "gen-1")?)?.body.clone();
    let two_evals =
        only_summary(&assemble("sub f { my $x = 1; eval \"a\"; eval \"b\"; }", "gen-1")?)?
            .body
            .clone();
    assert_ne!(one_eval, two_evals, "a changed boundary-site set is a different body");

    // Control: identical input keeps an identical body identity.
    let g_again = only_summary(&assemble("sub f { g(1) }", "gen-1")?)?.body.clone();
    assert_eq!(g, g_again);
    Ok(())
}

/// Unmodeled evidence limits Result too: an unmodeled expression may feed a
/// return, so `return <STDIN>;` must not report Result Complete (R1).
#[test]
fn callable_semantic_summary_result_limits_on_unmodeled_evidence() -> TestResult {
    let assembly = assemble("sub f { return <STDIN>; }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
    assert_ne!(
        facet_status(packet, SummaryFacetKind::Result)?,
        SummaryFacetStatus::Complete,
        "an unmodeled readline may feed the return value — Result must limit"
    );
    Ok(())
}

/// Equal-length payload edits change the body identity even when the caller
/// supplies no file body identity (R2).
#[test]
fn callable_semantic_summary_body_identity_reads_operation_payloads() -> TestResult {
    let mut context = ctx("gen-1");
    context.body = BodyIdentity::Unknown;
    let left = assemble_from_source("sub f { $x = 1; }", &context)?;
    let right = assemble_from_source("sub f { $y = 2; }", &context)?;
    let left_body = only_summary(&left)?.body.clone();
    let right_body = only_summary(&right)?.body.clone();
    assert_ne!(
        left_body, right_body,
        "equal-length but different operations ($x = 1 vs $y = 2) are different bodies"
    );
    Ok(())
}

/// Nested anonymous subs nest by containment: the outer declaration's
/// direct body is the maximal enclosed candidate, so both get summaries
/// with no false ambiguity blocker (R3).
#[test]
fn callable_semantic_summary_nested_anonymous_subs_pair_by_maximal_range() -> TestResult {
    let assembly = assemble("my $a = sub { my $b = sub { 1 }; 2 };", "gen-1")?;
    assert_eq!(
        assembly.summaries.len(),
        2,
        "both nested anonymous subs must be summarized: {assembly:?}"
    );
    assert!(
        assembly.blockers.is_empty(),
        "no false ambiguity blocker for nested anons: {:?}",
        assembly.blockers
    );
    for packet in &assembly.summaries {
        packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
        assert_eq!(packet.callable_name, None);
    }
    // Distinct bodies, distinct identities.
    assert_ne!(assembly.summaries[0].body, assembly.summaries[1].body);
    Ok(())
}

/// A static-receiver method call is honest about the unavailable class
/// identity: `Foo->run()` is Unknown, never the false precision of
/// Named("run") shared with `Bar->run()` (R4).
#[test]
fn callable_semantic_summary_method_call_class_is_unknown_not_false_precision() -> TestResult {
    let assembly = assemble("sub f { Foo->run(); }", "gen-1")?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;
    if packet.outbound_calls.len() != 1 {
        return Err(format!("expected one outbound dependency, got {packet:?}").into());
    }
    assert_eq!(
        packet.outbound_calls[0].callee,
        OutboundCallee::Unknown,
        "the HIR carries no receiver name — Named(\"run\") would be false precision"
    );
    Ok(())
}

/// A packet that fails its own contract validation becomes a blocker naming
/// the violations — never an invalid summary reported as success (R6).
#[test]
fn callable_semantic_summary_invalid_packet_is_a_blocker_not_a_summary() -> TestResult {
    let mut context = ctx("gen-1");
    context.work_budget = WorkBudget::new(0); // the envelope requires >= 1 unit
    let assembly = assemble_from_source("sub f { my $x = 1; }", &context)?;
    assert!(assembly.summaries.is_empty(), "an invalid packet must not be reported as a summary");
    assert_eq!(assembly.blockers.len(), 1);
    assert!(
        assembly.blockers[0].reason.contains("validation"),
        "the blocker names the validation failure: {}",
        assembly.blockers[0].reason
    );
    Ok(())
}

/// A `field` access is real work the callable does. Before core-class fields
/// had their own PIR operations a field read lowered to `StashRead` and landed
/// in the effect ledger; the field operations must not let it fall through the
/// wildcard arm and vanish, because the Place and Effect facets then declare
/// completeness over an access they never counted (#13817).
#[test]
fn callable_semantic_summary_counts_field_accesses() -> TestResult {
    let assembly = assemble(
        "use feature 'class';\nclass C {\n    field $n;\n    method bump { $n = 1; $n += 2; return $n; }\n}\n",
        "gen-1",
    )?;
    let packet = only_summary(&assembly)?;
    packet.validate().map_err(|v| format!("packet must validate: {v:?}"))?;

    let places: Vec<_> =
        packet.bindings.iter().filter(|place| place.name == "$n").map(|place| place.role).collect();
    assert!(
        !places.is_empty(),
        "field accesses must reach the place ledger, got bindings {:?} and effects {:?}",
        packet.bindings.iter().map(|place| (&place.name, place.role)).collect::<Vec<_>>(),
        packet.effects.iter().map(|effect| effect.kind).collect::<Vec<_>>()
    );
    assert!(
        places.contains(&perl_semantic_facts::interprocedural::PlaceRole::Modify),
        "the compound assignment must be recorded as a Modify place, got {places:?}"
    );
    assert!(
        packet
            .effects
            .iter()
            .any(|effect| effect.kind
                == perl_semantic_facts::interprocedural::EffectKind::FieldModify),
        "a field read-modify-write must be its own effect, not a lexical or stash one"
    );
    Ok(())
}
