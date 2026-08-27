#![deny(clippy::map_err_ignore)]
//! Discriminating tests for the pest parse-outcome vocabulary.
//!
//! These tests own the type-level contract. They do not assert that current
//! `parse()` recovery accounts for source, and they do not add `parse_strict()`.

use std::error::Error;

use perl_parser_pest::pure_rust_parser::Rule;
use perl_parser_pest::{
    OutcomeError, PARSE_OUTCOME_SCHEMA, PARSER_FAILURE_SCHEMA, ParseAttempt, ParseCompleteness,
    ParseDiagnostic, ParseDiagnosticKind, ParseOutcome, ParseOutcomeVocabulary, ParserFailure,
    ParserFailureKind, PureRustPerlParser, RecoveryAction, STRICT_PARSE_ERROR_SCHEMA, SourceRange,
    StrictParseError,
};
use pest::error::{Error as PestError, ErrorVariant};

fn inverted(start: usize, end: usize) -> Result<OutcomeError, Box<dyn Error>> {
    match SourceRange::try_new(start, end) {
        Err(error) => Ok(error),
        Ok(range) => Err(format!("inverted range must fail, got {range}").into()),
    }
}

fn over_source_error(
    start: usize,
    end: usize,
    source: &str,
) -> Result<OutcomeError, Box<dyn Error>> {
    match SourceRange::try_over_source(start, end, source) {
        Err(error) => Ok(error),
        Ok(range) => Err(format!("range over source must fail, got {range}").into()),
    }
}

fn diagnostic(
    kind: ParseDiagnosticKind,
    start: usize,
    end: usize,
    source: &str,
    message: &str,
    action: Option<RecoveryAction>,
) -> Result<ParseDiagnostic, Box<dyn Error>> {
    let range = SourceRange::try_over_source(start, end, source)?;
    Ok(ParseDiagnostic::new(kind, range, message, None, action))
}

#[test]
fn half_open_range_validation_rejects_inverted_and_empty_is_allowed() -> Result<(), Box<dyn Error>>
{
    let empty = SourceRange::try_new(4, 4)?;
    assert_eq!(empty.start(), 4);
    assert_eq!(empty.end(), 4);
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);

    let span = SourceRange::try_new(0, 4)?;
    assert_eq!(span.len(), 4);
    assert!(!span.is_empty());

    match inverted(2, 1)? {
        OutcomeError::InvertedRange { start: 2, end: 1 } => Ok(()),
        other => Err(format!("expected inverted range, got {other}").into()),
    }
}

#[test]
fn range_over_source_rejects_out_of_bounds_instead_of_clamping() -> Result<(), Box<dyn Error>> {
    let source = "ab";
    let full = SourceRange::try_over_source(0, source.len(), source)?;
    assert_eq!(full.end(), 2);

    match over_source_error(0, 3, source)? {
        OutcomeError::OutOfBounds { start: 0, end: 3, source_len: 2 } => {}
        other => return Err(format!("expected out of bounds, got {other}").into()),
    }

    match over_source_error(0, 1, "")? {
        OutcomeError::OutOfBounds { start: 0, end: 1, source_len: 0 } => Ok(()),
        other => Err(format!("empty source must reject [0, 1), got {other}").into()),
    }
}

#[test]
fn range_over_source_rejects_utf8_interior_offsets() -> Result<(), Box<dyn Error>> {
    let source = "éx"; // é is two bytes, x is one
    assert_eq!(source.len(), 3);
    let full = SourceRange::try_over_source(0, 3, source)?;
    assert_eq!(full.len(), 3);
    let after_accent = SourceRange::try_over_source(0, 2, source)?;
    assert_eq!(after_accent.end(), 2);

    match over_source_error(0, 1, source)? {
        OutcomeError::InvalidUtf8Boundary { start: 0, end: 1 } => {}
        other => return Err(format!("expected utf8 boundary error, got {other}").into()),
    }
    match over_source_error(1, 3, source)? {
        OutcomeError::InvalidUtf8Boundary { start: 1, end: 3 } => Ok(()),
        other => Err(format!("start interior must fail, got {other}").into()),
    }
}

