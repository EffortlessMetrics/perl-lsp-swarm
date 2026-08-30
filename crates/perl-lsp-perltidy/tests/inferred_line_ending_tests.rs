//! Property proof for the shared generated-line-ending rule (#13792).
//!
//! `inferred_line_ending` was previously duplicated byte-for-byte in
//! `perl-lsp-perltidy`'s native formatter and `perl-lsp-rs-core`'s LSP
//! whitespace projection. These properties pin the rule itself so those two
//! call sites cannot drift, and so a future re-derivation (for example the
//! #10239 convergence with `source_convention`) has to argue with a stated
//! contract rather than with a second copy of the same code.
//!
//! They do not speak for `next_edit::insertion_line_ending`, which still
//! decides the same question by a different rule — see the owner module's
//! documentation.

use perl_lsp_perltidy::native::inferred_line_ending;
use proptest::prelude::*;

/// Independent statement of the rule, formulated over string splits rather
/// than the implementation's byte-index arithmetic.
///
/// The document's convention is decided by its final LF: CRLF when that LF is
/// immediately preceded by a CR, LF otherwise, and LF when the document has no
/// LF at all. Expressed this way there is no `last_lf - 1` to underflow, so a
/// missing bounds guard in the implementation shows up as a disagreement
/// rather than as a shared blind spot.
fn oracle(source: &str) -> &'static str {
    match source.rsplit_once('\n') {
        Some((before_last_lf, _)) if before_last_lf.ends_with('\r') => "\r\n",
        Some(_) => "\n",
        None => "\n",
    }
}

/// Sources built from the alphabet that actually discriminates this rule:
/// terminator bytes, adjacent to ordinary and multi-byte content.
fn discriminating_source() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("\n".to_string()),
            Just("\r".to_string()),
            Just("\r\n".to_string()),
            Just("a".to_string()),
            Just(" ".to_string()),
            Just("\u{e9}".to_string()),
            Just("\u{1f600}".to_string()),
        ],
        0..12,
    )
    .prop_map(|parts| parts.concat())
}

/// Arbitrary text interleaved with explicit line terminators.
///
/// `(?s).{0,4}` supplies unrestricted content (including CR and LF); the
/// literal terminators guarantee the CRLF-vs-LF branch is actually exercised
/// rather than left to chance.
fn terminator_rich_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            2 => "(?s).{0,4}",
            1 => Just("\n".to_string()),
            1 => Just("\r\n".to_string()),
            1 => Just("\r".to_string()),
        ],
        0..10,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #[test]
    fn matches_the_independently_stated_rule(source in discriminating_source()) {
        prop_assert_eq!(inferred_line_ending(&source), oracle(&source), "source: {:?}", source);
    }

    /// Unrestricted content, but with terminators injected often enough to
    /// reach the decision.
    ///
    /// A plain `".*"` cannot discriminate here for two compounding reasons:
    /// proptest compiles the pattern with `regex-syntax` defaults, where `.`
    /// matches CR but *not* LF, so every sample takes the no-LF fallback; and
    /// even with `(?s)` set, randomly drawing a CR immediately followed by an
    /// LF at the deciding position is rare enough that the CRLF branch is
    /// effectively unreachable. Either way an implementation returning `"\n"`
    /// unconditionally would pass. Interleaving arbitrary chunks with explicit
    /// terminators keeps the content unrestricted while making both branches
    /// live — this property fails against that mutation, `discriminating_source`
    /// is not carrying it alone.
    #[test]
    fn matches_the_independently_stated_rule_on_terminator_rich_text(
        source in terminator_rich_text(),
    ) {
        prop_assert_eq!(inferred_line_ending(&source), oracle(&source), "source: {:?}", source);
    }

    /// The result is always a terminator this crate is willing to generate.
    /// Bare CR is a supported *source* sequence (see `source_convention`) but
    /// is never synthesized here.
    ///
    /// This is a tautology over the current two return literals rather than a
    /// discriminating check; it is kept to pin the contract against a future
    /// edit that widens the return set.
    #[test]
    fn only_ever_generates_lf_or_crlf(source in discriminating_source()) {
        let ending = inferred_line_ending(&source);
        prop_assert!(matches!(ending, "\n" | "\r\n"), "generated {:?}", ending);
    }

    /// Appending the inferred ending is idempotent with respect to the rule:
    /// a document terminated with its own convention still reports that
    /// convention, so a final-newline insert never flips the document's kind.
    ///
    /// The precondition is exactly the state both production call sites
    /// establish before they append — see
    /// `a_surviving_bare_cr_tail_would_absorb_an_appended_lf` for why a bare
    /// CR tail is excluded rather than asserted.
    #[test]
    fn appending_the_inferred_ending_preserves_the_convention(source in discriminating_source()) {
        prop_assume!(!source.ends_with('\r'));
        let ending = inferred_line_ending(&source);
        let appended = format!("{source}{ending}");
        prop_assert_eq!(inferred_line_ending(&appended), ending, "source: {:?}", source);
    }

    /// A CR that is not adjacent to the deciding LF must not make the document
    /// CRLF. This is the case a naive "contains \r" implementation gets wrong;
    /// the separating body is non-empty so the CR really is detached.
    #[test]
    fn a_detached_cr_does_not_imply_crlf(body in "[a-z]{1,8}") {
        prop_assert_eq!(inferred_line_ending(&format!("{body}\r{body}\n")), "\n");
    }
}

#[test]
fn short_and_empty_buffers_fall_back_to_lf() {
    for source in ["", "\r", "a", "\r\r", "abc"] {
        assert_eq!(inferred_line_ending(source), "\n", "source: {source:?}");
    }
}

#[test]
fn a_leading_lone_lf_does_not_underflow_the_crlf_lookback() {
    assert_eq!(inferred_line_ending("\n"), "\n");
    assert_eq!(inferred_line_ending("\na"), "\n");
}

/// A source whose final byte is a bare CR infers LF, so concatenating the two
/// would yield a CRLF the source never used.
///
/// This is a property of the helper in isolation, not a production defect, and
/// it is pinned here so the boundary stays visible instead of hiding inside an
/// over-strong idempotence claim. Neither call site can reach it:
/// `NativeFormatter::apply_final_newline` trims the trailing `\r`/`\n` run
/// before appending, and the LSP whitespace projection appends only when the
/// tail is not already terminated — where a bare CR counts as terminated.
///
/// If #10239 converges this rule with `source_convention` (which does treat
/// bare CR as a first-class sequence), this is the case that has to be decided
/// deliberately rather than inherited.
#[test]
fn a_surviving_bare_cr_tail_would_absorb_an_appended_lf() {
    assert_eq!(inferred_line_ending("my $x = 1;\r"), "\n");
    assert_eq!(inferred_line_ending("my $x = 1;\r\n"), "\r\n");
}
