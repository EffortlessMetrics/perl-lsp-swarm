//! Regression coverage for explicit and implicit subroutine return inference.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::type_inference::{
    PerlType, ScalarType, TypeInferenceEngine,
};
use perl_tdd_support::must;

fn inferred_return_type(code: &str, name: &str) -> Option<PerlType> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let mut engine = TypeInferenceEngine::new();
    let result = engine.infer(&ast);
    assert!(result.is_ok(), "type inference failed: {result:?}");

    match engine.get_subroutine(name) {
        Some(PerlType::Subroutine { returns, .. }) => returns.into_iter().next(),
        _ => None,
    }
}

#[test]
fn same_explicit_return_types_collapse_to_one_type() {
    let code = r#"
sub same_kind {
    if ($flag) {
        return 1;
    }
    return 2;
}
"#;

    assert_eq!(
        inferred_return_type(code, "same_kind"),
        Some(PerlType::Scalar(ScalarType::Integer))
    );
}

#[test]
fn distinct_explicit_return_paths_remain_a_union() {
    let code = r#"
sub classify {
    if ($flag) {
        return 1;
    }
    return "zero";
}
"#;

    assert_eq!(
        inferred_return_type(code, "classify"),
        Some(PerlType::Union(vec![
            PerlType::Scalar(ScalarType::Integer),
            PerlType::Scalar(ScalarType::String),
        ]))
    );
}

#[test]
fn explicit_and_implicit_returns_are_unified() {
    let code = r#"
sub explicit_or_fallback {
    if ($flag) {
        return 1;
    }
    "fallback";
}
"#;

    assert_eq!(
        inferred_return_type(code, "explicit_or_fallback"),
        Some(PerlType::Union(vec![
            PerlType::Scalar(ScalarType::Integer),
            PerlType::Scalar(ScalarType::String),
        ]))
    );
}

#[test]
fn return_statement_modifiers_participate_in_the_summary() {
    let code = r#"
sub modified_return {
    return 1 if $flag;
    return "fallback";
}
"#;

    assert_eq!(
        inferred_return_type(code, "modified_return"),
        Some(PerlType::Union(vec![
            PerlType::Scalar(ScalarType::Integer),
            PerlType::Scalar(ScalarType::String),
        ]))
    );
}

#[test]
fn implicit_only_subroutine_keeps_its_last_statement_type() {
    let code = r#"
sub implicit_only {
    "value";
}
"#;

    assert_eq!(
        inferred_return_type(code, "implicit_only"),
        Some(PerlType::Scalar(ScalarType::String))
    );
}

#[test]
fn bare_returns_resolve_to_void() {
    let code = r#"
sub stop {
    if ($flag) {
        return;
    }
    return;
}
"#;

    assert_eq!(inferred_return_type(code, "stop"), Some(PerlType::Void));
}

#[test]
fn nested_subroutine_returns_do_not_pollute_the_outer_summary() {
    let code = r#"
sub outer {
    sub inner {
        return "inner";
    }
    return 1;
}
"#;

    assert_eq!(
        inferred_return_type(code, "outer"),
        Some(PerlType::Scalar(ScalarType::Integer))
    );
}

#[test]
fn explicit_returns_capture_the_environment_at_the_return_site() {
    let code = r#"
sub timeline {
    my $value = 1;
    if ($flag) {
        return $value;
    }
    $value = "later";
    return $value;
}
"#;

    assert_eq!(
        inferred_return_type(code, "timeline"),
        Some(PerlType::Union(vec![
            PerlType::Scalar(ScalarType::Integer),
            PerlType::Scalar(ScalarType::String),
        ]))
    );
}
