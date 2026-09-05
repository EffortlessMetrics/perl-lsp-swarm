//! Executable contract for the validated token-stream subject (#9623, A05a).
//!
//! The claim under proof is narrow and falsifiable: a [`ValidatedTokenStream`]
//! exists only when the token sequence, the exact source it spans, and the
//! identity it claims all agree. Every negative case below is a stream that a
//! bare `Vec<Token> + &str` pair would have accepted without complaint.
//!
//! Tests return `Result` and use `ok_or`/`?` rather than `expect`/`panic`, per
//! the crate's integration-test lint policy.

use perl_lexer::{LexerConfig, LocalSymbolTable};
use perl_parser_core::ParserConfigIdentity;
use perl_parser_core::tokens::token_stream::{Token, TokenKind};
use perl_parser_core::tokens::token_subject::{
    ClassificationAuthority, LexerConfigIdentity, TerminalState, TokenStreamProvenance,
    TokenSubjectError, TokenSubjectIdentity, ValidatedTokenStream,
};
use perl_source_identity::{
    ContentDigest, ContentRevision, LogicalSourceId, ProjectId, SourceGeneration, WorkspaceRootId,
};

type R = Result<(), Box<dyn std::error::Error>>;

// ── Fixtures ──────────────────────────────────────────────────────────────────

fn logical_source(path: &str) -> LogicalSourceId {
    let project = ProjectId::from_canonical_name("https://example.invalid/token-subject");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root-key");
    LogicalSourceId::from_root_and_path(&root, path)
}

/// Identity bound to the exact bytes of `source`, at generation `generation`.
fn identity_for(source: &str, path: &str, generation: &str) -> TokenSubjectIdentity {
    TokenSubjectIdentity::new(
        ContentRevision::new(logical_source(path), ContentDigest::of_bytes(source.as_bytes())),
        SourceGeneration::known(generation),
        LexerConfigIdentity::production_default(),
        ParserConfigIdentity::production_default(),
    )
}

fn identity(source: &str) -> TokenSubjectIdentity {
    identity_for(source, "lib/Demo.pm", "1")
}

/// A lexer configuration that differs from the production default in a way that
/// changes token segmentation.
fn flipped_lexer_config() -> LexerConfig {
    LexerConfig {
        parse_interpolation: !LexerConfigIdentity::production_default().parse_interpolation(),
        ..LexerConfig::default()
    }
}

fn complete(source: &str) -> TerminalState {
    TerminalState::CompleteEof { at: source.len() }
}

fn lex(source: &str) -> Result<Vec<Token>, Box<dyn std::error::Error>> {
    Ok(ValidatedTokenStream::lex_for_subject(source)?)
}

/// A valid production subject over `source`, the baseline every negative case
/// perturbs exactly once.
fn fresh_subject(source: &str) -> Result<ValidatedTokenStream<'_>, Box<dyn std::error::Error>> {
    Ok(ValidatedTokenStream::from_fresh_lex(identity(source), source)?)
}

/// A structurally-validated fixture over a caller-supplied token vector.
///
/// `from_fresh_lex` deliberately takes no tokens (it lexes itself), so every
/// case that must inject a *corrupted* stream goes through the fixture seam.
/// The structural rules live in one shared validation plane, so they are
/// exercised identically either way — and routing corrupt streams here is the
/// point of having a non-production seam at all.
fn fixture_with(
    source: &str,
    tokens: Vec<Token>,
    terminal: TerminalState,
) -> Result<ValidatedTokenStream<'_>, TokenSubjectError> {
    ValidatedTokenStream::from_test_fixture(identity(source), source, tokens, terminal)
}

/// Rebuild the token at `index` with a new kind/text/span, leaving the rest of
/// the stream untouched.
fn replace_token(
    tokens: &mut [Token],
    index: usize,
    kind: TokenKind,
    text: &str,
    start: usize,
    end: usize,
) -> R {
    let replacement = Token::new_checked(kind, text, start, end)?;
    *tokens.get_mut(index).ok_or("token index out of range")? = replacement;
    Ok(())
}

fn err_of<T>(result: Result<T, TokenSubjectError>) -> Result<TokenSubjectError, String> {
    match result {
        Ok(_) => Err("expected the subject to be rejected, but it was accepted".to_owned()),
        Err(error) => Ok(error),
    }
}

