//! Discriminating proof for iterative exact/bounded AST reads (#8867).
//!
//! These tests fail realistic wrong implementations:
//! - silent `MAX_AST_DEPTH` truncation returned as ordinary `usize` / `Some`
//! - last-writer lookup where a later shallow overlap beats a deeper match
//! - a second child-match table that omits a #8424 field
//! - bounded helpers that coerce `Truncated` into `usize` / `Some`
//!
//! They do not cover native debug rendering (`to_sexp` / `render_debug_sexp`,
//! #8832), Clone/Eq/Debug (#6900), or source-index caches (#8044).

use perl_ast::ast::MAX_AST_DEPTH;
use perl_ast::{
    AstReadExact, AstReadLimits, AstReadResult, AstReadTruncation, Node, NodeKind, SourceLocation,
    node_kind_fixtures,
};

const SMALL_STACK_BYTES: usize = 256 * 1024;
const DEEP_DEPTH: usize = 50_000;

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn number_leaf(start: usize, end: usize) -> Node {
    Node::new(NodeKind::Number { value: "1".to_string() }, loc(start, end))
}

fn wrap_expr(inner: Node, span: SourceLocation) -> Node {
    Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, span)
}

fn wrap_unary(inner: Node, span: SourceLocation) -> Node {
    Node::new(NodeKind::Unary { op: "-".to_string(), operand: Box::new(inner) }, span)
}

/// `n` wrappers around one leaf: independently constructed size is `n + 1`.
fn deep_chain(n: usize) -> Node {
    let span = loc(0, 1);
    let mut node = number_leaf(0, 1);
    for _ in 0..n {
        node = wrap_expr(node, span);
    }
    node
}

fn independent_count(node: &Node) -> usize {
    let mut count = 1usize;
    node.for_each_child(|child| {
        count += independent_count(child);
    });
    count
}

fn run_on_small_stack<F>(body: F) -> Result<(), String>
where
    F: FnOnce() + Send + 'static,
{
    let handle = std::thread::Builder::new()
        .stack_size(SMALL_STACK_BYTES)
        .spawn(body)
        .map_err(|error| format!("failed to spawn small-stack worker: {error}"))?;
    handle.join().map_err(|_| "small-stack worker aborted (likely stack overflow)".to_string())
}

#[test]
fn chain_past_legacy_ceiling_returns_exact_count_not_truncated_512() {
    // 513 wrappers + leaf = 514 nodes. The old recursive guard stopped at
    // depth 512 and returned 513 as an ordinary usize.
    let tree = deep_chain(513);
    let expected = 514;
    assert_eq!(tree.count_nodes(), expected);
    match tree.count_nodes_exact() {
        AstReadExact::Complete { value, work } => {
            assert_eq!(value, expected);
            assert_eq!(work.nodes_visited, expected);
            assert_eq!(work.edges_visited, expected - 1);
        }
        other => {
            assert!(
                matches!(other, AstReadExact::Complete { .. }),
                "exact count must complete, got {other:?}"
            );
        }
    }
}

#[test]
fn fifty_thousand_node_count_and_lookup_are_exact_on_small_stack() -> Result<(), String> {
    run_on_small_stack(|| {
        let expected = DEEP_DEPTH + 1;
        let tree = deep_chain(DEEP_DEPTH);
        assert_eq!(tree.count_nodes(), expected, "50k chain count must match builder size");
        assert_eq!(
            tree.find_deepest_containing_offset(0).map(|node| node.kind.kind_name()),
            Some("Number"),
            "50k lookup must reach the leaf below depth 512"
        );
        drop(tree);
    })
}