#[test]
fn line_column_is_derived_and_not_a_second_range_authority() -> Result<(), Box<dyn Error>> {
    let lf = "a\nb";
    let crlf = "a\r\nb";
    let cr = "a\rb";
    let range_lf = SourceRange::try_over_source(2, 3, lf)?;
    let range_crlf = SourceRange::try_over_source(3, 4, crlf)?;
    let range_cr = SourceRange::try_over_source(2, 3, cr)?;

    let lf_lc = range_lf.line_column(lf)?;
    let crlf_lc = range_crlf.line_column(crlf)?;
    let cr_lc = range_cr.line_column(cr)?;
    assert_eq!((lf_lc.start_line(), lf_lc.start_column()), (1, 0));
    assert_eq!((crlf_lc.start_line(), crlf_lc.start_column()), (1, 0));
    assert_eq!((cr_lc.start_line(), cr_lc.start_column()), (1, 0));

    let accent = "é\n";
    let after_accent = SourceRange::try_over_source(2, 2, accent)?;
    let lc = after_accent.line_column(accent)?;
    assert_eq!((lc.start_line(), lc.start_column()), (0, 1));

    let json = serde_json::to_string(&range_lf)?;
    if json.contains("line") || json.contains("column") {
        return Err(format!("source range serde must not store line/column: {json}").into());
    }
    Ok(())
}

#[test]
fn overlapping_recovery_helper_fails_without_panicking() -> Result<(), Box<dyn Error>> {
    let source = "abcdefgh";
    let left = SourceRange::try_over_source(0, 5, source)?;
    let right = SourceRange::try_over_source(4, 8, source)?;
    match SourceRange::sort_and_check_disjoint(vec![right, left]) {
        Err(OutcomeError::OverlappingRecovery { left: a, right: b }) => {
            assert_eq!(a, left);
            assert_eq!(b, right);
        }
        other => return Err(format!("expected overlap error, got {other:?}").into()),
    }

    let first = SourceRange::try_over_source(0, 4, source)?;
    let second = SourceRange::try_over_source(4, 8, source)?;
    let adjacent = SourceRange::sort_and_check_disjoint(vec![second, first])?;
    assert_eq!(adjacent, [first, second]);

    let empty = SourceRange::try_over_source(3, 3, source)?;
    match SourceRange::sort_and_check_disjoint(vec![empty, empty]) {
        Err(OutcomeError::OverlappingRecovery { .. }) => {}
        other => return Err(format!("duplicate empty ranges must overlap, got {other:?}").into()),
    }

    match serde_json::from_str::<SourceRange>(r#"{"start":5,"end":2}"#) {
        Err(_) => {}
        Ok(range) => {
            return Err(format!("inverted serde range must fail, got {range}").into());
        }
    }
    Ok(())
}

#[test]
fn diagnostics_order_deterministically_by_range_kind_and_message() -> Result<(), Box<dyn Error>> {
    let source = "abcdefghij";
    let late = diagnostic(
        ParseDiagnosticKind::SkippedSource,
        6,
        8,
        source,
        "skip-b",
        Some(RecoveryAction::Skip),
    )?;
    let early_unsupported =
        diagnostic(ParseDiagnosticKind::UnsupportedSyntax, 0, 2, source, "unsupported", None)?;
    let early_skip_z = diagnostic(
        ParseDiagnosticKind::SkippedSource,
        0,
        2,
        source,
        "skip-z",
        Some(RecoveryAction::Skip),
    )?;
    let early_skip_a = diagnostic(
        ParseDiagnosticKind::SkippedSource,
        0,
        2,
        source,
        "skip-a",
        Some(RecoveryAction::Skip),
    )?;

    let ordered = ParseDiagnostic::ordered_for_source(
        vec![late.clone(), early_unsupported.clone(), early_skip_z.clone(), early_skip_a.clone()],
        source,
    )?;
    let kinds: Vec<&str> = ordered.iter().map(|item| item.kind().as_str()).collect();
    let messages: Vec<&str> = ordered.iter().map(ParseDiagnostic::message).collect();
    assert_eq!(kinds, ["skipped-source", "skipped-source", "unsupported-syntax", "skipped-source"]);
    assert_eq!(messages, ["skip-a", "skip-z", "unsupported", "skip-b"]);

    let again = ParseDiagnostic::ordered_for_source(
        vec![early_skip_z, late, early_skip_a, early_unsupported],
        source,
    )?;
    assert_eq!(ordered, again);

    let skip = ParseDiagnostic::new(
        ParseDiagnosticKind::SkippedSource,
        SourceRange::try_over_source(0, 2, source)?,
        "same",
        None,
        Some(RecoveryAction::Skip),
    );
    let resume = ParseDiagnostic::new(
        ParseDiagnosticKind::SkippedSource,
        SourceRange::try_over_source(0, 2, source)?,
        "same",
        None,
        Some(RecoveryAction::ResumeAfter),
    );
    let action_first =
        ParseDiagnostic::ordered_for_source(vec![resume.clone(), skip.clone()], source)?;
    let action_second = ParseDiagnostic::ordered_for_source(vec![skip, resume], source)?;
    assert_eq!(action_first, action_second);
    assert_eq!(action_first[0].recovery_action(), Some(RecoveryAction::Skip));
    assert_eq!(action_first[1].recovery_action(), Some(RecoveryAction::ResumeAfter));
    Ok(())
}

