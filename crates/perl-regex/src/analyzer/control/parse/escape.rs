use super::{
    ParsedOperand, PatternBoundaryKind, PatternControlDiagnosticCode, PatternControlEffect,
    PatternControlKind, PatternReferenceSyntax, RawControl, RegexRange, ResolutionRequest,
    bounded_spelling, delimited_operand, invalid_reference, parse_operand, scan_ascii_digits,
    scan_signed_digits,
};

pub(super) fn parse_escape_control(pattern: &str, start: usize) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    if bytes.get(start) != Some(&b'\\') {
        return None;
    }
    match bytes.get(start + 1).copied()? {
        b'K' => Some(RawControl {
            kind: PatternControlKind::KeepAnchor,
            range: RegexRange { start, end: start + 2 },
            operand_range: None,
            request: ResolutionRequest::None,
            effect: PatternControlEffect::ReportedMatchStart,
            boundary: None,
            diagnostic: None,
        }),
        b'1'..=b'9' => {
            let end = scan_ascii_digits(bytes, start + 1);
            let operand_range = RegexRange { start: start + 1, end };
            let number =
                pattern.get(operand_range.start..operand_range.end)?.parse::<u32>().ok()?;
            Some(RawControl {
                kind: PatternControlKind::NumericBackreference {
                    number,
                    syntax: PatternReferenceSyntax::PlainNumeric,
                },
                range: RegexRange { start, end },
                operand_range: Some(operand_range),
                request: ResolutionRequest::Number {
                    number,
                    ambiguous_plain_escape: operand_range.len() > 1,
                },
                effect: PatternControlEffect::CaptureRead,
                boundary: None,
                diagnostic: None,
            })
        }
        b'g' => parse_g_reference(pattern, start),
        b'k' => parse_k_reference(pattern, start),
        _ => None,
    }
}

fn parse_g_reference(pattern: &str, start: usize) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    let operand_start = start + 2;
    let (operand_range, end) = match bytes.get(operand_start).copied() {
        Some(b'{') => match delimited_operand(bytes, operand_start, b'}') {
            Some(value) => value,
            None => return Some(invalid_reference(pattern, start, pattern.len())),
        },
        Some(b'\'') => {
            let end = delimited_operand(bytes, operand_start, b'\'')
                .map_or(pattern.len(), |(_, end)| end);
            return Some(invalid_reference(pattern, start, end));
        }
        Some(b'<') => {
            let end =
                delimited_operand(bytes, operand_start, b'>').map_or(pattern.len(), |(_, end)| end);
            return Some(invalid_reference(pattern, start, end));
        }
        Some(b'+' | b'-' | b'0'..=b'9') => {
            let end = scan_signed_digits(bytes, operand_start);
            (RegexRange { start: operand_start, end }, end)
        }
        _ => {
            return Some(invalid_reference(pattern, start, (start + 2).min(pattern.len())));
        }
    };
    raw_reference(pattern, start, end, operand_range, PatternReferenceSyntax::GReference, false)
}

fn parse_k_reference(pattern: &str, start: usize) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    let open = start + 2;
    let (operand_range, end) = match bytes.get(open).copied() {
        Some(b'<') => match delimited_operand(bytes, open, b'>') {
            Some(value) => value,
            None => return Some(invalid_reference(pattern, start, pattern.len())),
        },
        Some(b'{') => match delimited_operand(bytes, open, b'}') {
            Some(value) => value,
            None => return Some(invalid_reference(pattern, start, pattern.len())),
        },
        Some(b'\'') => match delimited_operand(bytes, open, b'\'') {
            Some(value) => value,
            None => return Some(invalid_reference(pattern, start, pattern.len())),
        },
        _ => return Some(invalid_reference(pattern, start, (start + 2).min(pattern.len()))),
    };
    raw_reference(pattern, start, end, operand_range, PatternReferenceSyntax::KReference, true)
}