/// Assert both the reason token and the specific rule that produced it.
///
/// Several rules share a reason token, so a reason-only assertion can pass
/// because a *different* rule fired. Pinning a fragment of the detail keeps each
/// rule independently falsifiable.
fn assert_rule(error: &TokenSubjectError, reason: &str, detail_fragment: &str) -> R {
    assert_eq!(error.reason(), reason, "unexpected reason for: {error}");
    let rendered = error.to_string();
    if !rendered.contains(detail_fragment) {
        return Err(
            format!("expected detail containing {detail_fragment:?}, got {rendered:?}").into()
        );
    }
    Ok(())
}

// ── Positive: the contract accepts real streams ───────────────────────────────

#[test]
fn fresh_full_lex_over_exact_source_is_production_valid() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;

    assert!(subject.is_production_valid());
    assert_eq!(subject.provenance(), &TokenStreamProvenance::FreshFullLex);
    assert_eq!(subject.provenance().label(), "fresh_full_lex");
    assert_eq!(subject.source(), source);
    assert_eq!(subject.terminal(), TerminalState::CompleteEof { at: source.len() });
    assert_eq!(subject.classification_authority(), ClassificationAuthority::LiveUndirectedLex);
    assert!(!subject.tokens().is_empty());
    Ok(())
}

/// Malformed Perl still lexes. A subject is a statement about token/source
/// agreement, not about the source being syntactically valid, so a subject over
/// broken code must still be constructible — otherwise error-recovery parsing
/// could never use one.
#[test]
fn malformed_source_still_forms_a_valid_subject() -> R {
    let source = "if ( { my $x =";
    let subject = fresh_subject(source)?;
    assert!(subject.is_production_valid());
    Ok(())
}

/// Context-sensitive and source-backed constructs are exactly where a
/// token/source mismatch would do the most damage, so each must round-trip.
#[test]
fn context_sensitive_and_source_backed_streams_validate() -> R {
    let cases = [
        ("double-quoted interpolation", "my $s = \"hi $x\";"),
        ("single-quoted", "my $s = 'hi';"),
        ("quote words", "my @a = qw(a b);"),
        ("regex versus division", "$x =~ /ab+c/i; my $q = $a / $b;"),
        ("heredoc body in the gap", "my $t = <<EOT;\nbody\nEOT\nprint $t;\n"),
        ("pod block", "=pod\n\ntext\n\n=cut\nmy $x = 1;\n"),
        ("format", "format STDOUT =\n@<<<\n$x\n.\nmy $y = 1;\n"),
        ("prototype", "sub f($$) { 1 }\n"),
        ("signature", "sub g ($a, $b) { $a + $b }\n"),
        ("data section", "my $x = 1;\n__DATA__\nraw\n"),
        ("comment", "# c\nmy $x = 1;"),
    ];

    for (name, source) in cases {
        let subject = fresh_subject(source).map_err(|e| format!("{name}: {e}"))?;
        assert!(subject.is_production_valid(), "{name} should be production valid");
    }
    Ok(())
}

#[test]
fn encoding_edge_cases_validate() -> R {
    let cases = [
        ("empty source", ""),
        ("unicode identifier and literal", "my $\u{e9}x = \"caf\u{e9}\";"),
        ("bom prefix", "\u{feff}my $x = 1;"),
        ("lf newlines", "my $x = 1;\nmy $y = 2;\n"),
        ("crlf newlines", "my $x = 1;\r\nmy $y = 2;\r\n"),
        ("bare cr", "my $x = 1;\rmy $y = 2;\r"),
        ("astral plane literal", "my $e = \"\u{1f600}\";"),
    ];

    for (name, source) in cases {
        let subject = fresh_subject(source).map_err(|e| format!("{name}: {e}"))?;
        assert_eq!(subject.source(), source, "{name} must retain its exact source");
    }
    Ok(())
}

/// The EOF token is the source-backed empty-payload event: empty text over an
/// empty span at the end of source. It must remain valid geometry.
#[test]
fn terminal_eof_is_valid_empty_payload_geometry() -> R {
    let source = "my $x = 1;";
    let tokens = lex(source)?;
    let last = tokens.last().ok_or("expected at least one token")?;

    assert_eq!(last.kind(), TokenKind::Eof);
    assert_eq!(last.start(), source.len());
    assert_eq!(last.end(), source.len());
    assert!(last.text.is_empty());

    fresh_subject(source)?;
    Ok(())
}

