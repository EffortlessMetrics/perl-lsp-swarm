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
        Ok(mut ast) => session.finish(source, Some(&mut ast)),
        Err(_) => session.finish(source, None),
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
    }
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
    let table = session.finish(source, None);

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
        .find(|record| record.availability == RegexAnalysisAvailability::TransliterationNotRegex)
        .expect("transliteration must be retained with a typed non-regex availability");
    assert!(record.pattern.is_none(), "a transliteration body is not analyzed as a regex");
}
