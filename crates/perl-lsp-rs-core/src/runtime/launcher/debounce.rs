//! `--diagnostic-debounce-ms` token validation for the launcher prevalidate path.
//!
//! Owns the option's token grammar, accepted range, and rejection wording so
//! that clap's `ParseIntError` text never becomes user-visible authority
//! (#16806).
//!
//! Classification mirrors the landed `--port` sibling (`port.rs`, #16526): a
//! token that `u64::from_str` refuses is either an out-of-range number or not a
//! number at all, decided by shape rather than by a wider re-parse.

use super::LaunchParseError;
use super::offending_value::{
    HAS_SURROUNDING_WHITESPACE_REASON, has_surrounding_whitespace, render_offending_value,
};

/// Upper bound of the accepted range, spelled out for the user-facing message.
///
/// The rejection reasons are built from this constant rather than repeating the
/// digits, so the message and the parsed type cannot drift apart;
/// `accepted_upper_bound_matches_u64_max` pins the constant to the type.
const DEBOUNCE_MAX_MS_TEXT: &str = "18446744073709551615";

/// Rejection reason for a token padded with surrounding whitespace.
///
/// Same disposition as the `--port` sibling, and for the same reason: the
/// padding is rejected rather than trimmed, and it is named before the range
/// so that a padded in-range value is not reported as a malformed number
/// (#16561). The text is owned by `offending_value` so the two options cannot
/// drift apart.
const DEBOUNCE_HAS_SURROUNDING_WHITESPACE: &str = HAS_SURROUNDING_WHITESPACE_REASON;

/// Rejection reason for a token that is a number outside `0..=u64::MAX`.
fn debounce_out_of_range_reason() -> String {
    format!(
        "That is outside the accepted 0-{DEBOUNCE_MAX_MS_TEXT} millisecond range, where 0 publishes diagnostics immediately."
    )
}

/// Rejection reason for a token that is not an unsigned whole number.
fn debounce_not_a_number_reason() -> String {
    format!(
        "Expected a whole number of milliseconds in 0-{DEBOUNCE_MAX_MS_TEXT}, where 0 publishes diagnostics immediately."
    )
}

/// Accept a `--diagnostic-debounce-ms` token, or reject it with a semantic reason.
///
/// Returning `Ok` proves the token parses as `u64`, which is what makes the
/// later clap parse unable to disagree: `prevalidate_cli_values` runs first, so
/// by the time clap sees the token it is already known-acceptable and its own
/// `value_parser` can only fail on a token this function would have rejected
/// first. The accepted set therefore has exactly one owner.
pub(super) fn validate_debounce_token(raw_value: &str) -> Result<(), LaunchParseError> {
    if raw_value.parse::<u64>().is_ok() {
        return Ok(());
    }

    Err(LaunchParseError::InvalidDiagnosticDebounceMs {
        raw_value: raw_value.to_string(),
        reason: debounce_rejection_reason(raw_value),
    })
}

/// Render the complete user-facing rejection for a rejected token.
///
/// Lives beside the reason constants so the option's whole vocabulary — the
/// option name, the echoed value, the accepted range, and the meaning of `0` —
/// is written in one place rather than split between the classifier and
/// `Display`.
pub(super) fn render_debounce_rejection(raw_value: &str, reason: &str) -> String {
    format!(
        "Invalid --diagnostic-debounce-ms value: {}. {reason}",
        render_offending_value(raw_value)
    )
}

fn debounce_rejection_reason(raw_value: &str) -> String {
    if has_surrounding_whitespace(raw_value) {
        DEBOUNCE_HAS_SURROUNDING_WHITESPACE.to_string()
    } else if is_numeric_out_of_range(raw_value) {
        debounce_out_of_range_reason()
    } else {
        debounce_not_a_number_reason()
    }
}

fn is_numeric_out_of_range(raw_value: &str) -> bool {
    let digits = raw_value.strip_prefix(['+', '-']).unwrap_or(raw_value);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }

    // Negative zero (`-0`, `-000`) is a spelling of 0 that `u64` refuses
    // because of the minus, so it is not a number here rather than out of
    // range. Same disposition as the `--port` sibling.
    !(raw_value.starts_with('-') && digits.bytes().all(|byte| byte == b'0'))
}

#[cfg(test)]
pub(super) fn debounce_rejection_reason_for_test(raw_value: &str) -> String {
    debounce_rejection_reason(raw_value)
}

#[cfg(test)]
mod tests {
    use super::{
        DEBOUNCE_HAS_SURROUNDING_WHITESPACE, DEBOUNCE_MAX_MS_TEXT, debounce_not_a_number_reason,
        debounce_out_of_range_reason, debounce_rejection_reason, render_debounce_rejection,
        validate_debounce_token,
    };
    use crate::runtime::launcher::{LaunchParseError, parse_args};
    use perl_tdd_support::{must, must_err};

