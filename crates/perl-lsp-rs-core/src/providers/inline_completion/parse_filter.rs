use perl_parser_core::{Parser, RecoverySalvageProfile};
use perl_position_tracking::utf16_line_col_to_offset;

use super::InlineCompletionItem;

pub(super) struct ParseDamage {
    terminated_early: bool,
    error_node_count: usize,
    diagnostics_count: usize,
    recovered_count: usize,
}

impl ParseDamage {
    pub(super) fn worse_than(&self, baseline: &Self) -> bool {
        (self.terminated_early && !baseline.terminated_early)
            || self.error_node_count > baseline.error_node_count
            || self.diagnostics_count > baseline.diagnostics_count
            || self.recovered_count > baseline.recovered_count
    }
}

pub(super) fn parse_damage_for_probe(source: &str) -> ParseDamage {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let salvage = RecoverySalvageProfile::from_parse(&output.ast, &output.diagnostics, false);

    ParseDamage {
        terminated_early: output.terminated_early,
        error_node_count: salvage.error_node_count,
        diagnostics_count: output.error_count(),
        recovered_count: output.recovered_count,
    }
}

pub(super) fn parse_probe_after_item(
    current_line: &str,
    item: &InlineCompletionItem,
    line: u32,
    character: u32,
) -> Option<String> {
    let (start_character, end_character) = item
        .range
        .as_ref()
        .map(|range| {
            if range.start.line != line || range.end.line != line {
                return None;
            }
            Some((range.start.character, range.end.character))
        })
        .unwrap_or(Some((character, character)))?;

    let start = utf16_line_col_to_offset(current_line, 0, start_character);
    let end = utf16_line_col_to_offset(current_line, 0, end_character);
    if start > end {
        return None;
    }

    let mut probe = String::with_capacity(current_line.len() + item.insert_text.len());
    probe.push_str(&current_line[..start]);
    probe.push_str(item.insert_text.as_str());
    probe.push_str(&current_line[end..]);
    Some(probe)
}
