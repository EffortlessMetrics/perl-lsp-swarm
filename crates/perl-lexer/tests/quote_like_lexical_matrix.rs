//! Versioned quote-like lexical matrix and public-observation harness (#7274).
//!
//! This suite owns the accepted denominator for `q/qq/qw/qx/qr/m/s/tr/y`:
//! stable case IDs, a pinned Perl compile/parse profile, exact token
//! kind/text/range, next ordinary token, terminal lexer state class, and dual
//! observation through public `PerlLexer` and parser `TokenStream`.
//!
//! It does not change production tokenization. Whitespace-separated `s` is
//! included where current-main already admits it. `tr`/`y` comment-gap rows
//! record current-main Error over-consumption against a Perl compile-accept
//! oracle; production repair remains #7279.

mod quote_like_matrix;

use quote_like_matrix::{
    Axis, ExpectedKind, NextOrdinary, OperatorFamily, OracleResult, PERL_PROFILE, SCHEMA_VERSION,
    all_rows, observe_and_assert, probe_identity, validate, without_operator,
};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

#[test]
fn matrix_is_complete_and_uniquely_identified() -> R {
    validate(&all_rows()).map_err(missing)?;
    Ok(())
}

#[test]
fn every_matrix_row_matches_lexer_token_stream_and_oracle() -> R {
    let rows = all_rows();
    validate(&rows).map_err(missing)?;
    let mut failures = Vec::new();
    for row in &rows {
        if let Err(error) = observe_and_assert(row) {
            failures.push(error);
        }
    }
    if !failures.is_empty() {
        return Err(missing(failures.join("\n")));
    }
    Ok(())
}

#[test]
fn removing_all_tr_or_y_rows_fails_completeness_while_s_rows_remain() -> R {
    let rows = all_rows();
    assert!(rows.iter().any(|row| row.operator == OperatorFamily::S));
    let without_tr = without_operator(&rows, OperatorFamily::Tr);
    let without_y = without_operator(&rows, OperatorFamily::Y);
    let tr_error = match validate(&without_tr) {
        Err(error) => error,
        Ok(()) => return Err(missing("tr rows must be independently required")),
    };
    let y_error = match validate(&without_y) {
        Err(error) => error,
        Ok(()) => return Err(missing("y rows must be independently required")),
    };
    assert!(tr_error.contains("tr"), "{tr_error}");
    assert!(y_error.contains("y"), "{y_error}");
    Ok(())
}

#[test]
fn immediate_hash_delimiter_is_not_the_whitespace_comment_hash_row() -> R {
    let rows = all_rows();
    let delimiter = rows
        .iter()
        .find(|row| row.id == "q.immediate_hash.delimiter")
        .ok_or_else(|| missing("missing q.immediate_hash.delimiter"))?;
    let comment = rows
        .iter()
        .find(|row| row.id == "q.whitespace_hash.comment_not_delimiter")
        .ok_or_else(|| missing("missing q.whitespace_hash.comment_not_delimiter"))?;
    assert!(delimiter.expected.iter().any(|token| token.kind == ExpectedKind::QuoteSingle));
    assert!(comment.expected.iter().all(|token| token.kind != ExpectedKind::QuoteSingle));
    assert_ne!(delimiter.source, comment.source);
    assert!(matches!(comment.next_ordinary, NextOrdinary::EatenByComment));
    Ok(())
}

#[test]
fn mixed_second_delimiters_are_independent_for_s_tr_and_y() -> R {
    let rows = all_rows();
    for operator in [OperatorFamily::S, OperatorFamily::Tr, OperatorFamily::Y] {
        let mixed = rows
            .iter()
            .filter(|row| {
                row.operator == operator && row.axes.contains(&Axis::MixedSecondDelimiter)
            })
            .count();
        assert!(
            mixed >= 1,
            "{} must own mixed-second-delimiter rows, got {mixed}",
            operator.as_str()
        );
    }
    Ok(())
}

#[test]
fn malformed_rows_name_following_code_and_assert_full_extent() -> R {
    let rows = all_rows();
    let malformed =
        rows.iter().filter(|row| row.axes.contains(&Axis::MalformedFollower)).collect::<Vec<_>>();
    assert!(malformed.len() >= OperatorFamily::ALL.len());
    for row in malformed {
        assert!(
            row.source.contains("after"),
            "{} must name following code so over-consumption is observable",
            row.id
        );
        assert!(row.expected.len() >= 2, "{} must not be first-token-only", row.id);
        assert!(
            matches!(row.next_ordinary, NextOrdinary::EatenByError | NextOrdinary::Present { .. }),
            "{} must declare next-ordinary or eaten-by-error",
            row.id
        );
    }
    Ok(())
}

#[test]
fn changing_perl_profile_without_schema_transition_is_rejected() -> R {
    let mut rows = all_rows();
    rows[0].perl_profile = "perl5-newest-silent";
    let error = match validate(&rows) {
        Err(error) => error,
        Ok(()) => return Err(missing("profile drift must fail")),
    };
    assert!(error.contains(SCHEMA_VERSION) || error.contains("profile"), "{error}");
    assert!(!error.contains("silently"));
    let _ = PERL_PROFILE;
    Ok(())
}

#[test]
fn first_token_only_rows_fail_completeness() -> R {
    let mut rows = all_rows();
    rows[0].expected = Box::leak(vec![rows[0].expected[0]].into_boxed_slice());
    match validate(&rows) {
        Err(error) => {
            assert!(error.contains("first-token-only") || error.contains("EOF"), "{error}");
            Ok(())
        }
        Ok(()) => Err(missing("first-token-only row must fail completeness")),
    }
}

#[test]
fn oracle_identity_is_recorded_or_explicitly_not_proven() -> R {
    match probe_identity() {
        OracleResult::Proven { identity, .. } => {
            assert!(identity.executable.exists());
            assert!(identity.version.starts_with("5."));
            assert!(identity.invocation.contains("perl -c"));
        }
        OracleResult::NotProven { reason } => {
            assert!(!reason.is_empty(), "NOT_PROVEN must name the instrument failure");
        }
    }
    Ok(())
}