/// A replay is validated in full, but its provenance is the producer's word:
/// it supplies its own tokens, and nothing here can check they came from a
/// checkpoint. So it is constructible and coherent, and deliberately **not**
/// production-valid until #9625 supplies the checkpoint witness. Admitting it
/// would reopen exactly the forgery this type exists to prevent.
#[test]
fn a_checkpoint_replay_is_validated_but_not_yet_production_valid() -> R {
    let source = "my $x = 1;";
    let subject = ValidatedTokenStream::from_checkpoint_replay(
        identity_for(source, "lib/Demo.pm", "7"),
        source,
        lex(source)?,
        complete(source),
        SourceGeneration::known("6"),
        ClassificationAuthority::LiveUndirectedLex,
    )?;

    assert_eq!(subject.provenance().label(), "checkpoint_replay_to_eof");
    assert!(
        !subject.is_production_valid(),
        "replay provenance is a producer attestation and must not be production-valid until \
         #9625 supplies the checkpoint witness"
    );
    Ok(())
}

/// The replay counterpart of the fresh-seam forgery control: a caller-supplied
/// stream with an altered token kind passes every structural rule and is
/// accepted as a replay, and must still never be production-valid.
#[test]
fn a_forged_replay_can_never_be_production_valid() -> R {
    let source = "my $x = 1;";
    let mut forged = lex(source)?;
    replace_token(&mut forged, 0, TokenKind::Identifier, "my", 0, 2)?;

    let subject = ValidatedTokenStream::from_checkpoint_replay(
        identity_for(source, "lib/Demo.pm", "7"),
        source,
        forged,
        complete(source),
        SourceGeneration::known("6"),
        ClassificationAuthority::LiveUndirectedLex,
    )?;

    assert!(!subject.is_production_valid());
    Ok(())
}

/// Only the seam this module performs itself is production-valid.
#[test]
fn only_a_module_performed_fresh_lex_is_production_admissible() -> R {
    assert!(TokenStreamProvenance::FreshFullLex.is_production_admissible());
    assert!(
        !TokenStreamProvenance::CheckpointReplayToEof {
            predecessor_generation: SourceGeneration::known("6")
        }
        .is_production_admissible()
    );
    assert!(!TokenStreamProvenance::TestFixtureUnchecked.is_production_admissible());
    assert!(!TokenStreamProvenance::ExactSuffixSync.is_production_admissible());
    assert!(!TokenStreamProvenance::UnsupportedOrIncomplete.is_production_admissible());
    Ok(())
}

/// A fixture is structurally validated but must never be mistaken for an
/// admitted production stream — that separation is the whole point of the
/// provenance vocabulary.
#[test]
fn test_fixture_is_structurally_valid_but_never_production_valid() -> R {
    let source = "my $x = 1;";
    let subject = ValidatedTokenStream::from_test_fixture(
        identity(source),
        source,
        lex(source)?,
        complete(source),
    )?;

    assert!(!subject.is_production_valid());
    assert_eq!(subject.provenance().label(), "test_fixture_unchecked");
    Ok(())
}

/// A fixture may be a partial stream; a production subject may not. The same
/// input therefore has to be accepted here and refused in
/// `incomplete_production_stream_is_rejected`.
#[test]
fn incomplete_stream_is_allowed_for_a_non_production_fixture() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    tokens.retain(|token| token.kind() != TokenKind::Eof);
    tokens.truncate(2);

    let subject = ValidatedTokenStream::from_test_fixture(
        identity(source),
        source,
        tokens,
        TerminalState::Incomplete { stopped_at: 5 },
    )?;

    assert!(!subject.is_production_valid());
    assert!(!subject.terminal().is_complete());
    assert_eq!(subject.terminal().offset(), 5);
    Ok(())
}

#[test]
fn a_matching_consumer_identity_verifies() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;
    subject.verify_against(&identity(source))?;
    Ok(())
}

// ── Positive: deterministic identity ──────────────────────────────────────────

#[test]
fn repeated_construction_yields_an_identical_fingerprint() -> R {
    let source = "my $x = 1;";
    let first = fresh_subject(source)?;
    let second = fresh_subject(source)?;

    assert_eq!(first.subject_fingerprint(), second.subject_fingerprint());
    assert_eq!(first.identity(), second.identity());
    Ok(())
}