    #[test]
    fn accepted_upper_bound_matches_u64_max() {
        assert_eq!(DEBOUNCE_MAX_MS_TEXT, u64::MAX.to_string());
    }

    #[test]
    fn classifier_separates_range_failures_from_non_numeric_tokens() {
        let out_of_range = debounce_out_of_range_reason();
        let not_a_number = debounce_not_a_number_reason();
        assert_ne!(out_of_range, not_a_number, "the two reasons must stay distinguishable");

        for token in
            ["18446744073709551616", "99999999999999999999999999", "-1", "+18446744073709551616"]
        {
            assert_eq!(debounce_rejection_reason(token), out_of_range, "token={token}");
        }

        for token in ["abc", "1e2", "0x10", "250ms", "80_80", "+", "-", "-0", "-000"] {
            assert_eq!(debounce_rejection_reason(token), not_a_number, "token={token}");
        }
    }

    /// The reason for a rejected token must never be blank even when the token
    /// is, because a blank reason would render `Invalid ...: .` with no advice.
    #[test]
    fn empty_token_still_names_the_accepted_range() {
        let rendered = render_debounce_rejection("", &debounce_rejection_reason(""));
        assert!(rendered.contains("0-18446744073709551615"), "{rendered}");
        assert!(!rendered.ends_with(". "), "trailing blank reason: {rendered}");
    }

    /// A value carrying control characters must not be echoed verbatim, or the
    /// rejected value could forge extra rendered lines. Asserted through the
    /// real render path, so this stays pinned to what a user actually sees
    /// rather than to the helper the renderer happens to call.
    #[test]
    fn unprintable_values_are_not_echoed_verbatim() {
        let unrenderable = super::super::offending_value::UNRENDERABLE_TOKEN;

        for raw in ["line\nInjected: fake", "tab\there", "\u{7}bell"] {
            let rendered = render_debounce_rejection(raw, &debounce_rejection_reason(raw));
            assert!(rendered.contains(unrenderable), "must stand in for {raw:?}: {rendered}");
            assert!(!rendered.contains(raw), "control characters must not survive into {rendered}");
        }
    }

    /// A padded value is rejected for the padding, and the padding is visible
    /// in the rendered rejection. Before #16561 the message classified `" 250"`
    /// as a malformed number, which is false — it is a whole number of
    /// milliseconds, wrapped in spaces.
    #[test]
    fn padded_values_are_rejected_for_the_padding_and_show_it() {
        for raw in [" 250", "250 ", " 250 "] {
            assert_eq!(debounce_rejection_reason(raw), DEBOUNCE_HAS_SURROUNDING_WHITESPACE);
        }

        let rendered = render_debounce_rejection(" 250", &debounce_rejection_reason(" 250"));
        assert!(
            rendered.starts_with("Invalid --diagnostic-debounce-ms value: \" 250\"."),
            "the padding must be visible: {rendered}"
        );
        assert!(
            !rendered.contains("whole number of milliseconds"),
            "a padded in-range value must not be called a malformed number: {rendered}"
        );
    }

    /// The accepted set is unchanged: a padded value is still a rejection, not
    /// a silently trimmed success. Widening the accepted set here would make
    /// the CLI accept a spelling the documented grammar does not allow.
    #[test]
    fn padded_values_are_still_rejected_rather_than_trimmed() {
        for token in [" 0", "0 ", " 250", "250 ", " 18446744073709551615"] {
            assert!(
                validate_debounce_token(token).is_err(),
                "padded token {token:?} must stay rejected, not be trimmed"
            );
            assert_eq!(
                validate_debounce_token(token).is_ok(),
                token.parse::<u64>().is_ok(),
                "the accepted set must still be exactly what u64 accepts, for {token:?}"
            );
        }
    }

    /// Change-detector only: the prevalidate accept set is exactly the set
    /// `u64::from_str` accepts, because the validator is defined as that call.
    /// This cannot fail for any token today — it exists to make an edit to
    /// either side visible in review. The *real* oracle for "clap and the
    /// prevalidator cannot drift" is
    /// `values_above_narrower_integer_types_reach_tuning_unparsed`, which
    /// exercises the public path and would fail if the clap field were ever
    /// narrowed away from a full-range `u64`.
    #[test]
    fn prevalidate_accepts_exactly_what_u64_accepts() {
        let tokens = [
            "0",
            "1",
            "250",
            "+0",
            "+5",
            "007",
            "18446744073709551615",
            "-0",
            "-1",
            "+1",
            "abc",
            "1e2",
            "0x10",
            "250ms",
            "",
            " ",
            "18446744073709551616",
            "99999999999999999999999",
            "1.0",
            "++5",
            "--5",
        ];

        for token in tokens {
            assert_eq!(
                validate_debounce_token(token).is_ok(),
                token.parse::<u64>().is_ok(),
                "prevalidate and u64 disagree for {token:?}"
            );
        }
    }

