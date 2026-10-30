//! `--port` token validation shared by launcher prevalidate and clap.
//!
//! Names the accepted TCP range (0-65535) instead of forwarding
//! `ParseIntError`'s Display into user-facing output (#16526, #16562), and
//! renders a rejected value so that invisible padding is visible (#16561).

use super::LaunchParseError;
use super::offending_value::{
    HAS_SURROUNDING_WHITESPACE_REASON, has_surrounding_whitespace, render_offending_value,
};

/// Rejection reason for a `--port` token that is a number outside 0-65535.
const PORT_OUT_OF_RANGE: &str = "Expected a port in 0-65535.";

/// Rejection reason for a `--port` token that is not an unsigned whole number.
const PORT_NOT_A_NUMBER: &str = "Expected a whole number in 0-65535.";

/// A `--port` token padded with surrounding whitespace is rejected, not
/// trimmed: silently accepting a padded value would make the CLI accept a
/// spelling the documented grammar does not allow, and trimming before the
/// shape test would classify a padded valid port as out of range (#16561). The
/// reason text is shared with the `--diagnostic-debounce-ms` sibling.
const PORT_HAS_SURROUNDING_WHITESPACE: &str = HAS_SURROUNDING_WHITESPACE_REASON;

/// Parse a `u16` port token, or reject it with an actionable range reason.
///
/// Classification is by token shape rather than a wider-integer re-parse: an
/// ASCII-digit token (optionally signed) that `u16` refused is out of range,
/// and anything else is not a number. An arbitrarily long digit run therefore
/// stays on the range side. Negative zero (`-0`, `-000`) is a spelling of 0
/// that `u16` rejects because of the minus, so it is reported as not a number
/// rather than as out of range. Surrounding whitespace is its own class,
/// checked before either of those (#16561).
///
/// This is the sole grammar/range authority for `--port`. Launcher
/// prevalidation and the shared clap `value_parser` both delegate here so
/// `perllsp` and `perl-dap` cannot accept or reject different token classes.
pub(super) fn parse_port_token(raw_port: &str) -> Result<u16, LaunchParseError> {
    match raw_port.parse::<u16>() {
        Ok(port) => Ok(port),
        Err(_) => Err(LaunchParseError::InvalidPort {
            raw_port: raw_port.to_string(),
            reason: port_rejection_reason(raw_port).to_string(),
        }),
    }
}

/// Accept a `u16` port token, or reject it with an actionable range reason.
pub(super) fn validate_port_token(raw_port: &str) -> Result<(), LaunchParseError> {
    parse_port_token(raw_port).map(|_| ())
}

/// Render the complete user-facing rejection for a rejected `--port` token.
///
/// Lives beside the reason constants so the option's whole vocabulary — the
/// option name, the echoed value, the accepted range, and the whitespace rule —
/// is written in one place rather than split between the classifier and
/// `Display`.
pub(super) fn render_port_rejection(raw_port: &str, reason: &str) -> String {
    format!("Invalid port value: {}. {reason}", render_offending_value(raw_port))
}

fn port_rejection_reason(raw_port: &str) -> &'static str {
    // Whitespace is checked first: a padded token is rejected for the padding
    // whether or not its digits would also have been out of range, and naming
    // the range for `" 65535 "` would tell the user nothing about the real
    // cause (#16561).
    if has_surrounding_whitespace(raw_port) {
        PORT_HAS_SURROUNDING_WHITESPACE
    } else if is_numeric_out_of_range(raw_port) {
        PORT_OUT_OF_RANGE
    } else {
        PORT_NOT_A_NUMBER
    }
}

fn is_numeric_out_of_range(raw_port: &str) -> bool {
    let digits = raw_port.strip_prefix(['+', '-']).unwrap_or(raw_port);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }

    let negative_zero = raw_port.starts_with('-') && digits.bytes().all(|byte| byte == b'0');
    !negative_zero
}

#[cfg(test)]
mod tests {
    use super::{
        PORT_HAS_SURROUNDING_WHITESPACE, PORT_NOT_A_NUMBER, PORT_OUT_OF_RANGE,
        port_rejection_reason, validate_port_token,
    };
    use crate::runtime::launcher::{LaunchParseError, LspArgs, TransportMode, parse_args};
    use clap::Parser;
    use perl_tdd_support::{must, must_err};

