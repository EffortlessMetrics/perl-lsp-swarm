//! Body-HIR regex anchors resolve against the retained analysis table (#7136).
//!
//! #7136 requires every regex pattern in canonical body HIR to link to its
//! generation-aligned #7018 analysis record, or to an explicit unavailable
//! reason. Body lowering has no table handle, so the link is an anchor that a
//! consumer resolves itself — which is only a real link if it actually
//! resolves, and only a *correct* link if it cannot resolve to the wrong
//! operator.
//!
//! The earlier proof for this checked only that an anchor spanned the right
//! source text, and only for an unbound `qr//`, where the construct's range
//! and the record's operator range coincide. That is the one case where the
//! distinction is invisible. For a bound `$x =~ /foo/` the node range also
//! covers the target and binding operator (`0..11`) while the record keys on
//! the operator alone (`6..11`), so exact-range lookup returns nothing.
//!
//! Resolution is therefore family-bearing and containment-based, and both
//! filters guard different confusions:
//!
//! - the last-starting tie-break stops a regex in the *target* winning;
//! - the family filter stops a record nested inside the operator's own body
//!   (a regex in an `/e` replacement) winning.
//!
//! These tests resolve every anchor end to end against a real
//! `RegexAnalysisTable`, and pin both filters with negative controls.
//!
//! Tests use `perl_tdd_support` helpers rather than `expect`/`panic`, per the
//! workspace lint policy.

use perl_parser_core::hir::{HirExpr, lower_ast};
use perl_parser_core::syntax::regex_analysis::{RegexAnalysisFamily, RegexAnalysisTable};
use perl_parser_core::{SourceLocation, parse_source_with_regex_analysis};
use perl_tdd_support::must_some_with;

/// One anchor as body HIR records it: its range and the family it may resolve to.
type Anchor = (SourceLocation, RegexAnalysisFamily);

/// Every regex-family anchor in `source`, in body order.
fn anchors(source: &str) -> Vec<Anchor> {
    let output = parse_source_with_regex_analysis(source);
    let hir = lower_ast(&output.parse_output.ast);
    hir.bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter_map(|expr| match expr {
            HirExpr::Regex(r) => Some((r.analysis.full_range, r.analysis.family)),
            HirExpr::Match(m) => Some((m.analysis.full_range, m.analysis.family)),
            HirExpr::Substitution(s) => Some((s.analysis.full_range, s.analysis.family)),
            // Transliteration deliberately carries no analysis anchor.
            _ => None,
        })
        .collect()
}

fn table_for(source: &str) -> RegexAnalysisTable {
    parse_source_with_regex_analysis(source).regex_analysis
}

/// Resolve one anchor and return the source text of the record it names.
fn resolved_operator_text(source: &str, anchor: Anchor) -> String {
    let (range, family) = anchor;
    let table = table_for(source);
    let record = must_some_with(
        table.find_enclosed_by(range, family),
        format!("anchor {range:?} ({family:?}) in {source:?} must resolve to a retained record"),
    );
    let span = record.full_range;
    must_some_with(source.get(span.start..span.end), "record range must be in bounds").to_string()
}

#[test]
fn unbound_regex_anchor_resolves() {
    let source = "my $r = qr/foo/i;";
    let found = anchors(source);
    assert_eq!(found.len(), 1, "expected one anchor in {source:?}");
    assert_eq!(found[0].1, RegexAnalysisFamily::Regex);
    assert_eq!(resolved_operator_text(source, found[0]), "qr/foo/i");
}

#[test]
fn bound_match_anchor_resolves_to_the_operator_not_the_binding_expression() {
    // The regression this file exists for: the anchor spans `$x =~ /foo/`
    // while the record keys on `/foo/`.
    for source in ["$x =~ /foo/;", "$x !~ /foo/;"] {
        let found = anchors(source);
        assert_eq!(found.len(), 1, "expected one anchor in {source:?}");
        assert_eq!(found[0].1, RegexAnalysisFamily::Match);
        assert_eq!(
            resolved_operator_text(source, found[0]),
            "/foo/",
            "bound match anchor must resolve to the regex operator in {source:?}"
        );
    }
}

