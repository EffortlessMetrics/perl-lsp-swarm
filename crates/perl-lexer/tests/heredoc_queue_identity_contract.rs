//! Queue-admission identity contract for #13502.
//!
//! Every successfully queued heredoc carries one immutable, source-backed
//! declaration identity captured during the original opener scan. Token
//! presence is not proof: every field and exact UTF-8 byte range is asserted.
//! The public [`perl_lexer::Token`] API is not the consumer seam.

use std::ops::Range;

use perl_lexer::{
    Checkpointable, HeredocOpenerForm, HeredocOperator, HeredocQueueIdentity, PerlLexer, Token,
    TokenType,
};
use perl_source_identity::{
    ContentDigest, LogicalSourceId, ProjectId, SourceGeneration, WorkspaceRootId,
};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn require(condition: bool, message: impl Into<String>) -> R {
    if condition { Ok(()) } else { Err(missing(message)) }
}

fn require_eq<T: std::fmt::Debug + PartialEq>(
    actual: &T,
    expected: &T,
    context: impl Into<String>,
) -> R {
    if actual == expected {
        Ok(())
    } else {
        Err(missing(format!("{}: expected {expected:?}, got {actual:?}", context.into())))
    }
}

fn logical_source(path: &str) -> LogicalSourceId {
    let project = ProjectId::from_canonical_name("https://github.com/acme/widget");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root");
    LogicalSourceId::from_root_and_path(&root, path)
}

fn advance_until_semicolon(lexer: &mut PerlLexer<'_>) -> R {
    loop {
        let token = lexer.next_token().ok_or_else(|| missing("source ended before semicolon"))?;
        if matches!(token.token_type, TokenType::Semicolon) {
            return Ok(());
        }
        if matches!(token.token_type, TokenType::EOF) {
            return Err(missing("hit EOF before semicolon"));
        }
    }
}

fn queued(source: &str) -> R<(PerlLexer<'_>, Vec<HeredocQueueIdentity>)> {
    let mut lexer = PerlLexer::new(source);
    advance_until_semicolon(&mut lexer)?;
    let identities = lexer.pending_heredoc_identities();
    Ok((lexer, identities))
}

fn assert_char_boundaries(source: &str, range: Range<usize>, label: &str) -> R {
    require(
        source.is_char_boundary(range.start) && source.is_char_boundary(range.end),
        format!("{label} is not on UTF-8 boundaries"),
    )
}

fn assert_slice(source: &str, range: Range<usize>, expected: &str, label: &str) -> R {
    assert_char_boundaries(source, range.clone(), label)?;
    require_eq(&source.get(range.clone()), &Some(expected), format!("{label} source slice"))?;
    require_eq(&(range.end - range.start), &expected.len(), format!("{label} byte length"))
}

fn assert_identity_geometry(
    source: &str,
    identity: &HeredocQueueIdentity,
    opener: &str,
    operator: &str,
    raw_label: &str,
    decoded: &str,
    form: HeredocOpenerForm,
    operator_kind: HeredocOperator,
) -> R {
    let opener_range = identity.opener_range().as_range();
    let operator_range = identity.operator_range().as_range();
    let raw_range = identity.raw_label_range().as_range();
    assert_slice(source, opener_range.clone(), opener, "opener")?;
    assert_slice(source, operator_range.clone(), operator, "operator")?;
    assert_slice(source, raw_range.clone(), raw_label, "raw label")?;
    require_eq(&identity.raw_label_spelling(), &raw_label, "raw spelling")?;
    require_eq(&identity.decoded_label(), &decoded, "decoded label")?;
    require_eq(&identity.form(), &form, "form")?;
    require_eq(&identity.operator(), &operator_kind, "operator kind")?;
    require_eq(&identity.interpolates(), &form.interpolates(), "interpolation disposition")?;
    require_eq(&identity.is_command(), &form.is_command(), "command disposition")?;
    require_eq(
        &identity.allow_indent(),
        &matches!(operator_kind, HeredocOperator::Indented),
        "indent policy",
    )?;
    require(
        identity.raw_label_spelling() == raw_label,
        "raw spelling must be the captured scan text, not a later source search",
    )?;
    require(
        opener_range.start == operator_range.start,
        "opener and operator must share the first `<`",
    )?;
    require(operator_range.end <= raw_range.start, "operator must precede the raw label")?;
    require(raw_range.end == opener_range.end, "opener must end at the raw label")?;
    require_eq(
        identity.source().content_digest(),
        &ContentDigest::of_bytes(source.as_bytes()),
        "admitted content digest",
    )?;
    Ok(())
}

