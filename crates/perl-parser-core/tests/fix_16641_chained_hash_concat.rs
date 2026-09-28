//! Discriminating proof for #16641.
//!
//! Current truth on `origin/main`: an isolated well-formed concat of
//! `$h->{a}{b} . ...` does not emit `InsertedCloser`. The production FAIL on
//! `test_corpus/signatures_parameters_production_enhanced.pl:85` is displaced
//! recovery: chained `->{named}{y}` lexes `y` as transliteration because
//! `->{...}` does not open hash-subscript brace depth, so the following
//! concat line is parsed inside a fake `y}...}...}` body.
//!
//! This claim stays on that recovery false-positive. It does not take open
//! angle scanning or heredoc lexer work.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::Parser;
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};
use perl_tdd_support::must;

const CORPUS: &str =
    include_str!("../../../test_corpus/signatures_parameters_production_enhanced.pl");

fn parse_errors(src: &str) -> (perl_parser_core::Node, Vec<ParseError>) {
    let mut parser = Parser::new(src);
    let ast = must(parser.parse());
    let errors = parser.errors().to_vec();
    (ast, errors)
}

fn hash_subscript_inserted_closers(errors: &[ParseError]) -> Vec<&ParseError> {
    errors
        .iter()
        .filter(|e| {
            matches!(
                e,
                ParseError::Recovered {
                    site: RecoverySite::HashSubscript,
                    kind: RecoveryKind::InsertedCloser,
                    ..
                }
            )
        })
        .collect()
}

fn assert_no_hash_subscript_inserted_closer(src: &str) {
    let (ast, errors) = parse_errors(src);
    let recovered = hash_subscript_inserted_closers(&errors);
    assert!(
        recovered.is_empty(),
        "Clean chained-subscript source must not infer a missing hash closer.\n\
         source: {src}\n\
         recovered: {recovered:?}\n\
         all errors: {errors:?}\n\
         sexp: {}",
        ast.to_sexp()
    );
    assert!(
        matches!(ast.kind, perl_parser_core::NodeKind::Program { .. }),
        "Parser must return a Program node for {src:?}"
    );
    assert_clean_parse(src);
}

fn assert_hash_subscript_inserted_closer(src: &str) {
    let (ast, errors) = parse_errors(src);
    let recovered = hash_subscript_inserted_closers(&errors);
    assert!(
        !recovered.is_empty(),
        "Truly missing hash closer must still emit InsertedCloser at HashSubscript.\n\
         source: {src}\n\
         all errors: {errors:?}\n\
         sexp: {}",
        ast.to_sexp()
    );
}

// ---------------------------------------------------------------------------
// Characterization: isolated concat without quote-op chained keys
// ---------------------------------------------------------------------------

#[test]
fn isolated_concat_line_without_quote_op_key_is_already_clean() {
    // The issue's reported one-liner. On current main this path is already
    // clean; the production FAIL needs the preceding `{y}` statement.
    assert_no_hash_subscript_inserted_closer(
        r#"return $params->{named}{greeting} . ", " . $params->{named}{name} . "!";"#,
    );
}

#[test]
fn chained_arrow_hash_concat_minimal_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b} . $h->{c}{d};"#);
}

#[test]
fn chained_direct_hash_concat_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h{a}{b} . $h{c}{d};"#);
}

#[test]
fn chained_arrow_hash_concat_string_literal_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b} . "z";"#);
}

#[test]
fn chained_arrow_hash_concat_no_spaces_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b}."z".$h->{c}{d};"#);
}

#[test]
fn triple_chain_under_concat_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b}{c} . "z";"#);
}

#[test]
fn chained_hash_under_concat_inside_sub_is_clean() {
    assert_no_hash_subscript_inserted_closer(
        r#"sub greet { return $params->{named}{greeting} . ", " . $params->{named}{name} . "!"; }"#,
    );
}

#[test]
fn chained_hash_then_array_under_concat_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}[0] . $h->{b}{c};"#);
}

#[test]
fn single_hash_under_concat_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a} . $h->{b};"#);
}

#[test]
fn chained_hash_without_concat_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b};"#);
}

#[test]
fn chained_hash_concat_quoted_keys_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{'a'}{'b'} . $h->{"c"}{"d"};"#);
}

#[test]
fn chained_hash_concat_sigil_keys_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{$k}{$j} . $h->{$m}{$n};"#);
}

// ---------------------------------------------------------------------------
// Discriminators: chained quote-op keys after `->` (the live false-positive)
// ---------------------------------------------------------------------------

