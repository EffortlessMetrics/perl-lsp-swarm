mod cpan_test_helpers;
use cpan_test_helpers::*;

/// Test COMMENT 1: What AST node kind does goto &$dispatch produce?
#[test]
fn test_goto_ampersand_variable_ast_structure() {
    let source = r#"goto &$dispatch;"#;
    let node = parse(source);

    // Get the first child (Goto statement)
    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("Goto with form: {:?}", form);
                eprintln!("Target kind: {:?}", target.kind);
                eprintln!("Target: {:#?}", target);
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}

/// Test COMMENT 1: goto &{ $code } — dereferenced coderef
#[test]
fn test_goto_ampersand_coderef_deref_ast() {
    let source = r#"goto &{ $code };"#;
    let node = parse(source);

    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("Goto with form: {:?}", form);
                eprintln!("Target kind: {:?}", target.kind);
                eprintln!("Target: {:#?}", target);
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}

/// Test COMMENT 2: goto E . $suffix — bare identifier followed by string concat
#[test]
fn test_goto_bareword_concat_expression() {
    let source = r#"goto E . $suffix;"#;
    let node = parse(source);

    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("goto E . $suffix");
                eprintln!("  form: {:?} (should be Expr, not Label)", form);
                eprintln!("  target.kind: {:?}", target.kind);
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}

/// Test COMMENT 2: plain goto LABEL — should remain Label
#[test]
fn test_goto_plain_label() {
    let source = r#"goto LABEL;"#;
    let node = parse(source);

    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("goto LABEL");
                eprintln!("  form: {:?} (should be Label)", form);
                eprintln!("  target.kind: {:?}", target.kind);
                assert_eq!(format!("{:?}", form), "Label", "Plain goto LABEL should be Label form");
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}

/// Test COMMENT 2: goto foo() — function call expression, should be Expr
#[test]
fn test_goto_function_call_expr() {
    let source = r#"goto foo();"#;
    let node = parse(source);

    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("goto foo()");
                eprintln!("  form: {:?} (should be Expr, not Label)", form);
                eprintln!("  target.kind: {:?}", target.kind);
                assert_eq!(
                    format!("{:?}", form),
                    "Expr",
                    "goto foo() should be Expr form (not Label)"
                );
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}

/// Test COMMENT 2: goto &sub_name — should remain Sub
#[test]
fn test_goto_ampersand_named_sub() {
    let source = r#"goto &foo;"#;
    let node = parse(source);

    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("goto &foo");
                eprintln!("  form: {:?} (should be Sub)", form);
                eprintln!("  target.kind: {:?}", target.kind);
                assert_eq!(format!("{:?}", form), "Sub", "goto &foo should be Sub form");
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}

/// Test COMMENT 2: goto &Pkg::bar — qualified sub reference, should remain Sub
#[test]
fn test_goto_ampersand_qualified_sub() {
    let source = r#"goto &Pkg::bar;"#;
    let node = parse(source);

    let children = node.children();
    if let Some(stmt) = children.first() {
        match &stmt.kind {
            perl_parser_core::NodeKind::Goto { target, form } => {
                eprintln!("goto &Pkg::bar");
                eprintln!("  form: {:?} (should be Sub)", form);
                eprintln!("  target.kind: {:?}", target.kind);
                assert_eq!(format!("{:?}", form), "Sub", "goto &Pkg::bar should be Sub form");
            }
            _ => panic!("Expected Goto statement"),
        }
    }
}