#[test]
fn complete_recovered_and_unsupported_cannot_be_conflated() -> Result<(), Box<dyn Error>> {
    let source = "my $x = 1;\n";
    let complete = ParseOutcome::complete("complete-ast");
    assert_eq!(complete.completeness(), ParseCompleteness::Complete);
    assert!(complete.diagnostics().is_empty());
    assert!(complete.recovery_ranges().is_empty());
    assert_eq!(complete.into_ast(), "complete-ast");
    let complete = ParseOutcome::complete("complete-ast");

    match ParseOutcome::try_new(
        "bad-complete",
        ParseCompleteness::Complete,
        vec![diagnostic(ParseDiagnosticKind::SkippedSource, 0, 2, source, "skip", None)?],
        Vec::new(),
        source,
    ) {
        Err(OutcomeError::CompleteWithRecovery) => {}
        other => return Err(format!("complete with diagnostics must fail, got {other:?}").into()),
    }

    match ParseOutcome::try_recovered("empty", Vec::new(), Vec::new(), source) {
        Err(OutcomeError::RecoveredWithoutEvidence) => {}
        other => return Err(format!("empty recovered must fail, got {other:?}").into()),
    }

    let skip = diagnostic(
        ParseDiagnosticKind::SkippedSource,
        3,
        7,
        source,
        "skipped",
        Some(RecoveryAction::Skip),
    )?;
    let recovered = ParseOutcome::try_recovered(
        "recovered-ast",
        vec![skip.clone()],
        vec![skip.range()],
        source,
    )?;
    assert_eq!(recovered.completeness(), ParseCompleteness::Recovered);
    assert_eq!(recovered.completeness().as_str(), "recovered");

    match ParseOutcome::try_unsupported("no-unsupported", vec![skip], Vec::new(), source) {
        Err(OutcomeError::UnsupportedWithoutDiagnostic) => {}
        other => {
            return Err(
                format!("unsupported without unsupported-syntax must fail, got {other:?}").into()
            );
        }
    }

    let unsupported = diagnostic(
        ParseDiagnosticKind::UnsupportedSyntax,
        0,
        source.len(),
        source,
        "heredoc-body",
        None,
    )?;
    let unsupported_outcome =
        ParseOutcome::try_unsupported("unsupported-ast", vec![unsupported], Vec::new(), source)?;
    assert_eq!(unsupported_outcome.completeness(), ParseCompleteness::Unsupported);
    assert_ne!(unsupported_outcome.completeness(), recovered.completeness());
    assert_ne!(unsupported_outcome.completeness(), complete.completeness());
    Ok(())
}