#[test]
fn chained_arrow_hash_key_y_is_a_key_not_transliteration() {
    assert_no_hash_subscript_inserted_closer(r#"return $params->{named}{y};"#);
}

#[test]
fn chained_arrow_hash_key_s_is_a_key_not_substitution() {
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h->{outer}{s};"#);
}

#[test]
fn chained_arrow_hash_key_m_is_a_key_not_match() {
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h->{outer}{m};"#);
}

#[test]
fn chained_arrow_hash_key_tr_is_a_key_not_transliteration() {
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h->{outer}{tr};"#);
}

#[test]
fn chained_arrow_hash_key_qw_is_a_key_not_quote_words() {
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h->{outer}{qw};"#);
}

#[test]
fn chained_arrow_hash_with_spaces_and_quote_op_key_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h-> {named} {y} . "z";"#);
}

#[test]
fn extra_arrow_before_quote_op_key_is_clean() {
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h->{named}->{y} . "z";"#);
}

#[test]
fn first_arrow_hash_key_y_already_stays_a_key() {
    // Control: `$h->{y}` is already protected by `after_arrow`.
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h->{y};"#);
}

#[test]
fn direct_chained_hash_key_y_already_stays_a_key() {
    // Control: `$h{a}{y}` already increments brace depth from the `$h` path.
    assert_no_hash_subscript_inserted_closer(r#"my $v = $h{a}{y};"#);
}

#[test]
fn corpus_plus_then_concat_does_not_infer_missing_hash_closer() {
    // File-local reproduction of lines 80 and 85: `{y}` must not eat the
    // following concat statement and fabricate a missing `}`.
    assert_no_hash_subscript_inserted_closer(
        r#"
sub greet {
    return $params->{named}{x} + $params->{named}{y};
    return $params->{named}{greeting} . ", " . $params->{named}{name} . "!";
}
"#,
    );
}

#[test]
fn chained_quote_op_key_then_concat_does_not_infer_missing_hash_closer() {
    assert_no_hash_subscript_inserted_closer(r#"my $x = $h->{a}{y} . $h->{b}{s};"#);
}

#[test]
fn corpus_file_does_not_infer_missing_hash_closer() {
    let (ast, errors) = parse_errors(CORPUS);
    let recovered = hash_subscript_inserted_closers(&errors);
    assert!(
        recovered.is_empty(),
        "signatures_parameters_production_enhanced.pl must not infer a missing hash closer.\n\
         recovered: {recovered:?}\n\
         blocking: {:?}",
        errors.iter().filter(|e| e.blocks_clean_parse()).collect::<Vec<_>>()
    );
    assert!(
        matches!(ast.kind, perl_parser_core::NodeKind::Program { .. }),
        "Parser must return a Program node for the corpus file"
    );
}

// ---------------------------------------------------------------------------
// Opposite-direction controls: genuine missing `}` still recovers
// ---------------------------------------------------------------------------

#[test]
fn missing_inner_closer_before_concat_still_recovers() {
    assert_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b . "z";"#);
}

#[test]
fn missing_outer_closer_before_semicolon_still_recovers() {
    assert_hash_subscript_inserted_closer(r#"my $x = $h->{a;"#);
}

#[test]
fn missing_chained_closer_before_semicolon_still_recovers() {
    assert_hash_subscript_inserted_closer(r#"my $x = $h->{a}{b;"#);
}

#[test]
fn missing_hash_closer_before_concat_operand_still_recovers() {
    assert_hash_subscript_inserted_closer(r#"my $x = $h->{a . "z";"#);
}

#[test]
fn missing_closer_on_quote_op_chained_key_still_recovers() {
    assert_hash_subscript_inserted_closer(r#"my $x = $h->{a}{y;"#);
}

// ---------------------------------------------------------------------------
// Quote-op statements after a completed subscript must stay quote-ops
// ---------------------------------------------------------------------------

#[test]
fn real_transliteration_after_chained_subscript_still_parses() {
    assert_clean_parse("$h->{a}{b}; y/a/b/;");
}

#[test]
fn real_match_after_chained_subscript_still_parses() {
    assert_clean_parse("$h->{a}{b}; m/foo/;");
}

#[test]
fn regex_in_block_after_chained_arrow_condition_still_parses() {
    // Retention: `)->{a}{y}` must not leak brace depth into the following
    // `if`/`while` block and suppress `m//` / `s///` (#2844 shape).
    assert_clean_parse("if ($h->{a}{y}) { m/foo/; }");
    assert_clean_parse("while ($h->{a}{s}) { s/a/b/; }");
}
