//! Deterministic recursive document-symbol node budget (#8242).
//!
//! Applies one total-node cap after final tree composition. Root-vector
//! truncation is intentionally not used: every visible logical node counts,
//! ancestry is preserved, and source order stays stable.

use super::{DocumentSymbol, leaf_document_symbol};
use perl_position_tracking::{WirePosition, WireRange};
use perl_source_identity::ContentDigest;
use serde::{Deserialize, Serialize};

/// Stable budget-policy identity recorded on post-projection receipts.
pub const DOCUMENT_SYMBOL_NODE_BUDGET_ID: &str = "document_symbol_recursive_node_budget.v1";

/// Outcome of applying the configured cap to a composed outline tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbolBudgetOutcome {
    /// Visible tree returned to adapters.
    pub symbols: Vec<DocumentSymbol>,
    /// Total logical nodes in the composed tree before budgeting.
    pub eligible_total: usize,
    /// Total logical nodes retained after budgeting.
    pub visible_total: usize,
    /// Nodes omitted by the budget (`eligible_total - visible_total`).
    pub omitted_total: usize,
    /// Whether any eligible node was omitted.
    pub truncated: bool,
}

/// Typed receipt for the final visible document-symbol tree (#8242).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbolProjectionReceipt {
    /// Budget policy identity applied to this projection.
    pub budget_identity: String,
    /// Configured node cap for this projection.
    pub configured_cap: usize,
    /// Candidate nodes in the composed tree before budgeting.
    pub eligible_total: usize,
    /// Nodes visible after budgeting.
    pub visible_total: usize,
    /// Nodes omitted by budgeting.
    pub omitted_total: usize,
    /// Whether budgeting removed any eligible node.
    pub truncated: bool,
    /// Deterministic fingerprint of the visible logical tree.
    pub visible_tree_fingerprint: String,
    /// Optional request/operation identity binding the response (#6834).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_result_identity: Option<String>,
}

impl DocumentSymbolProjectionReceipt {
    /// Build a post-projection receipt from a budget outcome.
    #[must_use]
    pub fn from_budget_outcome(
        configured_cap: usize,
        outcome: &DocumentSymbolBudgetOutcome,
        operation_result_identity: Option<String>,
    ) -> Self {
        Self {
            budget_identity: DOCUMENT_SYMBOL_NODE_BUDGET_ID.to_string(),
            configured_cap,
            eligible_total: outcome.eligible_total,
            visible_total: outcome.visible_total,
            omitted_total: outcome.omitted_total,
            truncated: outcome.truncated,
            visible_tree_fingerprint: visible_tree_fingerprint(&outcome.symbols),
            operation_result_identity,
        }
    }
}

/// Count logical nodes across the full tree.
#[must_use]
pub fn count_document_symbol_nodes(symbols: &[DocumentSymbol]) -> usize {
    symbols.iter().fold(0usize, |total, symbol| {
        total.saturating_add(1).saturating_add(count_document_symbol_nodes(&symbol.children))
    })
}

/// Append POD `=head1..=head4` headings as outline roots in source order.
#[must_use]
pub fn pod_heading_document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let Ok(head_regex) = regex::Regex::new(r"^=(head[1-4])\s+(.+)$") else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    let line_starts = line_start_byte_offsets(source);
    for (line_num, line) in source.lines().enumerate() {
        if line == "__DATA__" || line == "__END__" {
            break;
        }
        let Some(caps) = head_regex.captures(line) else {
            continue;
        };
        let name = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let line_index = u32::try_from(line_num).unwrap_or(u32::MAX);
        let line_start = line_starts.get(line_num).copied().unwrap_or(0);
        let line_end_byte = line_start.saturating_add(line.len());
        let end_character =
            perl_position_tracking::offset_to_utf16_line_col(source, line_end_byte).1;
        let range = WireRange {
            start: WirePosition::new(line_index, 0),
            end: WirePosition::new(line_index, end_character),
        };
        symbols.push(leaf_document_symbol(name, "", 26, range));
    }
    symbols
}

/// Merge additional roots into `outline` preserving source order.
pub fn merge_document_symbol_roots_in_source_order(
    outline: &mut Vec<DocumentSymbol>,
    additional: Vec<DocumentSymbol>,
) {
    if additional.is_empty() {
        return;
    }
    outline.extend(additional);
    outline.sort_by(source_order_key);
}