    #[test]
    fn accepted_boundary_tokens_do_not_produce_an_error() {
        for token in ["0", "1", "250", "+0", "18446744073709551615"] {
            assert!(validate_debounce_token(token).is_ok(), "should accept {token}");
        }
    }

    /// The oracle that actually holds the single-owner claim together.
    ///
    /// `validate_debounce_token` returning `Ok` only proves the token is a
    /// full-range `u64`; it does not prove the value clap finally hands to
    /// `RuntimeTuning` came out of a full-range parse. If the clap field were
    /// ever narrowed — `Option<u32>`, say — prevalidation would keep accepting
    /// `5000000000` while clap rejected it with its own wording routed back
    /// through `ParserDiagnostic`: the exact leak this issue forbids, and the
    /// exact state the message constant would still claim was fine.
    ///
    /// Asserting through the public `parse_args` path that values above every
    /// narrower integer type arrive intact is what fails under that narrowing.
    #[test]
    fn values_above_narrower_integer_types_reach_tuning_unparsed() {
        let cases: &[(&[&str], u64)] = &[
            // Above u32::MAX and above i64::MAX: only a full-range u64 parse
            // yields these, so a narrowed field would reject them here.
            (["perl-lsp", "--diagnostic-debounce-ms", "5000000000"].as_slice(), 5_000_000_000),
            (["perl-lsp", "--diagnostic-debounce-ms", "18446744073709551615"].as_slice(), u64::MAX),
        ];

        for (argv, expected) in cases {
            let plan = must(parse_args(argv.iter().copied()));
            assert_eq!(
                plan.config.runtime_tuning.diagnostic_debounce_ms, *expected,
                "argv={argv:?}"
            );
        }
    }

    /// `--diagnostic-debounce-ms` rejections must name the option, the
    /// offending value, the accepted range, and what `0` means, for both
    /// spellings — instead of forwarding clap's `ParseIntError` wording
    /// (#16806).
    #[test]
    fn invalid_debounce_states_the_range_instead_of_parse_int_error() {
        struct Case {
            argv: &'static [&'static str],
            expected: &'static str,
        }

