#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
//! Discriminating tests for the canonical `ParseError` union (`src/error.rs`)
//! and the Pratt operator table.
//!
//! `ParseError` is the error type returned by the crate's parsing APIs:
//! `Rejected` (parser-domain rejection, wraps `StrictParseError`) and `Failed`
//! (operational/instrument failure, wraps `ParserFailure`). Vocabulary
//! construction keeps its own `OutcomeError` and is deliberately not folded in.
//!
//! These tests pin that the two arms are populated correctly by
//! `PureRustPerlParser::parse`, are not interconvertible by type, round-trip
//! through serde, reject stale schema payloads loudly, report rejection ranges
//! as caller-source offsets even when `parse()` rewrote the source, and that
//! every public fallible API returns `ParseError` rather than
//! `Box<dyn std::error::Error>`.

use std::error::Error;

use perl_parser_pest::pratt_parser::Associativity;
use perl_parser_pest::pure_rust_parser::{PerlParser, Rule};
use perl_parser_pest::{
    AstNode, ParseError, ParserFailure, PrattParser, PureRustPerlParser, SourceRange,
    StrictParseError,
};
use pest::iterators::{Pair, Pairs};

// ---------------------------------------------------------------------------
// Structural proof: every public fallible API returns `ParseError`, never
// `Box<dyn std::error::Error>`. These wrapper functions only compile if the
// callee's signature is exactly what it claims; a regression back to
// `Box<dyn std::error::Error>` (or to any other error type) fails to build.
// ---------------------------------------------------------------------------

fn call_parse(parser: &mut PureRustPerlParser, source: &str) -> Result<AstNode, ParseError> {
    parser.parse(source)
}

fn call_build_ast(
    parser: &mut PureRustPerlParser,
    pairs: Pairs<Rule>,
) -> Result<AstNode, ParseError> {
    parser.build_ast(pairs)
}

fn call_build_node(
    parser: &mut PureRustPerlParser,
    pair: Pair<'_, Rule>,
) -> Result<Option<AstNode>, ParseError> {
    parser.build_node(pair)
}

fn call_pratt_expression_from_pairs<'a>(
    pratt: &PrattParser,
    pairs: Vec<Pair<'a, Rule>>,
    parser: &mut PureRustPerlParser,
) -> Result<AstNode, ParseError> {
    pratt.parse_expression_from_pairs(pairs, parser)
}

/// Parse `source` and require a [`ParseError::Rejected`], returning its inner
/// [`StrictParseError`]. Used by both the rejection-shape test and the
/// normalization/range-caveat test below.
fn expect_rejected(
    parser: &mut PureRustPerlParser,
    source: &str,
) -> Result<StrictParseError, Box<dyn Error>> {
    match parser.parse(source) {
        Err(ParseError::Rejected(rejection)) => Ok(rejection),
        other => Err(format!("expected Rejected for {source:?}, got {other:?}").into()),
    }
}

#[test]
fn public_fallible_apis_return_typed_parse_error() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();

    // `call_parse`/`call_build_ast`/`call_build_node`/`call_pratt_expression_from_pairs`
    // above are the compile-time proof; exercising them here also gives a
    // runtime check that the wrappers are not dead weight.
    let ast = call_parse(&mut parser, "my $x = 1;\n")?;
    let AstNode::Program(_) = &ast else {
        return Err(format!("expected Program root, got {ast:?}").into());
    };

    let pairs = <PerlParser as pest::Parser<Rule>>::parse(Rule::program, "my $y = 2;\n")?;
    let _ast_from_build_ast = call_build_ast(&mut parser, pairs)?;

    let mut pairs = <PerlParser as pest::Parser<Rule>>::parse(Rule::program, "my $z = 3;\n")?;
    let first_pair = pairs.next().ok_or("expected at least one pest pair")?;
    let _node = call_build_node(&mut parser, first_pair)?;

    let pratt = PrattParser::new();
    let pairs: Vec<Pair<'_, Rule>> =
        <PerlParser as pest::Parser<Rule>>::parse(Rule::program, "my $w = 4;\n")?.collect();
    let _pratt_ast = call_pratt_expression_from_pairs(&pratt, pairs, &mut parser)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// The caller-source guarantee across the heredoc pre-pass (#8220).
