//! Parse-identity binding for retained regex analysis (#14752).
//!
//! `RetainedRegexSession::finish` must not analyze this session's source under
//! a tree from another parse. A foreign tree is a typed mismatch; the same
//! parse reuses; an incremental edit invalidates the retained table.

use perl_parser_core::{Parser, RegexAnalysisAvailability, RetainedRegexSession};
use perl_regex::analyzer::FeatureState;
use perl_tdd_support::{must_err_with, must_with};

/// Equal-length sources, regex operator at the same byte offset, bodies differ.
///
/// This is the #14751 fixture shape: anything less similar is repaired by
/// geometry re-extraction and cannot prove provenance.
const NESTED_BODY: &str = "my $a = qr/(a+)+b/;\n";
const CLEAN_BODY: &str = "my $a = qr/(x+)-b/;\n";

/// Equal-length sources, same regex body and offsets, pragma state differs.
///
/// Geometry re-extraction cannot discriminate these: the operator, range, and
/// pattern text match. The hole #14752 records is that `finish` would then
/// analyze *this* source under *the other document's* `use utf8` state.
const UTF8_ON: &str = "use utf8;\nmy $re = qr/(a+)+b/;\n";
const UTF8_OFF: &str = "no  utf8;\nmy $re = qr/(a+)+b/;\n";

fn parse_ast(source: &str) -> perl_parser_core::Node {
    let mut parser = Parser::new(source);
    must_with(parser.parse(), "fixture must parse")
}

fn finish_ok(
    session: RetainedRegexSession<'_>,
    ast: &mut perl_parser_core::Node,
) -> perl_parser_core::RegexAnalysisTable {
    must_with(session.finish(Some(ast)), "same-source parse must bind")
}

#[test]
fn fixtures_discriminate_only_while_equal_length() {
    assert_eq!(NESTED_BODY.len(), CLEAN_BODY.len());
    assert_eq!(UTF8_ON.len(), UTF8_OFF.len());
}

/// The same parse that produced the tree is accepted and may be reused.
#[test]
fn the_same_parse_reuses_its_retained_analysis() {
    let session = RetainedRegexSession::begin(NESTED_BODY);
    let mut ast = parse_ast(NESTED_BODY);
    let table = finish_ok(session, &mut ast);

    assert!(table.source_matches(NESTED_BODY), "table binds the source it analyzed");
    assert!(table.matches_ast(&ast), "the producing tree must match the retained identity");
    assert!(
        table.parse_identity().is_some(),
        "a successful bound finish must publish a parse identity"
    );
    assert!(table.require_ast(&ast).is_ok(), "require_ast must accept the producing tree");

    let nested = table.records.iter().any(|record| {
        record.availability == RegexAnalysisAvailability::Analyzed
            && record
                .pattern
                .as_ref()
                .is_some_and(|pattern| !pattern.structural.diagnostics.is_empty())
    });
    assert!(nested, "control: the nested-quantifier fixture must actually analyze: {table:#?}");
}

/// A tree from parse B cannot be finished into parse A's session.
#[test]
fn analysis_from_parse_a_is_rejected_when_finished_with_parse_b() {
    let session = RetainedRegexSession::begin(NESTED_BODY);
    let mut parser = Parser::new(CLEAN_BODY);
    let mut foreign_ast = must_with(parser.parse(), "foreign fixture must parse");

    let mismatch = must_err_with(
        session.finish(Some(&mut foreign_ast)),
        "a foreign tree must not produce a bound table",
    );

    assert!(
        mismatch.expected().is_none(),
        "no parse of the session source ran, so nothing was bound: {mismatch:?}"
    );
    assert!(
        mismatch.observed().is_some(),
        "the supplied tree must be identified as the observed identity: {mismatch:?}"
    );

    let unbound = mismatch.into_unbound_table();
    assert!(
        unbound.source_matches(NESTED_BODY),
        "the fallback table still binds the session source, not the foreign one"
    );
    assert!(
        !unbound.matches_ast(&foreign_ast),
        "the unbound table must not claim the foreign tree"
    );
    for record in &unbound.records {
        assert_ne!(
            record.availability,
            RegexAnalysisAvailability::Analyzed,
            "a foreign tree must not yield analyzed records: {unbound:#?}"
        );
        assert!(
            record.pattern.is_none(),
            "a foreign tree must not yield pattern analysis: {unbound:#?}"
        );
    }
}

