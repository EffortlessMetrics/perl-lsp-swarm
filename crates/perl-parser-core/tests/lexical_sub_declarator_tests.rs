mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

/// Project the optional declarator from the named top-level subroutine.
///
/// Returns `Ok(None)` when the subroutine exists but has no declarator (plain `sub`).
/// Returns `Err` when no top-level subroutine with `expected_name` is present, so
/// regressions cannot pass by observing `None` from a missing or wrong subroutine.
fn top_level_sub_declarator(ast: &Node, expected_name: &str) -> Result<Option<String>, String> {
    if let NodeKind::Program { statements } = &ast.kind {
        for stmt in statements {
            if let NodeKind::Subroutine { declarator, name: Some(n), .. } = &stmt.kind {
                if n == expected_name {
                    return Ok(declarator.clone());
                }
            }
        }
    }
    Err(format!("AST must contain top-level sub {expected_name}"))
}

/// Extract declarator fields from ALL top-level named subroutines in program order.
fn extract_all_subroutine_declarators(ast: &Node) -> Vec<(String, Option<String>)> {
    let mut result = Vec::new();
    if let NodeKind::Program { statements } = &ast.kind {
        for stmt in statements {
            if let NodeKind::Subroutine { declarator, name: Some(n), .. } = &stmt.kind {
                result.push((n.clone(), declarator.clone()));
            }
        }
    }
    result
}

// ── Basic cases ──────────────────────────────────────────────────────────────

#[test]
fn test_my_sub_declarator() {
    let source = r#"my sub counter { return 42; }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "counter").expect("fixture sub counter");
    assert_eq!(
        declarator,
        Some("my".to_string()),
        "my sub should have declarator field set to 'my'"
    );
}

#[test]
fn test_our_sub_declarator() {
    let source = r#"our sub global_sub { return 42; }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "global_sub").expect("fixture sub global_sub");
    assert_eq!(
        declarator,
        Some("our".to_string()),
        "our sub should have declarator field set to 'our'"
    );
}

#[test]
fn test_state_sub_declarator() {
    let source = r#"state sub memo { return 42; }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "memo").expect("fixture sub memo");
    assert_eq!(
        declarator,
        Some("state".to_string()),
        "state sub should have declarator field set to 'state'"
    );
}

#[test]
fn test_regular_sub_no_declarator() {
    let source = r#"sub regular { return 42; }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "regular").expect("fixture sub regular");
    assert_eq!(declarator, None, "regular sub should have no declarator (None)");
}

// ── Edge cases ───────────────────────────────────────────────────────────────

/// A `my sub` with a prototype — the declarator must survive the prototype
/// parsing path (`my sub NAME ($$) { ... }`).
#[test]
fn test_my_sub_with_prototype_has_declarator() {
    let source = r#"my sub add ($$) { $_[0] + $_[1] }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "add").expect("fixture sub add");
    assert_eq!(
        declarator,
        Some("my".to_string()),
        "my sub with prototype should still carry declarator='my'"
    );
}

/// A `my sub` with a Perl 5.20+ signature — declarator must survive the
/// signature parsing path.
#[test]
fn test_my_sub_with_signature_has_declarator() {
    let source = r#"my sub greet ($name) { "hello $name" }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "greet").expect("fixture sub greet");
    assert_eq!(
        declarator,
        Some("my".to_string()),
        "my sub with signature should still carry declarator='my'"
    );
}

/// A `my sub` forward declaration (stub, no body) — the forward-declaration
/// path in parse_subroutine() must not suppress the declarator injected
/// by the statements.rs caller.
#[test]
fn test_my_sub_forward_declaration_has_declarator() {
    let source = r#"my sub helper;"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = top_level_sub_declarator(&ast, "helper").expect("fixture sub helper");
    assert_eq!(
        declarator,
        Some("my".to_string()),
        "my sub forward declaration (stub) should carry declarator='my'"
    );
}

/// Multiple scoped subs in sequence — each must get its own correct declarator,
/// not inherit the declarator of a sibling.
#[test]
fn test_multiple_scoped_subs_have_correct_declarators() {
    let source = r#"
my sub lexical { 1 }
our sub exported { 2 }
state sub memoized { 3 }
sub plain { 4 }
"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let subs = extract_all_subroutine_declarators(&ast);

    assert_eq!(subs.len(), 4, "expected 4 subroutines, got {}", subs.len());
    assert_eq!(
        subs[0],
        ("lexical".to_string(), Some("my".to_string())),
        "first sub (my) wrong: {:?}",
        subs[0]
    );
    assert_eq!(
        subs[1],
        ("exported".to_string(), Some("our".to_string())),
        "second sub (our) wrong: {:?}",
        subs[1]
    );
    assert_eq!(
        subs[2],
        ("memoized".to_string(), Some("state".to_string())),
        "third sub (state) wrong: {:?}",
        subs[2]
    );
    assert_eq!(subs[3], ("plain".to_string(), None), "fourth sub (plain) wrong: {:?}", subs[3]);
}

/// A `my sub` nested inside a closure should not corrupt the outer closure's
/// state. The outer named sub (plain `sub`) should have no declarator.
#[test]
fn test_nested_my_sub_does_not_affect_outer_sub() {
    let source = r#"
sub outer {
    my sub inner { 99 }
    inner()
}
"#;
    assert_clean_parse(source);
    let ast = parse(source);
    // outer is a plain sub — no declarator
    let declarator = top_level_sub_declarator(&ast, "outer").expect("fixture sub outer");
    assert_eq!(
        declarator, None,
        "outer plain sub should have no declarator even when it contains a my sub"
    );
}
