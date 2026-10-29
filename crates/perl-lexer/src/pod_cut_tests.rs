//! POD `=cut` directive handling.
//!
//! A line-initial `=cut` outside POD opens a POD block that runs to the NEXT `=cut`,
//! or to EOF when there is none. Perl accepts such a file, so the lexer must treat
//! the block as POD rather than as code.
//!
//! The oracle for every case here is `perl -c` (perl 5.42). Each case records the
//! oracle's verdict, so a test and the behaviour it claims stay checkable against
//! real perl rather than against the lexer's own opinion.

use super::*;
use perl_tdd_support::must_with;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// Significant (non-whitespace) token texts.
fn significant(source: &str) -> Vec<String> {
    PerlLexer::new(source)
        .collect_tokens()
        .into_iter()
        .filter(|token| {
            !matches!(
                &token.token_type,
                TokenType::Whitespace | TokenType::Newline | TokenType::EOF
            )
        })
        .map(|token| token.text.to_string())
        .collect()
}

/// Error-token messages produced for `source`.
fn errors(source: &str) -> Vec<String> {
    PerlLexer::new(source)
        .collect_tokens()
        .into_iter()
        .filter_map(|token| match &token.token_type {
            TokenType::Error(message) => Some(message.to_string()),
            _ => None,
        })
        .collect()
}

/// Fail when `condition` does not hold, reporting `detail`.
fn require(condition: bool, detail: String) {
    must_with(
        if condition { Ok(()) } else { Err(detail) },
        "the lexer must agree with the perl oracle",
    );
}

/// A stray `=cut` at statement start opens POD that runs to EOF.
///
/// `perl -c` accepts this file. Before the fix the `=` and `cut` were lexed as code
/// and the POD body produced error tokens, so the lexer disagreed with perl.
#[test]
fn stray_cut_at_statement_start_opens_pod_to_end_of_file() -> TestResult {
    let source = "use strict;\n=cut\nthis is not valid perl @@@ ###\n";
    let tokens = significant(source);
    let errors = errors(source);

    require(
        errors.is_empty(),
        format!("perl accepts this file, so the lexer must not report {errors:?}"),
    );
    require(
        !tokens.iter().any(|text| text == "=" || text == "cut"),
        format!("the stray =cut must not be lexed as code, got {tokens:?}"),
    );
    require(
        tokens.iter().any(|text| text == "use"),
        format!("the code before the stray =cut must still be lexed, got {tokens:?}"),
    );
    Ok(())
}

/// The `=cut` that opens a block does not close it: the block runs to the NEXT `=cut`.
///
/// `perl -c` accepts this file and treats the text between the two `=cut` lines as
/// POD, so only the code after the second `=cut` may be lexed.
#[test]
fn stray_cut_opens_pod_until_the_next_cut() -> TestResult {
    let source = "use strict;\n=cut\nnot code at all @@@ ###\n=cut\nmy $x = 1;\n";
    let tokens = significant(source);
    let errors = errors(source);

    require(
        errors.is_empty(),
        format!("perl accepts this file, so the lexer must not report {errors:?}"),
    );
    require(
        !tokens.iter().any(|text| text == "cut"),
        format!("both =cut directives are POD, so neither may be lexed as code: {tokens:?}"),
    );
    require(
        !tokens.iter().any(|text| ["not", "code", "at", "all"].contains(&text.as_str())),
        format!("the POD body must not be lexed as code, got {tokens:?}"),
    );
    require(
        tokens.iter().any(|text| text == "my"),
        format!("the code after the closing =cut must still be lexed, got {tokens:?}"),
    );
    Ok(())
}