//
// `parse()` removes heredoc bodies before normalization, so a Pest offset is
// in the *stripped* coordinate system — two rewrites away from the caller's
// text, not one. `NormalizationMap::behind_removals` prepends the scan's
// removals as the map's first pass so one reverse fold still reaches the
// caller's source. Without it these ranges would be short by exactly the
// removed bytes and would name the wrong part of the caller's text while
// still looking well-formed.
// ---------------------------------------------------------------------------

/// Require a rejection whose range starts on the caller's `???`.
///
/// Every fixture below is unparseable *and* unrecoverable — recovery returns
/// `Rejected` only when it salvages no statement at all, which the unclosed
/// `sub f {` guarantees — so each one reaches the mapped-rejection path.
fn assert_rejection_starts_at_question_marks(
    source: &str,
    untranslated_hint: &str,
) -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    let rejection = expect_rejected(&mut parser, source)?;
    rejection.range().check_over_source(source)?;

    let expected = source.find("???").ok_or("fixture must contain `???`")?;
    let start = rejection.range().start();
    if start < expected {
        return Err(format!(
            "range must be a caller-source offset: expected the `???` at {expected}, got {start}. \
             {untranslated_hint}"
        )
        .into());
    }
    match source.as_bytes().get(start) {
        Some(b'?') => Ok(()),
        other => Err(format!(
            "caller-source byte {start} should be a `?`, got {:?}",
            other.map(|b| *b as char)
        )
        .into()),
    }
}

#[test]
fn a_rejection_after_a_stripped_heredoc_body_indexes_the_callers_source()
-> Result<(), Box<dyn Error>> {
    // The 18-byte body and terminator are removed before Pest sees the text,
    // so Pest reports the `???` at stripped offset 23. Untranslated that lands
    // inside `body line one` in the caller's source — well-formed, in range,
    // and wrong.
    assert_rejection_starts_at_question_marks(
        "sub f {\nmy $x = <<EOF;\nbody line one\nEOF\n???\n",
        "Offset 23 is the untranslated stripped coordinate, which names `body line one`.",
    )
}

#[test]
fn a_rejection_after_two_stripped_heredoc_bodies_accumulates_every_removal()
-> Result<(), Box<dyn Error>> {
    // Two separate removals. Applying only one still lands short, so this row
    // pins that the removals accumulate rather than that one happens to work.
    assert_rejection_starts_at_question_marks(
        "sub f {\nmy $a = <<A;\nfirst body\nA\nmy $b = <<B;\nsecond body\nB\n???\n",
        "Applying only the first removal lands inside the second heredoc's body.",
    )
}

#[test]
fn a_rejection_after_a_removal_and_a_normalization_rewrite_composes_both_passes()
-> Result<(), Box<dyn Error>> {
    // `$$name` is rewritten to `${$name}` *after* the body is stripped, so the
    // offset must unwind through the normalization pass and then through the
    // removal pass. Dropping either one moves the answer.
    assert_rejection_starts_at_question_marks(
        "sub f {\nmy $x = <<EOF;\nbody line one\nbody two\nEOF\n$$name ???\n",
        "Skipping the removal pass lands in the body; skipping normalization shifts by two.",
    )
}

// ---------------------------------------------------------------------------
// `Rejected` from an actual `parse()` call.
// ---------------------------------------------------------------------------