#[test]
fn fingerprint_separates_every_identity_field() -> R {
    let source = "my $x = 1;";
    let other_source = "my $y = 2;";
    let base = fresh_subject(source)?.subject_fingerprint();

    // Different content.
    assert_ne!(base, fresh_subject(other_source)?.subject_fingerprint());

    // Same bytes, different logical source.
    let elsewhere =
        ValidatedTokenStream::from_fresh_lex(identity_for(source, "lib/Other.pm", "1"), source)?;
    assert_ne!(base, elsewhere.subject_fingerprint());

    // Same bytes and logical source, different generation. Edit-then-undo
    // returns to the same digest but is not the same subject.
    let regenerated =
        ValidatedTokenStream::from_fresh_lex(identity_for(source, "lib/Demo.pm", "2"), source)?;
    assert_ne!(base, regenerated.subject_fingerprint());

    // Same bytes, logical source and generation, different lexer configuration.
    // A non-default lexer configuration cannot go through `from_fresh_lex` (that
    // seam lexes under the default), so compare the identity fingerprint
    // directly — the field still has to separate the two.
    let reconfigured = TokenSubjectIdentity::new(
        ContentRevision::new(
            logical_source("lib/Demo.pm"),
            ContentDigest::of_bytes(source.as_bytes()),
        ),
        SourceGeneration::known("1"),
        LexerConfigIdentity::of(&flipped_lexer_config()),
        ParserConfigIdentity::production_default(),
    );
    assert_ne!(base, reconfigured.fingerprint());

    Ok(())
}

// ── Negative: source binding ──────────────────────────────────────────────────

/// The headline failure the bare `Vec<Token> + &str` pair cannot detect.
///
/// `from_fresh_lex` can no longer express this mistake at all — it lexes the
/// source it is given — so the case is posed where a caller *does* supply
/// tokens: an identity bound to source A, paired with source B.
#[test]
fn tokens_from_source_a_paired_with_source_b_are_rejected() -> R {
    let source_a = "my $x = 1;";
    let source_b = "my $y = 2;";

    let error = err_of(ValidatedTokenStream::from_test_fixture(
        identity(source_a),
        source_b,
        lex(source_a)?,
        complete(source_b),
    ))?;

    assert_rule(&error, "wrong_source", "hashes to")?;
    Ok(())
}

/// The stale-content case at the verification plane: the subject is internally
/// perfect, but the consumer has moved on to different bytes. Construction
/// cannot catch this — only the consumer's own expectation can.
#[test]
fn a_subject_over_different_content_fails_consumer_verification() -> R {
    let source = "my $x = 1;";
    let moved_on = "my $x = 2;";
    let subject = fresh_subject(source)?;

    let error = err_of(subject.verify_against(&identity(moved_on)))?;

    assert_rule(&error, "wrong_source", "does not match expected")?;
    Ok(())
}

#[test]
fn identical_bytes_under_a_different_logical_source_are_a_different_subject() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;

    let error = err_of(subject.verify_against(&identity_for(source, "lib/Other.pm", "1")))?;

    assert_rule(&error, "wrong_source", "different logical source")?;
    Ok(())
}

// ── Negative: token geometry and payload ──────────────────────────────────────

/// A span shifted by one byte while the payload stays put: the pair is still
/// well-formed, the width still matches, and only source comparison catches it.
#[test]
fn a_shifted_span_with_unchanged_payload_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    replace_token(&mut tokens, 0, TokenKind::My, "my", 1, 3)?;

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "payload_source_mismatch", "index 0")?;
    Ok(())
}

#[test]
fn a_changed_payload_over_unchanged_source_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    replace_token(&mut tokens, 3, TokenKind::Number, "2", 8, 9)?;

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "payload_source_mismatch", "index 3")?;
    Ok(())
}

#[test]
fn a_span_past_the_end_of_source_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    replace_token(&mut tokens, 4, TokenKind::Semicolon, ";", source.len(), source.len() + 1)?;

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    // An out-of-range offset is also not a character boundary, so a reason-only
    // assertion here would still pass with the bounds rule deleted. Pin the
    // bounds rule specifically.
    assert_rule(&error, "invalid_token_range", "exceeds source length")?;
    Ok(())
}

/// Slicing a non-boundary span would abort the process, so the validator has to
/// reject it before it ever indexes the source.
#[test]
fn a_span_off_a_utf8_character_boundary_is_rejected() -> R {
    let source = "my $\u{e9}x = 1;";
    assert!(!source.is_char_boundary(5), "fixture must straddle a multi-byte character");

    let mut tokens = lex(source)?;
    replace_token(&mut tokens, 1, TokenKind::Identifier, "x", 5, 6)?;

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "invalid_token_range", "UTF-8 character boundary")?;
    Ok(())
}

