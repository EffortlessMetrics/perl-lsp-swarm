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
            let number = pattern.get(operand_range.start..operand_range.end)?.parse::<u32>().ok()?;
            Some(RawControl {
                kind: PatternControlKind::NumericBackreference {
                    number,
                    syntax: PatternReferenceSyntax::PlainNumeric,
                },
                range: RegexRange { start, end },
                operand_range: Some(operand_range),
                request: ResolutionRequest::Number {
                    number,
                    ambiguous_plain_escape: true,
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
            let end = delimited_operand(bytes, operand_start, b'>')
                .map_or(pattern.len(), |(_, end)| end);
            return Some(invalid_reference(pattern, start, end));
        }
        Some(b'+' | b'-' | b'0'..=b'9') => {
            let end = scan_signed_digits(bytes, operand_start);
            (RegexRange { start: operand_start, end }, end)
        }
        _ => {
            return Some(invalid_reference(
                pattern,
                start,
                (start + 2).min(pattern.len()),
            ));
        }
    };
    raw_reference(
        pattern,
        start,
        end,
        operand_range,
        PatternReferenceSyntax::GReference,
        false,
    )
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
    raw_reference(
        pattern,
        start,
        end,
        operand_range,
        PatternReferenceSyntax::KReference,
        true,
    )
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
    let (kind, request, diagnostic) = match operand {
        ParsedOperand::Number(number) if !named_only => (
            PatternControlKind::NumericBackreference { number, syntax },
            ResolutionRequest::Number { number, ambiguous_plain_escape: false },
            None,
        ),
        ParsedOperand::Relative(offset) if !named_only && offset < 0 => (
            PatternControlKind::RelativeBackreference { offset, syntax },
            ResolutionRequest::Relative(offset),
            None,
        ),
        ParsedOperand::Name(name) => (
            PatternControlKind::NamedBackreference { name: name.clone(), syntax },
            ResolutionRequest::Name(name),
            None,
        ),
        _ => (
            PatternControlKind::Unsupported {
                spelling: bounded_spelling(pattern, RegexRange { start, end }),
            },
            ResolutionRequest::None,
            Some(PatternControlDiagnosticCode::InvalidReference),
        ),
    };
    Some(RawControl {
        kind,
        range: RegexRange { start, end },
        operand_range: Some(operand_range),
        request,
        effect: PatternControlEffect::CaptureRead,
        boundary: diagnostic.map(|_| PatternBoundaryKind::UnsupportedControl),
        diagnostic,
    })
}
