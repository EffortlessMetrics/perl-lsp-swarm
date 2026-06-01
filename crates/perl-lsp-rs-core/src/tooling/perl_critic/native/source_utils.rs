//! Source text and LSP range helpers for native critic rules.

use perl_parser_core::position::{Position, Range};

pub(super) fn full_line_range_for_byte_span(source: &str, start: usize, end: usize) -> Range {
    let line_start = source[..start].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = source[end..].find('\n').map_or(source.len(), |pos| end + pos + 1);
    range_for_byte_span(source, line_start, line_end)
}

pub(super) fn has_use_statement(content: &str, feature: &str) -> bool {
    content.lines().any(|line| has_use_statement_line(line, feature))
}

pub(super) fn has_use_statement_line(line: &str, feature: &str) -> bool {
    let code_portion = line.split('#').next().unwrap_or_default();
    let mut tokens = code_portion.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
    if first != "use" {
        return false;
    }
    let Some(module) = tokens.next() else {
        return false;
    };
    module.trim_end_matches(';') == feature
}

pub(super) fn range_for_byte_span(content: &str, start: usize, end: usize) -> Range {
    let start = start.min(content.len());
    let end = end.min(content.len()).max(start);
    let start_position = position_for_byte_offset(content, start);
    let end_position = position_for_byte_offset(content, end);

    Range { start: start_position, end: end_position }
}

fn position_for_byte_offset(content: &str, offset: usize) -> Position {
    let offset = offset.min(content.len());
    let prefix = &content[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
    let column = content[line_start..offset].chars().count();

    Position { byte: offset, line: usize_to_u32(line), column: usize_to_u32(column) }
}

fn usize_to_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}