fn raw_reference(
    pattern: &str,
    start: usize,
    end: usize,
    operand_range: RegexRange,
    syntax: PatternReferenceSyntax,
    named_only: bool,
) -> Option<RawControl> {
    let operand = parse_operand(pattern.get(operand_range.start..operand_range.end)?);
    let (kind, request, effect, diagnostic) = match operand {
        ParsedOperand::Number(number) if !named_only => (
            PatternControlKind::NumericBackreference { number, syntax },
            ResolutionRequest::Number { number, ambiguous_plain_escape: false },
            PatternControlEffect::CaptureRead,
            None,
        ),
        ParsedOperand::Relative(offset) if !named_only && offset < 0 => (
            PatternControlKind::RelativeBackreference { offset, syntax },
            ResolutionRequest::Relative(offset),
            PatternControlEffect::CaptureRead,
            None,
        ),
        ParsedOperand::Name(name) => (
            PatternControlKind::NamedBackreference { name: name.clone(), syntax },
            ResolutionRequest::Name(name),
            PatternControlEffect::CaptureRead,
            None,
        ),
        // An operand this reference spelling cannot carry is not a capture read, so the
        // effect has to agree with the unsupported kind rather than claim a read that
        // never resolves.
        _ => (
            PatternControlKind::Unsupported {
                spelling: bounded_spelling(pattern, RegexRange { start, end }),
            },
            ResolutionRequest::None,
            PatternControlEffect::Unsupported,
            Some(PatternControlDiagnosticCode::InvalidReference),
        ),
    };
    Some(RawControl {
        kind,
        range: RegexRange { start, end },
        operand_range: Some(operand_range),
        request,
        effect,
        boundary: diagnostic.map(|_| PatternBoundaryKind::UnsupportedControl),
        diagnostic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(pattern: &str) -> Option<PatternControlKind> {
        parse_escape_control(pattern, 0).map(|control| control.kind)
    }

    fn range_of(pattern: &str) -> Option<(usize, usize)> {
        parse_escape_control(pattern, 0).map(|control| (control.range.start, control.range.end))
    }

    #[test]
    fn non_escape_bytes_are_not_escape_controls() {
        // The dispatcher offers every byte; only a backslash may open one here.
        assert!(parse_escape_control("K", 0).is_none());
        assert!(parse_escape_control("(?<x>a)", 0).is_none());
        // A trailing lone backslash has no second byte to classify.
        assert!(parse_escape_control(r"\", 0).is_none());
    }

    #[test]
    fn keep_anchor_spans_exactly_the_two_escape_bytes() {
        assert_eq!(kind_of(r"\Kabc"), Some(PatternControlKind::KeepAnchor));
        assert_eq!(range_of(r"\Kabc"), Some((0, 2)));
    }

    #[test]
    fn traditional_numeric_backreference_carries_its_number() {
        assert!(matches!(
            kind_of(r"\1"),
            Some(PatternControlKind::NumericBackreference { number: 1, .. })
        ));
    }

    #[test]
    fn g_reference_accepts_braced_numeric_named_and_relative_operands() {
        assert!(matches!(
            kind_of(r"\g{2}"),
            Some(PatternControlKind::NumericBackreference { number: 2, .. })
        ));
        assert!(matches!(
            kind_of(r"\g{name}"),
            Some(PatternControlKind::NamedBackreference { .. })
        ));
        assert!(matches!(
            kind_of(r"\g{-1}"),
            Some(PatternControlKind::RelativeBackreference { .. })
        ));
        // The whole `\g{...}` run belongs to the fact, not just the escape pair.
        assert_eq!(range_of(r"\g{2}"), Some((0, 5)));
    }

    #[test]
    fn unterminated_g_operand_reports_invalid_rather_than_a_guessed_target() {
        // Failing closed matters here: a truncated operand must not resolve to
        // whatever digits happen to precede the end of the pattern.
        let control = perl_test_must::must_some(parse_escape_control(r"\g{2", 0));
        assert!(matches!(control.kind, PatternControlKind::Unsupported { .. }));
        assert_eq!(control.diagnostic, Some(PatternControlDiagnosticCode::InvalidReference));
        assert!(matches!(control.request, ResolutionRequest::None));
        assert_eq!((control.range.start, control.range.end), (0, 4));
    }

    #[test]
    fn k_reference_is_named_only_across_all_three_spellings() {
        for pattern in [r"\k<name>", r"\k{name}", r"\k'name'"] {
            assert!(
                matches!(kind_of(pattern), Some(PatternControlKind::NamedBackreference { .. })),
                "{pattern} should be a named backreference"
            );
        }
        // `\k` has no numeric spelling, so digits must not be promoted to a numeric
        // capture read; they fail closed as an invalid reference instead.
        let control =
            perl_test_must::must_some_with(parse_escape_control(r"\k<1>", 0), "k reference fact");
        assert!(matches!(control.kind, PatternControlKind::Unsupported { .. }));
        assert_eq!(control.diagnostic, Some(PatternControlDiagnosticCode::InvalidReference));
    }

    #[test]
    fn escape_controls_are_found_at_a_non_zero_start() {
        // Ranges are absolute in the pattern, not relative to `start`.
        let control =
            perl_test_must::must_some_with(parse_escape_control(r"ab\K", 2), "keep anchor fact");
        assert_eq!((control.range.start, control.range.end), (2, 4));
    }
}