        let cases = [
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms", "abc"],
                expected: "Invalid --diagnostic-debounce-ms value: abc. Expected a whole number of milliseconds in 0-18446744073709551615, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms=abc"],
                expected: "Invalid --diagnostic-debounce-ms value: abc. Expected a whole number of milliseconds in 0-18446744073709551615, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms", "18446744073709551616"],
                expected: "Invalid --diagnostic-debounce-ms value: 18446744073709551616. That is outside the accepted 0-18446744073709551615 millisecond range, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms=18446744073709551616"],
                expected: "Invalid --diagnostic-debounce-ms value: 18446744073709551616. That is outside the accepted 0-18446744073709551615 millisecond range, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms=-1"],
                expected: "Invalid --diagnostic-debounce-ms value: -1. That is outside the accepted 0-18446744073709551615 millisecond range, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms=+18446744073709551616"],
                expected: "Invalid --diagnostic-debounce-ms value: +18446744073709551616. That is outside the accepted 0-18446744073709551615 millisecond range, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms", "99999999999999999999999999"],
                expected: "Invalid --diagnostic-debounce-ms value: 99999999999999999999999999. That is outside the accepted 0-18446744073709551615 millisecond range, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms", "1e2"],
                expected: "Invalid --diagnostic-debounce-ms value: 1e2. Expected a whole number of milliseconds in 0-18446744073709551615, where 0 publishes diagnostics immediately.",
            },
            Case {
                argv: &["perl-lsp", "--diagnostic-debounce-ms", "0x10"],
                expected: "Invalid --diagnostic-debounce-ms value: 0x10. Expected a whole number of milliseconds in 0-18446744073709551615, where 0 publishes diagnostics immediately.",
            },
        ];

        for case in cases {
            let error = must_err(parse_args(case.argv.iter().copied()));
            let rendered = error.to_string();
            assert_eq!(rendered, case.expected, "argv={:?}", case.argv);
            for leak in [
                "invalid digit found in string",
                "number too large to fit in target type",
                "cannot parse integer from empty string",
            ] {
                assert!(
                    !rendered.contains(leak),
                    "leaked clap/ParseIntError wording for {:?}: {rendered}",
                    case.argv
                );
            }
        }
    }

    /// `0` and the upper bound keep their existing meaning and acceptance, and
    /// the parsed value still reaches the runtime tuning that consumes it.
    #[test]
    fn accepted_values_reach_runtime_tuning_unchanged() {
        let cases: &[(&[&str], u64)] = &[
            (&["perl-lsp", "--diagnostic-debounce-ms", "0"], 0),
            (&["perl-lsp", "--diagnostic-debounce-ms=0"], 0),
            (&["perl-lsp", "--diagnostic-debounce-ms", "1"], 1),
            (&["perl-lsp", "--diagnostic-debounce-ms", "250"], 250),
            (&["perl-lsp", "--diagnostic-debounce-ms=250"], 250),
            (&["perl-lsp", "--diagnostic-debounce-ms", "18446744073709551615"], u64::MAX),
            (&["perl-lsp", "--diagnostic-debounce-ms=18446744073709551615"], u64::MAX),
        ];

        for (argv, expected) in cases {
            let plan = must(parse_args(argv.iter().copied()));
            assert_eq!(
                plan.config.runtime_tuning.diagnostic_debounce_ms, *expected,
                "argv={argv:?}"
            );
        }
    }

    /// `0` is the immediate/no-debounce value the help text advertises; proving
    /// it here keeps the message and the runtime meaning from drifting.
    #[test]
    fn zero_still_means_immediate_publication() {
        let plan = must(parse_args(["perl-lsp", "--diagnostic-debounce-ms", "0"]));
        assert!(plan.config.runtime_tuning.diagnostic_debounce_is_immediate());

        let plan = must(parse_args(["perl-lsp", "--diagnostic-debounce-ms", "1"]));
        assert!(!plan.config.runtime_tuning.diagnostic_debounce_is_immediate());
    }

    /// Issue test item 9: the help and usage surfaces must describe the accepted
    /// option consistently with what the parser now enforces. `0 = immediate` in
    /// help is the same fact the rejection message states as "0 publishes
    /// diagnostics immediately", and the runtime agrees with both.
    #[test]
    fn help_text_and_rejection_agree_on_what_zero_means() {
        let help = super::super::help_text();
        assert!(
            help.contains("--diagnostic-debounce-ms"),
            "help must still advertise the option: {help}"
        );
        assert!(
            help.contains("0 = immediate"),
            "help must keep stating that 0 is immediate: {help}"
        );

        let rejected = render_debounce_rejection("abc", &debounce_not_a_number_reason());
        assert!(
            rejected.contains("0 publishes diagnostics immediately"),
            "rejection must keep stating what 0 means: {rejected}"
        );

        let immediate = must(parse_args(["perl-lsp", "--diagnostic-debounce-ms", "0"]));
        assert!(immediate.config.runtime_tuning.diagnostic_debounce_is_immediate());
    }

    /// A missing value is reported as missing, not reclassified as an invalid
    /// number — including when the next token is another option.
    #[test]
    fn missing_value_is_still_reported_as_missing() {
        for argv in [
            ["perl-lsp", "--diagnostic-debounce-ms"].as_slice(),
            ["perl-lsp", "--diagnostic-debounce-ms="].as_slice(),
            ["perl-lsp", "--diagnostic-debounce-ms", "--health"].as_slice(),
            ["perl-lsp", "--health", "--diagnostic-debounce-ms"].as_slice(),
        ] {
            let error = must_err(parse_args(argv.iter().copied()));
            assert!(
                matches!(&error, LaunchParseError::MissingValue { option } if option == "--diagnostic-debounce-ms"),
                "expected MissingValue for {argv:?}, got {error:?}"
            );
        }
    }

    /// A hyphen-leading token in the split spelling is a *flag*, not a value —
    /// clap refuses to read one as this option's value. Prevalidation must not
    /// claim it as a value and reject it as a bad number, because that turns a
    /// working invocation into a failure: `perllsp --diagnostic-debounce-ms -h`
    /// used to print help.
    ///
    /// This pins the accept-set repair from review: `-1` and `-h` are missing
    /// values in the split spelling, while `--diagnostic-debounce-ms=-1` (which
    /// does reach clap's value parser) still earns the out-of-range reason.
    #[test]
    fn hyphen_leading_split_values_are_missing_not_bad_numbers() {
        for argv in [
            ["perl-lsp", "--diagnostic-debounce-ms", "-1"].as_slice(),
            ["perl-lsp", "--diagnostic-debounce-ms", "-h"].as_slice(),
            ["perl-lsp", "--diagnostic-debounce-ms", "--health"].as_slice(),
        ] {
            let error = must_err(parse_args(argv.iter().copied()));
            assert!(
                matches!(&error, LaunchParseError::MissingValue { option } if option == "--diagnostic-debounce-ms"),
                "expected MissingValue for {argv:?}, got {error:?}"
            );
        }

        // The equals spelling still names the range, because there clap really
        // does receive the token as a value.
        let error = must_err(parse_args(["perl-lsp", "--diagnostic-debounce-ms=-1"]));
        assert!(
            error.to_string().contains("That is outside the accepted"),
            "expected the out-of-range reason, got: {error}"
        );
    }
}