#[test]
fn every_supported_form_preserves_raw_decoded_geometry_and_mode() -> R {
    let cases = [
        (
            "print <<EOF;",
            "<<EOF",
            "<<",
            "EOF",
            "EOF",
            HeredocOpenerForm::Bare,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<'EOF';",
            "<<'EOF'",
            "<<",
            "'EOF'",
            "EOF",
            HeredocOpenerForm::SingleQuoted,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<\"EOF\";",
            "<<\"EOF\"",
            "<<",
            "\"EOF\"",
            "EOF",
            HeredocOpenerForm::DoubleQuoted,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<`CMD`;",
            "<<`CMD`",
            "<<",
            "`CMD`",
            "CMD",
            HeredocOpenerForm::Backtick,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<\\RAW;",
            "<<\\RAW",
            "<<",
            "\\RAW",
            "RAW",
            HeredocOpenerForm::Backslash,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<~END;",
            "<<~END",
            "<<~",
            "END",
            "END",
            HeredocOpenerForm::Bare,
            HeredocOperator::Indented,
        ),
        (
            "print <<~ 'END';",
            "<<~ 'END'",
            "<<~",
            "'END'",
            "END",
            HeredocOpenerForm::SingleQuoted,
            HeredocOperator::Indented,
        ),
        (
            "print <<  END;",
            "<<  END",
            "<<",
            "END",
            "END",
            HeredocOpenerForm::Bare,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<\"\";",
            "<<\"\"",
            "<<",
            "\"\"",
            "",
            HeredocOpenerForm::DoubleQuoted,
            HeredocOperator::Ordinary,
        ),
        (
            "print <<\"EO\\\\tF\";",
            "<<\"EO\\\\tF\"",
            "<<",
            "\"EO\\\\tF\"",
            "EO\\\\tF",
            HeredocOpenerForm::DoubleQuoted,
            HeredocOperator::Ordinary,
        ),
        ("print <<α;", "<<α", "<<", "α", "α", HeredocOpenerForm::Bare, HeredocOperator::Ordinary),
    ];
    for (source, opener, operator, raw, decoded, form, operator_kind) in cases {
        let (_, identities) = queued(source)?;
        require_eq(&identities.len(), &1, format!("queued count for {source}"))?;
        let identity = &identities[0];
        require_eq(&identity.ordinal(), &0, format!("ordinal for {source}"))?;
        require_eq(&identity.body_start(), &0, format!("pre-newline body_start for {source}"))?;
        require(
            !identity.source().is_publication_ready(),
            "unbound lexer must stay identity-unavailable",
        )?;
        require(
            identity.source().logical_source().is_none(),
            "unbound lexer has no logical source",
        )?;
        require(
            identity.source().generation().is_unknown(),
            "Unknown generation must not be promoted",
        )?;
        assert_identity_geometry(
            source,
            identity,
            opener,
            operator,
            raw,
            decoded,
            form,
            operator_kind,
        )?;
        require(
            identity.raw_label_spelling() != identity.decoded_label()
                || !matches!(
                    form,
                    HeredocOpenerForm::SingleQuoted
                        | HeredocOpenerForm::DoubleQuoted
                        | HeredocOpenerForm::Backtick
                        | HeredocOpenerForm::Backslash
                ),
            "quoted/backslash raw spelling must stay distinct from the decoded label",
        )?;
    }
    Ok(())
}