#[test]
fn overlapping_tokens_are_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    // "y " at 1..3 overlaps "my" at 0..2 and is a correct payload for its span,
    // so only the ordering rule can reject it.
    replace_token(&mut tokens, 1, TokenKind::Identifier, "y ", 1, 3)?;

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "invalid_token_range", "overlaps or precedes")?;
    Ok(())
}

/// Tokens are ordered but not adjacent: trivia, POD, and heredoc bodies live in
/// the gaps. A validator that demanded contiguity would reject almost every
/// real file, so this is the negative control on the ordering rule itself.
#[test]
fn non_contiguous_tokens_are_accepted_because_trivia_occupies_the_gaps() -> R {
    let source = "# leading comment\nmy $x = 1;";
    let tokens = lex(source)?;
    let first = tokens.first().ok_or("expected at least one token")?;

    assert!(first.start() > 0, "the comment must leave a gap before the first token");
    fresh_subject(source)?;
    Ok(())
}

// ── Negative: terminal state ──────────────────────────────────────────────────

#[test]
fn a_terminal_eof_claimed_before_the_end_of_source_is_rejected() -> R {
    let source = "my $x = 1;";
    let error = err_of(fixture_with(source, lex(source)?, TerminalState::CompleteEof { at: 5 }))?;

    assert_rule(&error, "invalid_terminal_state", "complete EOF claimed at")?;
    Ok(())
}

#[test]
fn an_eof_token_disagreeing_with_the_terminal_offset_is_rejected() -> R {
    let source = "my $x = 1;";
    let tokens = vec![
        Token::new_checked(TokenKind::My, "my", 0, 2)?,
        Token::new_checked(TokenKind::Eof, "", 5, 5)?,
    ];

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "invalid_terminal_state", "does not agree with the terminal EOF")?;
    Ok(())
}

/// A complete EOF must be carried, not merely asserted.
#[test]
fn a_complete_eof_without_a_terminal_eof_token_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    tokens.retain(|token| token.kind() != TokenKind::Eof);

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "invalid_terminal_state", "requires a terminal EOF token")?;
    Ok(())
}

/// The degenerate case the loop-based rules cannot see: with no tokens at all
/// there is nothing to find fault with, so the completeness claim has to be
/// challenged directly or an empty stream "validates" against real source.
#[test]
fn an_empty_token_stream_over_non_empty_source_is_rejected() -> R {
    let source = "my $x = 1;";

    let error = err_of(fixture_with(source, Vec::new(), complete(source)))?;

    assert_rule(&error, "invalid_terminal_state", "requires a terminal EOF token")?;
    Ok(())
}

#[test]
fn a_token_after_the_terminal_eof_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    tokens.push(Token::new_checked(TokenKind::Eof, "", source.len(), source.len())?);

    let error = err_of(fixture_with(source, tokens, complete(source)))?;

    assert_rule(&error, "invalid_terminal_state", "follows the terminal EOF token")?;
    Ok(())
}

#[test]
fn an_incomplete_stream_carrying_an_eof_token_is_rejected() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_test_fixture(
        identity(source),
        source,
        lex(source)?,
        TerminalState::Incomplete { stopped_at: 5 },
    ))?;

    assert_rule(
        &error,
        "invalid_terminal_state",
        "incomplete stream carries a terminal EOF token",
    )?;
    Ok(())
}

/// Without this rule the stop offset is decorative: a stream could report
/// stopping at byte 5 while carrying tokens that run past it.
#[test]
fn an_incomplete_stream_whose_tokens_outrun_its_stop_offset_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    tokens.retain(|token| token.kind() != TokenKind::Eof);

    let error = err_of(ValidatedTokenStream::from_test_fixture(
        identity(source),
        source,
        tokens,
        TerminalState::Incomplete { stopped_at: 5 },
    ))?;

    assert_rule(&error, "invalid_terminal_state", "but its last token ends at")?;
    Ok(())
}