#[test]
fn bound_substitution_anchor_resolves() {
    let source = "$x =~ s/a/b/g;";
    let found = anchors(source);
    assert_eq!(found.len(), 1, "expected one anchor in {source:?}");
    assert_eq!(found[0].1, RegexAnalysisFamily::Substitution);
    assert_eq!(resolved_operator_text(source, found[0]), "s/a/b/g");
}

#[test]
fn implicit_topic_substitution_anchor_resolves() {
    let source = "s/a/b/;";
    let found = anchors(source);
    assert_eq!(found.len(), 1, "expected one anchor in {source:?}");
    assert_eq!(resolved_operator_text(source, found[0]), "s/a/b/");
}

#[test]
fn every_anchor_in_a_multi_operation_body_resolves_to_its_own_operator() {
    // Distinct operations must not collapse onto one record.
    let source = "$a =~ /one/; $b =~ s/two/x/; my $r = qr/three/;";
    let found = anchors(source);
    assert_eq!(found.len(), 3, "expected three anchors in {source:?}");

    let resolved: Vec<String> =
        found.iter().map(|anchor| resolved_operator_text(source, *anchor)).collect();
    assert_eq!(resolved, vec!["/one/", "s/two/x/", "qr/three/"]);
}

#[test]
fn a_regex_in_the_target_does_not_capture_the_operator_anchor() {
    // The last-starting tie-break exists for this case: the target contributes
    // its own anchor, so the body carries two. Each must resolve to itself.
    let source = "foo(/inner/) =~ s/a/b/;";
    let found = anchors(source);

    let nested: Vec<Anchor> =
        found.iter().copied().filter(|(_, f)| *f == RegexAnalysisFamily::Regex).collect();
    let operator: Vec<Anchor> =
        found.iter().copied().filter(|(_, f)| *f == RegexAnalysisFamily::Substitution).collect();
    assert_eq!(nested.len(), 1, "expected the target's own regex anchor in {source:?}");
    assert_eq!(operator.len(), 1, "expected the substitution anchor in {source:?}");

    assert_eq!(
        resolved_operator_text(source, operator[0]),
        "s/a/b/",
        "the operator must win over a regex inside its own target"
    );
    assert_eq!(
        resolved_operator_text(source, nested[0]),
        "/inner/",
        "the nested regex must still resolve to itself"
    );
}

#[test]
fn transliteration_carries_no_anchor_to_resolve() {
    // tr/// is not a regex; the absence of an anchor is the structural
    // guarantee that it can never be routed through pattern analysis.
    assert!(
        anchors("$x =~ tr/a-z/A-Z/;").is_empty(),
        "transliteration must expose no regex analysis anchor"
    );
}

#[test]
fn a_range_enclosing_no_record_resolves_to_none() {
    // Negative control: resolution is not a lookup that always succeeds, and a
    // miss means unavailable rather than clean.
    let source = "my $n = 1;";
    let table = table_for(source);
    let whole = SourceLocation { start: 0, end: source.len() };
    for family in
        [RegexAnalysisFamily::Regex, RegexAnalysisFamily::Match, RegexAnalysisFamily::Substitution]
    {
        assert!(
            table.find_enclosed_by(whole, family).is_none(),
            "a body with no regex must resolve no record for {family:?}"
        );
    }
}

#[test]
fn resolution_prefers_an_exact_operator_range() {
    // An anchor equal to a record's own full_range resolves to that record
    // directly, so the unbound path does not depend on the containment rule.
    let source = "my $r = qr/foo/i;";
    let table = table_for(source);
    let record = must_some_with(
        table.find_enclosed_by(SourceLocation { start: 8, end: 16 }, RegexAnalysisFamily::Regex),
        "exact operator range must resolve",
    );
    assert_eq!(record.full_range, SourceLocation { start: 8, end: 16 });
}

// ── Family filter: negative controls ─────────────────────────────────────────