#[test]
fn mixed_forms_on_one_line_keep_declaration_order_and_distinct_ordinals() -> R {
    let source = "print <<A, <<'A', <<\"A\", <<`A`, <<\\A;";
    let (lexer, identities) = queued(source)?;
    require_eq(&identities.len(), &5, "mixed queue length")?;
    let expected = [
        (HeredocOpenerForm::Bare, "A", "A", false),
        (HeredocOpenerForm::SingleQuoted, "'A'", "A", false),
        (HeredocOpenerForm::DoubleQuoted, "\"A\"", "A", false),
        (HeredocOpenerForm::Backtick, "`A`", "A", true),
        (HeredocOpenerForm::Backslash, "\\A", "A", false),
    ];
    let mut seen_raw_starts = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let (form, raw, decoded, is_command) = expected[index];
        require_eq(&identity.ordinal(), &(index as u32), "declaration ordinal")?;
        require_eq(&identity.form(), &form, "mixed form")?;
        require_eq(&identity.raw_label_spelling(), &raw, "mixed raw spelling")?;
        require_eq(&identity.decoded_label(), &decoded, "mixed decoded label")?;
        require_eq(&identity.is_command(), &is_command, "mixed command")?;
        require_eq(&identity.interpolates(), &form.interpolates(), "mixed interpolation")?;
        let start = identity.raw_label_range().start();
        require(
            !seen_raw_starts.contains(&start),
            "duplicate labels must not share a searched raw range",
        )?;
        seen_raw_starts.push(start);
        require(
            identity.opener_range().slice(source).is_some(),
            "mixed opener range must be an exact source slice",
        )?;
    }
    require(
        lexer.front_heredoc_identity().map(HeredocQueueIdentity::ordinal) == Some(0),
        "#7285 front-entry identity must be the first admitted declaration",
    )?;
    Ok(())
}

#[test]
fn duplicate_and_near_collision_labels_remain_distinct() -> R {
    let source = "print <<END, <<END, <<END1;";
    let (_, identities) = queued(source)?;
    require_eq(&identities.len(), &3, "duplicate queue length")?;
    require_eq(&identities[0].decoded_label(), &"END", "first decoded")?;
    require_eq(&identities[1].decoded_label(), &"END", "second decoded")?;
    require_eq(&identities[2].decoded_label(), &"END1", "near-collision decoded")?;
    require(
        identities[0].ordinal() != identities[1].ordinal(),
        "duplicate labels must not share an ordinal",
    )?;
    require(
        identities[0].raw_label_range().as_range() != identities[1].raw_label_range().as_range(),
        "searching current source for END would collapse duplicate raw ranges",
    )?;
    require(
        identities[0].ordinal() < identities[1].ordinal()
            && identities[1].ordinal() < identities[2].ordinal(),
        "declaration order must not be reordered by label text",
    )?;
    Ok(())
}

#[test]
fn later_entry_body_start_is_the_declaration_line_boundary() -> R {
    let source = "print <<A, <<B;\na\nA\nb\nB\n";
    let mut lexer = PerlLexer::with_body_tokens(source);
    let mut saw_first_body = false;
    loop {
        let token = lexer.next_token().ok_or_else(|| missing("source ended early"))?;
        if matches!(
            token.token_type,
            TokenType::HeredocBody(_) | TokenType::InterpolatedHeredocBody(_)
        ) && !saw_first_body
        {
            saw_first_body = true;
            let front = lexer
                .front_heredoc_identity()
                .ok_or_else(|| missing("second entry must remain queued after the first body"))?;
            require_eq(&front.decoded_label(), &"B", "front after first body")?;
            require_eq(&front.ordinal(), &1, "second ordinal survives the first drain")?;
            let expected_body_start = source
                .find("\nb\n")
                .map(|index| index + 1)
                .ok_or_else(|| missing("missing second body"))?;
            require_eq(&front.body_start(), &expected_body_start, "logical body-start boundary")?;
            require(
                source.is_char_boundary(front.body_start()),
                "body_start is not on a UTF-8 boundary",
            )?;
        }
        if matches!(token.token_type, TokenType::EOF) {
            break;
        }
    }
    require(saw_first_body, "first body token was not emitted")?;
    Ok(())
}