/// The same partial stream the fixture above accepts must be refused under a
/// provenance that claims completeness. `from_fresh_lex` cannot express it (it
/// lexes to EOF itself), so the completeness-claiming side is posed as a replay
/// — which still owes the terminal rules even though it is not production-valid.
#[test]
fn incomplete_production_stream_is_rejected() -> R {
    let source = "my $x = 1;";
    let mut tokens = lex(source)?;
    tokens.retain(|token| token.kind() != TokenKind::Eof);
    tokens.truncate(2);

    let error = err_of(ValidatedTokenStream::from_checkpoint_replay(
        identity_for(source, "lib/Demo.pm", "7"),
        source,
        tokens,
        TerminalState::Incomplete { stopped_at: 5 },
        SourceGeneration::known("6"),
        ClassificationAuthority::LiveUndirectedLex,
    ))?;

    assert_rule(&error, "incomplete_stream", "must reach a complete terminal EOF")?;
    assert!(error.requires_full_source_fallback());
    Ok(())
}

// ── Negative: generation and provenance ───────────────────────────────────────

#[test]
fn a_production_subject_without_a_known_generation_is_rejected() -> R {
    let source = "my $x = 1;";
    let unknown = TokenSubjectIdentity::new(
        ContentRevision::new(
            logical_source("lib/Demo.pm"),
            ContentDigest::of_bytes(source.as_bytes()),
        ),
        SourceGeneration::Unknown,
        LexerConfigIdentity::production_default(),
        ParserConfigIdentity::production_default(),
    );

    let error = err_of(ValidatedTokenStream::from_fresh_lex(unknown, source))?;

    assert_rule(&error, "wrong_generation", "requires a known generation")?;
    Ok(())
}

/// The exact failure #8132 names: predecessor tokens relabelled as
/// final-generation tokens. The stream is otherwise perfectly coherent.
#[test]
fn a_replay_naming_its_own_generation_as_predecessor_is_rejected() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_checkpoint_replay(
        identity_for(source, "lib/Demo.pm", "7"),
        source,
        lex(source)?,
        complete(source),
        SourceGeneration::known("7"),
        ClassificationAuthority::LiveUndirectedLex,
    ))?;

    assert_rule(&error, "wrong_generation", "cannot be relabelled")?;
    Ok(())
}

#[test]
fn a_replay_without_a_known_predecessor_generation_is_rejected() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_checkpoint_replay(
        identity_for(source, "lib/Demo.pm", "7"),
        source,
        lex(source)?,
        complete(source),
        SourceGeneration::Unknown,
        ClassificationAuthority::LiveUndirectedLex,
    ))?;

    assert_rule(&error, "wrong_generation", "known predecessor generation")?;
    Ok(())
}

#[test]
fn a_replay_whose_kinds_were_never_re_resolved_is_rejected() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_checkpoint_replay(
        identity_for(source, "lib/Demo.pm", "7"),
        source,
        lex(source)?,
        complete(source),
        SourceGeneration::known("6"),
        ClassificationAuthority::CarriedFromPredecessor,
    ))?;

    assert_rule(&error, "missing_classification_authority", "resolved by a live pass")?;
    assert!(error.requires_full_source_fallback());
    Ok(())
}

/// The reserved class is nameable but not emittable: #6986 has not admitted it.
#[test]
fn exact_suffix_sync_provenance_is_refused() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_exact_suffix_sync(
        identity(source),
        source,
        lex(source)?,
        complete(source),
    ))?;

    assert_rule(&error, "unsupported_provenance", "exact_suffix_sync")?;
    assert!(error.requires_full_source_fallback());
    assert!(!TokenStreamProvenance::ExactSuffixSync.is_production_admissible());
    assert!(!TokenStreamProvenance::UnsupportedOrIncomplete.is_production_admissible());
    Ok(())
}

// ── Negative: configuration ───────────────────────────────────────────────────

#[test]
fn an_unknown_subject_schema_is_rejected() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_fresh_lex(
        identity(source).with_schema_version(999),
        source,
    ))?;

    assert_rule(&error, "wrong_configuration", "unknown subject schema")?;
    Ok(())
}

#[test]
fn a_stale_lexer_configuration_fails_consumer_verification() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;

    let default_identity = LexerConfigIdentity::production_default();
    let flipped_config = LexerConfig {
        parse_interpolation: !default_identity.parse_interpolation(),
        ..LexerConfig::default()
    };
    let expected = TokenSubjectIdentity::new(
        ContentRevision::new(
            logical_source("lib/Demo.pm"),
            ContentDigest::of_bytes(source.as_bytes()),
        ),
        SourceGeneration::known("1"),
        LexerConfigIdentity::of(&flipped_config),
        ParserConfigIdentity::production_default(),
    );

    let error = err_of(subject.verify_against(&expected))?;

    assert_rule(&error, "wrong_configuration", "lexer configuration identity")?;
    Ok(())
}

