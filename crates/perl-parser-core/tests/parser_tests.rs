use perl_parser_core::{
    NodeKind as V1NodeKind,
    ParseError,
    ParseOutput,
    // Parser
    Parser,
};
use perl_tdd_support::must;

#[test]
fn parse_simple_assignment() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("my $var = 42;");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            assert!(!statements.is_empty(), "should parse at least one statement");
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }
    Ok(())
}

#[test]
fn parse_empty_input() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            assert!(statements.is_empty(), "empty source should yield no statements");
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }
    assert!(parser.errors().is_empty());
    Ok(())
}

#[test]
fn parse_with_recovery_returns_output() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("my $x = ;");
    let output: ParseOutput = parser.parse_with_recovery();

    match &output.ast.kind {
        V1NodeKind::Program { .. } => { /* ok */ }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }
    // The output should contain diagnostics for the syntax error
    // or recovery nodes within the AST
    Ok(())
}

#[test]
fn parse_with_recovery_empty() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("");
    let output = parser.parse_with_recovery();

    match &output.ast.kind {
        V1NodeKind::Program { statements } => {
            assert!(statements.is_empty());
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }
    assert!(output.diagnostics.is_empty());
    Ok(())
}

#[test]
fn parser_errors_initially_empty() -> Result<(), Box<dyn std::error::Error>> {
    let parser = Parser::new("my $x = 1;");
    assert!(parser.errors().is_empty(), "should have no errors before parsing");
    Ok(())
}

#[test]
fn new_with_recovery_config_creates_parser() -> Result<(), Box<dyn std::error::Error>> {
    let parser = Parser::new_with_recovery_config("my $x = 1;", ());
    assert!(parser.errors().is_empty());
    Ok(())
}

#[test]
fn parse_subroutine_declaration() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("sub hello { print 'hi'; }");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            assert!(!statements.is_empty(), "should parse subroutine");
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }
    Ok(())
}

#[test]
fn parse_namespaced_class_declaration() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("class My::App::Service { method run { 1; } }");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            let Some(class_node) = statements.first() else {
                return Err("expected a class statement".into());
            };

            match &class_node.kind {
                V1NodeKind::Class { name, .. } => {
                    assert_eq!(name, "My::App::Service");
                }
                other => return Err(format!("expected Class node, got {:?}", other).into()),
            }
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }

    assert!(parser.errors().is_empty(), "unexpected parser errors: {:?}", parser.errors());
    Ok(())
}

#[test]
fn reject_class_declaration_with_trailing_separator() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("class My::App:: { method run { 1; } }");
    let ast = parser.parse()?;

    let mut accepted_trailing_class = false;
    let mut error_node_mentions_identifier = false;
    inspect_rejected_class(&ast, &mut accepted_trailing_class, &mut error_node_mentions_identifier);

    let saw_identifier_error = error_node_mentions_identifier
        || parser.errors().iter().any(|error| match error {
            ParseError::UnexpectedToken { expected, .. } => {
                expected.contains("identifier after ::")
            }
            _ => error.to_string().contains("identifier after ::"),
        });
    assert!(saw_identifier_error, "expected a missing identifier error, got {:?}", parser.errors());
    assert!(
        !accepted_trailing_class,
        "class declaration should not be accepted with a trailing ::"
    );
    Ok(())
}

fn inspect_rejected_class(
    node: &perl_parser_core::Node,
    accepted: &mut bool,
    identifier_error: &mut bool,
) {
    match &node.kind {
        V1NodeKind::Class { name, .. } if name == "My::App::" => *accepted = true,
        V1NodeKind::Error { message, .. } if message.contains("identifier after ::") => {
            *identifier_error = true;
        }
        _ => {}
    }
    node.for_each_child(|child| inspect_rejected_class(child, accepted, identifier_error));
}

#[test]
fn parse_package_declaration_with_trailing_separator() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("package My::App::;");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            let Some(package_node) = statements.first() else {
                return Err("expected a package statement".into());
            };

            match &package_node.kind {
                V1NodeKind::Package { name, block, .. } => {
                    assert_eq!(name, "My::App::");
                    assert!(block.is_none(), "package statement form should not have a block");
                }
                other => return Err(format!("expected Package node, got {:?}", other).into()),
            }
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }

    assert!(parser.errors().is_empty(), "unexpected parser errors: {:?}", parser.errors());
    Ok(())
}

#[test]
fn parse_package_declaration_with_trailing_separator_and_block()
-> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("package My::App:: { my $x = 1; }");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            let Some(package_node) = statements.first() else {
                return Err("expected a package statement".into());
            };

            match &package_node.kind {
                V1NodeKind::Package { name, block, .. } => {
                    assert_eq!(name, "My::App::");
                    assert!(block.is_some(), "package block form should include a block node");
                }
                other => return Err(format!("expected Package node, got {:?}", other).into()),
            }
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }

    assert!(parser.errors().is_empty(), "unexpected parser errors: {:?}", parser.errors());
    Ok(())
}

#[test]
fn parse_package_declaration_with_vstring_version() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("package My::App v5.38;");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            let Some(package_node) = statements.first() else {
                return Err("expected a package statement".into());
            };

            match &package_node.kind {
                V1NodeKind::Package { name, block, .. } => {
                    // Version is parsed but NOT concatenated into name since #5265 —
                    // package-to-file mapping and diagnostic messages must use the bare name.
                    assert_eq!(name, "My::App");
                    assert!(block.is_none(), "package statement form should not have a block");
                }
                other => return Err(format!("expected Package node, got {:?}", other).into()),
            }
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }

    assert!(parser.errors().is_empty(), "unexpected parser errors: {:?}", parser.errors());
    Ok(())
}

#[test]
fn parse_multiple_statements() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("my $x = 1; my $y = 2; my $z = 3;");
    let ast = must(parser.parse());

    match &ast.kind {
        V1NodeKind::Program { statements } => {
            assert!(
                statements.len() >= 3,
                "should parse three statements, got {}",
                statements.len()
            );
        }
        other => return Err(format!("expected Program, got {:?}", other).into()),
    }
    Ok(())
}

#[test]
fn parse_output_budget_usage_tracked() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("my $x = ;");
    let output = parser.parse_with_recovery();

    // Live tracker: depth was recorded and unwound. Diagnostic charging is B02.
    assert_eq!(output.budget_usage.current_depth, 0);
    assert!(output.budget_usage.max_depth_reached > 0);
    assert_eq!(output.budget_usage.errors_emitted, 0);
    Ok(())
}