    /// The behavior #16561 fixes, asserted through the public `parse_args`
    /// path for both spellings.
    ///
    /// Before the fix this rendered `Invalid port value:  65535. Expected a
    /// whole number in 0-65535.` — the echoed token was codepoints
    /// `32 54 53 53 51 53`, so the leading space sat invisibly against the
    /// punctuation and the sentence read as though a perfectly valid port had
    /// been misrecognised. Two things had to become true: the padding has to be
    /// visible, and the reason has to stop being false.
    #[test]
    fn whitespace_padded_port_names_the_whitespace_instead_of_guessing_the_number() {
        struct Case {
            argv: &'static [&'static str],
            expected: &'static str,
        }

        let cases = [
            Case {
                argv: &["perl-lsp", "--port", " 65535", "--health"],
                expected: "Invalid port value: \" 65535\". Remove the leading or trailing whitespace.",
            },
            Case {
                argv: &["perl-lsp", "--port= 65535"],
                expected: "Invalid port value: \" 65535\". Remove the leading or trailing whitespace.",
            },
            Case {
                argv: &["perl-lsp", "--port", "65535 "],
                expected: "Invalid port value: \"65535 \". Remove the leading or trailing whitespace.",
            },
            Case {
                argv: &["perl-lsp", "--port", " 0"],
                expected: "Invalid port value: \" 0\". Remove the leading or trailing whitespace.",
            },
        ];