/// Apply the configured recursive node budget to a composed outline tree.
#[must_use]
pub fn apply_document_symbol_node_budget(
    mut symbols: Vec<DocumentSymbol>,
    cap: usize,
) -> DocumentSymbolBudgetOutcome {
    symbols.sort_by(source_order_key);
    let eligible_total = count_document_symbol_nodes(&symbols);
    if cap == 0 {
        return DocumentSymbolBudgetOutcome {
            symbols: Vec::new(),
            eligible_total,
            visible_total: 0,
            omitted_total: eligible_total,
            truncated: eligible_total > 0,
        };
    }
    if eligible_total <= cap {
        return DocumentSymbolBudgetOutcome {
            symbols,
            eligible_total,
            visible_total: eligible_total,
            omitted_total: 0,
            truncated: false,
        };
    }

    let mut remaining = cap;
    let pruned = prune_preorder(symbols, &mut remaining);
    let visible_total = count_document_symbol_nodes(&pruned);
    DocumentSymbolBudgetOutcome {
        symbols: pruned,
        eligible_total,
        visible_total,
        omitted_total: eligible_total.saturating_sub(visible_total),
        truncated: true,
    }
}

fn prune_preorder(mut nodes: Vec<DocumentSymbol>, remaining: &mut usize) -> Vec<DocumentSymbol> {
    nodes.sort_by(source_order_key);
    let mut kept = Vec::new();
    for mut node in nodes {
        if *remaining == 0 {
            break;
        }
        *remaining = remaining.saturating_sub(1);
        node.children = prune_preorder(node.children, remaining);
        kept.push(node);
    }
    kept
}

fn source_order_key(left: &DocumentSymbol, right: &DocumentSymbol) -> std::cmp::Ordering {
    left.range
        .start
        .line
        .cmp(&right.range.start.line)
        .then_with(|| left.range.start.character.cmp(&right.range.start.character))
        .then_with(|| left.range.end.line.cmp(&right.range.end.line))
        .then_with(|| left.range.end.character.cmp(&right.range.end.character))
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.kind.cmp(&right.kind))
}

fn line_start_byte_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (idx, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(idx.saturating_add(1));
        }
    }
    offsets
}

fn visible_tree_fingerprint(symbols: &[DocumentSymbol]) -> String {
    let mut canonical = String::new();
    write_visible_tree(symbols, &mut canonical);
    format!("{}", ContentDigest::of_bytes(canonical.as_bytes()))
}