#[test]
fn bound_source_identity_is_snapshotted_and_unknown_generation_is_not_promoted() -> R {
    let source = "print <<EOF;";
    let mut lexer = PerlLexer::new(source);
    lexer.bind_logical_source(logical_source("lib/Widget.pm"));
    lexer.bind_generation(SourceGeneration::known("7"));
    advance_until_semicolon(&mut lexer)?;
    let identity =
        lexer.front_heredoc_identity().ok_or_else(|| missing("missing queued identity"))?;
    require(
        identity.source().is_publication_ready(),
        "bound source+generation is publication-ready",
    )?;
    require_eq(
        &identity.source().logical_source(),
        &Some(&logical_source("lib/Widget.pm")),
        "logical source",
    )?;
    require_eq(
        identity.source().generation(),
        &SourceGeneration::known("7"),
        "generation snapshot",
    )?;
    require_eq(
        identity.source().content_digest(),
        &ContentDigest::of_bytes(source.as_bytes()),
        "content digest snapshot",
    )?;

    let mut unknown = PerlLexer::new(source);
    unknown.bind_logical_source(logical_source("lib/Widget.pm"));
    advance_until_semicolon(&mut unknown)?;
    let unknown_identity = unknown
        .front_heredoc_identity()
        .ok_or_else(|| missing("missing unknown-generation identity"))?;
    require(
        !unknown_identity.source().is_publication_ready(),
        "Unknown generation must not become current merely because tokenization succeeded",
    )?;
    require(unknown_identity.source().generation().is_unknown(), "generation stays Unknown")?;
    Ok(())
}

#[test]
fn malformed_shift_string_comment_and_truncated_candidates_are_not_queued() -> R {
    let cases = [
        "my $x = 1 << 2;",
        "print \"<<END\";",
        "print '<<END';",
        "# <<END\nprint 1;",
        "print <<;",
        "print <<~;",
        "print <<\"EOF;",
        "print <<",
    ];
    for source in cases {
        let mut lexer = PerlLexer::new(source);
        let mut tokens = Vec::new();
        while let Some(token) = lexer.next_token() {
            let eof = matches!(token.token_type, TokenType::EOF);
            tokens.push(token);
            if eof {
                break;
            }
        }
        require(
            lexer.pending_heredoc_identities().is_empty(),
            format!("{source:?} must not admit a complete queue entry"),
        )?;
        require(
            lexer.front_heredoc_identity().is_none(),
            format!("{source:?} must not expose a front-entry identity"),
        )?;
        require(
            tokens.iter().all(|token| !matches!(token.token_type, TokenType::HeredocStart)),
            format!("{source:?} must not emit HeredocStart"),
        )?;
    }
    Ok(())
}

#[test]
fn restore_preserves_queued_identity_including_ordinal() -> R {
    let source = "print <<A, <<'B';\n";
    let mut lexer = PerlLexer::new(source);
    advance_until_semicolon(&mut lexer)?;
    let before = lexer.pending_heredoc_identities();
    require_eq(&before.len(), &2, "two queued identities")?;
    let checkpoint = lexer.checkpoint();
    require_eq(&checkpoint.pending_heredocs().len(), &2, "checkpoint queue")?;
    require_eq(&checkpoint.pending_heredocs()[0].identity.ordinal(), &0, "checkpoint ordinal")?;
    require_eq(
        &checkpoint.pending_heredocs()[0].identity.form(),
        &HeredocOpenerForm::Bare,
        "checkpoint form",
    )?;
    require_eq(
        &checkpoint.pending_heredocs()[1].identity.form(),
        &HeredocOpenerForm::SingleQuoted,
        "checkpoint second form",
    )?;
    let mut restored = PerlLexer::new(source);
    restored.restore(&checkpoint).map_err(|error| error.to_string())?;
    let after = restored.pending_heredoc_identities();
    require_eq(&after, &before, "restore must preserve complete identity")?;
    Ok(())
}

#[test]
fn token_stream_does_not_carry_queue_identity() -> R {
    let source = "print <<EOF;\nbody\nEOF\n";
    let tokens: Vec<Token> = {
        let mut lexer = PerlLexer::with_body_tokens(source);
        let mut tokens = Vec::new();
        while let Some(token) = lexer.next_token() {
            let eof = matches!(token.token_type, TokenType::EOF);
            tokens.push(token);
            if eof {
                break;
            }
        }
        tokens
    };
    require(
        tokens.iter().any(|token| matches!(token.token_type, TokenType::HeredocStart)),
        "opener token is still emitted",
    )?;
    require(
        !tokens.iter().any(|token| format!("{:?}", token.token_type).contains("QueueIdentity")),
        "Token must not grow a half identity API",
    )
}