#[test]
fn later_shallow_overlap_cannot_beat_deeper_match() {
    let span = loc(0, 10);
    let mut deep = number_leaf(0, 10);
    for _ in 0..900 {
        deep = wrap_unary(deep, span);
    }
    // Root + 900 unaries + leaf = depth 901 at the Number.
    let shallow = Node::new(NodeKind::Identifier { name: "later".to_string() }, span);
    let program = Node::new(NodeKind::Program { statements: vec![deep, shallow] }, span);

    let found = program.find_deepest_containing_offset(5);
    assert!(found.is_some(), "offset 5 is inside 0..10");
    assert_eq!(found.map(|node| node.kind.kind_name()), Some("Number"));

    match program.find_deepest_containing_offset_exact(5) {
        AstReadExact::Complete { value: Some(matched), work } => {
            assert_eq!(matched.node.kind.kind_name(), "Number");
            assert_eq!(matched.depth, 901);
            assert!(
                matched.path.to_diagnostic_string("Program").contains("operand"),
                "path must retain #8424 field identity, got {}",
                matched.path.to_diagnostic_string("Program")
            );
            // Work must record the later overlapping sibling, not just depth+1.
            assert!(
                work.nodes_visited > matched.depth + 1,
                "work reconstructed from match depth ({}) would miss the later sibling; visited {}",
                matched.depth,
                work.nodes_visited
            );
        }
        other => {
            assert!(
                matches!(other, AstReadExact::Complete { value: Some(_), .. }),
                "expected complete Number match, got {other:?}"
            );
        }
    }
}

#[test]
fn equal_depth_overlap_keeps_earliest_canonical_path() {
    let span = loc(0, 10);
    let first = number_leaf(0, 10);
    let second = Node::new(NodeKind::Identifier { name: "later".to_string() }, span);
    let program = Node::new(NodeKind::Program { statements: vec![first, second] }, span);
    let found = program.find_deepest_containing_offset(4);
    assert_eq!(
        found.map(|node| node.kind.kind_name()),
        Some("Number"),
        "last-writer lookup would return the later Identifier"
    );

    let swapped = Node::new(
        NodeKind::Program {
            statements: vec![
                Node::new(NodeKind::Identifier { name: "later".to_string() }, span),
                number_leaf(0, 10),
            ],
        },
        span,
    );
    let found = swapped.find_deepest_containing_offset(4);
    assert!(found.is_some(), "contained");
    assert_eq!(
        found.map(|node| node.kind.kind_name()),
        Some("Identifier"),
        "canonical tie follows visit-table order, not node kind"
    );
}

#[test]
fn omitted_optional_field_fails_representative_exact_counts() {
    for fixture in node_kind_fixtures() {
        let expected = independent_count(&fixture.sample);
        assert_eq!(
            fixture.sample.count_nodes(),
            expected,
            "{}: cursor must visit every #8424 child; omitting one optional/repeated field must fail",
            fixture.sample.kind.kind_name()
        );
    }
}

#[test]
fn bounded_count_must_not_return_ordinary_usize_after_truncation() {
    let tree = deep_chain(513);
    let exact = tree.count_nodes();
    assert_eq!(exact, 514);

    match tree.count_nodes_bounded(AstReadLimits::max_depth(MAX_AST_DEPTH)) {
        AstReadResult::Truncated { reason, partial, work } => {
            assert!(
                matches!(reason, AstReadTruncation::DepthLimit { limit } if limit == MAX_AST_DEPTH)
            );
            assert!(partial < exact, "truncated partial {partial} must not equal exact {exact}");
            assert_eq!(work.nodes_visited, partial);
            assert_ne!(
                exact, partial,
                "a wrapper that returns Truncated.partial as usize preserves the defect"
            );
        }
        other => {
            assert!(
                matches!(other, AstReadResult::Truncated { .. }),
                "bounded depth-512 walk on a 513-deep chain must be Truncated, got {other:?}"
            );
        }
    }
}

#[test]
fn bounded_lookup_must_not_return_ordinary_some_after_truncation() {
    let tree = deep_chain(600);
    assert_eq!(
        tree.find_deepest_containing_offset(0).map(|node| node.kind.kind_name()),
        Some("Number"),
        "exact wrapper must still reach the leaf"
    );

    match tree.find_deepest_containing_offset_bounded(0, AstReadLimits::max_depth(MAX_AST_DEPTH)) {
        AstReadResult::Truncated { partial, .. } => {
            let kind = partial.map(|matched| matched.node.kind.kind_name());
            assert_ne!(
                kind,
                Some("Number"),
                "truncated lookup must not present the leaf as a complete Some"
            );
            assert_eq!(kind, Some("ExpressionStatement"));
        }
        other => {
            assert!(
                matches!(other, AstReadResult::Truncated { .. }),
                "bounded lookup must be Truncated, got {other:?}"
            );
        }
    }
}

