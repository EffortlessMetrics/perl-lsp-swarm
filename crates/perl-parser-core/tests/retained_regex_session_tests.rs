//! Caller-driven retained regex analysis (#7024).
//!
//! A host that drives its own [`Parser::parse`] — the language server does, because
//! it owns its failure handling — must still get the one canonical regex table that
//! [`parse_source_with_regex_analysis`] produces. These tests pin that equivalence,
//! and pin the boundaries where the session must refuse to invent evidence.

use perl_parser_core::{
    Parser, RegexAnalysisAvailability, RetainedRegexSession, parse_source_with_regex_analysis,
};

/// Source with one finding of each canonical class that this layer can reach:
/// a risk advisory (nested quantifier), a dynamic boundary (embedded code), a
/// syntax-class modifier finding, and a clean substitution as a negative control.
const MIXED: &str = r#"my $re = qr/(a+)+b/;
my $x = /(?{ print 1 })/;
if ($s =~ m/foo/zz) { }
my $y = $s =~ s/(x)/y/gr;
"#;

fn session_table(source: &str) -> perl_parser_core::RegexAnalysisTable {
    let session = RetainedRegexSession::begin(source);
    let mut parser = Parser::new(source);
    let ast = parser.parse();
    match ast {
        Ok(mut ast) => session.finish(Some(&mut ast)),
        Err(_) => session.finish(None),
    }
}

/// The load-bearing claim: driving the parse yourself does not produce a different
/// regex authority. A wrong implementation that rescanned the source, or that lost
/// the parser-owned geometry, would disagree here.
#[test]
fn caller_driven_session_matches_the_whole_parse_entry_point() {
    let canonical = parse_source_with_regex_analysis(MIXED).regex_analysis;
    let session = session_table(MIXED);

    assert_eq!(
        session.records.len(),
        canonical.records.len(),
        "caller-driven retention must retain the same record count"
    );

    for (from_session, from_entry_point) in session.records.iter().zip(canonical.records.iter()) {
        assert_eq!(from_session.operator, from_entry_point.operator);
        assert_eq!(from_session.full_range, from_entry_point.full_range);
        assert_eq!(from_session.availability, from_entry_point.availability);
        assert_eq!(from_session.pattern_range(), from_entry_point.pattern_range());

        let session_diagnostics = from_session
            .pattern
            .as_ref()
            .map(|pattern| pattern.structural.diagnostics.clone())
            .unwrap_or_default();
        let entry_point_diagnostics = from_entry_point
            .pattern
            .as_ref()
            .map(|pattern| pattern.structural.diagnostics.clone())
            .unwrap_or_default();
        assert_eq!(
            session_diagnostics, entry_point_diagnostics,
            "canonical diagnostics must not depend on which entry point drove the parse"
        );

        let session_modifiers =
            from_session.modifiers.as_ref().map(|analysis| analysis.diagnostics.clone());
        let entry_point_modifiers =
            from_entry_point.modifiers.as_ref().map(|analysis| analysis.diagnostics.clone());
        assert_eq!(session_modifiers, entry_point_modifiers);

        // Control facts are the other half of the retained pattern analysis, and the
        // LSP projection publishes capture diagnostics straight out of them. Comparing
        // only structural and modifier findings would let a dropped `controls` value
        // pass here while client diagnostics silently differ.
        let session_controls = from_session.pattern.as_ref().map(|pattern| &pattern.controls);
        let entry_point_controls =
            from_entry_point.pattern.as_ref().map(|pattern| &pattern.controls);
        assert_eq!(
            session_controls.is_some(),
            entry_point_controls.is_some(),
            "both entry points must agree on whether control facts were retained"
        );
        if let (Some(from_session), Some(from_entry_point)) =
            (session_controls, entry_point_controls)
        {
            assert_eq!(from_session.captures.declarations, from_entry_point.captures.declarations);
            assert_eq!(from_session.captures.diagnostics, from_entry_point.captures.diagnostics);
            assert_eq!(
                from_session.captures.named_families,
                from_entry_point.captures.named_families
            );
            assert_eq!(from_session.status, from_entry_point.status);
        }
    }
}

/// The corpus above must actually carry capture/control evidence, or the control
/// comparison added to it compares two empty structures and proves nothing.
#[test]
fn the_equivalence_corpus_carries_capture_evidence() {
    let session = session_table(MIXED);
    let declarations: usize = session
        .records
        .iter()
        .filter_map(|record| record.pattern.as_ref())
        .map(|pattern| pattern.controls.captures.declarations.len())
        .sum();
    assert!(declarations >= 2, "corpus must declare captures, got {declarations}");
}