#[test]
fn pest_rejection_from_malformed_source_carries_context_and_valid_range()
-> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    // No `$$name`/`= ~expr` normalization trigger in this source, so the
    // normalized text pest parses is byte-identical to `source` and the
    // range can be validated directly against it.
    let source = "my = ; ???\n";

    let rejection = expect_rejected(&mut parser, source)?;

    if rejection.pest_context().trim().is_empty() {
        return Err("pest_context must not be empty".into());
    }
    if !rejection.pest_context().contains("expected") {
        return Err(format!(
            "pest_context should retain Pest's own expectation text, got {:?}",
            rejection.pest_context()
        )
        .into());
    }
    // Validates that `parse()` bound the range to the text it actually
    // parsed rather than to some other buffer.
    rejection.range().check_over_source(source)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative control: operational/instrument failure is never produced by
// malformed-but-otherwise-well-formed-instrument Perl source on its own.
// ---------------------------------------------------------------------------

#[test]
fn malformed_perl_never_yields_operational_failure() -> Result<(), Box<dyn Error>> {
    let malformed_sources =
        ["<<", "my = ; ???\n", "$$name ???\n", "sub {", "my $x = ;", "1 + ", "if ( {"];

    for source in malformed_sources {
        let mut parser = PureRustPerlParser::new();
        match parser.parse(source) {
            Ok(_) | Err(ParseError::Rejected(_)) => {}
            Err(ParseError::Failed(failure)) => {
                return Err(format!(
                    "malformed source {source:?} must not be classified as an operational \
                     failure, got {failure:?}"
                )
                .into());
            }
            Err(other) => {
                return Err(format!("unexpected ParseError arm for {source:?}: {other:?}").into());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The converse of the control above: grammar-valid Perl that the AST builder
// cannot lower reaches the caller as `Failed`, not `Rejected`. Pinning the
// real behaviour rather than asserting a guarantee this parser cannot back.
// ---------------------------------------------------------------------------

#[test]
fn builder_gap_on_grammar_valid_perl_is_instrument_not_rejection() -> Result<(), Box<dyn Error>> {
    // `if` / `elsif` conditions starting with a unary prefix operator are
    // ordinary Perl that Pest accepts and the builder then fails to lower.
    // This predates the typed-error work (the same inputs fail on the base
    // commit with an untyped "Failed to build condition node"); the value
    // added here is that the failure is now *classified*.
    let grammar_valid_but_unlowered =
        ["if (!1) { 1; }\n", "if (!defined $x) { 1; }\n", "if (-1) { 1; }\n"];

    for source in grammar_valid_but_unlowered {
        let mut parser = PureRustPerlParser::new();
        match parser.parse(source) {
            // `Ok` would mean the builder gap was fixed — a real improvement,
            // and this test should then be revisited rather than kept green
            // by accident. It is not a failure of the error contract.
            Ok(_) => {}
            // The load-bearing assertion: the caller's Perl is valid, so
            // calling it a parser-domain rejection would be a false statement
            // about their source.
            Err(ParseError::Failed(_)) => {}
            Err(ParseError::Rejected(rejection)) => {
                return Err(format!(
                    "grammar-valid source {source:?} must never be reported as a \
                     parser-domain rejection; got Rejected({:?})",
                    rejection.message()
                )
                .into());
            }
            Err(other) => {
                return Err(format!("unexpected ParseError arm for {source:?}: {other:?}").into());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The two arms are not interconvertible by type.
// ---------------------------------------------------------------------------

#[test]
fn rejected_and_failed_are_not_interconvertible_by_type() -> Result<(), Box<dyn Error>> {
    let range = SourceRange::try_new(0, 0)?;
    let rejection = StrictParseError::new(range, "unexpected token", "pest-display");
    let failure = ParserFailure::instrument("builder invariant violated");

    let rejected: ParseError = rejection.clone().into();
    let failed: ParseError = failure.clone().into();
    let rejected_ctor = ParseError::rejected(rejection.clone());
    let failed_ctor = ParseError::failed(failure.clone());
    assert_eq!(rejected, rejected_ctor);
    assert_eq!(failed, failed_ctor);

    if rejected.as_failed().is_some() {
        return Err("Rejected must never report as_failed".into());
    }
    if failed.as_rejected().is_some() {
        return Err("Failed must never report as_rejected".into());
    }
    match rejected.as_rejected() {
        Some(inner) if *inner == rejection => {}
        other => return Err(format!("expected the original rejection back, got {other:?}").into()),
    }
    match failed.as_failed() {
        Some(inner) if *inner == failure => {}
        other => return Err(format!("expected the original failure back, got {other:?}").into()),
    }
    assert_ne!(rejected, failed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Serde round-trip for both arms.
// ---------------------------------------------------------------------------

#[test]
fn parse_error_serde_round_trips_both_arms() -> Result<(), Box<dyn Error>> {
    let range = SourceRange::try_new(2, 5)?;
    let rejected: ParseError =
        StrictParseError::new(range, "unexpected token", "pest-display").into();
    let failed: ParseError = ParserFailure::panic("boom").into();

    for value in [rejected, failed] {
        let json = serde_json::to_string(&value)?;
        let decoded: ParseError = serde_json::from_str(&json)?;
        assert_eq!(decoded, value, "round trip must be lossless for {json}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stale/unknown-schema payloads fail loudly rather than being silently
// misread as the wrong arm (or as any arm at all).
// ---------------------------------------------------------------------------

#[test]
fn unknown_schema_and_legacy_shaped_payloads_fail_loudly() -> Result<(), Box<dyn Error>> {
    let range = SourceRange::try_new(0, 3)?;
    let rejected: ParseError = StrictParseError::new(range, "nope", "pest").into();
    let json = serde_json::to_string(&rejected)?;
    if !json.contains("perl-parser-pest.strict_parse_error.v1") {
        return Err(format!("expected embedded schema marker in {json}").into());
    }

    // Corrupting the embedded schema string must fail deserialization, not
    // silently accept a stale/unknown-schema `StrictParseError` payload.
    let stale_schema = json.replace(
        "perl-parser-pest.strict_parse_error.v1",
        "perl-parser-pest.strict_parse_error.v0",
    );
    match serde_json::from_str::<ParseError>(&stale_schema) {
        Err(_) => {}
        Ok(value) => {
            return Err(format!("stale schema payload must not deserialize, got {value:?}").into());
        }
    }

    // A payload shaped like the deleted pre-#9250 `ParseError` (a bare
    // stringly variant, no schema, no `Rejected`/`Failed` wrapper) must not
    // silently deserialize as either arm of the new contract.
    let legacy_shaped = r#"{"InvalidToken":"boom"}"#;
    match serde_json::from_str::<ParseError>(legacy_shaped) {
        Err(_) => {}
        Ok(value) => {
            return Err(format!("legacy-shaped payload must not deserialize, got {value:?}").into());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Normalization and ranges: `parse()` rewrites `source` before Pest ever sees
// it, so Pest's offsets index the rewritten buffer. They are translated back
// before they reach `Rejected`, because `StrictParseError` documents its range
// as an offset into the caller's own source.
// ---------------------------------------------------------------------------

#[test]
fn rejected_range_from_parse_is_translated_back_to_the_caller_source() -> Result<(), Box<dyn Error>>
{
    let mut parser = PureRustPerlParser::new();
    // `$$name` (6 bytes) is rewritten to `${$name}` (8 bytes) before Pest
    // parses it, shifting everything after it by two. Pest reports the failure
    // inside `???` of the normalized `${$name} ???\n`; untranslated, that
    // offset would land on the trailing newline of the caller's 11-byte source
    // and silently contradict `StrictParseError`'s caller-source contract.
    let source = "$$name ???\n";
    let question_marks = 7..10;
    if source.len() != 11 || &source[question_marks.clone()] != "???" {
        return Err(format!("fixture assumption broke: {source:?}").into());
    }

    let rejection = expect_rejected(&mut parser, source)?;
    let (start, end) = (rejection.range().start(), rejection.range().end());

    // The translated offset must land on the `???` the caller actually wrote.
    // Untranslated it would be 10 — the trailing newline — so this assertion
    // fails if the translation is removed.
    if !question_marks.contains(&start) {
        return Err(format!(
            "expected the rejection range inside the caller's `???` at {question_marks:?}, \
             got [{start}, {end}); byte 10 (the untranslated normalized offset) is the \
             trailing newline"
        )
        .into());
    }
    if source.as_bytes()[start] != b'?' {
        return Err(format!(
            "caller-source byte {start} should be a `?`, got {:?}",
            source.as_bytes()[start] as char
        )
        .into());
    }
    // Pest's own rendering still describes the normalized buffer it parsed;
    // that is retained verbatim as context and is not the range authority.
    if !rejection.pest_context().contains("???") {
        return Err(format!(
            "pest_context should retain pest's own rendering, got {:?}",
            rejection.pest_context()
        )
        .into());
    }
    Ok(())
}

/// When normalization changes nothing, translation must be the identity: the
/// reported offset has to equal the offset Pest independently reports for the
/// very same text. This is the control that keeps the mapping from drifting on
/// the overwhelmingly common un-rewritten path.
#[test]
fn rejected_range_is_exact_when_normalization_is_a_no_op() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    // Contains no `$$name` or `= ~expr` trigger, so `parse()` hands Pest this
    // exact text.
    let source = "my = ; ???\n";

    // Independently ask Pest where it fails, without going through `parse()`.
    let pest_offset = match <PerlParser as pest::Parser<Rule>>::parse(Rule::program, source) {
        Ok(_) => return Err("fixture must be rejected by pest".into()),
        Err(error) => match error.location {
            pest::error::InputLocation::Pos(pos) => pos,
            pest::error::InputLocation::Span((start, _)) => start,
        },
    };

    let rejection = expect_rejected(&mut parser, source)?;
    let (start, end) = (rejection.range().start(), rejection.range().end());
    if start != pest_offset {
        return Err(format!(
            "un-rewritten source must report pest's own offset {pest_offset}, got [{start}, {end})"
        )
        .into());
    }
    rejection.range().check_over_source(source)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy behavior preserved: existing recovery/failure smoke tests.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Pratt operator table (unrelated to the error contract; preserved coverage).
// ---------------------------------------------------------------------------

#[test]
fn pratt_operator_table_exposes_precedence_and_associativity() -> Result<(), Box<dyn Error>> {
    let parser = PrattParser::default();

    let assignment = parser.get_operator_info("=").ok_or("expected assignment operator info")?;
    assert_eq!(assignment.precedence.0, 3);
    assert_eq!(assignment.associativity, Associativity::Right);

    let range = parser.get_operator_info("...").ok_or("expected range operator info")?;
    assert_eq!(range.precedence.0, 5);
    assert_eq!(range.associativity, Associativity::None);

    let match_operator = parser.get_operator_info("=~").ok_or("expected match operator info")?;
    assert_eq!(match_operator.precedence.0, 29);
    assert_eq!(match_operator.associativity, Associativity::Left);

    assert!(parser.get_operator_info("~~not-an-operator~~").is_none());

    Ok(())
}

#[test]
fn pratt_prefix_and_postfix_operator_classifiers_cover_perl_specific_forms()
-> Result<(), Box<dyn Error>> {
    for op in ["!", "not", "~.", "\\", "defined", "state"] {
        assert!(PrattParser::is_prefix_operator(op), "expected {op} to be prefix");
    }

    for op in ["++", "--"] {
        assert!(PrattParser::is_prefix_operator(op), "expected {op} to be prefix");
        assert!(PrattParser::is_postfix_operator(op), "expected {op} to be postfix");
    }

    for op in ["=", "=>", "print", "~~not-an-operator~~"] {
        assert!(!PrattParser::is_prefix_operator(op), "expected {op} not to be prefix");
        assert!(!PrattParser::is_postfix_operator(op), "expected {op} not to be postfix");
    }

    Ok(())
}
