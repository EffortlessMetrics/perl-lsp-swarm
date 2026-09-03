//! Test2 (and Test::More) subtest discovery.
//!
//! A `subtest NAME => sub { ... };` call names a nested group of assertions.
//! This module walks a parsed AST and reconstructs the *structure* of those
//! groups — a tree of [`DiscoveredSubtest`] — without executing anything. The
//! tree drives document symbols, code lenses, and "run/debug nearest subtest".
//!
//! We only read structure. Dynamic subtest names (a variable or expression
//! rather than a string literal) are reported as [`SubtestName::Dynamic`]
//! rather than guessed — the caller decides how to present an unknown name.

use crate::providers::document_symbols::{CALLABLE_DOCUMENT_SYMBOL_PRIORITY, DocumentSymbol};
use perl_parser_core::ast::{Node, NodeKind};
use perl_position_tracking::WireRange;
use perl_semantic_analyzer::symbol::SymbolKind;

/// LSP `SymbolKind` used for subtests in the document outline. `12` is
/// `Function`, which renders with a runnable-looking glyph in common clients.
const SUBTEST_SYMBOL_KIND: u32 = 12;

/// The name of a discovered subtest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtestName {
    /// A statically known name from a string-literal first argument.
    Named(String),
    /// The name is computed at runtime (variable/expression) and not knowable
    /// statically. Callers should present it as unknown, not guess.
    Dynamic,
}

impl SubtestName {
    /// A display label for outlines/lenses.
    pub fn label(&self) -> String {
        match self {
            SubtestName::Named(name) => name.clone(),
            SubtestName::Dynamic => "subtest (dynamic)".to_string(),
        }
    }

    /// The statically known name, if any.
    pub fn as_static(&self) -> Option<&str> {
        match self {
            SubtestName::Named(name) => Some(name.as_str()),
            SubtestName::Dynamic => None,
        }
    }
}

/// A subtest discovered in the source, plus any subtests nested inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSubtest {
    /// The subtest's name (static or dynamic).
    pub name: SubtestName,
    /// Full range of the `subtest ... => sub { ... }` call.
    pub range: WireRange,
    /// Range of the name argument (used as the outline selection range).
    pub name_range: WireRange,
    /// Nested subtests declared inside this subtest's block.
    pub children: Vec<DiscoveredSubtest>,
}

/// Discover the subtest tree in a parsed document.
pub fn discover_subtests(ast: &Node, source: &str) -> Vec<DiscoveredSubtest> {
    let mut out = Vec::new();
    walk(ast, source, &mut out);
    out
}

/// Convert a discovered subtest tree into LSP document symbols.
pub fn subtest_document_symbols(subtests: &[DiscoveredSubtest]) -> Vec<DocumentSymbol> {
    subtests.iter().map(to_document_symbol).collect()
}

/// Merge discovered subtests into an existing document-symbol outline, nesting
/// each root subtest under its closest enclosing outline scope (#1792).
///
/// Placement follows only what the outline already proves about scopes, with
/// the same conventions [`super::super::document_symbols`] uses to assemble
/// parents and children:
///
/// - a subtest declared inside a named `sub` becomes a child of that sub's
///   symbol (strict range containment);
/// - a subtest inside a package/module's member region becomes a child of that
///   package symbol, exactly like every other member — those symbols display
///   only their declaration line but own the members that follow (scope-
///   anchored assembly), so their region runs until the next same-family
///   start rather than ending at the printed range;
/// - the closest such owner wins (largest start, then latest in outline
///   order), mirroring lexical file scope;
/// - a subtest with no enclosing scope stays at its outline level instead of
///   being floated to the root.
///
/// Name, kind, detail, and selection-range conventions are exactly those of
/// [`subtest_document_symbols`]; sibling order follows assembler priority and
/// then source position.
pub fn nest_subtest_symbols_in_outline(
    outline: &mut Vec<DocumentSymbol>,
    subtests: &[DiscoveredSubtest],
    source: &str,
) {
    for symbol in subtest_document_symbols(subtests) {
        let Some(path) = find_owner_path(outline, &symbol.range, source) else {
            insert_by_source_position(outline, symbol);
            continue;
        };
        let mut cursor: &mut Vec<DocumentSymbol> = &mut *outline;
        for index in path {
            cursor = &mut cursor[index].children;
        }
        insert_by_source_position(cursor, symbol);
    }
}

/// Outline node kinds that can lexically own a subtest: packages/classes and
/// their module/namespace aliases (these display only their declaration line
/// while owning trailing members), plus subroutine-shaped scopes.
const OWNER_KIND_MODULE_FAMILY: [u32; 5] = [
    SymbolKind::Package.to_lsp_kind_document_symbol(),
    3,
    4,
    SymbolKind::Class.to_lsp_kind_document_symbol(),
    SymbolKind::Role.to_lsp_kind_document_symbol(),
];
const OWNER_KIND_CALLABLE: [u32; 2] = [
    SymbolKind::Subroutine.to_lsp_kind_document_symbol(),
    SymbolKind::Method.to_lsp_kind_document_symbol(),
];

