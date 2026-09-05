//! Queue-order regressions for duplicate and colliding heredoc labels.
//!
//! These cases complement `heredoc_queue_contract.rs` by proving behavior that
//! label text alone cannot identify: duplicate declarations remain distinct,
//! only the front entry may terminate, and an empty first body cannot collapse
//! the following queue entry.

use perl_lexer::{PerlLexer, Token, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn require(condition: bool, message: impl Into<String>) -> R {
    if condition { Ok(()) } else { Err(missing(message)) }
}

fn require_eq<T>(actual: &T, expected: &T, context: impl Into<String>) -> R
where
    T: std::fmt::Debug + PartialEq + ?Sized,
{
    if actual == expected {
        Ok(())
    } else {
        Err(missing(format!("{}: expected {expected:?}, got {actual:?}", context.into())))
    }
}

fn body_tokens(tokens: &[Token]) -> Vec<&Token> {
    // Interpolating openers (`<<EOF`, `<<"EOF"`, `<<~END`) emit
    // `InterpolatedHeredocBody` segments; non-interpolating controls and
    // opaque backtick bodies stay `HeredocBody`. Queue collision behavior is
    // proven identically for both representations (#8779).
    tokens
        .iter()
        .filter(|token| {
            matches!(
                &token.token_type,
                TokenType::HeredocBody(_) | TokenType::InterpolatedHeredocBody(_)
            )
        })
        .collect()
}

fn assert_clean_continuation(source: &str, tokens: &[Token], marker: &str) -> R {
    let marker_start =
        source.find(marker).ok_or_else(|| missing("missing continuation marker in fixture"))?;
    require(
        tokens.iter().any(|token| {
            token.start == marker_start
                && token.text.as_ref() == "my"
                && matches!(&token.token_type, TokenType::Keyword(keyword) if keyword.as_ref() == "my")
        }),
        "source after the final heredoc terminator was not tokenized as Perl code",
    )?;
    require(
        tokens.iter().all(|token| !token.token_type.is_recovery_token()),
        "clean queued heredoc fixture emitted a recovery token",
    )?;
    require_eq(
        &tokens.iter().filter(|token| matches!(&token.token_type, TokenType::EOF)).count(),
        &1,
        "terminal EOF count",
    )?;
    let terminal = tokens.last().ok_or_else(|| missing("token stream is empty"))?;
    require(matches!(&terminal.token_type, TokenType::EOF), "last token is not EOF")?;
    require_eq(&(terminal.start, terminal.end), &(source.len(), source.len()), "EOF geometry")
}

fn assert_queue_case(
    source: &str,
    expected_openers: &[&str],
    expected_bodies: &[&str],
    expected_terminators: &[&str],
    continuation_marker: &str,
) -> R {
    require_eq(
        &expected_bodies.len(),
        &expected_terminators.len(),
        "fixture body/terminator count",
    )?;

    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();
    let openers = tokens
        .iter()
        .filter(|token| matches!(&token.token_type, TokenType::HeredocStart))
        .map(|token| token.text.as_ref())
        .collect::<Vec<_>>();
    require_eq(openers.as_slice(), expected_openers, "queued opener order")?;

    let bodies = body_tokens(&tokens);
    require_eq(&bodies.len(), &expected_bodies.len(), "queued body count")?;
    let mut body_slices = Vec::with_capacity(bodies.len());
    for body in &bodies {
        require(
            source.is_char_boundary(body.start) && source.is_char_boundary(body.end),
            "heredoc body range is not on UTF-8 boundaries",
        )?;
        let body_slice = source
            .get(body.start..body.end)
            .ok_or_else(|| missing("heredoc body token has invalid source geometry"))?;
        body_slices.push(body_slice);
        require(body.text.is_empty(), "heredoc body token must remain opaque")?;
    }
    require_eq(body_slices.as_slice(), expected_bodies, "queued body order and source slices")?;

    let continuation_start = source
        .find(continuation_marker)
        .ok_or_else(|| missing("missing continuation marker in queue fixture"))?;
    for (index, body) in bodies.iter().enumerate() {
        let next_start = bodies.get(index + 1).map_or(continuation_start, |next| next.start);
        let terminator = source
            .get(body.end..next_start)
            .ok_or_else(|| missing("terminator gap has invalid source geometry"))?;
        let expected = expected_terminators
            .get(index)
            .ok_or_else(|| missing("fixture omitted an expected terminator"))?;
        require_eq(terminator, *expected, format!("terminator after queued body {index}"))?;
    }

    assert_clean_continuation(source, &tokens, continuation_marker)
}

#[test]
fn duplicate_labels_remain_distinct_fifo_entries() -> R {
    let source = concat!(
        "print <<END, <<END;\n",
        "first\n",
        "END\n",
        "second\n",
        "END\n",
        "my $after = 1;\n",
    );

    assert_queue_case(
        source,
        &["<<END", "<<END"],
        &["first\n", "second\n"],
        &["END\n", "END\n"],
        "my $after = 1;",
    )
}

#[test]
fn a_later_entry_label_cannot_terminate_the_front_entry() -> R {
    let source = concat!(
        "print <<END2, <<END;\n",
        "first\n",
        "END\n",
        "END2\n",
        "second\n",
        "END\n",
        "my $after = 2;\n",
    );

    assert_queue_case(
        source,
        &["<<END2", "<<END"],
        &["first\nEND\n", "second\n"],
        &["END2\n", "END\n"],
        "my $after = 2;",
    )
}

#[test]
fn an_empty_duplicate_body_does_not_collapse_the_next_entry() -> R {
    let source =
        concat!("print <<END, <<END;\n", "END\n", "second\n", "END\n", "my $after = 3;\n",);

    assert_queue_case(
        source,
        &["<<END", "<<END"],
        &["", "second\n"],
        &["END\n", "END\n"],
        "my $after = 3;",
    )
}

#[test]
fn duplicate_labels_across_mixed_forms_keep_declaration_order() -> R {
    let source = concat!(
        "print <<CMD, <<'CMD', <<`CMD`;\n",
        "plain\n",
        "CMD\n",
        "literal\n",
        "CMD\n",
        "command\n",
        "CMD\n",
        "my $after = 4;\n",
    );

    assert_queue_case(
        source,
        &["<<CMD", "<<'CMD'", "<<`CMD`"],
        &["plain\n", "literal\n", "command\n"],
        &["CMD\n", "CMD\n", "CMD\n"],
        "my $after = 4;",
    )
}

#[test]
fn duplicate_unicode_labels_keep_distinct_body_geometry() -> R {
    let source = concat!(
        "use utf8;\n",
        "print <<Δ, <<Δ;\n",
        "α\n",
        "Δ\n",
        "β\n",
        "Δ\n",
        "my $after = 5;\n",
    );

    assert_queue_case(source, &["<<Δ", "<<Δ"], &["α\n", "β\n"], &["Δ\n", "Δ\n"], "my $after = 5;")
}
