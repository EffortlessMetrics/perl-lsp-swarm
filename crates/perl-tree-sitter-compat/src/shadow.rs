//! Best-effort shadow comparison against the Rust-native tree-sitter facade.

use perl_parser_core::Node;
use std::sync::atomic::{AtomicU64, Ordering};
use tree_sitter_perl_rs::Parser;

static RUNS: AtomicU64 = AtomicU64::new(0);
static FACADE_TREES: AtomicU64 = AtomicU64::new(0);
static MATCHES: AtomicU64 = AtomicU64::new(0);
static FALLBACKS: AtomicU64 = AtomicU64::new(0);

/// Result of one non-authoritative facade shadow comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShadowComparison {
    /// Whether the facade returned a tree.
    pub facade_tree: bool,
    /// Whether the facade root span matches the established AST.
    pub root_span_match: bool,
    /// Whether both trees contain the same number of nodes.
    pub node_count_match: bool,
    /// Whether both trees render the same S-expression.
    pub sexp_match: bool,
}

/// Aggregate shadow-run counters for the current process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShadowStats {
    /// Number of shadow parses attempted.
    pub runs: u64,
    /// Number of facade trees returned.
    pub facade_trees: u64,
    /// Number of comparisons whose checked facts matched.
    pub matches: u64,
    /// Number of times the established path was retained because the facade
    /// did not produce a matching result.
    pub fallbacks: u64,
}

/// Return aggregate shadow-run counters.
#[must_use]
pub fn shadow_stats() -> ShadowStats {
    ShadowStats {
        runs: RUNS.load(Ordering::Relaxed),
        facade_trees: FACADE_TREES.load(Ordering::Relaxed),
        matches: MATCHES.load(Ordering::Relaxed),
        fallbacks: FALLBACKS.load(Ordering::Relaxed),
    }
}

/// Compare the established AST with the facade without changing the caller's
/// authoritative result.
pub(crate) fn compare(source: &str, native_root: &Node) -> ShadowComparison {
    RUNS.fetch_add(1, Ordering::Relaxed);
    let mut parser = Parser::new();
    let Some(tree) = parser.parse(source) else {
        FALLBACKS.fetch_add(1, Ordering::Relaxed);
        return ShadowComparison {
            facade_tree: false,
            root_span_match: false,
            node_count_match: false,
            sexp_match: false,
        };
    };

    FACADE_TREES.fetch_add(1, Ordering::Relaxed);
    let root = tree.root_node();
    let root_span_match = root.start_byte() == native_root.location.start
        && root.end_byte() == native_root.location.end.min(source.len());
    let node_count_match = count_facade_nodes(root) == native_root.count_nodes();
    let sexp_match = root.to_sexp() == native_root.to_sexp();
    let comparison =
        ShadowComparison { facade_tree: true, root_span_match, node_count_match, sexp_match };
    if !comparison.root_span_match || !comparison.node_count_match || !comparison.sexp_match {
        FALLBACKS.fetch_add(1, Ordering::Relaxed);
    } else {
        MATCHES.fetch_add(1, Ordering::Relaxed);
    }
    comparison
}

fn count_facade_nodes(node: tree_sitter_perl_rs::Node<'_>) -> usize {
    1 + node.children().map(count_facade_nodes).sum::<usize>()
}