/// Depth-first selection of the closest owner of `target`, mirroring how the
/// assembler scopes children:
///
/// 1. subroutine-shaped symbols strictly containing `target` win, tightest
///    source position then deepest traversal order;
/// 2. otherwise the module-family symbol owning the surrounding member region
///    wins — the latest-starting one whose region is not interrupted by an
///    equal-or-later-starting sibling module before `target` begins (these
///    symbols display only their declaration line but own trailing members);
/// 3. otherwise there is no owner and the subtest stays at its level.
///
/// Returns the child-index path to the winning node.
fn find_owner_path(
    nodes: &[DocumentSymbol],
    target: &WireRange,
    source: &str,
) -> Option<Vec<usize>> {
    struct Hits {
        strict: Option<((u32, u32), usize, Vec<usize>)>,
        modules: Vec<((u32, u32), Vec<usize>)>,
    }

    fn visit(
        nodes: &[DocumentSymbol],
        target: &WireRange,
        source: &str,
        prefix: &mut Vec<usize>,
        hits: &mut Hits,
    ) {
        for (index, node) in nodes.iter().enumerate() {
            prefix.push(index);
            let depth = prefix.len();
            let start_key = (node.range.start.line, node.range.start.character);
            if OWNER_KIND_CALLABLE.contains(&node.kind)
                && (start_key <= (target.start.line, target.start.character))
                && ((target.end.line, target.end.character)
                    <= (node.range.end.line, node.range.end.character))
            {
                let dominates = match &hits.strict {
                    None => true,
                    Some(((best_line, best_char), best_depth, _)) => {
                        (*best_line, *best_char) < start_key
                            || ((*best_line, *best_char) == start_key && *best_depth < depth)
                    }
                };
                if dominates {
                    hits.strict = Some((start_key, depth, prefix.clone()));
                }
            }
            if OWNER_KIND_MODULE_FAMILY.contains(&node.kind)
                && module_region_contains(node, target, source)
            {
                hits.modules.push((start_key, prefix.clone()));
            }
            visit(&node.children, target, source, prefix, hits);
            prefix.pop();
        }
    }

    let mut prefix = Vec::new();
    let mut hits = Hits { strict: None, modules: Vec::new() };
    visit(nodes, target, source, &mut prefix, &mut hits);

    if let Some((_, _, path)) = hits.strict {
        return Some(path);
    }

    // Statement-scoped package regions run until the next package-family
    // start. Block-scoped package/class/role regions are admitted only while
    // the target remains inside their source range, so a later root-level
    // subtest cannot leak back into a completed `package Inner { ... }` block.
    hits.modules.sort_by_key(|(start_key, _)| *start_key);
    let (_, path) = hits.modules.last()?;
    Some(path.clone())
}

/// Return whether a namespace symbol owns the target's source region.
///
/// The compiler outline uses the declaration range for both forms of Perl
/// package declaration. A statement-scoped `package Foo;` owns the remainder
/// of the current package, while `package Foo { ... }`, `class Foo { ... }`,
/// and equivalent role blocks end at their closing brace. The source is the
/// only reliable discriminator because the wire symbol deliberately exposes
/// the same source-backed range shape for both namespace families.
fn module_region_contains(node: &DocumentSymbol, target: &WireRange, source: &str) -> bool {
    let target_start = target.start.to_byte_offset(source);
    let node_start = node.range.start.to_byte_offset(source);
    if target_start < node_start {
        return false;
    }

    let target_end = target.end.to_byte_offset(source);
    let node_end = node.range.end.to_byte_offset(source);
    if target_end <= node_end {
        return true;
    }

    let Some(declaration) = source.get(node_start..node_end) else {
        return false;
    };

    // A brace before the declaration's first semicolon identifies the
    // self-contained block form (`package Foo { ... }`). This also accepts a
    // trailing semicolon after the closing brace without treating the block
    // as an open-ended statement package.
    let Some(open_brace) = declaration.find('{') else {
        return true;
    };
    declaration[..open_brace].contains(';')
}

/// Insert keeping existing siblings in place and positioning the new symbol
/// inside the established priority group, then by source position. The source
/// document-symbol assembler sorts children by semantic priority first; a
/// source-only partition point is not valid for that vector.
fn insert_by_source_position(children: &mut Vec<DocumentSymbol>, symbol: DocumentSymbol) {
    let sort_key = document_symbol_sort_key(&symbol);
    let position = children.partition_point(|child| document_symbol_sort_key(child) <= sort_key);
    children.insert(position, symbol);
}

fn document_symbol_sort_key(symbol: &DocumentSymbol) -> (u8, u32, u32, u32) {
    (
        symbol.sort_priority,
        symbol.range.start.line,
        symbol.range.start.character,
        symbol.range.end.line,
    )
}

/// Find the innermost subtest whose range contains the 0-based `line`.
///
/// Used by "run/debug nearest subtest": given the cursor line, resolve which
/// subtest the caller is inside. Returns `None` when the line is not inside any
/// subtest. Descends into children so the *innermost* enclosing subtest wins.
pub fn nearest_subtest_at_line(
    subtests: &[DiscoveredSubtest],
    line: u32,
) -> Option<&DiscoveredSubtest> {
    for candidate in subtests {
        if candidate.range.start.line <= line && line <= candidate.range.end.line {
            return Some(nearest_subtest_at_line(&candidate.children, line).unwrap_or(candidate));
        }
    }
    None
}