#[test]
fn a_stale_generation_fails_consumer_verification() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;

    let error = err_of(subject.verify_against(&identity_for(source, "lib/Demo.pm", "2")))?;

    assert_rule(&error, "wrong_generation", "does not match expected")?;
    Ok(())
}

#[test]
fn a_different_schema_fails_consumer_verification() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;

    let error = err_of(subject.verify_against(&identity(source).with_schema_version(2)))?;

    assert_rule(&error, "wrong_configuration", "schema v")?;
    Ok(())
}

#[test]
fn an_unsupported_or_incomplete_provenance_is_refused() -> R {
    let source = "my $x = 1;";
    let error = err_of(ValidatedTokenStream::from_unsupported(
        identity(source),
        source,
        lex(source)?,
        complete(source),
    ))?;

    assert_rule(&error, "unsupported_provenance", "unsupported_or_incomplete")?;
    Ok(())
}

/// A refused provenance is refused before anything else is judged, so a
/// producer is told the real reason rather than a downstream symptom.
#[test]
fn a_refused_provenance_outranks_every_other_defect() -> R {
    let source = "my $x = 1;";
    let other = "my $y = 2;";

    // Wrong source, wrong generation and a broken terminal all at once.
    let error = err_of(ValidatedTokenStream::from_exact_suffix_sync(
        identity(other).with_schema_version(999),
        source,
        lex(source)?,
        TerminalState::CompleteEof { at: 1 },
    ))?;

    assert_rule(&error, "unsupported_provenance", "exact_suffix_sync")?;
    Ok(())
}

// ── The documented example ────────────────────────────────────────────────────

/// `perl-parser-core` sets `doctest = false`, so the module's rustdoc example
/// never executes. This mirrors it exactly so the documented usage is proven
/// rather than merely written down.
#[test]
fn the_documented_quick_start_example_runs() -> R {
    let source = "my $x = 1;";

    let project = ProjectId::from_canonical_name("https://example.invalid/demo");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root");
    let logical = LogicalSourceId::from_root_and_path(&root, "lib/Demo.pm");
    let revision = ContentRevision::new(logical, ContentDigest::of_bytes(source.as_bytes()));

    let subject_identity = TokenSubjectIdentity::new(
        revision,
        SourceGeneration::known("1"),
        LexerConfigIdentity::production_default(),
        ParserConfigIdentity::production_default(),
    );

    let subject = ValidatedTokenStream::from_fresh_lex(subject_identity, source)?;

    assert!(subject.is_production_valid());
    Ok(())
}

/// The forgery `from_fresh_lex` now structurally prevents.
///
/// Each of these streams satisfies every structural rule — payloads match the
/// source they span, ordering holds, the terminal EOF is carried — yet none is
/// the canonical lex. Structural validation cannot tell the difference, which is
/// exactly why the fresh seam performs the lex itself instead of accepting
/// tokens. Posed here through the fixture seam, they are accepted structurally
/// but are never production-valid.
#[test]
fn a_structurally_valid_forgery_can_never_be_production_valid() -> R {
    let source = "my $x = 1;";

    // A wrong token kind: "my" relabelled as an identifier.
    let mut wrong_kind = lex(source)?;
    replace_token(&mut wrong_kind, 0, TokenKind::Identifier, "my", 0, 2)?;
    let forged = fixture_with(source, wrong_kind, complete(source))
        .map_err(|e| format!("wrong-kind stream should still be structurally valid: {e}"))?;
    assert!(!forged.is_production_valid());

    // Omitted semantic tokens: indistinguishable from the gaps trivia leaves.
    let mut truncated = lex(source)?;
    truncated.retain(|token| matches!(token.kind(), TokenKind::My | TokenKind::Eof));
    let gapped = fixture_with(source, truncated, complete(source))
        .map_err(|e| format!("gapped stream should still be structurally valid: {e}"))?;
    assert!(!gapped.is_production_valid());

    // And the production seam cannot be handed either one: it takes no tokens.
    let honest = fresh_subject(source)?;
    assert!(honest.is_production_valid());
    assert!(honest.tokens().len() > gapped.tokens().len());
    Ok(())
}