/// A stray `=cut` after a blank line opens POD.
///
/// `perl -c` accepts this file: a blank line is statement-start context, so the
/// `=cut` is a directive rather than an assignment continuation.
#[test]
fn stray_cut_after_a_blank_line_opens_pod() -> TestResult {
    let source = "use strict;\n\n=cut\nthis is not valid perl @@@ ###\n";
    let tokens = significant(source);
    let errors = errors(source);

    require(
        errors.is_empty(),
        format!("perl accepts this file, so the lexer must not report {errors:?}"),
    );
    require(
        !tokens.iter().any(|text| text == "=" || text == "cut"),
        format!("the stray =cut must not be lexed as code, got {tokens:?}"),
    );
    Ok(())
}

/// A stray `=cut` carrying a trailing `;` still opens POD.
///
/// `perl -c` accepts this file: `=cut;` is the `cut` directive with trailing text.
#[test]
fn stray_cut_with_a_semicolon_opens_pod() -> TestResult {
    let source = "use strict;\n=cut;\nthis is not valid perl @@@ ###\n";
    let tokens = significant(source);
    let errors = errors(source);

    require(
        errors.is_empty(),
        format!("perl accepts this file, so the lexer must not report {errors:?}"),
    );
    require(
        !tokens.iter().any(|text| text == "cut"),
        format!("the stray =cut must not be lexed as code, got {tokens:?}"),
    );
    Ok(())
}

/// A `=cut` that continues an expression stays code.
///
/// `perl -c` rejects the file: after a term, `=cut` is an assignment continuation
/// and not a directive. This guards the statement-start requirement, so opening POD
/// on `=cut` must not swallow an expression.
#[test]
fn cut_after_a_term_stays_code() -> TestResult {
    let source = "my $x = 1\n=cut\n";
    let tokens = significant(source);

    require(
        tokens.iter().any(|text| text == "="),
        format!("a =cut that continues an expression must stay code, got {tokens:?}"),
    );
    Ok(())
}

/// A `=cut` inside a regex literal does not open POD.
///
/// The text after the `=cut` must still be lexed as the regex body and as code, so
/// the stray `=cut` did not open a POD block.
#[test]
fn cut_inside_a_regex_does_not_open_pod() -> TestResult {
    let source = "my $r = /\n=cut\n/;\nprint \"ok\";\n";
    let tokens = significant(source);
    let errors = errors(source);

    require(
        errors.is_empty(),
        format!("the regex must still lex, so the lexer must not report {errors:?}"),
    );
    require(
        tokens.iter().any(|text| text == "print"),
        format!("code after the regex must still be lexed, got {tokens:?}"),
    );
    Ok(())
}

/// `=cutlery` is a different POD command and does not close a block.
///
/// `perl -c` accepts this file, so the POD opened by `=head1` still covers the text
/// after `=cutlery`. A prefix match on `=cut` would end the block early and turn POD
/// text into code.
#[test]
fn cutlery_does_not_terminate_a_pod_block() -> TestResult {
    let source = "=head1 NAME\ndoc\n=cutlery\nthis is not valid perl @@@ ###\n";
    let tokens = significant(source);
    let errors = errors(source);

    require(
        errors.is_empty(),
        format!("perl accepts this file, so the lexer must not report {errors:?}"),
    );
    require(
        tokens.is_empty(),
        format!("the whole file is POD, so no tokens may be produced, got {tokens:?}"),
    );
    Ok(())
}

/// `=cut;` closes a POD block.
///
/// `perl -c` rejects the file, so the block ends at `=cut;` and the text after it is
/// code. This guards the directive-word boundary, which must not require the `=cut`
/// to be alone on its line.
#[test]
fn cut_with_a_semicolon_terminates_a_pod_block() -> TestResult {
    let source = "=head1 NAME\ndoc\n=cut;\nthis is not valid perl @@@ ###\n";
    let tokens = significant(source);

    require(
        !tokens.iter().any(|text| text == "cut"),
        format!("=cut; closes the block, so it must not be lexed as code, got {tokens:?}"),
    );
    require(
        tokens.iter().any(|text| text == "valid"),
        format!("the text after =cut; must be lexed as code, got {tokens:?}"),
    );
    Ok(())
}