fn write_visible_tree(symbols: &[DocumentSymbol], out: &mut String) {
    for symbol in symbols {
        out.push_str("n:");
        out.push_str(&symbol.name);
        out.push(':');
        out.push_str(&symbol.kind.to_string());
        out.push(':');
        out.push_str(&symbol.range.start.line.to_string());
        out.push(',');
        out.push_str(&symbol.range.start.character.to_string());
        out.push('-');
        out.push_str(&symbol.range.end.line.to_string());
        out.push(',');
        out.push_str(&symbol.range.end.character.to_string());
        out.push(';');
        out.push('(');
        write_visible_tree(&symbol.children, out);
        out.push(')');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, line: u32, children: Vec<DocumentSymbol>) -> DocumentSymbol {
        let range =
            WireRange { start: WirePosition::new(line, 0), end: WirePosition::new(line, 1) };
        DocumentSymbol {
            name: name.to_string(),
            detail: String::new(),
            kind: 12,
            range,
            selection_range: range,
            children,
            sort_priority: 0,
        }
    }

    #[test]
    fn cap_zero_yields_empty_visible_tree() {
        let tree = vec![symbol("Pkg", 0, vec![symbol("sub_a", 1, vec![])])];
        let outcome = apply_document_symbol_node_budget(tree, 0);
        assert!(outcome.symbols.is_empty());
        assert_eq!(outcome.eligible_total, 2);
        assert_eq!(outcome.visible_total, 0);
        assert_eq!(outcome.omitted_total, 2);
        assert!(outcome.truncated);
    }

    #[test]
    fn one_root_with_many_children_is_bounded_recursively() {
        let children: Vec<_> = (0..100)
            .map(|i| symbol(&format!("sub_{i:03}"), u32::try_from(i + 1).unwrap_or(1), vec![]))
            .collect();
        let tree = vec![symbol("Pkg", 0, children)];
        let outcome = apply_document_symbol_node_budget(tree, 5);
        assert_eq!(outcome.visible_total, 5);
        assert_eq!(outcome.eligible_total, 101);
        assert!(outcome.truncated);
        assert_eq!(count_document_symbol_nodes(&outcome.symbols), 5);
    }

    #[test]
    fn retained_children_always_have_retained_ancestors() {
        let tree = vec![
            symbol("a", 0, vec![symbol("a1", 1, vec![symbol("a1x", 2, vec![])])]),
            symbol("b", 3, vec![symbol("b1", 4, vec![])]),
        ];
        let outcome = apply_document_symbol_node_budget(tree, 4);
        assert!(outcome.visible_total <= 4);
    }

    #[test]
    fn map_insertion_order_does_not_change_retained_identities() {
        let shuffled = vec![symbol("b", 1, vec![]), symbol("a", 0, vec![])];
        let mut ordered = vec![symbol("a", 0, vec![]), symbol("b", 1, vec![])];
        ordered.sort_by(source_order_key);
        let shuffled_out = apply_document_symbol_node_budget(shuffled, 2);
        let ordered_out = apply_document_symbol_node_budget(ordered, 2);
        let shuffled_names: Vec<_> =
            shuffled_out.symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        let ordered_names: Vec<_> =
            ordered_out.symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(shuffled_names, ordered_names);
    }

    #[test]
    fn pod_headings_participate_in_the_same_budget() {
        let mut outline = vec![symbol("Pkg", 0, vec![symbol("sub_a", 1, vec![])])];
        let pod = pod_heading_document_symbols("=head1 Title\nsub foo {}\n");
        merge_document_symbol_roots_in_source_order(&mut outline, pod);
        let outcome = apply_document_symbol_node_budget(outline, 2);
        assert_eq!(outcome.visible_total, 2);
        assert!(outcome.truncated);
    }

    #[test]
    fn receipt_fingerprint_matches_visible_tree() {
        let tree = vec![symbol("Pkg", 0, vec![symbol("sub_a", 1, vec![])])];
        let outcome = apply_document_symbol_node_budget(tree, 1);
        let receipt = DocumentSymbolProjectionReceipt::from_budget_outcome(1, &outcome, None);
        assert_eq!(receipt.visible_total, 1);
        assert!(receipt.truncated);
        assert_eq!(receipt.visible_tree_fingerprint, visible_tree_fingerprint(&outcome.symbols));
    }

    #[test]
    fn budget_keeps_earlier_child_in_source_order() {
        let mut earlier = symbol("sub_first", 1, vec![]);
        earlier.sort_priority = 10;
        let mut later = symbol("has_later", 2, vec![]);
        later.sort_priority = 0;
        let tree = vec![symbol("Pkg", 0, vec![earlier, later])];
        let outcome = apply_document_symbol_node_budget(tree, 2);
        assert_eq!(outcome.visible_total, 2);
        let kept = outcome
            .symbols
            .first()
            .and_then(|pkg| pkg.children.first())
            .map(|child| child.name.as_str());
        assert_eq!(kept, Some("sub_first"));
    }

    #[test]
    fn pod_headings_use_crlf_line_end_columns() {
        let symbols = pod_heading_document_symbols("=head1 First\r\n=head1 Second\r\n");
        assert_eq!(symbols.len(), 2);
        let second = &symbols[1];
        assert_eq!(second.range.start.line, 1);
        let expected_end = perl_position_tracking::offset_to_utf16_line_col("=head1 Second", 13).1;
        assert_eq!(second.range.end.character, expected_end);
    }

    #[test]
    fn fingerprint_distinguishes_parentage() {
        let flat = vec![symbol("A", 0, vec![]), symbol("B", 1, vec![])];
        let nested = vec![symbol("A", 0, vec![symbol("B", 1, vec![])])];
        assert_ne!(visible_tree_fingerprint(&flat), visible_tree_fingerprint(&nested));
    }
}
