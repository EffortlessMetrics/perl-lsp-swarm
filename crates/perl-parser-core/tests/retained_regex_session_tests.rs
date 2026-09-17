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

/// Retain a table the caller-driven way: begin a session, drive the parse, finish.
///
/// A parse that yields no tree still finishes the session with `None` rather than
/// being skipped, because abandoning a guard is itself behavior under test.
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

/// A parse of a *different* buffer must not contribute geometry to this session,
/// even when the two are the same length.
///
/// Geometry is offered to whichever session is active, so admitting it on length
/// alone lets a parse of another document anchor spans in a table built from this
/// one — text nobody analyzed, carrying a digest that still matches. The session
/// owns one buffer, and only that buffer's parse may contribute.
///
/// The fixture needs equal lengths and a regex operator at the same byte offset,
/// differing only in body, for the same reason as the out-of-order test: anything
/// less similar is silently repaired by re-extraction and proves nothing.
#[test]
fn a_parse_of_another_buffer_cannot_contribute_geometry() {
    let session_source = "my $a = qr/(a+)+b/;\n";
    let parser_source = "my $a = qr/(x+)-b/;\n";
    assert_eq!(
        session_source.len(),
        parser_source.len(),
        "the fixture only discriminates while both sources are the same length"
    );

    let session = RetainedRegexSession::begin(session_source);
    // Deliberately parse the *other* buffer inside this session.
    let mut parser = Parser::new(parser_source);
    let parsed = parser.parse();
    assert!(parsed.is_ok(), "the parser source must parse: {parsed:?}");
    let Ok(mut ast) = parsed else { return };
    let table = session.finish(Some(&mut ast));

    assert!(
        table.source_matches(session_source),
        "the table binds the source the session was begun for"
    );

    // The measured difference, and the reason this is a data-integrity bug rather
    // than a lost finding: admitting the other buffer's geometry produces an
    // `Analyzed` record whose body was `(x+)-b`, so the table reports a *clean*
    // pattern for a document whose actual body is `(a+)+b`. Refusing is the honest
    // outcome — an explicit `GeometryUnavailable` record with no analysis.
    // The loop below is satisfied by an empty table, so require the witness first:
    // a candidate whose geometry was rejected must still be *retained* as explicit
    // unavailable evidence. Dropping the record entirely would also pass the loop
    // while silently losing the record of a refusal.
    assert!(
        table.records.iter().any(|record| {
            record.availability == RegexAnalysisAvailability::GeometryUnavailable
                && record.pattern.is_none()
        }),
        "a foreign parse must retain explicit unavailable evidence: {table:#?}"
    );

    for record in &table.records {
        assert_ne!(
            record.availability,
            RegexAnalysisAvailability::Analyzed,
            "a foreign parse must not yield an analyzed record: {table:#?}"
        );
        assert!(
            record.pattern.is_none(),
            "a foreign parse must not yield pattern analysis: {table:#?}"
        );
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

/// A session that recorded nothing retains nothing, and says so through a
/// source-bound empty table. The falsifier this pins is fabricating a clean result
/// for a document this session never analyzed.
///
/// Note what this does *not* say. No parse runs here at all, so no geometry is ever
/// recorded — the emptiness comes from having observed nothing, not from the absence
/// of a tree. A parse that ran and then failed does retain what it recorded before
/// failing; that is
/// [`a_fatal_parse_still_retains_geometry_recorded_before_it_failed`]. Reading this
/// test as "a failed parse retains no records" would state a contract the seam
/// deliberately does not have, because that contract loses findings.
#[test]
fn a_session_that_recorded_nothing_retains_no_fabricated_records() {
    let source = "my $re = qr/(a+)+b/;";
    let session = RetainedRegexSession::begin(source);
    let table = session.finish(None);

    assert!(table.records.is_empty(), "nothing observed means nothing retained");
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

/// A fatal parse must not silently drop findings the parser already recorded (#7024).
///
/// `finish(None)` previously returned an empty table, which meant a document holding
/// both a regex finding and a fatal structural failure lost the finding outright: the
/// session suppresses the legacy per-operator scan, so with nothing canonical retained
/// there was nothing left to publish. Measured before the fix, the parse below reported
/// one backtracking advisory without a session and none with one.
#[test]
fn a_fatal_parse_still_retains_geometry_recorded_before_it_failed() {
    // Deep nesting exhausts the parser and yields no tree; the regex sits before it.
    let source = format!("my $re = qr/(a+)+b/;\n{}\n", "if (1) {".repeat(3000));

    let session = RetainedRegexSession::begin(&source);
    let mut parser = Parser::new(&source);
    let parsed = parser.parse();
    assert!(parsed.is_err(), "fixture must actually fail to parse, or it proves nothing");
    let table = session.finish(None);

    assert!(
        !table.records.is_empty(),
        "geometry recorded before the failure must survive it: {table:#?}"
    );
    let analyzed = table
        .records
        .iter()
        .any(|record| record.availability == RegexAnalysisAvailability::Analyzed);
    assert!(analyzed, "the retained record must carry real analysis: {table:#?}");
}

// The session is bound to the thread that began it, and the type system enforces it.
//
// Its stack entry lives in a thread-local registered by `begin`. A session moved to
// another thread would retire an id that thread never registered — retaining nothing
// there while the originating thread keeps the entry for the rest of its life. No
// runtime check can close that; only `!Send` can.
//
// This is asserted at compile time rather than in a `#[test]`, deliberately: `Send`ness
// is a property of the type, so a change that makes the session `Send` again should
// fail to build rather than fail a run. The inherent `impl` below applies only when
// `T: Send` and wins over the trait default when it does, so `IS_SEND` reads that
// difference.
// `dead_code` does not count a const-assertion initializer as a use, so these read as
// unused even though the assertions below depend on them and fail to compile without
// them. The mutation receipt for that is in this commit's message.
#[allow(dead_code)]
struct IsSend<T>(core::marker::PhantomData<T>);
#[allow(dead_code)]
trait NotSend {
    const IS_SEND: bool = false;
}
impl<T> NotSend for IsSend<T> {}
impl<T: Send> IsSend<T> {
    #[allow(dead_code)]
    const IS_SEND: bool = true;
}

// Note the unqualified path. Writing `<IsSend<_> as NotSend>::IS_SEND` would name the
// trait constant explicitly and always read `false`, making this assertion vacuous no
// matter what the session does — an earlier draft of this test did exactly that.
// Unqualified, the inherent `impl` shadows the trait default whenever `T: Send`, which
// is the only form that actually discriminates.
const _SESSION_IS_NOT_SEND: () = assert!(!IsSend::<RetainedRegexSession<'static>>::IS_SEND);

// Control. The probe reports `true` for a type that really is `Send`, which rules out
// the degenerate reading of the assertion above: that the probe always answers `false`
// and would hold no matter what `RetainedRegexSession` does.
const _PROBE_CAN_REPORT_SEND: () = assert!(IsSend::<String>::IS_SEND);