#[test]
fn an_anchor_never_resolves_across_operator_families() {
    // The family filter is what stops a record nested inside an operator's own
    // body from winning the containment tie-break. Asking each real anchor for
    // the wrong family must find nothing, even though the range still contains
    // the record positionally.
    let source = "$x =~ s/a/b/g;";
    let table = table_for(source);
    let found = anchors(source);
    assert_eq!(found.len(), 1);
    let (range, family) = found[0];
    assert_eq!(family, RegexAnalysisFamily::Substitution);

    assert!(
        table.find_enclosed_by(range, RegexAnalysisFamily::Substitution).is_some(),
        "the substitution anchor must resolve for its own family"
    );
    assert!(
        table.find_enclosed_by(range, RegexAnalysisFamily::Match).is_none(),
        "a substitution record must not answer a match-family anchor"
    );
}

#[test]
fn a_match_record_does_not_answer_a_substitution_anchor() {
    let source = "$x =~ /foo/;";
    let table = table_for(source);
    let found = anchors(source);
    assert_eq!(found.len(), 1);
    let (range, _) = found[0];

    assert!(
        table.find_enclosed_by(range, RegexAnalysisFamily::Match).is_some(),
        "the match anchor must resolve for its own family"
    );
    assert!(
        table.find_enclosed_by(range, RegexAnalysisFamily::Substitution).is_none(),
        "a match record must not answer a substitution-family anchor"
    );
}

// ── Freshness: resolution is positional, not generation-checked ──────────────

#[test]
fn a_stale_table_resolves_an_anchor_and_only_the_digest_catches_it() {
    // Anchors are positions, and positions do not identify a source snapshot.
    // `HirFile` carries no source identity, so pairing HIR from one parse with a
    // table from another succeeds and answers about the wrong text — worst for
    // an edit that leaves the construct at the same offsets.
    //
    // This pins the hazard rather than asserting it away: the mismatch is real
    // and only `source_matches` detects it. A shared generation identity that
    // makes it unrepresentable is #14658.
    let lowered_from = "$x =~ s/a/b/;";
    let edited = "$x =~ s/a/c/;";
    assert_eq!(lowered_from.len(), edited.len(), "the edit must not move any offset");

    let found = anchors(lowered_from);
    assert_eq!(found.len(), 1);
    let (range, family) = found[0];

    let stale = table_for(edited);
    let record = must_some_with(
        stale.find_enclosed_by(range, family),
        "a stale table still resolves the anchor — that is the hazard",
    );
    let span = record.full_range;
    assert_eq!(
        must_some_with(edited.get(span.start..span.end), "record range must be in bounds"),
        "s/a/c/",
        "resolution reports the edited source, not the source the body was lowered from"
    );

    // The escape hatch, and the reason a consumer must use it before resolving.
    assert!(!stale.source_matches(lowered_from), "the digest must reject the stale pairing");
    assert!(stale.source_matches(edited), "and accept its own source");
}

#[test]
fn an_exact_range_does_not_bypass_the_family_filter_for_a_real_operator() {
    // Exact-range resolution deliberately accepts a record that carries *no*
    // operator, so an unavailable record can still report its reason. That
    // relaxation must not extend to a record whose operator is known and
    // belongs to another family: there the mismatch is real evidence.
    let source = "my $r = qr/foo/i;";
    let table = table_for(source);
    let exact = SourceLocation { start: 8, end: 16 };
    assert!(
        table.find_enclosed_by(exact, RegexAnalysisFamily::Regex).is_some(),
        "the record's own family must resolve at its exact range"
    );
    assert!(
        table.find_enclosed_by(exact, RegexAnalysisFamily::Substitution).is_none(),
        "an exact range must not resolve a record from a different operator family"
    );
}

#[test]
fn regex_and_match_families_accept_the_same_operator_set() {
    // Documented and deliberate: the parser does not distinguish `qr//` from an
    // unbound `m//` or a bare `/.../`, so these two families cannot be
    // separated at this layer. Pinning it keeps the limitation visible rather
    // than letting a future change quietly narrow one of them.
    let source = "my $r = qr/foo/i;";
    let table = table_for(source);
    let range = SourceLocation { start: 8, end: 16 };
    assert!(table.find_enclosed_by(range, RegexAnalysisFamily::Regex).is_some());
    assert!(table.find_enclosed_by(range, RegexAnalysisFamily::Match).is_some());
}