/// Discover subtests in `ast` and resolve the innermost one enclosing `line`.
pub fn nearest_subtest_in_source(ast: &Node, source: &str, line: u32) -> Option<DiscoveredSubtest> {
    let subtests = discover_subtests(ast, source);
    nearest_subtest_at_line(&subtests, line).cloned()
}

fn to_document_symbol(subtest: &DiscoveredSubtest) -> DocumentSymbol {
    DocumentSymbol {
        name: subtest.name.label(),
        detail: "subtest".to_string(),
        kind: SUBTEST_SYMBOL_KIND,
        range: subtest.range,
        selection_range: subtest.name_range,
        children: subtest.children.iter().map(to_document_symbol).collect(),
        sort_priority: CALLABLE_DOCUMENT_SYMBOL_PRIORITY,
    }
}

/// Walk `node`, appending every subtest found at this level to `out`. When a
/// subtest is found, its own nested subtests are collected into its `children`
/// and the walk does not descend into it again (so nesting stays a tree, not a
/// flattened list).
fn walk(node: &Node, source: &str, out: &mut Vec<DiscoveredSubtest>) {
    if let Some(subtest) = try_as_subtest(node, source) {
        out.push(subtest);
        return;
    }
    for child in structural_children(node) {
        walk(child, source, out);
    }
}

/// Whether `name` is a Test2 subtest-defining call. Shared by the discovery
/// path (this module) and the code-lens path so the two never diverge on which
/// call names are treated as subtests.
pub fn is_subtest_call_name(name: &str) -> bool {
    matches!(name, "subtest" | "subtest_buffered" | "subtest_streamed")
}

/// If `node` is a `subtest NAME => sub { ... }` call, build a
/// [`DiscoveredSubtest`] (with nested children discovered inside its block).
fn try_as_subtest(node: &Node, source: &str) -> Option<DiscoveredSubtest> {
    let NodeKind::FunctionCall { name, args } = &node.kind else {
        return None;
    };
    if !is_subtest_call_name(name) {
        return None;
    }

    let first = args.first()?;
    let name = subtest_name_from_arg(first);
    let name_range =
        WireRange::from_byte_offsets(source, first.location.start(), first.location.end());
    let range = WireRange::from_byte_offsets(source, node.location.start(), node.location.end());

    // Collect nested subtests from the block body of the anonymous sub argument.
    let mut children = Vec::new();
    for arg in args.iter().skip(1) {
        if let NodeKind::Subroutine { body, .. } = &arg.kind {
            walk(body, source, &mut children);
        }
    }

    Some(DiscoveredSubtest { name, range, name_range, children })
}

/// Extract a subtest name from its first argument. A string literal yields a
/// static name (quotes stripped); anything else is dynamic.
fn subtest_name_from_arg(arg: &Node) -> SubtestName {
    match &arg.kind {
        NodeKind::String { value, interpolated } => {
            let unquoted = strip_string_quotes(value);
            // An interpolated string with a variable is only partially static;
            // treat a purely literal (non-interpolated) string as the name and
            // anything interpolated as dynamic to avoid presenting a raw
            // `$var`-laden label as if it were the real name.
            if *interpolated && unquoted.contains('$') {
                SubtestName::Dynamic
            } else {
                SubtestName::Named(unquoted.to_string())
            }
        }
        _ => SubtestName::Dynamic,
    }
}

/// Strip a single layer of matching surrounding quotes from a string token.
/// The parser stores string values including their delimiters (e.g. `'outer'`).
fn strip_string_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'\'' || first == b'"') && first == last {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// The structural child nodes of `node` that a subtest could be nested within.
/// This intentionally covers the node kinds that appear in test files
/// (statements, blocks, calls, control flow) rather than every AST variant.
fn structural_children(node: &Node) -> Vec<&Node> {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            statements.iter().collect()
        }
        NodeKind::ExpressionStatement { expression } => vec![expression.as_ref()],
        NodeKind::FunctionCall { args, .. } => args.iter().collect(),
        NodeKind::Subroutine { body, .. } => vec![body.as_ref()],
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            let mut children: Vec<&Node> = vec![condition.as_ref(), then_branch.as_ref()];
            for (cond, branch) in elsif_branches {
                children.push(cond.as_ref());
                children.push(branch.as_ref());
            }
            if let Some(else_branch) = else_branch {
                children.push(else_branch.as_ref());
            }
            children
        }
        NodeKind::While { condition, body, .. } => vec![condition.as_ref(), body.as_ref()],
        NodeKind::For { condition, body, .. } => {
            let mut children: Vec<&Node> = Vec::new();
            if let Some(condition) = condition {
                children.push(condition.as_ref());
            }
            children.push(body.as_ref());
            children
        }
        NodeKind::Foreach { list, body, .. } => vec![list.as_ref(), body.as_ref()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