#[test]
fn half_open_containment_and_zero_width_are_preserved() {
    let parent = Node::new(
        NodeKind::Program {
            statements: vec![
                Node::new(NodeKind::Identifier { name: "a".to_string() }, loc(0, 4)),
                Node::new(NodeKind::MissingExpression, loc(4, 4)),
                Node::new(NodeKind::Number { value: "1".to_string() }, loc(4, 5)),
            ],
        },
        loc(0, 5),
    );

    assert_eq!(
        parent.find_deepest_containing_offset(4).map(|node| node.kind.kind_name()),
        Some("Number"),
        "offset == start is included; zero-width MissingExpression must not contain 4"
    );
    assert_eq!(parent.find_deepest_containing_offset(5), None, "end is exclusive");
    assert_eq!(
        parent.find_deepest_containing_offset(0).map(|node| node.kind.kind_name()),
        Some("Identifier")
    );
}

#[test]
fn unicode_byte_offsets_stay_half_open() {
    // "é" is two UTF-8 bytes. Containment is byte-based, not scalar-based.
    let node =
        Node::new(NodeKind::String { value: "é".to_string(), interpolated: false }, loc(0, 2));
    assert!(node.contains_offset(0));
    assert!(node.contains_offset(1));
    assert!(!node.contains_offset(2));
    assert_eq!(node.find_deepest_containing_offset(1).map(|n| n.kind.kind_name()), Some("String"));
    assert_eq!(node.find_deepest_containing_offset(2), None);
}

#[test]
fn nested_and_concurrent_walks_do_not_share_state() {
    let tree = deep_chain(64);
    tree.for_each_child(|child| {
        assert_eq!(child.count_nodes(), 64);
    });
    assert_eq!(tree.count_nodes(), 65);

    std::thread::scope(|scope| {
        scope.spawn(|| assert_eq!(tree.count_nodes(), 65));
        scope.spawn(|| {
            assert_eq!(
                tree.find_deepest_containing_offset(0).map(|node| node.kind.kind_name()),
                Some("Number")
            );
        });
    });
}

#[test]
fn wide_program_count_is_linear_in_statement_count() {
    const WIDTH: usize = 8_192;
    let statements: Vec<Node> = (0..WIDTH).map(|i| number_leaf(i, i + 1)).collect();
    let program = Node::new(NodeKind::Program { statements }, loc(0, WIDTH));
    let expected = WIDTH + 1;
    match program.count_nodes_exact() {
        AstReadExact::Complete { value, work } => {
            assert_eq!(value, expected, "wide Program count must equal statements + root");
            assert_eq!(work.edges_visited, WIDTH);
        }
        other => {
            assert!(
                matches!(other, AstReadExact::Complete { .. }),
                "wide Program exact count must complete, got {other:?}"
            );
        }
    }
    assert_eq!(
        program.find_deepest_containing_offset(WIDTH - 1).map(|node| node.kind.kind_name()),
        Some("Number"),
        "lookup into the last statement must not quadratic-walk earlier siblings to death"
    );
}

#[test]
fn child_outside_root_span_cannot_match() {
    let child = number_leaf(5, 8);
    let root = Node::new(NodeKind::Program { statements: vec![child] }, loc(0, 3));
    assert_eq!(
        root.find_deepest_containing_offset(5),
        None,
        "a recovery child outside the root span must not win at an offset the root does not contain"
    );
    match root.find_deepest_containing_offset_exact(5) {
        AstReadExact::Complete { value: None, work } => {
            assert_eq!(work.nodes_visited, 0);
            assert_eq!(work.edges_visited, 0);
        }
        other => {
            assert!(
                matches!(other, AstReadExact::Complete { value: None, .. }),
                "expected complete None without descending, got {other:?}"
            );
        }
    }
}

#[test]
fn bounded_node_limit_is_independent_of_depth_limit() {
    let tree = deep_chain(8);
    match tree.count_nodes_bounded(AstReadLimits::max_nodes(3)) {
        AstReadResult::Truncated { reason, partial, work } => {
            assert!(matches!(reason, AstReadTruncation::NodeLimit { limit: 3 }));
            assert_eq!(partial, 3);
            assert_eq!(work.nodes_visited, 3);
        }
        other => {
            assert!(
                matches!(other, AstReadResult::Truncated { .. }),
                "expected node-limit truncation, got {other:?}"
            );
        }
    }
}
