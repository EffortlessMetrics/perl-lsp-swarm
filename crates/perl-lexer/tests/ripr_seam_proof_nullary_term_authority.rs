//! RIPR seam-closure proof for the nullary `<<` term authority (#16165).
//!
//! Every test is a call-observation test: it drives the public `perl_lexer`
//! token stream and observes whether `<<` after a candidate nullary authority
//! lexes as the left-shift operator (`Operator("<<")`) or as a heredoc
//! introducer (`HeredocStart`). The observation point is the first token whose
//! text starts with `<<`, so a mutation at any targeted seam flips the observed
//! reading. Each source's reading was verified against a local `perl -c`
//! oracle (v5.42) before pinning.
//!
//! Seams targeted (lines introduced by #16165):
//!
//! | File | Line | Expression |
//! |------|------|-----------|
//! | `src/lexer/helpers/word_classification.rs` | 38 | `NULLARY_BUILTINS.contains(&word)` |
//! | `src/lib.rs` | 916 | `while let Some(ch) = before[..start].chars().next_back()` |
//! | `src/lib.rs` | 923 | sigil rejection `matches!(ch, '$' \| '@' \| '%' \| '&' \| '*')` |

use perl_lexer::{PerlLexer, TokenType};

/// The observed reading of the first `<<` token in the stream.
enum Reading {
    /// Left-shift operator: the preceding word counted as nullary authority.
    Shift,
    /// Heredoc introducer: the preceding word kept callable status.
    Heredoc,
}

fn observe(source: &str) -> Result<Reading, String> {
    let tokens = PerlLexer::new(source).collect_tokens();
    for token in &tokens {
        if !token.text.starts_with("<<") {
            continue;
        }
        return match &token.token_type {
            TokenType::HeredocStart => Ok(Reading::Heredoc),
            TokenType::Operator(op) if op.as_ref() == "<<" => Ok(Reading::Shift),
            _ => Err(format!(
                "first `<<`-prefixed token in {source:?} is neither the shift operator nor a \
                 heredoc introducer"
            )),
        };
    }
    Err(format!("no `<<` token was produced for {source:?}"))
}

fn expect_shift(source: &str) -> Result<(), String> {
    match observe(source)? {
        Reading::Shift => Ok(()),
        Reading::Heredoc => Err(format!(
            "{source:?}: `<<` consumed a heredoc; nullary authority did not complete the term"
        )),
    }
}

fn expect_heredoc(source: &str) -> Result<(), String> {
    match observe(source)? {
        Reading::Heredoc => Ok(()),
        Reading::Shift => Err(format!(
            "{source:?}: `<<` shifted; the preceding word wrongly counted as nullary authority"
        )),
    }
}

// ---------------------------------------------------------------------------
// `word_classification.rs:38` — `NULLARY_BUILTINS.binary_search(&word).is_ok()`
// ---------------------------------------------------------------------------

/// Bare `time` is in the bounded nullary list, so `<<` is left shift (oracle:
/// `print time <<END` shifts; the nullary call completes the term). Mutating
/// the membership seam to always-miss turns this into a heredoc.
#[test]
fn seam_38_nullary_builtin_member_completes_the_term() -> Result<(), String> {
    expect_shift("print time <<'END';")
}

/// Membership is exact, not a prefix match: a `time`-prefixed bareword call
/// keeps callable status, so `<<` stays a heredoc (oracle: `print time2 <<END`
/// is a heredoc). A `starts_with` mutation at the seam shifts instead.
#[test]
fn seam_38_membership_is_exact_not_a_prefix_match() -> Result<(), String> {
    expect_heredoc("print time2 <<'END';\nEND\n")
}

/// Membership is case-sensitive: `Time` is a plain bareword call, not the
/// builtin (oracle: `print Time <<END` is a heredoc).
#[test]
fn seam_38_membership_is_case_sensitive() -> Result<(), String> {
    expect_heredoc("print Time <<'END';\nEND\n")
}

// ---------------------------------------------------------------------------
// `src/lib.rs:916` — bareword back-scan class (`is_alphanumeric | _ | : | '`)
// ---------------------------------------------------------------------------

/// The `:` class member keeps a qualified name whole: `Foo::time` misses the
/// exact builtin membership, so `<<` stays a heredoc (oracle: `print
/// Foo::time <<END` is a heredoc). Dropping `:` from the back-scan would
/// capture plain `time` and wrongly shift.
#[test]
fn seam_916_qualified_name_survives_the_back_scan() -> Result<(), String> {
    expect_heredoc("print Foo::time <<'END';\nEND\n")
}

/// The loop body decrements by `ch.len_utf8()`, so a multibyte identifier
/// before `<<` back-scans without panicking and keeps callable status (oracle:
/// `print café <<END` under `use utf8` is a heredoc).
#[test]
fn seam_916_multibyte_identifier_back_scans_without_panicking() -> Result<(), String> {
    expect_heredoc("print café <<'END';\nEND\n")
}

/// The loop's exit boundary: a bareword at the very start of the input empties
/// `before[..start]`; `next_back()` yields `None` and the sigil check must
/// read no sigil. Statement-level `time` keeps its nullary shift reading
/// (oracle: `time <<END` shifts).
#[test]
fn seam_916_back_scan_exits_at_the_input_prefix_boundary() -> Result<(), String> {
    expect_shift("time <<'END';")
}

// ---------------------------------------------------------------------------
// `src/lib.rs:923` — sigil rejection (`$ | @ | % | & | *`)
// ---------------------------------------------------------------------------

/// A sigil voids nullary authority: `$time` is a variable, not the builtin, so
/// `<<` keeps its heredoc reading (oracle: `print $time <<END` is a heredoc —
/// perl reads `$time` as the indirect-object filehandle and the marker as a
/// heredoc). Deleting the sigil check would read `time` and wrongly shift.
#[test]
fn seam_923_sigil_voids_nullary_authority() -> Result<(), String> {
    expect_heredoc("print $time <<'END';\nEND\n")
}

/// The documented `print $fh <<END` contract: a variable filehandle never
/// counts as a bareword call, so its `<<` stays a heredoc (oracle: heredoc).
#[test]
fn seam_923_variable_filehandle_keeps_the_heredoc_contract() -> Result<(), String> {
    expect_heredoc("print $fh <<'END';\nEND\n")
}