        for case in cases {
            let error = must_err(parse_args(case.argv.iter().copied()));
            let rendered = error.to_string();
            assert_eq!(rendered, case.expected, "argv={:?}", case.argv);

            // The original defect: a valid port wrapped in spaces must never be
            // reported as a malformed number, because that is not what is wrong.
            assert!(
                !rendered.contains(PORT_NOT_A_NUMBER),
                "padded in-range value was called a malformed number for {:?}: {rendered}",
                case.argv
            );
            assert!(
                !rendered.contains(PORT_OUT_OF_RANGE),
                "padded in-range value was called out of range for {:?}: {rendered}",
                case.argv
            );
        }
    }

    /// Trimming before the shape test is the tempting wrong fix and it is
    /// worse: a padded valid port would then be classified on its trimmed
    /// digits, and ` 65536` would be told it was a valid port was expected. So
    /// the classifier must name the padding, and the padding must win over the
    /// range verdict.
    #[test]
    fn padding_is_named_before_the_range_verdict() {
        // Padded, in-range, and out-of-range all get the same reason, because
        // the padding is the first thing the user has to fix.
        for token in [" 65535", "65535 ", " 65536", " 99999", " 0", " -1", "  abc"] {
            assert_eq!(
                port_rejection_reason(token),
                PORT_HAS_SURROUNDING_WHITESPACE,
                "token={token:?}"
            );
        }

        // A padded token whose trimmed digits are entirely numeric must NOT be
        // sent down the out-of-range path, which would report a valid port as
        // invalid while still rejecting it.
        assert_ne!(port_rejection_reason(" 65535"), PORT_OUT_OF_RANGE);
    }

    /// The rendering is the other half of the claim: the value must be quoted
    /// so the whitespace is visible, without the quoting leaking into tokens
    /// that are already unambiguous.
    #[test]
    fn padded_port_is_quoted_while_ordinary_rejections_are_not() {
        let padded = must_err(parse_args(["perl-lsp", "--port", " 65535"])).to_string();
        assert!(padded.contains("\" 65535\""), "the padded value must be delimited: {padded}");

        // Tokens with no surrounding whitespace keep the pre-existing
        // unquoted rendering, so this change is not a blanket reformatting of
        // every port message.
        let plain = must_err(parse_args(["perl-lsp", "--port", "abc"])).to_string();
        assert_eq!(
            plain,
            format!("Invalid port value: abc. {PORT_NOT_A_NUMBER}"),
            "an unambiguous token must still render verbatim"
        );
    }

    /// The accepted set is unchanged. Whitespace is rejected, not trimmed:
    /// silently accepting a padded spelling would make the CLI accept tokens
    /// the documented grammar does not allow, and would make `perllsp` and
    /// `perl-dap` disagree about which spellings are ports.
    #[test]
    fn padded_ports_stay_rejected_rather_than_being_trimmed() {
        for token in [" 0", "0 ", " 65535", "65535 ", " 65535 ", "\t65535", "\u{a0}65535"] {
            assert!(
                validate_port_token(token).is_err(),
                "padded token {token:?} must stay rejected, not be trimmed"
            );
            // The accepted set remains exactly what `u16::from_str` accepts.
            assert_eq!(
                validate_port_token(token).is_ok(),
                token.parse::<u16>().is_ok(),
                "the accepted set must still be exactly what u16 accepts, for {token:?}"
            );
        }
    }

    /// The shared clap parser must reach the same verdict as prevalidation.
    /// `LspArgs::try_parse_from` skips `prevalidate_cli_values`, so a clap-only
    /// path is a realistic place for the two to drift (#16562).
    #[test]
    fn shared_clap_parser_gives_padded_ports_the_same_reason() {
        for argv in [
            ["perl-lsp", "--port", " 65535"].as_slice(),
            ["perl-lsp", "--port= 65535"].as_slice(),
            ["perl-lsp", "--port", "65535 "].as_slice(),
        ] {
            let error = must_err(LspArgs::try_parse_from(argv.iter().copied()));
            let rendered = error.to_string();
            assert!(
                rendered.contains(PORT_HAS_SURROUNDING_WHITESPACE),
                "argv={argv:?} missing the whitespace reason in {rendered}"
            );
            assert!(
                !rendered.contains(PORT_NOT_A_NUMBER),
                "clap must not call a padded in-range value a malformed number: {argv:?}: {rendered}"
            );
        }
    }

    /// The two option rejections must not drift: they share one rendering
    /// owner, so a value that is padded for `--port` is padded for
    /// `--diagnostic-debounce-ms` too.
    #[test]
    fn port_and_debounce_rejections_render_padding_the_same_way() {
        let port = must_err(parse_args(["perl-lsp", "--port", " 65535"])).to_string();
        let debounce =
            must_err(parse_args(["perl-lsp", "--diagnostic-debounce-ms", " 250"])).to_string();

        for rendered in [&port, &debounce] {
            assert!(rendered.contains("\" "), "a padded value must render delimited: {rendered}");
            assert!(
                !rendered.contains(":. "),
                "an undelimited value leaves the padding invisible: {rendered}"
            );
        }

        // The reason must be the same text too, not just a delimited value:
        // a user who makes the same mistake on either option should be told
        // the same thing. Both option constants alias the single owner in
        // `offending_value`, so comparing the two rendered sentences is what
        // pins the shared wording to what a user actually sees.
        let port_reason = port_rejection_reason(" 65535");
        let debounce_reason = super::super::debounce::debounce_rejection_reason_for_test(" 250");
        assert_eq!(port_reason, debounce_reason, "the two options must share one reason text");
        assert!(port.ends_with(port_reason), "the port message must carry that reason: {port}");
        assert!(
            debounce.ends_with(&debounce_reason),
            "the debounce message must carry that reason: {debounce}"
        );
    }

    /// A rejected port must still name the accepted range on the paths that
    /// are genuinely about the number, so the whitespace fix cannot quietly
    /// cost the range information the landed work added.
    #[test]
    fn range_and_number_rejections_still_name_the_accepted_range() {
        assert_eq!(port_rejection_reason("65536"), PORT_OUT_OF_RANGE);
        assert_eq!(port_rejection_reason("abc"), PORT_NOT_A_NUMBER);
        for token in [PORT_OUT_OF_RANGE, PORT_NOT_A_NUMBER] {
            assert!(token.contains("0-65535"), "reason must name the range: {token}");
        }
    }

    #[test]
    fn classifier_separates_range_failures_from_non_numeric_tokens() {
        assert_eq!(port_rejection_reason("65536"), PORT_OUT_OF_RANGE);
        assert_eq!(port_rejection_reason("99999"), PORT_OUT_OF_RANGE);
        assert_eq!(port_rejection_reason("-1"), PORT_OUT_OF_RANGE);
        assert_eq!(port_rejection_reason("+65536"), PORT_OUT_OF_RANGE);
        assert_eq!(port_rejection_reason("99999999999999999999999999"), PORT_OUT_OF_RANGE);

        assert_eq!(port_rejection_reason("abc"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("0x10"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("8080.0"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("1e2"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("80_80"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("+"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("-"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("-0"), PORT_NOT_A_NUMBER);
        assert_eq!(port_rejection_reason("-000"), PORT_NOT_A_NUMBER);

        // Padded tokens are their own class, not a range or shape failure: the
        // digits may be perfectly good, and the padding is what to fix.
        assert_eq!(port_rejection_reason(" 65535"), PORT_HAS_SURROUNDING_WHITESPACE);
        assert_eq!(port_rejection_reason("65535 "), PORT_HAS_SURROUNDING_WHITESPACE);
        assert_eq!(port_rejection_reason(" 65536"), PORT_HAS_SURROUNDING_WHITESPACE);

        // Interior whitespace is not padding — it is a malformed token, and
        // it must keep saying so.
        assert_eq!(port_rejection_reason("6 5535"), PORT_NOT_A_NUMBER);
    }

    /// `--port` rejections must name the accepted range instead of forwarding
    /// `ParseIntError`'s Display (`number too large to fit in target type`,
    /// `invalid digit found in string`). This is the first diagnostic a new
    /// user hits in socket mode (#16526).
    #[test]
    fn invalid_port_states_the_accepted_range_instead_of_parse_int_error() {
        struct Case {
            argv: &'static [&'static str],
            expected: &'static str,
        }

        let cases = [
            Case {
                argv: &["perl-lsp", "--port", "99999", "--health"],
                expected: "Invalid port value: 99999. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port=99999"],
                expected: "Invalid port value: 99999. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "65536"],
                expected: "Invalid port value: 65536. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port=65536"],
                expected: "Invalid port value: 65536. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "-1"],
                expected: "Invalid port value: -1. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "+65536"],
                expected: "Invalid port value: +65536. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "99999999999999999999999999"],
                expected: "Invalid port value: 99999999999999999999999999. Expected a port in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "abc", "--health"],
                expected: "Invalid port value: abc. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port=abc"],
                expected: "Invalid port value: abc. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "0x10"],
                expected: "Invalid port value: 0x10. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "8080.0"],
                expected: "Invalid port value: 8080.0. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "1e2"],
                expected: "Invalid port value: 1e2. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "80_80"],
                expected: "Invalid port value: 80_80. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "+"],
                expected: "Invalid port value: +. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "-"],
                expected: "Invalid port value: -. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port", "-0"],
                expected: "Invalid port value: -0. Expected a whole number in 0-65535.",
            },
            Case {
                argv: &["perl-lsp", "--port=-000"],
                expected: "Invalid port value: -000. Expected a whole number in 0-65535.",
            },
        ];

        for case in cases {
            let error = must_err(parse_args(case.argv.iter().copied()));
            let rendered = error.to_string();
            assert_eq!(rendered, case.expected, "argv={:?}", case.argv);
            assert!(
                !rendered.contains("fit in target type"),
                "leaked ParseIntError wording for {:?}: {rendered}",
                case.argv
            );
            assert!(
                !rendered.contains("invalid digit found in string"),
                "leaked ParseIntError wording for {:?}: {rendered}",
                case.argv
            );
        }
    }

    /// Range endpoints, signed in-range tokens, and leading zeros stay accepted
    /// so tightening the diagnostic must not narrow the accepted set.
    #[test]
    fn boundary_and_in_range_ports_are_still_accepted() {
        let cases: &[(&[&str], u16)] = &[
            (&["perl-lsp", "--port", "0"], 0),
            (&["perl-lsp", "--port", "65535"], 65535),
            (&["perl-lsp", "--port=0"], 0),
            (&["perl-lsp", "--port=65535"], 65535),
            (&["perl-lsp", "--port", "+0"], 0),
            (&["perl-lsp", "--port", "+65535"], 65535),
            (&["perl-lsp", "--port", "0000"], 0),
            (&["perl-lsp", "--port", "08080"], 8080),
        ];

        for (argv, port) in cases {
            let plan = must(parse_args(argv.iter().copied()));
            assert_eq!(
                plan.config.transport,
                TransportMode::Socket { port: *port },
                "argv={argv:?}"
            );
        }
    }

    /// Negative control: a missing value is still reported as missing, not
    /// reclassified as an invalid port.
    #[test]
    fn missing_port_value_is_still_reported_as_missing() {
        let cases: &[&[&str]] = &[
            &["perl-lsp", "--port"],
            &["perl-lsp", "--port="],
            &["perl-lsp", "--port", "--stdio"],
        ];

        for argv in cases {
            let error = must_err(parse_args(argv.iter().copied()));
            assert!(
                matches!(error, LaunchParseError::MissingValue { .. }),
                "expected MissingValue for {argv:?}, got {error:?}"
            );
        }
    }

    #[test]
    fn accepted_tokens_do_not_produce_invalid_port() {
        for raw in ["0", "65535", "+0", "+65535", "0000", "08080"] {
            assert!(validate_port_token(raw).is_ok(), "should accept {raw}");
            assert_eq!(
                must(super::parse_port_token(raw)),
                must(raw.parse::<u16>()),
                "accepted token {raw} must yield the same u16 clap will store"
            );
        }
    }

    /// The shared `TransportArgs.port` clap parser must consume the same
    /// authority as launcher prevalidation. `LspArgs::try_parse_from` skips
    /// `prevalidate_cli_values`, so a clap-only `u16` parse is a realistic
    /// wrong implementation (#16562).
    #[test]
    fn shared_clap_parser_states_the_accepted_range_instead_of_parse_int_error() {
        struct Case {
            argv: &'static [&'static str],
            reason: &'static str,
        }

        let cases = [
            Case { argv: &["perl-lsp", "--port", "65536"], reason: PORT_OUT_OF_RANGE },
            Case { argv: &["perl-lsp", "--port=65536"], reason: PORT_OUT_OF_RANGE },
            Case { argv: &["perl-lsp", "--port", "99999"], reason: PORT_OUT_OF_RANGE },
            Case {
                argv: &["perl-lsp", "--port", "99999999999999999999999999"],
                reason: PORT_OUT_OF_RANGE,
            },
            Case { argv: &["perl-lsp", "--port", "-1"], reason: PORT_OUT_OF_RANGE },
            Case { argv: &["perl-lsp", "--port=-1"], reason: PORT_OUT_OF_RANGE },
            Case { argv: &["perl-lsp", "--port", "abc"], reason: PORT_NOT_A_NUMBER },
            Case { argv: &["perl-lsp", "--port=abc"], reason: PORT_NOT_A_NUMBER },
            Case { argv: &["perl-lsp", "--port", "0x10"], reason: PORT_NOT_A_NUMBER },
        ];

        for case in cases {
            let error = must_err(LspArgs::try_parse_from(case.argv.iter().copied()));
            let rendered = error.to_string();
            assert!(
                rendered.contains(case.reason),
                "argv={:?} missing reason {reason:?} in {rendered}",
                case.argv,
                reason = case.reason
            );
            assert!(
                !rendered.contains("fit in target type"),
                "leaked ParseIntError wording for {:?}: {rendered}",
                case.argv
            );
            assert!(
                !rendered.contains("invalid digit found in string"),
                "leaked ParseIntError wording for {:?}: {rendered}",
                case.argv
            );
            assert!(
                !rendered.contains("cannot parse integer from empty string"),
                "leaked ParseIntError wording for {:?}: {rendered}",
                case.argv
            );
            assert!(
                !rendered.contains("0..="),
                "leaked Rust range syntax for {:?}: {rendered}",
                case.argv
            );
        }
    }

    #[test]
    fn shared_clap_parser_and_prevalidate_agree_on_token_acceptance() {
        let accepted = ["0", "65535", "+0", "+65535", "0000", "08080", "1"];
        for raw in accepted {
            assert!(validate_port_token(raw).is_ok(), "canonical validator must accept {raw}");
            let parsed = must(LspArgs::try_parse_from(["perl-lsp", "--port", raw]));
            let expected = must(raw.parse::<u16>());
            assert_eq!(parsed.transport.port, Some(expected), "clap accepted {raw}");
        }

        let rejected = ["65536", "99999", "abc", "-1", "+65536", "0x10", "8080.0"];
        for raw in rejected {
            assert!(validate_port_token(raw).is_err(), "canonical validator must reject {raw}");
            assert!(
                LspArgs::try_parse_from(["perl-lsp", "--port", raw]).is_err(),
                "shared clap parser must reject {raw}"
            );
        }
    }
}
