//! Final document-symbol tree composition: POD heading projection and
//! nesting (#18769) plus the deterministic recursive node budget (#8242).
//!
//! POD `=headN` headings are projected through one per-request line index and
//! nested under their innermost enclosing package container before the
//! budget runs, so the cap counts the composed tree users actually see.
//!
//! The budget applies one total-node cap after final tree composition.
//! Root-vector truncation is intentionally not used: every visible logical
//! node counts, ancestry is preserved, and source order stays stable.

use super::{CALLABLE_DOCUMENT_SYMBOL_PRIORITY, DocumentSymbol, leaf_document_symbol};
use perl_position_tracking::{LineStartsCache, WirePosition, WireRange};
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
///
/// The end column converts through one per-request [`LineStartsCache`]
/// instead of a whole-source rescan per heading (#18769).
#[must_use]
pub fn pod_heading_document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let Ok(head_regex) = regex::Regex::new(r"^=(head[1-4])\s+(.+)$") else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    let line_starts = LineStartsCache::new(source);
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
        let line_start = line_starts.position_to_offset(source, line_index, 0);
        let line_end_byte = line_start.saturating_add(line.len());
        let (_end_line, end_character) = line_starts.offset_to_position(source, line_end_byte);
        let range = WireRange {
            start: WirePosition::new(line_index, 0),
            end: WirePosition::new(line_index, end_character),
        };
        symbols.push(leaf_document_symbol(name, "", POD_HEADING_KIND, range));
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

/// Nest POD headings under their innermost enclosing package container,
/// dropping headings whose trimmed name duplicates an existing child of that
/// container (#18769).
///
/// The AST-path outline roots are namespace containers whose ranges span the
/// whole package extent, so a heading between the package statement and
/// `__END__`/`__DATA__` is enclosed by one innermost root. From that root the
/// search descends through nested package containers (a block-form
/// `package Inner { ... }` that the assembler placed under `Outer`), so the
/// heading lands under the tightest enclosing package, not merely the root.
/// A heading outside every root (file-header POD before the first package)
/// stays at the top level. Kind, name, and range of surviving headings are
/// unchanged; an empty outline falls back to the flat root merge.
///
/// Because this runs before [`apply_document_symbol_node_budget`], the cap
/// counts the nested tree: a deduplicated heading no longer consumes budget,
/// and a nested heading is visited in pre-order inside its package rather
/// than as a separate root.
pub fn nest_pod_headings_into_outline(
    outline: &mut Vec<DocumentSymbol>,
    pod_headings: Vec<DocumentSymbol>,
) {
    if pod_headings.is_empty() || outline.is_empty() {
        merge_document_symbol_roots_in_source_order(outline, pod_headings);
        return;
    }
    for mut heading in pod_headings {
        let heading_line = heading.range.start.line;
        // Only package containers own POD: in a package-free script the roots
        // are subs, and a heading inside a sub body stays at the top level.
        let Some(root_index) = innermost_enclosing(outline, heading_line, is_package_container)
        else {
            outline.push(heading);
            continue;
        };
        let mut parent = &mut outline[root_index];
        while let Some(child_index) =
            innermost_enclosing(&parent.children, heading_line, is_package_container)
        {
            parent = &mut parent.children[child_index];
        }
        let heading_name = heading.name.trim();
        // The code symbol already carries the anchor, detail, and selection
        // range; the same-name POD heading adds nothing on the wire, so the
        // `=head2 get_thing_01` / `sub get_thing_01` pair keeps one entry.
        // Only code symbols suppress a heading: a repeated POD section name
        // (two `=head2 Example` blocks) keeps every occurrence.
        if parent
            .children
            .iter()
            .any(|child| child.kind != POD_HEADING_KIND && child.name.trim() == heading_name)
        {
            continue;
        }
        // Rank nested headings with the callables so they interleave with
        // subs in source order instead of sorting ahead of every sub.
        heading.sort_priority = CALLABLE_DOCUMENT_SYMBOL_PRIORITY;
        insert_by_child_order_key(&mut parent.children, heading);
    }
    outline.sort_by(source_order_key);
}

/// LSP `SymbolKind` used for POD headings (26 = TypeParameter, the outline's
/// established POD kind).
const POD_HEADING_KIND: u32 = 26;

/// LSP `SymbolKind` values the assembler uses for package-like containers:
/// 2 = Module (package), 3 = Namespace, 4 = Package, 5 = Class,
/// 11 = Interface (role).
fn is_package_container(symbol: &DocumentSymbol) -> bool {
    matches!(symbol.kind, 2 | 3 | 4 | 5 | 11)
}

