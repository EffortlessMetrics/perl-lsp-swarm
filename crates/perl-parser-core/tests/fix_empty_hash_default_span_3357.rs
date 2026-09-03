/// Regression tests for issue #3357: signature parameter with an empty anonymous
/// hash default `{}` produced a reversed byte span (`start > end`).
///
/// Root cause: `parse_hash_or_block_inner` consumed the closing `}` via raw
/// `self.tokens.next()` (which does NOT update `last_end_position`), then
/// called `self.previous_position()` to obtain `end`.  In expression context
/// the `{` was itself consumed the same way, so `last_end_position` still
/// pointed at the end of whichever token preceded `{` (e.g. `$opts` or `=`).
/// When that stale position was before the `{`, `start > end` → reversed span.
///
/// Fix: capture the `}` token and use its `.end` field directly.
mod cpan_test_helpers;
use cpan_test_helpers::parse;
use perl_parser_core::{Node, NodeKind};

/// Walk the AST; return the first node matching `pred`.
fn find_node<'a, F>(node: &'a Node, pred: &F) -> Option<&'a Node>
where
    F: Fn(&Node) -> bool,
{
    if pred(node) {
        return Some(node);
    }
    for child in node.children() {
        if let Some(found) = find_node(child, pred) {
            return Some(found);
        }
    }
    None
}

/// Assert span is well-formed: start ≤ end and both within source bounds.
fn assert_span_valid(node: &Node, source: &str, label: &str) {
    let loc = &node.location;
    assert!(
        loc.start() <= loc.end(),
        "{label}: span is REVERSED — start({}) > end({})",
        loc.start(),
        loc.end(),
    );
    assert!(
        loc.end() <= source.len(),
        "{label}: end({}) exceeds source length({})",
        loc.end(),
        source.len(),
    );
}

// ── empty {} with plain `=` ──────────────────────────────────────────────────

#[test]
fn optional_param_empty_hash_default_span_is_not_reversed() -> Result<(), Box<dyn std::error::Error>>
{
    let source = r#"sub f ($opts = {}) { }"#;
    let ast = parse(source);

    let default_node = find_node(
        &ast,
        &|n| matches!(&n.kind, NodeKind::HashLiteral { pairs } if pairs.is_empty()),
    );

    let node =
        default_node.ok_or("should find an empty HashLiteral node for the `{}` default value")?;

    assert_span_valid(node, source, "empty HashLiteral default in `sub f ($opts = {})`");
    Ok(())
}

#[test]
fn optional_param_empty_hash_default_span_covers_braces() -> Result<(), Box<dyn std::error::Error>>
{
    // The empty `{}` is at bytes 15..17 in `sub f ($opts = {}) { }`
    // (0-indexed: s=0, u=1, b=2, ' '=3, f=4, ' '=5, (=6, $=7, o=8, p=9, t=10, s=11,
    //  ' '=12, ==13, ' '=14, {=15, }=16, )=17, ' '=18, {=19, ' '=20, }=21)
    let source = "sub f ($opts = {}) { }";
    let ast = parse(source);

    let default_node = find_node(
        &ast,
        &|n| matches!(&n.kind, NodeKind::HashLiteral { pairs } if pairs.is_empty()),
    )
    .ok_or("should find an empty HashLiteral for the `{}` default value")?;

    let loc = &default_node.location;

    // Slice the source using the span — must not panic (no reversed/out-of-bounds).
    let sliced = source
        .get(loc.start()..loc.end())
        .ok_or_else(|| format!("span {}..{} is out of bounds for source", loc.start(), loc.end()))?;

    assert_eq!(sliced, "{}", "the HashLiteral span should cover exactly `{{}}`");
    Ok(())
}

// ── empty {} in a named parameter with `=` ───────────────────────────────────

#[test]
fn named_param_empty_hash_default_span_is_not_reversed() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"sub f (:$rest = {}) { }"#;
    let ast = parse(source);

    let default_node = find_node(
        &ast,
        &|n| matches!(&n.kind, NodeKind::HashLiteral { pairs } if pairs.is_empty()),
    );

    let node = default_node
        .ok_or("should find an empty HashLiteral node for named param `{}` default value")?;

    assert_span_valid(node, source, "empty HashLiteral default in `sub f (:$rest = {})`");
    Ok(())
}

// ── non-empty hash default is still well-formed ──────────────────────────────

#[test]
fn optional_param_nonempty_hash_default_span_is_not_reversed()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"sub f ($opts = {key => 1}) { }"#;
    let ast = parse(source);

    // Find any HashLiteral that has at least one pair (the default expression).
    let default_node = find_node(
        &ast,
        &|n| matches!(&n.kind, NodeKind::HashLiteral { pairs } if !pairs.is_empty()),
    );

    let node =
        default_node.ok_or("should find a non-empty HashLiteral node for the default value")?;
    assert_span_valid(
        node,
        source,
        "non-empty HashLiteral default in `sub f ($opts = {key => 1})`",
    );
    Ok(())
}

// ── empty {} outside signature context is still valid ────────────────────────

#[test]
fn standalone_empty_hash_literal_span_is_not_reversed() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"my $h = {};"#;
    let ast = parse(source);

    let hash_node = find_node(
        &ast,
        &|n| matches!(&n.kind, NodeKind::HashLiteral { pairs } if pairs.is_empty()),
    )
    .ok_or("should find an empty HashLiteral in `my $h = {}`")?;

    assert_span_valid(hash_node, source, "empty HashLiteral in `my $h = {}`");

    let sliced = source
        .get(hash_node.location.start()..hash_node.location.end())
        .ok_or("span should be in bounds")?;
    assert_eq!(sliced, "{}", "standalone empty hash span should cover `{{}}`");
    Ok(())
}

// ── guard: plain default value still works (no regression) ───────────────────

#[test]
fn optional_param_scalar_default_span_is_valid() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"sub f ($x = 42) { $x }"#;
    let ast = parse(source);

    let num_node = find_node(&ast, &|n| matches!(&n.kind, NodeKind::Number { .. }))
        .ok_or("should find a Number node for the `42` default")?;

    assert_span_valid(num_node, source, "Number default in `sub f ($x = 42)`");
    Ok(())
}