/// `from_fresh_lex` lexes under the default configuration, so an identity
/// claiming another one would describe a lex that never happened.
#[test]
fn a_fresh_lex_under_a_non_default_lexer_configuration_is_refused() -> R {
    let source = "my $x = 1;";
    let reconfigured = TokenSubjectIdentity::new(
        ContentRevision::new(
            logical_source("lib/Demo.pm"),
            ContentDigest::of_bytes(source.as_bytes()),
        ),
        SourceGeneration::known("1"),
        LexerConfigIdentity::of(&flipped_lexer_config()),
        ParserConfigIdentity::production_default(),
    );

    let error = err_of(ValidatedTokenStream::from_fresh_lex(reconfigured, source))?;

    assert_rule(&error, "wrong_configuration", "default lexer configuration")?;
    Ok(())
}

/// A bound symbol table changes bareword/regex classification, but
/// `LocalSymbolTable` exposes no content identity, so two different tables look
/// identical here. Verification refuses rather than silently accepting a subject
/// lexed under a different table.
#[test]
fn a_bound_symbol_table_makes_the_configuration_unverifiable() -> R {
    let source = "my $x = 1;";
    let subject = fresh_subject(source)?;

    let table = LocalSymbolTable::scan_subs("sub helper { 1 }\n");
    let bound = LexerConfig { symbol_table: Some(table), ..LexerConfig::default() };
    let expected = TokenSubjectIdentity::new(
        ContentRevision::new(
            logical_source("lib/Demo.pm"),
            ContentDigest::of_bytes(source.as_bytes()),
        ),
        SourceGeneration::known("1"),
        LexerConfigIdentity::of(&bound),
        ParserConfigIdentity::production_default(),
    );
    assert!(expected.lexer_config().symbol_table_bound());

    let error = err_of(subject.verify_against(&expected))?;

    assert_rule(&error, "wrong_configuration", "no content identity")?;
    Ok(())
}

// ── Negative control on the suite itself ──────────────────────────────────────

/// Every negative case above perturbs a stream that is otherwise accepted. If
/// the baseline did not validate, those cases would pass for the wrong reason
/// and prove nothing. This pins the baseline.
#[test]
fn the_unperturbed_baseline_each_negative_case_starts_from_is_accepted() -> R {
    for source in ["my $x = 1;", "my $\u{e9}x = 1;", "# leading comment\nmy $x = 1;"] {
        let subject = fresh_subject(source)?;
        assert!(subject.is_production_valid(), "baseline {source:?} must validate");
    }
    Ok(())
}

/// Distinct failures must stay distinguishable: a consumer that maps every
/// rejection onto one reason cannot tell "fall back and re-lex" from
/// "this subject is wrong".
#[test]
fn reason_tokens_are_distinct_across_failure_planes() -> R {
    // Derived from real error values, never from a list of literals. Asserting
    // that a hardcoded array of strings has distinct elements proves only that
    // the literals differ: it would keep passing if `reason()` collapsed every
    // variant onto one token, and it silently rots when a variant is renamed.
    let all = [
        TokenSubjectError::WrongSource { detail: String::new() },
        TokenSubjectError::WrongGeneration { detail: String::new() },
        TokenSubjectError::WrongConfiguration { detail: String::new() },
        TokenSubjectError::InvalidTokenRange { index: 0, detail: String::new() },
        TokenSubjectError::PayloadSourceMismatch { index: 0, start: 0, end: 0 },
        TokenSubjectError::InvalidTerminalState { detail: String::new() },
        TokenSubjectError::IncompleteStream { detail: String::new() },
        TokenSubjectError::MissingClassificationAuthority { detail: String::new() },
        TokenSubjectError::InstrumentFailure { detail: String::new() },
        TokenSubjectError::UnsupportedProvenance { provenance: "any" },
    ];

    let observed: std::collections::BTreeSet<&str> = all.iter().map(|e| e.reason()).collect();
    assert_eq!(
        observed.len(),
        all.len(),
        "every variant needs its own reason token; a consumer that cannot tell two failures \
         apart cannot tell 'fall back and re-lex' from 'this subject is wrong': {observed:?}"
    );

    // Pin the tokens themselves: they are a machine-readable surface, so a
    // rename is a contract change and must not pass silently.
    let expected: std::collections::BTreeSet<&str> = [
        "wrong_source",
        "wrong_generation",
        "wrong_configuration",
        "invalid_token_range",
        "payload_source_mismatch",
        "invalid_terminal_state",
        "incomplete_stream",
        "missing_classification_authority",
        "instrument_failure",
        "unsupported_provenance",
    ]
    .into_iter()
    .collect();
    assert_eq!(observed, expected);
    Ok(())
}
