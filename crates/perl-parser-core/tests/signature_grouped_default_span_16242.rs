//! Tests for issue #16242 (follow-up to merged #16204 under #8915):
//! `parse_signature_param` derives the parameter/invalid-default range end
//! from `default.location.end` after `parse_ternary`. A grouped scalar
//! expression like `(1 + 2)` returns the inner expression span while the
//! parser has consumed the closing `)`, so the parameter's stored end
//! stops short of the consumed default extent.
//!
//! Before the fix, the `OptionalParameter` / `NamedParameter` covering
//! `($x = (1 + 2), $y)` ends at the end of `1 + 2` (before the closing
//! `)`), which breaks downstream consumers that expect the consumed
//! default extent to include the grouping delimiter.
//!
//! The fix derives the end from `max(default.location.end, parser
//! consumed-token endpoint)` so the parameter covers the full consumed
//! default including any closing grouping delimiter, without changing
//! the inner expression's own canonical range contract.

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

/// Slice `source[start..end]` with a contextual error that names the
/// expected substring and the offending span.
fn slice_or_fail<'a>(
    source: &'a str,
    span: std::ops::Range<usize>,
    expected: &str,
    context: &str,
) -> Result<&'a str, String> {
    source
        .get(span.clone())
        .ok_or_else(|| {
            format!(
                "{}: span {}..{} out of bounds for source len {} (source: {:?})",
                context,
                span.start,
                span.end,
                source.len(),
                source,
            )
        })
        .map(|s| {
            assert_eq!(
                s, expected,
                "{}: expected {:?} but got {:?} (span {}..{})",
                context, expected, s, span.start, span.end
            );
            s
        })
}

// ------------------------------------------------------------------
// Bug case: grouped scalar default extends past the inner expression.
// ------------------------------------------------------------------

/// `sub f($x = (1+2), $y) {}` — the `OptionalParameter` covering `$x`
/// must extend through the closing `)` of the grouped default. Before
/// the fix, its end lies before the `)`, so the span slices
/// `$x = (1+2` (missing the closing paren).
#[test]
fn optional_parameter_span_covers_grouped_default_closer() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "sub f($x = (1+2), $y) {}";
    let ast = parse(source);

    let opt = find_node_by_kind(&ast, "OptionalParameter")
        .ok_or("no OptionalParameter node found for ($x = (1+2), $y)")?;

    slice_or_fail(
        source,
        opt.location.start..opt.location.end,
        "$x = (1+2)",
        "OptionalParameter for grouped scalar default",
    )?;
    Ok(())
}

/// Named-parameter counterpart: `sub f(:$x = (1+2), :$y) {}`. The
/// `NamedParameter` covering `:$x` must extend through the closing
/// `)` of the grouped default.
#[test]
fn named_parameter_span_covers_grouped_default_closer() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f(:$x = (1+2), :$y) {}";
    let ast = parse(source);

    let named = find_node_by_kind(&ast, "NamedParameter").ok_or("no NamedParameter node found")?;

    // The NamedParameter must cover `:$x = (1+2)` — including the
    // closing `)` of the grouped default.
    slice_or_fail(
        source,
        named.location.start..named.location.end,
        ":$x = (1+2)",
        "NamedParameter for grouped scalar default",
    )?;
    Ok(())
}

// ------------------------------------------------------------------
// Bug case: nested grouping must preserve the outermost consumed end.
// ------------------------------------------------------------------

/// Nested grouped default: `sub f($x = ((1+2)), $y) {}`. The
/// `OptionalParameter` must cover `$x = ((1+2))` (two closing parens).
#[test]
fn optional_parameter_span_covers_nested_grouped_default_closers()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f($x = ((1+2)), $y) {}";
    let ast = parse(source);

    let opt = find_node_by_kind(&ast, "OptionalParameter")
        .ok_or("no OptionalParameter node found for ($x = ((1+2)), $y)")?;

    slice_or_fail(
        source,
        opt.location.start..opt.location.end,
        "$x = ((1+2))",
        "OptionalParameter for nested grouped scalar default",
    )?;
    Ok(())
}

// ------------------------------------------------------------------
// Bug case: following parameter still appears after the default.
// ------------------------------------------------------------------

