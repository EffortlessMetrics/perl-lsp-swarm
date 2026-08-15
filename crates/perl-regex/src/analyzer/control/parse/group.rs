use super::{
    ParsedOperand, PatternBoundaryKind, PatternControlDiagnosticCode, PatternControlEffect,
    PatternControlKind, PatternReferenceSyntax, RawControl, RegexRange, ResolutionRequest,
    bounded_spelling, find_byte, invalid_reference, parse_operand, scan_signed_digits,
    simple_control,
};

pub(super) fn parse_special_group_control(pattern: &str, start: usize) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    if !pattern.get(start..).is_some_and(|rest| rest.starts_with("(?")) {
        return None;
    }
    if pattern.get(start..).is_some_and(|rest| rest.starts_with("(?R)")) {
        return Some(simple_control(
            PatternControlKind::WholePatternRecursion,
            PatternControlEffect::SubpatternCall,
            start,
            start + 4,
        ));
    }
    if pattern.get(start..).is_some_and(|rest| rest.starts_with("(?0)")) {
        return Some(simple_control(
            PatternControlKind::WholePatternRecursion,
            PatternControlEffect::SubpatternCall,
            start,
            start + 4,
        ));
    }
    if pattern.get(start..).is_some_and(|rest| rest.starts_with("(?P=")) {
        return named_parenthesized_control(
            pattern,
            start,
            start + 4,
            PatternReferenceSyntax::PythonBackreference,
            ParenthesizedNamedKind::Backreference,
        );
    }
    if pattern.get(start..).is_some_and(|rest| rest.starts_with("(?P>")) {
        return named_parenthesized_control(
            pattern,
            start,
            start + 4,
            PatternReferenceSyntax::SubpatternCall,
            ParenthesizedNamedKind::SubpatternCall,
        );
    }
    if pattern.get(start..).is_some_and(|rest| rest.starts_with("(?&")) {
        return named_parenthesized_control(
            pattern,
            start,
            start + 3,
            PatternReferenceSyntax::SubpatternCall,
            ParenthesizedNamedKind::SubpatternCall,
        );
    }
    if pattern.get(start..).is_some_and(|rest| rest.starts_with("(?(")) {
        return parse_conditional(pattern, start);
    }

    let operand_start = start + 2;
    if matches!(bytes.get(operand_start), Some(b'+' | b'-' | b'0'..=b'9')) {
        let operand_end = scan_signed_digits(bytes, operand_start);
        if bytes.get(operand_end) != Some(&b')') {
            return Some(invalid_reference(pattern, start, operand_end));
        }
        let operand_range = RegexRange { start: operand_start, end: operand_end };
        let end = operand_end + 1;
        let operand = parse_operand(pattern.get(operand_start..operand_end)?);
        return Some(match operand {
            ParsedOperand::Number(0) => simple_control(
                PatternControlKind::WholePatternRecursion,
                PatternControlEffect::SubpatternCall,
                start,
                end,
            ),
            ParsedOperand::Number(number) => RawControl {
                kind: PatternControlKind::NumberedSubpatternCall { number },
                range: RegexRange { start, end },
                operand_range: Some(operand_range),
                request: ResolutionRequest::Number { number, ambiguous_plain_escape: false },
                effect: PatternControlEffect::SubpatternCall,
                boundary: None,
                diagnostic: None,
            },
            ParsedOperand::Relative(offset) => RawControl {
                kind: PatternControlKind::RelativeSubpatternCall { offset },
                range: RegexRange { start, end },
                operand_range: Some(operand_range),
                request: ResolutionRequest::Relative(offset),
                effect: PatternControlEffect::SubpatternCall,
                boundary: None,
                diagnostic: None,
            },
            _ => invalid_reference(pattern, start, end),
        });
    }
    None
}

#[derive(Debug, Clone, Copy)]
enum ParenthesizedNamedKind {
    Backreference,
    SubpatternCall,
}

fn named_parenthesized_control(
    pattern: &str,
    start: usize,
    operand_start: usize,
    syntax: PatternReferenceSyntax,
    kind: ParenthesizedNamedKind,
) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    let operand_end = match find_byte(bytes, operand_start, b')') {
        Some(value) => value,
        None => return Some(invalid_reference(pattern, start, pattern.len())),
    };
    let operand_range = RegexRange { start: operand_start, end: operand_end };
    let end = operand_end + 1;
    let name = pattern.get(operand_start..operand_end)?.to_string();
    if name.is_empty() {
        return Some(invalid_reference(pattern, start, end));
    }
    let (control_kind, effect) = match kind {
        ParenthesizedNamedKind::Backreference => (
            PatternControlKind::NamedBackreference { name: name.clone(), syntax },
            PatternControlEffect::CaptureRead,
        ),
        ParenthesizedNamedKind::SubpatternCall => (
            PatternControlKind::NamedSubpatternCall { name: name.clone() },
            PatternControlEffect::SubpatternCall,
        ),
    };
    Some(RawControl {
        kind: control_kind,
        range: RegexRange { start, end },
        operand_range: Some(operand_range),
        request: ResolutionRequest::Name(name),
        effect,
        boundary: None,
        diagnostic: None,
    })
}