#[test]
fn rejection_and_instrument_failure_are_distinct_parse_attempt_arms() -> Result<(), Box<dyn Error>>
{
    let source = "ab";
    let range = SourceRange::try_over_source(0, 0, source)?;
    let rejected = StrictParseError::new(range, "unexpected token", "pest-display");
    let failed = ParserFailure::instrument("parser process crashed");
    let outcome: ParseAttempt<&str> = ParseAttempt::outcome(ParseOutcome::complete("ast"));
    let rejected_attempt: ParseAttempt<&str> = ParseAttempt::rejected(rejected.clone());
    let failed_attempt: ParseAttempt<&str> = ParseAttempt::failed(failed.clone());

    if outcome.as_rejected().is_some() || outcome.as_failed().is_some() {
        return Err("complete attempt must not be rejection or failure".into());
    }
    if rejected_attempt.as_outcome().is_some() || rejected_attempt.as_failed().is_some() {
        return Err("rejection must not be outcome or instrument failure".into());
    }
    if failed_attempt.as_outcome().is_some() || failed_attempt.as_rejected().is_some() {
        return Err("instrument failure must not be outcome or rejection".into());
    }

    match failed.kind() {
        ParserFailureKind::Instrument { detail } if detail == "parser process crashed" => {}
        other => return Err(format!("expected instrument kind, got {other}").into()),
    }
    match ParserFailure::panic("boom").kind() {
        ParserFailureKind::Panic { message } if message == "boom" => {}
        other => return Err(format!("expected panic kind, got {other}").into()),
    }
    match ParserFailure::invalid_utf8("odd byte").kind() {
        ParserFailureKind::InvalidUtf8 { detail } if detail == "odd byte" => {}
        other => return Err(format!("expected invalid-utf8 kind, got {other}").into()),
    }
    assert_eq!(rejected.schema(), STRICT_PARSE_ERROR_SCHEMA);
    assert_eq!(failed.schema(), PARSER_FAILURE_SCHEMA);
    Ok(())
}

fn pest_pos_error(
    source: &str,
    pos: usize,
    message: &str,
) -> Result<PestError<Rule>, Box<dyn Error>> {
    let position = match pest::Position::new(source, pos) {
        Some(position) => position,
        None => return Err(format!("invalid pest position {pos} in {source:?}").into()),
    };
    Ok(PestError::new_from_pos(
        ErrorVariant::CustomError { message: message.to_string() },
        position,
    ))
}

#[test]
fn pest_rejection_maps_original_source_bytes_and_preserves_pest_context()
-> Result<(), Box<dyn Error>> {
    let source = "hello";
    let pest_error = pest_pos_error(source, 2, "unexpected token")?;
    let mapped = StrictParseError::from_pest(&pest_error, source)?;
    assert_eq!(mapped.range().start(), 2);
    assert_eq!(mapped.range().end(), 2);
    assert_eq!(mapped.message(), "unexpected token");
    if mapped.pest_context().trim().is_empty() {
        return Err("pest context must retain original Pest display".into());
    }
    if !mapped.pest_context().contains("unexpected token") {
        return Err(
            format!("pest context lost the original message: {}", mapped.pest_context()).into()
        );
    }

    let span = match pest::Span::new(source, 1, 4) {
        Some(span) => span,
        None => return Err("span [1, 4) should be valid on hello".into()),
    };
    let span_error: PestError<Rule> = PestError::new_from_span(
        ErrorVariant::ParsingError { positives: vec![Rule::EOI], negatives: Vec::new() },
        span,
    );
    let mapped_span = StrictParseError::from_pest(&span_error, source)?;
    assert_eq!((mapped_span.range().start(), mapped_span.range().end()), (1, 4));
    if !mapped_span.message().contains("EOI") {
        return Err(format!(
            "span rejection must retain expected rules, got {}",
            mapped_span.message()
        )
        .into());
    }

    match StrictParseError::from_pest(&pest_error, "") {
        Err(OutcomeError::OutOfBounds { .. }) => Ok(()),
        other => {
            Err(format!("shorter original source must fail as out of bounds, got {other:?}").into())
        }
    }
}

