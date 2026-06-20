mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

fn extract_subroutine_declarator(ast: &Node) -> Option<String> {
    match &ast.kind {
        NodeKind::Program { statements } => {
            for stmt in statements {
                if let NodeKind::Subroutine {
                    declarator,
                    name,
                    ..
                } = &stmt.kind
                {
                    if name.is_some() {
                        return declarator.clone();
                    }
                }
            }
        }
        _ => {}
    }
    None
}

#[test]
fn test_my_sub_declarator() {
    let source = r#"my sub counter { return 42; }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = extract_subroutine_declarator(&ast);
    assert_eq!(declarator, Some("my".to_string()), "my sub should have declarator field set to 'my'");
}

#[test]
fn test_our_sub_declarator() {
    let source = r#"our sub global_sub { return 42; }"#;
    assert_clean_parse(source);
    let ast = parse(source);
    let declarator = extract_subroutine_declarator(&ast);
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
    let declarator = extract_subroutine_declarator(&ast);
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
    let declarator = extract_subroutine_declarator(&ast);
    assert_eq!(
        declarator,
        None,
        "regular sub should have no declarator (None)"
    );
}