fn parse_conditional(pattern: &str, start: usize) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    let predicate_start = start + 3;
    let predicate_end = match find_byte(bytes, predicate_start, b')') {
        Some(value) => value,
        None => return Some(invalid_reference(pattern, start, pattern.len())),
    };
    let end = predicate_end + 1;
    let raw_predicate = pattern.get(predicate_start..predicate_end)?;
    let (operand_range, predicate) = unwrap_conditional_operand(raw_predicate, predicate_start);
    let (kind, request, boundary, diagnostic) = if predicate == "R"
        || predicate.starts_with("R&")
        || predicate
            .strip_prefix('R')
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
    {
        (PatternControlKind::RecursionConditional, ResolutionRequest::None, None, None)
    } else {
        match parse_operand(predicate) {
            ParsedOperand::Number(number) => (
                PatternControlKind::CaptureConditionalNumber { number },
                ResolutionRequest::Number { number, ambiguous_plain_escape: false },
                None,
                None,
            ),
            ParsedOperand::Name(name) if name != "DEFINE" => (
                PatternControlKind::CaptureConditionalName { name: name.clone() },
                ResolutionRequest::Name(name),
                None,
                None,
            ),
            ParsedOperand::Relative(_) | ParsedOperand::Name(_) => (
                PatternControlKind::Unsupported {
                    spelling: bounded_spelling(pattern, RegexRange { start, end }),
                },
                ResolutionRequest::None,
                Some(PatternBoundaryKind::UnsupportedControl),
                Some(PatternControlDiagnosticCode::UnsupportedControl),
            ),
            // Perl accepts an assertion as a conditional predicate, for example
            // `(?(?=x)yes|no)`. It is well-formed input this analysis does not model, so
            // it takes the unsupported code and stays distinct from malformed spelling.
            ParsedOperand::Invalid if is_assertion_predicate(predicate) => (
                PatternControlKind::Unsupported {
                    spelling: bounded_spelling(pattern, RegexRange { start, end }),
                },
                ResolutionRequest::None,
                Some(PatternBoundaryKind::UnsupportedControl),
                Some(PatternControlDiagnosticCode::UnsupportedControl),
            ),
            ParsedOperand::Invalid => (
                PatternControlKind::Unsupported {
                    spelling: bounded_spelling(pattern, RegexRange { start, end }),
                },
                ResolutionRequest::None,
                Some(PatternBoundaryKind::UnsupportedControl),
                Some(PatternControlDiagnosticCode::InvalidReference),
            ),
        }
    };
    Some(RawControl {
        kind,
        range: RegexRange { start, end },
        operand_range: Some(operand_range),
        request,
        effect: PatternControlEffect::ConditionalControl,
        boundary,
        diagnostic,
    })
}

/// Whether a conditional predicate is a lookahead or lookbehind assertion.
///
/// Only the assertion openers are accepted here; any other `(?`-style predicate stays
/// malformed so that unmodelled input is not confused with a spelling error.
fn is_assertion_predicate(predicate: &str) -> bool {
    let Some(rest) = predicate.strip_prefix('?') else {
        return false;
    };
    matches!(rest.as_bytes().first(), Some(b'=' | b'!'))
        || rest
            .strip_prefix('<')
            .is_some_and(|rest| matches!(rest.as_bytes().first(), Some(b'=' | b'!')))
}

fn unwrap_conditional_operand(raw: &str, start: usize) -> (RegexRange, &str) {
    let bytes = raw.as_bytes();
    if raw.len() >= 2 {
        let pair = (bytes[0], bytes[raw.len() - 1]);
        if matches!(pair, (b'<', b'>') | (b'\'', b'\'')) {
            return (
                RegexRange { start: start + 1, end: start + raw.len() - 1 },
                &raw[1..raw.len() - 1],
            );
        }
    }
    (RegexRange { start, end: start + raw.len() }, raw)
}

pub(super) fn parse_star_control(pattern: &str, start: usize) -> RawControl {
    let bytes = pattern.as_bytes();
    let end = find_balanced_star_end(pattern, start).unwrap_or(pattern.len());
    let range = RegexRange { start, end };
    if bytes.get(start + 2) == Some(&b'{') {
        RawControl {
            kind: PatternControlKind::OptimisticEmbeddedCode,
            range,
            operand_range: None,
            request: ResolutionRequest::None,
            effect: PatternControlEffect::DynamicExecution,
            boundary: Some(PatternBoundaryKind::EmbeddedCodeExecution),
            diagnostic: Some(PatternControlDiagnosticCode::EmbeddedCodeBoundary),
        }
    } else {
        unsupported_control(pattern, range, "(*")
    }
}

fn find_balanced_star_end(pattern: &str, start: usize) -> Option<usize> {
    let bytes = pattern.as_bytes();
    if !pattern.get(start..).is_some_and(|rest| rest.starts_with("(*")) {
        return None;
    }
    if bytes.get(start + 2) != Some(&b'{') {
        return find_byte(bytes, start + 2, b')').map(|end| end + 1);
    }
    let mut cursor = start + 3;
    let mut depth = 1usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < bytes.len() {
        let ch = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == b'\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        match ch {
            b'\'' | b'"' => quote = Some(ch),
            b'{' => depth = depth.saturating_add(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    cursor += 1;
                    if bytes.get(cursor) == Some(&b')') {
                        cursor += 1;
                    }
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

pub(super) fn unsupported_control(pattern: &str, range: RegexRange, fallback: &str) -> RawControl {
    let spelling = pattern
        .get(range.start..range.end)
        .filter(|value| !value.is_empty())
        .map_or_else(|| fallback.to_string(), ToString::to_string);
    RawControl {
        kind: PatternControlKind::Unsupported { spelling },
        range,
        operand_range: None,
        request: ResolutionRequest::None,
        effect: PatternControlEffect::Unsupported,
        boundary: Some(PatternBoundaryKind::UnsupportedControl),
        diagnostic: Some(PatternControlDiagnosticCode::UnsupportedControl),
    }
}
