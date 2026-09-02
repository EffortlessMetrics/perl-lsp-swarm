//! Body-HIR regex anchors resolve against the retained analysis table (#7136).
//!
//! #7136 requires every regex pattern in canonical body HIR to link to its
//! generation-aligned #7018 analysis record, or to an explicit unavailable
//! reason. Body lowering has no table handle, so the link is an anchor that a
//! consumer resolves itself — which is only a real link if it actually
//! resolves.
//!
//! The earlier proof for this checked only that an anchor spanned the right
//! source text, and only for an unbound `qr//`, where the construct's range
//! and the record's operator range coincide. That is exactly the case where
//! the distinction is invisible. For a bound `$x =~ /foo/` the node range also
//! covers the target and binding operator (`0..11`) while the record keys on
//! the operator alone (`6..11`), so exact-range lookup returns nothing.
//!
//! These tests resolve every anchor end to end against a real
//! `RegexAnalysisTable` built from the same source, so an anchoring regression
//! fails here instead of silently producing links that never resolve.
//!
//! Tests use `perl_tdd_support` helpers rather than `expect`/`panic`, per the
//! workspace lint policy.

use perl_parser_core::hir::{HirExpr, lower_ast};
use perl_parser_core::syntax::regex_analysis::RegexAnalysisTable;
use perl_parser_core::{SourceLocation, parse_source_with_regex_analysis};
use perl_tdd_support::must_some_with;

/// Every regex-family anchor in `source`, in body order.
fn anchors(source: &str) -> Vec<SourceLocation> {
    let output = parse_source_with_regex_analysis(source);
    let hir = lower_ast(&output.parse_output.ast);
    hir.bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter_map(|expr| match expr {
            HirExpr::Regex(r) => Some(r.analysis.full_range),
            HirExpr::Match(m) => Some(m.analysis.full_range),
            HirExpr::Substitution(s) => Some(s.analysis.full_range),
            // Transliteration deliberately carries no analysis anchor.
            _ => None,
        })
        .collect()
}

fn table_for(source: &str) -> RegexAnalysisTable {
    parse_source_with_regex_analysis(source).regex_analysis
}

/// Resolve one anchor and return the source text of the record it names.
fn resolved_operator_text(source: &str, anchor: SourceLocation) -> String {
    let table = table_for(source);
    let record = must_some_with(
        table.find_enclosed_by(anchor),
        format!("anchor {anchor:?} in {source:?} must resolve to a retained record"),
    );
    let range = record.full_range;
    must_some_with(source.get(range.start..range.end), "record range must be in bounds").to_string()
}

#[test]
fn unbound_regex_anchor_resolves() {
    let source = "my $r = qr/foo/i;";
    let found = anchors(source);
    assert_eq!(found.len(), 1, "expected one anchor in {source:?}");
    assert_eq!(resolved_operator_text(source, found[0]), "qr/foo/i");
}

#[test]
fn bound_match_anchor_resolves_to_the_operator_not_the_binding_expression() {
    // The regression this file exists for: the anchor spans `$x =~ /foo/`
    // while the record keys on `/foo/`.
    for source in ["$x =~ /foo/;", "$x !~ /foo/;"] {
        let found = anchors(source);
        assert_eq!(found.len(), 1, "expected one anchor in {source:?}");
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
    assert!(
        table.find_enclosed_by(SourceLocation { start: 0, end: source.len() }).is_none(),
        "a body with no regex must resolve no record"
    );
}

#[test]
fn resolution_prefers_an_exact_operator_range() {
    // An anchor equal to a record's own full_range resolves to that record
    // directly, so the unbound path does not depend on the containment rule.
    let source = "my $r = qr/foo/i;";
    let table = table_for(source);
    let record = must_some_with(
        table.find_enclosed_by(SourceLocation { start: 8, end: 16 }),
        "exact operator range must resolve",
    );
    assert_eq!(record.full_range, SourceLocation { start: 8, end: 16 });
}