#[test]
fn serde_round_trip_is_versioned_and_deterministic() -> Result<(), Box<dyn Error>> {
    let source = "abcdefgh";
    let skip = diagnostic(
        ParseDiagnosticKind::SkippedSource,
        2,
        4,
        source,
        "skipped",
        Some(RecoveryAction::Skip),
    )?;
    let outcome =
        ParseOutcome::try_recovered("ast", vec![skip.clone()], vec![skip.range()], source)?;
    let vocabulary = outcome.vocabulary();
    assert_eq!(vocabulary.schema(), PARSE_OUTCOME_SCHEMA);

    let json = serde_json::to_string(&vocabulary)?;
    let expected = concat!(
        r#"{"schema":"perl-parser-pest.parse_outcome.v1","completeness":"recovered","#,
        r#""diagnostics":[{"kind":"skipped-source","range":{"start":2,"end":4},"message":"skipped","recovery_action":"skip"}],"#,
        r#""recovery_ranges":[{"start":2,"end":4}]}"#
    );
    assert_eq!(json, expected);

    let decoded: ParseOutcomeVocabulary = serde_json::from_str(&json)?;
    assert_eq!(decoded, vocabulary);
    let rebuilt = decoded.try_into_outcome("ast", source)?;
    assert_eq!(rebuilt.completeness(), ParseCompleteness::Recovered);
    assert_eq!(rebuilt.recovery_ranges(), outcome.recovery_ranges());

    let wrong_schema = json.replace(PARSE_OUTCOME_SCHEMA, "perl-parser-pest.parse_outcome.v0");
    match serde_json::from_str::<ParseOutcomeVocabulary>(&wrong_schema) {
        Err(_) => {}
        Ok(value) => {
            return Err(
                format!("wrong schema must not deserialize, got {:?}", value.schema()).into()
            );
        }
    }

    let rejection = StrictParseError::new(skip.range(), "nope", "pest");
    let rejection_json = serde_json::to_string(&rejection)?;
    let rejection_decoded: StrictParseError = serde_json::from_str(&rejection_json)?;
    assert_eq!(rejection_decoded, rejection);
    match serde_json::from_str::<ParserFailure>(&rejection_json) {
        Err(_) => Ok(()),
        Ok(_) => Err("strict rejection JSON must not deserialize as instrument failure".into()),
    }
}

#[test]
fn public_enums_require_wildcard_matches_for_future_variants() {
    fn completeness_name(value: ParseCompleteness) -> &'static str {
        match value {
            ParseCompleteness::Complete => "complete",
            ParseCompleteness::Recovered => "recovered",
            ParseCompleteness::Unsupported => "unsupported",
            _ => "unknown",
        }
    }
    fn kind_name(value: ParseDiagnosticKind) -> &'static str {
        match value {
            ParseDiagnosticKind::OriginalRejection => "original-rejection",
            ParseDiagnosticKind::SkippedSource => "skipped-source",
            ParseDiagnosticKind::RecoveredFragment => "recovered-fragment",
            ParseDiagnosticKind::BuilderFailure => "builder-failure",
            ParseDiagnosticKind::UnaccountedTrailingInput => "unaccounted-trailing-input",
            ParseDiagnosticKind::UnsupportedSyntax => "unsupported-syntax",
            _ => "unknown",
        }
    }
    assert_eq!(completeness_name(ParseCompleteness::Complete), "complete");
    assert_eq!(kind_name(ParseDiagnosticKind::UnsupportedSyntax), "unsupported-syntax");
}

#[test]
fn wrapping_a_legacy_parse_ast_does_not_claim_source_accounting() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    let ast = match parser.parse("my $x = 42;\n") {
        Ok(ast) => ast,
        Err(error) => return Err(format!("legacy parse of simple scalar failed: {error}").into()),
    };
    let outcome = ParseOutcome::complete(ast);
    assert_eq!(outcome.completeness(), ParseCompleteness::Complete);
    assert!(outcome.recovery_ranges().is_empty());
    Ok(())
}

#[test]
fn legacy_parse_behavior_is_unchanged_for_seed_recovery_cases() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    let simple = include_str!("fixtures/sources/declaration-control-flow/simple-scalar.pl");
    if parser.parse(simple).is_err() {
        return Err("legacy parse of simple-scalar.pl must still succeed".into());
    }

    let valid_invalid_valid = include_str!("fixtures/sources/recovery/valid-invalid-valid.pl");
    if parser.parse(valid_invalid_valid).is_err() {
        return Err("legacy parse of valid-invalid-valid.pl must still succeed via recovery".into());
    }
    Ok(())
}