/// Same operator geometry, different compile-time pragma environment.
///
/// Without an identity check this finishes successfully and stamps the
/// session source with the *other* document's `use utf8` / `no utf8` profile.
#[test]
fn a_foreign_tree_cannot_supply_another_documents_pragma_profile() {
    let session = RetainedRegexSession::begin(UTF8_ON);
    let mut own_parser = Parser::new(UTF8_ON);
    let mut own_ast = must_with(own_parser.parse(), "utf8-on fixture must parse");

    let mut foreign_parser = Parser::new(UTF8_OFF);
    let mut foreign_ast = must_with(foreign_parser.parse(), "utf8-off fixture must parse");

    // Control: finishing with the producing tree keeps this source's utf8-on profile.
    let bound = finish_ok(session, &mut own_ast);
    let own_utf8 = bound
        .records
        .iter()
        .find(|record| record.availability == RegexAnalysisAvailability::Analyzed)
        .map(|record| record.profile.source_utf8);
    assert_eq!(
        own_utf8,
        Some(FeatureState::Enabled),
        "control: the utf8-on source must analyze under utf8: {bound:#?}"
    );

    let session = RetainedRegexSession::begin(UTF8_ON);
    let mut own_parser = Parser::new(UTF8_ON);
    let _own_bound = own_parser.parse();
    let mismatch = must_err_with(
        session.finish(Some(&mut foreign_ast)),
        "a same-geometry foreign tree must not be accepted; it would publish another document's pragma profile",
    );
    let unbound = mismatch.into_unbound_table();
    for record in &unbound.records {
        assert_ne!(
            record.profile.source_utf8,
            FeatureState::Disabled,
            "mismatch fallback must not stamp the foreign utf8-off profile onto this source: {unbound:#?}"
        );
    }
}

/// An incremental edit is a new parse. The previous table must not describe it.
#[test]
fn an_incremental_edit_invalidates_retained_analysis() {
    let session = RetainedRegexSession::begin(NESTED_BODY);
    let mut ast_a = parse_ast(NESTED_BODY);
    let table = finish_ok(session, &mut ast_a);

    let edited = CLEAN_BODY;
    assert_eq!(
        NESTED_BODY.len(),
        edited.len(),
        "edit keeps length so geometry alone would not save us"
    );
    let ast_b = parse_ast(edited);

    assert!(
        !table.source_matches(edited),
        "edited source must not match a table retained for the previous snapshot"
    );
    assert!(
        !table.matches_ast(&ast_b),
        "the post-edit tree must not match the pre-edit parse identity"
    );
    assert!(table.require_ast(&ast_b).is_err(), "require_ast must reject the post-edit tree");
}

/// Production fallback: `finish_or_unbound` never publishes stale analysis.
#[test]
fn finish_or_unbound_does_not_analyze_a_foreign_tree() {
    let session = RetainedRegexSession::begin(NESTED_BODY);
    let mut foreign_ast = parse_ast(CLEAN_BODY);
    let table = session.finish_or_unbound(Some(&mut foreign_ast));

    assert!(table.source_matches(NESTED_BODY));
    assert!(!table.matches_ast(&foreign_ast));
    for record in &table.records {
        assert_ne!(record.availability, RegexAnalysisAvailability::Analyzed);
        assert!(record.pattern.is_none());
    }
}

/// A tree parsed outside any session cannot be silently bound later.
#[test]
fn a_tree_parsed_without_the_session_is_a_mismatch() {
    let mut ast = parse_ast(NESTED_BODY);
    let session = RetainedRegexSession::begin(NESTED_BODY);
    let mismatch = must_err_with(
        session.finish(Some(&mut ast)),
        "an unbound tree must not finish as if it were this parse",
    );
    assert!(mismatch.expected().is_none());
}