/// Index of the eligible symbol whose range most tightly encloses `line`.
fn innermost_enclosing(
    symbols: &[DocumentSymbol],
    line: u32,
    eligible: impl Fn(&DocumentSymbol) -> bool,
) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (index, symbol) in symbols.iter().enumerate() {
        if !eligible(symbol) || line < symbol.range.start.line || line > symbol.range.end.line {
            continue;
        }
        let span = symbol.range.end.line.saturating_sub(symbol.range.start.line);
        if best.is_none_or(|(_, best_span)| span < best_span) {
            best = Some((index, span));
        }
    }
    best.map(|(index, _)| index)
}

/// Insert a child at its assembler-order position (priority, span start,
/// span end, name, kind — the same key the document-symbol assembler uses
/// for children). Children arrive sorted by this key, so a partition-point
/// insert preserves both the existing order and determinism.
fn insert_by_child_order_key(children: &mut Vec<DocumentSymbol>, symbol: DocumentSymbol) {
    let key = child_order_key(&symbol);
    let position = children.partition_point(|child| child_order_key(child) <= key);
    children.insert(position, symbol);
}

fn child_order_key(symbol: &DocumentSymbol) -> (u8, u32, u32, u32, u32, &str, u32) {
    (
        symbol.sort_priority,
        symbol.range.start.line,
        symbol.range.start.character,
        symbol.range.end.line,
        symbol.range.end.character,
        symbol.name.as_str(),
        symbol.kind,
    )
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

    /// A sub child as the assembler emits it (callable priority).
    fn callable(name: &str, line: u32, children: Vec<DocumentSymbol>) -> DocumentSymbol {
        let mut sub = symbol(name, line, children);
        sub.sort_priority = CALLABLE_DOCUMENT_SYMBOL_PRIORITY;
        sub
    }

    /// A package root shaped like the AST-path output: its range spans the
    /// whole package extent, and code members are its children.
    fn package_root(name: &str, end_line: u32, children: Vec<DocumentSymbol>) -> DocumentSymbol {
        let mut root = symbol(name, 0, children);
        root.kind = 2;
        root.range.end = WirePosition::new(end_line, 1);
        root
    }

    #[test]
    fn pod_headings_nest_under_the_enclosing_package_root() {
        // Root extent lines 0..=9; NAME lands on line 2, SEE ALSO on line 7.
        let mut outline = vec![package_root("Pkg", 9, vec![callable("sub_a", 4, vec![])])];
        let pod = pod_heading_document_symbols(
            "package Pkg;\n\n=head1 NAME\n\nPkg - demo\n\nnothing here\n=head1 SEE ALSO\n\nelsewhere\n",
        );
        nest_pod_headings_into_outline(&mut outline, pod);
        assert_eq!(outline.len(), 1, "no POD headings left at top level: {outline:?}");
        let children: Vec<_> = outline[0].children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            children.contains(&"NAME") && children.contains(&"SEE ALSO"),
            "headings nested under the package: {children:?}"
        );
    }

    #[test]
    fn pod_heading_matching_a_child_name_is_dropped() {
        let mut outline = vec![package_root("Pkg", 9, vec![callable("get_thing", 5, vec![])])];
        let pod = pod_heading_document_symbols("=head2 get_thing\n\nDocs.\n\n=cut\n");
        nest_pod_headings_into_outline(&mut outline, pod);
        assert_eq!(outline.len(), 1, "no POD headings left at top level: {outline:?}");
        let children: Vec<_> =
            outline[0].children.iter().map(|c| (c.name.as_str(), c.kind)).collect();
        assert_eq!(
            children,
            vec![("get_thing", 12)],
            "same-name POD heading dropped, code sub kept: {children:?}"
        );
    }

    #[test]
    fn pod_heading_outside_every_root_stays_at_top_level() {
        let mut outline = vec![package_root("Pkg", 9, vec![])];
        // Simulate a root whose extent starts on line 3 (the `package`
        // statement), leaving lines 0..2 outside every root.
        outline[0].range.start = WirePosition::new(3, 0);
        let pod =
            pod_heading_document_symbols("=head1 HEADER\n\nstuff\npackage Pkg;\n=head1 INNER\n");
        nest_pod_headings_into_outline(&mut outline, pod);
        let top: Vec<_> = outline.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert!(top.contains(&("HEADER", 26)), "file-header POD stays a root: {top:?}");
        assert!(
            !top.contains(&("INNER", 26)),
            "heading inside the root extent must not stay a root: {top:?}"
        );
        let children: Vec<_> =
            outline.iter().flat_map(|root| root.children.iter().map(|c| c.name.as_str())).collect();
        assert!(
            children.contains(&"INNER"),
            "heading inside the root extent nests under it: {children:?}"
        );
    }

    #[test]
    fn empty_outline_falls_back_to_the_flat_root_merge() {
        let mut outline = Vec::new();
        let pod = pod_heading_document_symbols("=head1 ONLY\n");
        nest_pod_headings_into_outline(&mut outline, pod);
        assert_eq!(outline.len(), 1, "heading kept as a root: {outline:?}");
        assert_eq!(outline[0].name, "ONLY");
    }

    #[test]
    fn pod_heading_nests_under_the_innermost_nested_package() {
        // Outer spans 0..=20; block-form Inner (Module kind) spans 5..=12
        // and is a child of Outer, the way the assembler nests it.
        let mut inner = package_root("Inner", 12, vec![callable("inner_sub", 10, vec![])]);
        inner.range.start = WirePosition::new(5, 0);
        inner.kind = 2;
        let mut outline =
            vec![package_root("Outer", 20, vec![callable("outer_sub", 2, vec![]), inner])];
        let mut pod = pod_heading_document_symbols("\n\n\n\n\n\n\n=head1 INNER_DOC\n");
        pod.extend(pod_heading_document_symbols(
            "\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n=head1 OUTER_DOC\n",
        ));
        nest_pod_headings_into_outline(&mut outline, pod);
        let outer_children: Vec<_> = outline[0].children.iter().map(|c| c.name.as_str()).collect();
        assert!(outer_children.contains(&"OUTER_DOC"), "after Inner closes: {outer_children:?}");
        assert!(!outer_children.contains(&"INNER_DOC"), "not on Outer: {outer_children:?}");
        let inner_children: Vec<_> = outline[0]
            .children
            .iter()
            .find(|c| c.name == "Inner")
            .map(|c| c.children.iter().map(|g| g.name.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(inner_children, vec!["INNER_DOC", "inner_sub"], "{outline:?}");
    }

    /// Pins how nesting composes with the recursive node budget: the cap
    /// counts the nested tree, so a nested heading is visited in pre-order
    /// inside its package and a deduplicated heading consumes no budget.
    #[test]
    fn capped_outline_counts_the_nested_tree() {
        // package P (0..=9) / =head1 A (line 1) / sub x (3) / sub y (5).
        let pod = pod_heading_document_symbols("package P;\n=head1 A\n");
        let mut outline =
            vec![package_root("P", 9, vec![callable("x", 3, vec![]), callable("y", 5, vec![])])];
        nest_pod_headings_into_outline(&mut outline, pod);
        let outcome = apply_document_symbol_node_budget(outline, 3);
        assert_eq!(outcome.eligible_total, 4);
        assert_eq!(outcome.visible_total, 3);
        assert!(outcome.truncated);
        assert_eq!(outcome.symbols.len(), 1, "one root under the cap: {:?}", outcome.symbols);
        let visible: Vec<_> = outcome.symbols[0].children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(visible, vec!["A", "x"], "pre-order P -> A -> x; y omitted");

        // A heading that duplicates a sub is dropped before budgeting.
        let pod = pod_heading_document_symbols("package P;\n=head2 x\n");
        let mut outline =
            vec![package_root("P", 9, vec![callable("x", 3, vec![]), callable("y", 5, vec![])])];
        nest_pod_headings_into_outline(&mut outline, pod);
        let outcome = apply_document_symbol_node_budget(outline, 3);
        assert_eq!(outcome.eligible_total, 3, "duplicate heading consumes no budget");
        assert!(!outcome.truncated);
    }

    #[test]
    fn heading_after_a_sub_follows_it_in_source_order() {
        // package P (0..=9) / sub first (line 1) / =head1 LATER (line 3).
        let mut outline = vec![package_root("P", 9, vec![callable("first", 1, vec![])])];
        let pod = pod_heading_document_symbols("package P;\nsub first {}\n\n=head1 LATER\n");
        nest_pod_headings_into_outline(&mut outline, pod);
        let outcome = apply_document_symbol_node_budget(outline, 500);
        assert!(!outcome.truncated);
        let children: Vec<_> =
            outcome.symbols[0].children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(children, vec!["first", "LATER"], "uncapped order follows the source");
    }

    #[test]
    fn repeated_pod_section_names_are_all_kept() {
        let mut outline = vec![package_root("P", 9, vec![])];
        let pod =
            pod_heading_document_symbols("package P;\n=head2 Example\n\nx\n\n=head2 Example\n");
        nest_pod_headings_into_outline(&mut outline, pod);
        let examples = outline[0].children.iter().filter(|c| c.name == "Example").count();
        assert_eq!(examples, 2, "both Example sections stay navigable: {outline:?}");
    }

    #[test]
    fn package_free_pod_inside_a_sub_stays_at_top_level() {
        // A script with no package: the root is a multi-line sub (kind 12).
        let mut main_sub = callable("main_sub", 0, vec![]);
        main_sub.range.end = WirePosition::new(6, 1);
        let mut outline = vec![main_sub];
        let pod = pod_heading_document_symbols("sub main_sub {\n\n=head1 INSIDE\n");
        nest_pod_headings_into_outline(&mut outline, pod);
        let top: Vec<_> = outline.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(top, vec!["main_sub", "INSIDE"], "heading not nested under a sub: {outline:?}");
        assert!(outline[0].children.is_empty());
    }
}
