//! `--port` token validation for the launcher prevalidate path.
//!
//! Names the accepted TCP range (0-65535) instead of forwarding
//! `ParseIntError`'s Display into user-facing output (#16526).

use super::LaunchParseError;

/// Rejection reason for a `--port` token that is a number outside 0-65535.
const PORT_OUT_OF_RANGE: &str = "Expected a port in 0-65535.";

/// Rejection reason for a `--port` token that is not an unsigned whole number.
const PORT_NOT_A_NUMBER: &str = "Expected a whole number in 0-65535.";

/// Accept a `u16` port token, or reject it with an actionable range reason.
///
/// Classification is by token shape rather than a wider-integer re-parse: an
/// ASCII-digit token (optionally signed) that `u16` refused is out of range,
/// and anything else is not a number. An arbitrarily long digit run therefore
/// stays on the range side. Negative zero (`-0`, `-000`) is a spelling of 0
/// that `u16` rejects because of the minus, so it is reported as not a number
/// rather than as out of range.
pub(super) fn validate_port_token(raw_port: &str) -> Result<(), LaunchParseError> {
    if raw_port.parse::<u16>().is_ok() {
        return Ok(());
    }

    Err(LaunchParseError::InvalidPort {
        raw_port: raw_port.to_string(),
        reason: port_rejection_reason(raw_port).to_string(),
    })
}

fn port_rejection_reason(raw_port: &str) -> &'static str {
    if is_numeric_out_of_range(raw_port) { PORT_OUT_OF_RANGE } else { PORT_NOT_A_NUMBER }
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
    use super::{PORT_NOT_A_NUMBER, PORT_OUT_OF_RANGE, port_rejection_reason, validate_port_token};
    use crate::runtime::launcher::{LaunchParseError, TransportMode, parse_args};
    use perl_tdd_support::{must, must_err};

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
        }
    }
}