/// Suffix/body sentinel: `sub f($x = (1+2), $y, $z) {}`. Both the
/// trailing parameters and the body sentinel must still parse after
/// the grouped default's closing `)`.
#[test]
fn signature_with_grouped_default_preserves_following_parameters_and_body()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f($x = (1+2), $y, $z) { BODY }";
    let ast = parse(source);

    // Collect every parameter node in source order. The grouped default
    // must end before `$y` so the parser does not absorb the trailing
    // parameters into the first parameter's span.
    let mut optionals = Vec::new();
    let mut mandatories = Vec::new();
    fn walk<'a>(node: &'a Node, optionals: &mut Vec<&'a Node>, mandatories: &mut Vec<&'a Node>) {
        match &node.kind {
            perl_parser_core::NodeKind::OptionalParameter { .. } => optionals.push(node),
            perl_parser_core::NodeKind::MandatoryParameter { .. } => mandatories.push(node),
            _ => {}
        }
        node.for_each_child(|c| walk(c, optionals, mandatories));
    }
    walk(&ast, &mut optionals, &mut mandatories);

    assert_eq!(
        optionals.len(),
        1,
        "exactly one OptionalParameter for ($x = (1+2), $y, $z); got {}",
        optionals.len()
    );
    assert_eq!(
        mandatories.len(),
        2,
        "two MandatoryParameter nodes for ($y, $z); got {}",
        mandatories.len()
    );

    // The OptionalParameter must not extend past its own default's
    // closing `)`. In particular, it must not absorb `$y` or `$z`.
    let opt = optionals[0];
    let opt_text = source.get(opt.location.start..opt.location.end).ok_or_else(|| {
        format!(
            "OptionalParameter span {}..{} out of bounds (source len {})",
            opt.location.start,
            opt.location.end,
            source.len()
        )
    })?;
    assert!(opt_text.starts_with("$x"), "OptionalParameter must start at $x; got {:?}", opt_text);
    assert!(
        !opt_text.contains("$y") && !opt_text.contains("$z"),
        "OptionalParameter absorbed trailing params; got {:?}",
        opt_text
    );

    // The body sentinel must still appear in the parsed source.
    assert!(
        source.contains("BODY"),
        "source must still contain BODY sentinel after grouped default"
    );
    Ok(())
}

// ------------------------------------------------------------------
// Control: a non-grouped default must keep its prior (unchanged) span.
// ------------------------------------------------------------------

/// Regression control: a literal default `$x = 1` already had its end
/// at the end of `1`. The fix must not change that span.
#[test]
fn optional_parameter_span_unchanged_for_literal_default() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "sub f($x = 1, $y) {}";
    let ast = parse(source);

    let opt = find_node_by_kind(&ast, "OptionalParameter")
        .ok_or("no OptionalParameter for ($x = 1, $y)")?;

    slice_or_fail(
        source,
        opt.location.start..opt.location.end,
        "$x = 1",
        "OptionalParameter for literal default (regression control)",
    )?;
    Ok(())
}

/// Regression control: a named parameter with a literal default must
/// keep its prior (unchanged) span.
#[test]
fn named_parameter_span_unchanged_for_literal_default() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f(:$x = 1, :$y) {}";
    let ast = parse(source);

    let named =
        find_node_by_kind(&ast, "NamedParameter").ok_or("no NamedParameter for (:$x = 1, :$y)")?;

    slice_or_fail(
        source,
        named.location.start..named.location.end,
        ":$x = 1",
        "NamedParameter for literal default (regression control)",
    )?;
    Ok(())
}

/// Regression control: a kept-default signature (no default at all)
/// keeps its prior (unchanged) span — the fix must not change spans
/// when there is no default.
#[test]
fn mandatory_parameter_span_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub f($x, $y) {}";
    let ast = parse(source);

    let mand = find_node_by_kind(&ast, "MandatoryParameter")
        .ok_or("no MandatoryParameter for ($x, $y)")?;

    slice_or_fail(
        source,
        mand.location.start..mand.location.end,
        "$x",
        "MandatoryParameter (no-default regression control)",
    )?;
    Ok(())
}