/// A session finished while a later session is still open must retain nothing.
///
/// Geometry is recorded against whichever session is on top of the thread-local
/// stack, so popping blindly would hand the outer session the *inner* parse's
/// geometry, which it would then anchor against its own source — mis-placed spans
/// carrying a digest that still matches. Retaining nothing is the honest outcome.
#[test]
fn an_out_of_order_finish_retains_nothing_rather_than_another_sessions_geometry() {
    // The two sources are deliberately the same length with a regex-family operator
    // at the same byte range, and differ only in pattern body. That is what makes the
    // swap observable at all: retained geometry is matched to an AST node by range,
    // so same-range geometry from another document is accepted verbatim and its
    // *body text* is analyzed. Any less similar pair falls back to re-extracting
    // geometry from the node's own source and silently repairs itself, which is why
    // an obvious fixture here proves nothing.
    let outer_source = "my $a = qr/(a+)+b/;\n";
    let inner_source = "my $a = qr/(x+)-b/;\n";
    assert_eq!(
        outer_source.len(),
        inner_source.len(),
        "the fixture only discriminates while both sources are the same length"
    );

    let outer = RetainedRegexSession::begin(outer_source);
    let inner = RetainedRegexSession::begin(inner_source);

    let mut inner_parser = Parser::new(inner_source);
    let inner_parsed = inner_parser.parse();
    assert!(inner_parsed.is_ok(), "inner source must parse: {inner_parsed:?}");
    let Ok(mut inner_ast) = inner_parsed else { return };

    // Finish the OUTER session first, while the inner one is still active.
    let mut outer_parser = Parser::new(outer_source);
    let outer_parsed = outer_parser.parse();
    assert!(outer_parsed.is_ok(), "outer source must parse: {outer_parsed:?}");
    let Ok(mut outer_ast) = outer_parsed else { return };
    let outer_table = outer.finish(Some(&mut outer_ast));

    assert!(
        outer_table.source_matches(outer_source),
        "the table still binds the source it was built for"
    );
    // `(a+)+b` is a nested quantifier; `(x+)-b` is not. Consuming the inner session's
    // geometry would analyze the inner body and lose this finding.
    let outer_findings: usize = outer_table
        .records
        .iter()
        .filter_map(|record| record.pattern.as_ref())
        .map(|pattern| pattern.structural.diagnostics.len())
        .sum();
    assert_eq!(
        outer_findings, 1,
        "the outer table must describe the outer pattern, not another document's: {outer_table:#?}"
    );

    // The inner session is untouched and still returns its own analysis.
    let inner_table = inner.finish(Some(&mut inner_ast));
    assert!(
        inner_table.source_matches(inner_source),
        "the inner session keeps its own source identity"
    );
    let inner_findings: usize = inner_table
        .records
        .iter()
        .filter_map(|record| record.pattern.as_ref())
        .map(|pattern| pattern.structural.diagnostics.len())
        .sum();
    assert_eq!(
        inner_findings, 0,
        "the inner pattern has no nested quantifier and must not inherit one"
    );
}

/// Negative control for the equivalence above. It passes only because the session
/// actually retained something; an empty table would make the comparison vacuous.
#[test]
fn the_equivalence_corpus_is_not_vacuous() {
    let session = session_table(MIXED);
    assert_eq!(session.records.len(), 4, "corpus must retain four regex-family records");

    let structural: usize = session
        .records
        .iter()
        .filter_map(|record| record.pattern.as_ref())
        .map(|pattern| pattern.structural.diagnostics.len())
        .sum();
    let modifier: usize = session
        .records
        .iter()
        .filter_map(|record| record.modifiers.as_ref())
        .map(|analysis| analysis.diagnostics.len())
        .sum();
    assert!(
        structural >= 2 && modifier >= 2,
        "corpus must carry both structural and modifier findings, got {structural} and {modifier}"
    );
}

/// The session binds the exact source it analyzed. A consumer that held a table
/// across an edit must be able to see that it is stale rather than read it as a
/// clean current answer.
#[test]
fn a_retained_table_reports_staleness_against_edited_source() {
    let table = session_table(MIXED);
    assert!(table.source_matches(MIXED), "the analyzed source must match");

    let edited = MIXED.replace("(a+)+b", "plain");
    assert!(
        !table.source_matches(&edited),
        "an edited source must not match a table retained for the previous snapshot"
    );
}

/// A failed parse retains no records, and says so through a source-bound empty
/// table. The falsifier this pins is the opposite: fabricating a clean result.
#[test]
fn a_parse_without_a_usable_tree_retains_no_fabricated_records() {
    let source = "my $re = qr/(a+)+b/;";
    let session = RetainedRegexSession::begin(source);
    let table = session.finish(None);

    assert!(table.records.is_empty(), "no tree means no records");
    assert!(
        table.source_matches(source),
        "an empty table still binds its source so a consumer can tell empty from stale"
    );
}

/// Dropping a session without finishing it must release the thread-local slot, so a
/// later session on the same thread still sees only its own geometry.
#[test]
fn an_abandoned_session_does_not_leak_into_the_next_one() {
    {
        let _abandoned = RetainedRegexSession::begin(MIXED);
        let mut parser = Parser::new(MIXED);
        let _ = parser.parse();
    }

    let table = session_table(MIXED);
    let canonical = parse_source_with_regex_analysis(MIXED).regex_analysis;
    assert_eq!(
        table.records.len(),
        canonical.records.len(),
        "an abandoned session must not add records to the next one"
    );
}

/// Transliteration shares the operator family but is not a regex body. It must stay
/// retained-and-typed rather than analyzed as a pattern.
#[test]
fn transliteration_is_retained_without_regex_body_analysis() {
    let source = "my $n = ($s =~ tr/a-z/A-Z/);\n";
    let table = session_table(source);

    let record = table
        .records
        .iter()
        .find(|record| record.availability == RegexAnalysisAvailability::TransliterationNotRegex);
    assert!(
        record.is_some(),
        "transliteration must be retained with a typed non-regex availability: {table:#?}"
    );
    let Some(record) = record else { return };
    assert!(record.pattern.is_none(), "a transliteration body is not analyzed as a regex");
}
