use super::{
    ParsedOperand, PatternBoundaryKind, PatternControlDiagnosticCode, PatternControlEffect,
    PatternControlKind, PatternReferenceSyntax, RawControl, RegexRange, ResolutionRequest,
    bounded_spelling, find_byte, invalid_reference, parse_operand, scan_signed_digits,
    simple_control,
};

pub(super) fn parse_special_group_control(pattern: &str, start: usize) -> Option<RawControl> {
    let bytes = pattern.as_bytes();
    let rest = pattern.get(start..)?;
    if !rest.starts_with("(?") {
        return None;
    }
    // `(?R)` and `(?0)` are the same whole-pattern recursion spelled two ways.
    if rest.starts_with("(?R)") || rest.starts_with("(?0)") {
        return Some(simple_control(
            PatternControlKind::WholePatternRecursion,
            PatternControlEffect::SubpatternCall,
            start,
            start + 4,
        ));
    }
    if rest.starts_with("(?P=") {
        return named_parenthesized_control(
            pattern,
            start,
            start + 4,
            PatternReferenceSyntax::PythonBackreference,
            ParenthesizedNamedKind::Backreference,
        );
    }
    if rest.starts_with("(?P>") {
        return named_parenthesized_control(
            pattern,
            start,
            start + 4,
            PatternReferenceSyntax::SubpatternCall,
            ParenthesizedNamedKind::SubpatternCall,
        );
    }
    if rest.starts_with("(?&") {
        return named_parenthesized_control(
            pattern,
            start,
            start + 3,
            PatternReferenceSyntax::SubpatternCall,
            ParenthesizedNamedKind::SubpatternCall,
        );
    }
    if rest.starts_with("(?(") {
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

/// Extent of a `(*...)` control, including a `(*{ ... })` block body.
///
/// The body is Perl code, and this is a brace counter, not a Perl lexer. It deliberately
/// tracks backslash escaping only inside a quoted run, because outside one a backslash is
/// Perl's reference operator and does not escape the following byte: in `\{ a => 1 }` the
/// braces are real and balanced, so skipping the `{` as "escaped" would drop depth a level
/// early and truncate the construct.
///
/// The residual case this cannot get right is a brace escaped inside a nested regex literal
/// (`s/\{/[/`), where the `{` is counted but never closed. That direction is the safe one:
/// the scan runs to the end, returns `None`, and the caller falls back to `pattern.len()`.
/// The construct is reported as an unsupported boundary either way, so an over-long extent
/// stays conservative, whereas truncating early would hand the tail back to the pattern
/// scanner as if it were regex source. Resolving this properly needs a Perl code lexer,
/// which is out of this layer's scope — embedded code is a typed boundary here, not parsed.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn control(pattern: &str) -> RawControl {
        perl_test_must::must_some(parse_special_group_control(pattern, 0))
    }

    #[test]
    fn only_question_mark_groups_are_special_group_controls() {
        // The dispatcher offers every `(`; plain and star groups belong elsewhere.
        assert!(parse_special_group_control("(abc)", 0).is_none());
        assert!(parse_special_group_control("(*ACCEPT)", 0).is_none());
        // An out-of-range or non-boundary start yields no fact rather than panicking.
        assert!(parse_special_group_control("(?R)", 99).is_none());
    }

    #[test]
    fn both_whole_pattern_recursion_spellings_agree() {
        // `(?R)` and `(?0)` are the same control; they must not diverge.
        for pattern in ["(?R)", "(?0)"] {
            let fact = control(pattern);
            assert_eq!(fact.kind, PatternControlKind::WholePatternRecursion, "{pattern}");
            assert_eq!(fact.effect, PatternControlEffect::SubpatternCall, "{pattern}");
            assert_eq!((fact.range.start, fact.range.end), (0, 4), "{pattern}");
        }
    }

    #[test]
    fn parenthesized_named_forms_split_reads_from_calls() {
        // `(?P=x)` reads a capture; `(?P>x)` and `(?&x)` call a subpattern. Publishing a
        // call as a capture read would misreport what the construct does.
        let read = control("(?P=x)");
        assert!(matches!(read.kind, PatternControlKind::NamedBackreference { .. }));
        assert_eq!(read.effect, PatternControlEffect::CaptureRead);

        for pattern in ["(?P>x)", "(?&x)"] {
            let call = control(pattern);
            assert!(
                matches!(call.kind, PatternControlKind::NamedSubpatternCall { .. }),
                "{pattern}"
            );
            assert_eq!(call.effect, PatternControlEffect::SubpatternCall, "{pattern}");
        }
    }

    #[test]
    fn an_empty_named_operand_is_invalid_rather_than_an_empty_name() {
        let fact = control("(?&)");
        assert!(matches!(fact.kind, PatternControlKind::Unsupported { .. }));
        assert_eq!(fact.diagnostic, Some(PatternControlDiagnosticCode::InvalidReference));
    }

    #[test]
    fn numeric_subpattern_calls_separate_absolute_from_relative() {
        assert!(matches!(
            control("(?1)").kind,
            PatternControlKind::NumberedSubpatternCall { number: 1 }
        ));
        assert!(matches!(
            control("(?+1)").kind,
            PatternControlKind::RelativeSubpatternCall { offset: 1 }
        ));
        assert!(matches!(
            control("(?-1)").kind,
            PatternControlKind::RelativeSubpatternCall { offset: -1 }
        ));
        // `(?0)` is whole-pattern recursion, not a call to capture zero.
        assert_eq!(control("(?0)").kind, PatternControlKind::WholePatternRecursion);
    }

    #[test]
    fn an_unclosed_numeric_operand_does_not_become_a_call() {
        let fact = control("(?12");
        assert!(matches!(fact.kind, PatternControlKind::Unsupported { .. }));
    }

    #[test]
    fn conditionals_distinguish_capture_number_name_and_recursion() {
        assert!(matches!(
            control("(?(1)yes|no)").kind,
            PatternControlKind::CaptureConditionalNumber { number: 1 }
        ));
        assert!(matches!(
            control("(?(<x>)yes|no)").kind,
            PatternControlKind::CaptureConditionalName { .. }
        ));
        for pattern in ["(?(R)yes|no)", "(?(R1)yes|no)", "(?(R&x)yes|no)"] {
            assert_eq!(
                control(pattern).kind,
                PatternControlKind::RecursionConditional,
                "{pattern}"
            );
        }
    }

    #[test]
    fn assertion_predicates_are_unsupported_not_malformed() {
        // Perl accepts these; they are unmodelled input, and the two must stay
        // distinguishable so a reader is not told their pattern is misspelled.
        for pattern in ["(?(?=x)y|n)", "(?(?!x)y|n)", "(?(?<=x)y|n)", "(?(?<!x)y|n)"] {
            let fact = perl_test_must::must_some(parse_special_group_control(pattern, 0));
            assert_eq!(
                fact.diagnostic,
                Some(PatternControlDiagnosticCode::UnsupportedControl),
                "{pattern}"
            );
        }
        // A predicate that is not an assertion stays an invalid-reference report.
        let fact = control("(?(?#x)y|n)");
        assert_eq!(fact.diagnostic, Some(PatternControlDiagnosticCode::InvalidReference));
    }

    #[test]
    fn star_controls_separate_code_blocks_from_verbs() {
        let code = parse_star_control("(*{ 1 })", 0);
        assert_eq!(code.kind, PatternControlKind::OptimisticEmbeddedCode);
        assert_eq!(code.effect, PatternControlEffect::DynamicExecution);
        assert_eq!(code.boundary, Some(PatternBoundaryKind::EmbeddedCodeExecution));

        let verb = parse_star_control("(*ACCEPT)", 0);
        assert!(matches!(verb.kind, PatternControlKind::Unsupported { .. }));
        assert_eq!((verb.range.start, verb.range.end), (0, 9));
    }

    #[test]
    fn a_star_code_block_extent_counts_balanced_braces_and_skips_quoted_ones() {
        // A brace inside a quoted run must not close the block early.
        let quoted = parse_star_control(r#"(*{ $x = "}" })ab"#, 0);
        assert_eq!(quoted.range.end, 15);
        // Nested braces are balanced, so the outer one closes the block.
        let nested = parse_star_control("(*{ { } })ab", 0);
        assert_eq!(nested.range.end, 10);
    }

    #[test]
    fn an_unterminated_star_block_fails_open_to_the_pattern_end() {
        // Over-extending is the safe direction: the construct is reported as an
        // unsupported boundary either way, and truncating early would hand the tail
        // back to the pattern scanner as if it were regex source.
        let fact = parse_star_control("(*{ unterminated", 0);
        assert_eq!(fact.range.end, "(*{ unterminated".len());
    }

    #[test]
    fn unsupported_control_uses_the_source_spelling_when_it_has_one() {
        let range = RegexRange { start: 0, end: 5 };
        let named = unsupported_control("(*XY)", range, "(*");
        assert!(
            matches!(named.kind, PatternControlKind::Unsupported { spelling } if spelling == "(*XY)")
        );
        // An empty slice falls back rather than publishing an empty spelling.
        let empty = unsupported_control("", RegexRange { start: 0, end: 0 }, "(*");
        assert!(
            matches!(empty.kind, PatternControlKind::Unsupported { spelling } if spelling == "(*")
        );
    }
}
