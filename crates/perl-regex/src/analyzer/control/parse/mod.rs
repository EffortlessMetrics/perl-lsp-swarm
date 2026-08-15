use crate::validator::RegexRange;

use super::model::{
    PatternBoundaryKind, PatternControlDiagnosticCode, PatternControlEffect, PatternControlKind,
    PatternReferenceSyntax,
};

#[derive(Debug, Clone)]
pub(super) struct RawControl {
    pub(super) kind: PatternControlKind,
    pub(super) range: RegexRange,
    pub(super) operand_range: Option<RegexRange>,
    pub(super) request: ResolutionRequest,
    pub(super) effect: PatternControlEffect,
    pub(super) boundary: Option<PatternBoundaryKind>,
    pub(super) diagnostic: Option<PatternControlDiagnosticCode>,
}

#[derive(Debug, Clone)]
pub(super) enum ResolutionRequest {
    None,
    Number { number: u32, ambiguous_plain_escape: bool },
    Name(String),
    Relative(i32),
}

#[derive(Debug, Clone)]
enum ParsedOperand {
    Number(u32),
    Relative(i32),
    Name(String),
    Invalid,
}

mod escape;
mod group;

pub(super) fn parse_escape_control(pattern: &str, start: usize) -> Option<RawControl> {
    escape::parse_escape_control(pattern, start)
}

pub(super) fn parse_special_group_control(pattern: &str, start: usize) -> Option<RawControl> {
    group::parse_special_group_control(pattern, start)
}

pub(super) fn parse_star_control(pattern: &str, start: usize) -> RawControl {
    group::parse_star_control(pattern, start)
}

pub(super) fn unsupported_control(pattern: &str, range: RegexRange, fallback: &str) -> RawControl {
    group::unsupported_control(pattern, range, fallback)
}

fn invalid_reference(pattern: &str, start: usize, end: usize) -> RawControl {
    let range = RegexRange { start, end: end.max(start).min(pattern.len()) };
    RawControl {
        kind: PatternControlKind::Unsupported { spelling: bounded_spelling(pattern, range) },
        range,
        operand_range: None,
        request: ResolutionRequest::None,
        effect: PatternControlEffect::Unsupported,
        boundary: Some(PatternBoundaryKind::UnsupportedControl),
        diagnostic: Some(PatternControlDiagnosticCode::InvalidReference),
    }
}

fn simple_control(
    kind: PatternControlKind,
    effect: PatternControlEffect,
    start: usize,
    end: usize,
) -> RawControl {
    RawControl {
        kind,
        range: RegexRange { start, end },
        operand_range: None,
        request: ResolutionRequest::None,
        effect,
        boundary: None,
        diagnostic: None,
    }
}

fn parse_operand(raw: &str) -> ParsedOperand {
    if raw.is_empty() {
        return ParsedOperand::Invalid;
    }
    if raw.starts_with('+') || raw.starts_with('-') {
        let rest = &raw[1..];
        if rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()) {
            return ParsedOperand::Invalid;
        }
        return raw.parse::<i32>().map_or(ParsedOperand::Invalid, ParsedOperand::Relative);
    }
    if raw.chars().all(|ch| ch.is_ascii_digit()) {
        return raw.parse::<u32>().map_or(ParsedOperand::Invalid, ParsedOperand::Number);
    }
    if valid_reference_name(raw) {
        ParsedOperand::Name(raw.to_string())
    } else {
        ParsedOperand::Invalid
    }
}

fn valid_reference_name(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn delimited_operand(bytes: &[u8], open: usize, close: u8) -> Option<(RegexRange, usize)> {
    let operand_start = open + 1;
    let operand_end = find_byte(bytes, operand_start, close)?;
    Some((RegexRange { start: operand_start, end: operand_end }, operand_end + 1))
}

fn find_byte(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    bytes
        .get(start..)?
        .iter()
        .position(|candidate| *candidate == needle)
        .map(|relative| start + relative)
}

fn scan_ascii_digits(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes.get(end).is_some_and(|ch| ch.is_ascii_digit()) {
        end += 1;
    }
    end
}

fn scan_signed_digits(bytes: &[u8], start: usize) -> usize {
    let digit_start = if matches!(bytes.get(start), Some(b'+' | b'-')) { start + 1 } else { start };
    let end = scan_ascii_digits(bytes, digit_start);
    if end == digit_start { start } else { end }
}

fn bounded_spelling(pattern: &str, range: RegexRange) -> String {
    const MAX_SPELLING_BYTES: usize = 64;
    let value = pattern.get(range.start..range.end).unwrap_or_default();
    if value.len() <= MAX_SPELLING_BYTES {
        return value.to_string();
    }
    let mut end = MAX_SPELLING_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
