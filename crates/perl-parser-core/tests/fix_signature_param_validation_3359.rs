/// Tests for signature parameter ordering and rule validation (issue #3359).
///
/// Perl 5.20+ signatures enforce ordering rules:
/// 1. Slurpy parameters (`@array` or `%hash`) must come last.
/// 2. Can't have both `@` and `%` slurpy parameters.
/// 3. A mandatory parameter cannot follow an optional parameter.
///
/// The parser should emit diagnostics for these violations while still
/// producing a usable AST (error-recovery mode).
use perl_parser_core::{Node, NodeKind, Parser};

/// Helper: parse and return diagnostic messages as a Vec<String>
fn parse_and_collect_errors(source: &str) -> Vec<String> {
    let mut parser = Parser::new(source);
    let _ast = parser.parse().ok();
    parser.errors().iter().map(|e| e.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Rule 1: Slurpy parameter must be last
// ---------------------------------------------------------------------------

#[test]
fn test_slurpy_not_last_emits_diagnostic() {
    // @rest must come after $x — ordering violation
    let errors = parse_and_collect_errors("sub foo (@rest, $x) { }");
    assert!(
        errors.iter().any(|e| e.to_lowercase().contains("slurpy")),
        "Expected a 'slurpy' diagnostic for @rest before $x, got: {:?}",
        errors
    );
}

#[test]
fn test_slurpy_hash_not_last_emits_diagnostic() {
    // %opts must come after $x — ordering violation
    let errors = parse_and_collect_errors("sub bar (%opts, $x) { }");
    assert!(
        errors.iter().any(|e| e.to_lowercase().contains("slurpy")),
        "Expected a 'slurpy' diagnostic for %opts before $x, got: {:?}",
        errors
    );
}

#[test]
fn test_slurpy_last_is_valid() {
    // @rest at end is correct; should produce no slurpy diagnostic
    let errors = parse_and_collect_errors("sub foo ($x, $y, @rest) { }");
    assert!(
        !errors.iter().any(|e| e.to_lowercase().contains("slurpy")),
        "Unexpected slurpy diagnostic for valid signature, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// Rule 2: Can't have both @ and % slurpy parameters
// ---------------------------------------------------------------------------

#[test]
fn test_both_array_and_hash_slurpy_emits_diagnostic() {
    // @arr and %hash — can't have both slurpy types
    let errors = parse_and_collect_errors("sub bar (@arr, %hash) { }");
    assert!(
        errors.iter().any(|e| {
            let lower = e.to_lowercase();
            lower.contains("slurpy") || lower.contains("multiple")
        }),
        "Expected a diagnostic for both @ and % slurpy params, got: {:?}",
        errors
    );
}

#[test]
fn test_single_array_slurpy_is_valid() {
    let errors = parse_and_collect_errors("sub foo ($x, @rest) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for valid single-slurpy signature, got: {:?}",
        errors
    );
}

#[test]
fn test_single_hash_slurpy_is_valid() {
    let errors = parse_and_collect_errors("sub foo ($x, %opts) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for valid single-slurpy signature, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// Rule 3: Mandatory parameter cannot follow optional parameter
// ---------------------------------------------------------------------------

#[test]
fn test_mandatory_after_optional_emits_diagnostic() {
    // $x has default, $y is mandatory — invalid ordering
    let errors = parse_and_collect_errors("sub baz ($x = 1, $y) { }");
    assert!(
        errors.iter().any(|e| {
            let lower = e.to_lowercase();
            lower.contains("mandatory") || lower.contains("optional") || lower.contains("required")
        }),
        "Expected a diagnostic for mandatory after optional, got: {:?}",
        errors
    );
}

#[test]
fn test_optional_before_optional_is_valid() {
    // Both optional — should be fine
    let errors = parse_and_collect_errors("sub foo ($x = 1, $y = 2) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for all-optional signature, got: {:?}",
        errors
    );
}

#[test]
fn test_mandatory_before_optional_is_valid() {
    // Mandatory then optional — valid
    let errors = parse_and_collect_errors("sub foo ($x, $y = 2) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for mandatory-then-optional signature, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// Combined violations
// ---------------------------------------------------------------------------

#[test]
fn test_slurpy_not_last_with_optional_before_mandatory() {
    // Multiple violations: @rest is not last AND $y is mandatory after $x = 1
    let errors = parse_and_collect_errors("sub multi (@rest, $x = 1, $y) { }");
    assert!(
        !errors.is_empty(),
        "Expected at least one diagnostic for multiple violations, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// Regression: valid signatures produce no spurious diagnostics
// ---------------------------------------------------------------------------

#[test]
fn test_all_mandatory_no_diagnostics() {
    let errors = parse_and_collect_errors("sub foo ($a, $b, $c) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for all-mandatory signature, got: {:?}",
        errors
    );
}

#[test]
fn test_empty_signature_no_diagnostics() {
    let errors = parse_and_collect_errors("sub foo () { }");
    assert!(errors.is_empty(), "Unexpected diagnostics for empty signature, got: {:?}", errors);
}

#[test]
fn test_method_invocant_separator_is_accepted() {
    let errors = parse_and_collect_errors("method run ($self: $arg) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for valid invocant signature, got: {:?}",
        errors
    );
}

#[test]
fn test_sub_signature_invocant_separator_is_accepted() {
    let errors = parse_and_collect_errors("sub run ($class: $arg, $opt = 1) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for valid invocant separator in sub signature, got: {:?}",
        errors
    );
}

#[test]
fn test_invocant_only_signature_is_accepted() {
    // Invocant with no additional params — edge case for the guard reset.
    let errors = parse_and_collect_errors("method run ($self:) { }");
    assert!(
        errors.is_empty(),
        "Unexpected diagnostics for invocant-only signature, got: {:?}",
        errors
    );
}

#[test]
fn test_double_invocant_separator_is_rejected() {
    // A second `:` in the same signature is a syntax error — the flag must block it.
    let errors = parse_and_collect_errors("sub bad ($a: $b: $c) { }");
    assert!(
        !errors.is_empty(),
        "Expected a syntax error for double invocant separator, got no diagnostics",
    );
}

#[test]
fn test_signature_param_attributes_parse_without_signature_separator_error() {
    let errors = parse_and_collect_errors("sub build ($x :param, $y :reader(name)) { }");
    assert!(
        !errors.iter().any(|e| e.contains("Expected comma or closing parenthesis in signature")),
        "parameter attributes must not trip signature separator recovery, got: {:?}",
        errors
    );
}

#[test]
fn test_typed_parameter_after_invocant_separator() -> Result<(), String> {
    fn signature(node: &Node) -> Option<&Node> {
        if matches!(node.kind, NodeKind::Signature { .. }) {
            return Some(node);
        }
        node.children().into_iter().find_map(signature)
    }

    for source in [
        "method run ($self: Str $arg) { }",
        "method run ($self: Str $arg, $other) { }",
        "method run ($self :param, Str $arg) { }",
    ] {
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|e| format!("{source}: {e}"))?;
        assert!(parser.errors().is_empty(), "{source}: {:?}", parser.errors());
        let method = ast
            .children()
            .into_iter()
            .find(|node| matches!(node.kind, NodeKind::Method { .. }))
            .ok_or_else(|| format!("missing method: {source}"))?;
        let sig = signature(method).ok_or_else(|| format!("missing signature: {source}"))?;
        let NodeKind::Signature { parameters } = &sig.kind else { unreachable!() };
        let names: Vec<_> = parameters
            .iter()
            .filter_map(|param| match &param.kind {
                NodeKind::MandatoryParameter { variable } => match &variable.kind {
                    NodeKind::Variable { name, .. } => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let expected = if source.contains("$other") {
            vec!["self", "arg", "other"]
        } else {
            vec!["self", "arg"]
        };
        assert_eq!(names, expected, "{source}");
        if source.contains(":param") {
            let first = &parameters[0].location;
            assert_eq!(&source[first.start..first.end], "$self :param");
        }
    }
    Ok(())
}

#[test]
fn test_second_invocant_separator_before_typed_parameter_is_rejected() {
    let source = "method run ($self: $arg: Str $other) { }";
    let errors = parse_and_collect_errors(source);
    assert!(
        errors.iter().any(|error| error.contains("Expected comma or closing parenthesis")),
        "second separator should be rejected for {source}: {errors:?}"
    );
}
