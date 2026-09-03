/// Tests for issue #4861: a bare valueless `return;` must get a well-formed
/// (non-inverted) source range covering at least the `return` keyword.
///
/// Before the fix, `parse_return` / `parse_return_expr` consumed the `return`
/// token via `self.tokens.next()` directly (bypassing `consume_token`), so
/// `last_end_position` was never advanced past `return`. When the return had no
/// value, `end = self.previous_position()` returned the end of the *preceding*
/// token, producing an inverted span such as `[8, 7)` for `sub f { return; }`.
mod cpan_test_helpers;
use cpan_test_helpers::parse;
use perl_parser_core::Node;

/// Walk the AST and return the first node whose kind_name matches the given name.
fn find_node_by_kind<'a>(node: &'a Node, target: &str) -> Option<&'a Node> {
    if node.kind.kind_name() == target {
        return Some(node);
    }
    for child in node.children() {
        if let Some(found) = find_node_by_kind(child, target) {
            return Some(found);
        }
    }
    None
}

fn return_node(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let ast = parse(source);
    let node = find_node_by_kind(&ast, "Return")
        .ok_or_else(|| format!("no Return node found in AST for source: {source}"))?;
    Ok(node.clone())
}

/// The core bug: a valueless `return;` must not produce an inverted span.
#[test]
fn test_valueless_return_span_is_wellformed() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f { return; }";
    let ret = return_node(source)?;

    assert!(
        ret.location.end() >= ret.location.start(),
        "valueless return span must not be inverted (got {}..{})",
        ret.location.start(),
        ret.location.end()
    );

    let sliced = source.get(ret.location.start()..ret.location.end()).ok_or_else(|| {
        format!(
            "span {}..{} out of bounds for source len {}",
            ret.location.start(),
            ret.location.end(),
            source.len()
        )
    })?;
    assert_eq!(
        sliced, "return",
        "valueless return span should cover the `return` keyword, got {sliced:?}"
    );
    Ok(())
}

/// The value form must be unchanged: it still spans keyword through operand.
#[test]
fn test_value_return_span_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f { return $x; }";
    let ret = return_node(source)?;

    let sliced = source.get(ret.location.start()..ret.location.end()).ok_or_else(|| {
        format!("span {}..{} out of bounds", ret.location.start(), ret.location.end())
    })?;
    assert_eq!(
        sliced, "return $x",
        "value-bearing return span should cover keyword through operand, got {sliced:?}"
    );
    Ok(())
}

/// A top-level bare `return;` (no enclosing sub) must also be well-formed.
#[test]
fn test_toplevel_valueless_return_span() -> Result<(), Box<dyn std::error::Error>> {
    let source = "return;";
    let ret = return_node(source)?;

    assert!(
        ret.location.end() >= ret.location.start(),
        "top-level valueless return span must not be inverted (got {}..{})",
        ret.location.start(),
        ret.location.end()
    );
    let sliced = source.get(ret.location.start()..ret.location.end()).ok_or("span out of bounds")?;
    assert_eq!(sliced, "return", "top-level return span should cover the keyword, got {sliced:?}");
    Ok(())
}

/// A valueless `return` reached through the expression-context parser
/// (`parse_return_expr`, here via a ternary branch) must also be well-formed.
#[test]
fn test_expression_context_valueless_return_span() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f { $ok ? return : 0; }";
    let ret = return_node(source)?;

    assert!(
        ret.location.end() >= ret.location.start(),
        "expression-context valueless return span must not be inverted (got {}..{})",
        ret.location.start(),
        ret.location.end()
    );
    let sliced = source.get(ret.location.start()..ret.location.end()).ok_or("span out of bounds")?;
    assert_eq!(
        sliced, "return",
        "expression-context return span should cover the keyword, got {sliced:?}"
    );
    Ok(())
}
