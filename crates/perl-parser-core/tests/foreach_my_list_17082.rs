//! Perl 5.40.1 accepts declarations in implicit-topic foreach lists, including
//! the `for (my @filename = @_)` idiom in core parent.pm 0.241. The delimiter
//! after the complete list, not its leading `my`, selects foreach vs C-style.

use perl_parser_core::{Node, NodeKind, ParseError, Parser};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn parse(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    assert!(parser.errors().is_empty(), "{source}: {:?}", parser.errors());
    Ok(ast)
}

fn find_foreach(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::Foreach { .. }) {
        return Some(node);
    }
    node.children().into_iter().find_map(find_foreach)
}

fn implicit_list<'a>(ast: &'a Node, source: &str, expected: &str) -> Result<&'a Node, String> {
    let node = find_foreach(ast).ok_or_else(|| format!("missing foreach: {}", ast.to_sexp()))?;
    let NodeKind::Foreach { variable, list, body, continue_block } = &node.kind else {
        return Err("expected foreach".into());
    };
    assert!(
        matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "_")
    );
    assert_eq!(variable.location.start, node.location.start);
    assert_eq!(variable.location.end, node.location.start, "implicit topic has no source token");
    assert_eq!(&source[list.location.start..list.location.end], expected);
    assert_eq!(&source[body.location.start..body.location.end], "{ print $_; }");
    assert_eq!(node.location.end, body.location.end);
    assert!(source[node.location.start..].starts_with("for"));
    assert!(continue_block.is_none());
    Ok(list)
}

#[test]
fn scalar_and_array_declarations_stay_in_the_list_with_exact_ranges() -> TestResult {
    for keyword in ["for", "foreach"] {
        for (declaration, sigil, name, initializer) in [
            ("my @x = (1, 2)", "@", "x", "(1, 2)"),
            ("my $x = 7", "$", "x", "7"),
            ("my @filename = @_", "@", "filename", "@_"),
        ] {
            // Non-ASCII prefix and multiline whitespace exercise byte geometry.
            let source = format!("# café\n{keyword} (\n  {declaration}\n) {{ print $_; }}");
            let ast = parse(&source)?;
            let list = implicit_list(&ast, &source, declaration)?;
            let NodeKind::VariableDeclaration { declarator, variable, initializer: init, .. } =
                &list.kind
            else {
                return Err(format!("declaration lost from list: {}", list.to_sexp()).into());
            };
            assert_eq!(declarator, "my");
            assert!(
                matches!(&variable.kind, NodeKind::Variable { sigil: s, name: n } if s == sigil && n == name)
            );
            let init = init.as_ref().ok_or("initializer disappeared")?;
            assert_eq!(&source[init.location.start..init.location.end], initializer);
        }
    }
    Ok(())
}

#[test]
fn list_declaration_keeps_both_declared_variables() -> TestResult {
    for keyword in ["for", "foreach"] {
        let declaration = "my ($x, $y) = (1, 2)";
        let source = format!("{keyword} ({declaration}) {{ print $_; }}");
        let ast = parse(&source)?;
        let list = implicit_list(&ast, &source, declaration)?;
        let NodeKind::VariableListDeclaration { declarator, variables, initializer, .. } =
            &list.kind
        else {
            return Err(format!("expected list declaration: {}", list.to_sexp()).into());
        };
        assert_eq!(declarator, "my");
        assert_eq!(variables.len(), 2);
        for (variable, expected) in variables.iter().zip(["$x", "$y"]) {
            assert_eq!(&source[variable.location.start..variable.location.end], expected);
        }
        assert!(initializer.is_some());
    }
    Ok(())
}

#[test]
fn declaration_comma_and_word_operator_continuations_remain_in_list() -> TestResult {
    for keyword in ["for", "foreach"] {
        for (expression, operator) in [("my $x = 1, $y", ","), ("my $x = 1 or die", "or")] {
            let source = format!("my $y = 2; {keyword} ({expression}) {{ print $_; }}");
            let ast = parse(&source)?;
            let list = implicit_list(&ast, &source, expression)?;
            let declaration = match &list.kind {
                NodeKind::ArrayLiteral { elements } if operator == "," => {
                    assert_eq!(elements.len(), 2);
                    assert!(
                        matches!(&elements[1].kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "y")
                    );
                    &elements[0]
                }
                NodeKind::Binary { op, left, .. } if op == operator => left,
                _ => return Err(format!("continuation lost: {}", list.to_sexp()).into()),
            };
            assert!(
                matches!(&declaration.kind, NodeKind::VariableDeclaration { declarator, .. } if declarator == "my")
            );
        }
    }
    Ok(())
}

#[test]
fn uninitialized_declaration_binary_operators_remain_in_list() -> TestResult {
    for keyword in ["for", "foreach"] {
        for (operator, rhs) in [("&&", "$ready"), ("==", "1"), ("+", "2"), ("||", "3"), ("**", "2")]
        {
            let expression = format!("my $x {operator} {rhs}");
            let source = format!("# café\n{keyword} (\n {expression}\n) {{ print $_; }}");
            let ast = parse(&source)?;
            let list = implicit_list(&ast, &source, &expression)?;
            let NodeKind::Binary { op, left, right } = &list.kind else {
                return Err(format!("binary continuation lost: {}", list.to_sexp()).into());
            };
            assert_eq!(op, operator);
            assert!(matches!(
                &left.kind,
                NodeKind::VariableDeclaration { declarator, initializer: None, .. }
                    if declarator == "my"
            ));
            assert_eq!(&source[left.location.start..left.location.end], "my $x");
            assert_eq!(&source[right.location.start..right.location.end], rhs);
        }
    }
    Ok(())
}

#[test]
fn uninitialized_declaration_ternary_remains_in_list() -> TestResult {
    for keyword in ["for", "foreach"] {
        let expression = "my $x ? 1 : 2";
        let source = format!("{keyword} ({expression}) {{ print $_; }}");
        let ast = parse(&source)?;
        let list = implicit_list(&ast, &source, expression)?;
        let NodeKind::Ternary { condition, then_expr, else_expr } = &list.kind else {
            return Err(format!("ternary continuation lost: {}", list.to_sexp()).into());
        };
        assert!(matches!(
            &condition.kind,
            NodeKind::VariableDeclaration { declarator, initializer: None, .. }
                if declarator == "my"
        ));
        for (part, expected) in [(condition, "my $x"), (then_expr, "1"), (else_expr, "2")] {
            assert_eq!(&source[part.location.start..part.location.end], expected);
        }
    }
    Ok(())
}

#[test]
fn symbolic_continuation_precedes_comma_and_word_operators() -> TestResult {
    for keyword in ["for", "foreach"] {
        let expression = "my $x && $ready, $other or die";
        let source = format!("{keyword} ({expression}) {{ print $_; }}");
        let ast = parse(&source)?;
        let list = implicit_list(&ast, &source, expression)?;
        let NodeKind::Binary { op, left, .. } = &list.kind else {
            return Err(format!("word continuation lost: {}", list.to_sexp()).into());
        };
        assert_eq!(op, "or");
        let NodeKind::ArrayLiteral { elements } = &left.kind else {
            return Err(format!("comma continuation lost: {}", left.to_sexp()).into());
        };
        assert_eq!(elements.len(), 2);
        assert!(matches!(&elements[0].kind, NodeKind::Binary { op, left, .. }
            if op == "&&" && matches!(left.kind, NodeKind::VariableDeclaration { initializer: None, .. })));
        assert_eq!(&source[elements[1].location.start..elements[1].location.end], "$other");
    }
    Ok(())
}

fn assert_declaration_power(node: &Node, source: &str, expected_rhs: &str) -> TestResult {
    let NodeKind::Binary { op, left, right } = &node.kind else {
        return Err(format!("power continuation lost: {}", node.to_sexp()).into());
    };
    assert_eq!(op, "**");
    assert!(matches!(
        &left.kind,
        NodeKind::VariableDeclaration { declarator, initializer: None, .. } if declarator == "my"
    ));
    assert_eq!(&source[left.location.start..left.location.end], "my $x");
    assert_eq!(&source[right.location.start..right.location.end], expected_rhs);
    Ok(())
}

#[test]
fn declaration_power_is_right_associative_before_lower_precedence() -> TestResult {
    for keyword in ["for", "foreach"] {
        let expression = "my $x ** 2 ** 3 * 4, $other or die";
        let source = format!("# café\n{keyword} (\n {expression}\n) {{ print $_; }}");
        let ast = parse(&source)?;
        let list = implicit_list(&ast, &source, expression)?;
        let NodeKind::Binary { op, left, .. } = &list.kind else {
            return Err("word operator lost".into());
        };
        assert_eq!(op, "or");
        let NodeKind::ArrayLiteral { elements } = &left.kind else {
            return Err("comma list lost".into());
        };
        assert_eq!(elements.len(), 2);
        let NodeKind::Binary { op, left: power, right } = &elements[0].kind else {
            return Err("multiplication lost".into());
        };
        assert_eq!(op, "*");
        assert_eq!(&source[right.location.start..right.location.end], "4");
        assert_declaration_power(power, &source, "2 ** 3")?;
        let NodeKind::Binary { right, .. } = &power.kind else {
            return Err("power lost".into());
        };
        assert!(matches!(&right.kind, NodeKind::Binary { op, left, right }
            if op == "**"
                && matches!(&left.kind, NodeKind::Number { value } if value == "2")
                && matches!(&right.kind, NodeKind::Number { value } if value == "3")));
        assert_eq!(&source[elements[1].location.start..elements[1].location.end], "$other");
    }
    Ok(())
}

#[test]
fn declaration_power_is_shared_by_conditions_and_call_arguments() -> TestResult {
    for source in [
        "# café\nif (\n my $x ** 2\n) { print $_; }",
        "# café\nprint($before,\n my $x ** 2, $after);",
    ] {
        let ast = parse(source)?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected program".into());
        };
        let expression = match &statements[0].kind {
            NodeKind::If { condition, .. } => condition.as_ref(),
            NodeKind::ExpressionStatement { expression } => match &expression.kind {
                NodeKind::FunctionCall { name, args } if name == "print" => {
                    assert_eq!(args.len(), 3, "power must not absorb neighboring arguments");
                    assert_eq!(&source[args[0].location.start..args[0].location.end], "$before");
                    assert_eq!(&source[args[2].location.start..args[2].location.end], "$after");
                    &args[1]
                }
                _ => return Err(format!("expected print call: {}", ast.to_sexp()).into()),
            },
            _ => return Err(format!("expected condition or call: {}", ast.to_sexp()).into()),
        };
        assert_declaration_power(expression, source, "2")?;
        assert_eq!(&source[expression.location.start..expression.location.end], "my $x ** 2");
    }
    Ok(())
}

#[test]
fn initialized_power_and_power_assignment_keep_their_existing_shapes() -> TestResult {
    for keyword in ["for", "foreach"] {
        for expression in ["my $x = 2 ** 3", "my $x **= 2"] {
            let source = format!("{keyword} ({expression}) {{ print $_; }}");
            let ast = parse(&source)?;
            let list = implicit_list(&ast, &source, expression)?;
            let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &list.kind
            else {
                return Err(format!("declaration initializer lost: {}", list.to_sexp()).into());
            };
            match &initializer.kind {
                NodeKind::Binary { op, .. } if op == "**" && expression.contains(" = ") => {
                    assert_eq!(
                        &source[initializer.location.start..initializer.location.end],
                        "2 ** 3"
                    );
                }
                NodeKind::Assignment { op, lhs, rhs } if op == "**=" => {
                    assert!(matches!(&lhs.kind, NodeKind::Variable { sigil, name }
                        if sigil == "$" && name == "x"));
                    assert_eq!(&source[rhs.location.start..rhs.location.end], "2");
                }
                _ => {
                    return Err(format!(
                        "initialized/assignment shape changed: {}",
                        list.to_sexp()
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

#[test]
fn declaration_power_requires_a_rhs_in_every_shared_consumer() {
    for source in [
        "for (my $x **) { print $_; }",
        "foreach (my $x **; $i < 2; ++$i) { print $_; }",
        "if (my $x **) { print $_; }",
        "f(my $x **);",
    ] {
        let mut parser = Parser::new(source);
        let result = parser.parse();
        assert!(
            result.is_err() || parser.errors().iter().any(|error| error.blocks_clean_parse()),
            "missing power RHS silently accepted: {source}"
        );
    }
}

#[test]
fn core_parent_shaped_helper_keeps_implicit_topic() -> TestResult {
    let source =
        r#"sub f { for (my @filename = @_) { s{::|'}{/}g; print "$_\n"; } } f("A::B", "C::D");"#;
    let ast = parse(source)?;
    let loop_node = find_foreach(&ast).ok_or("parent.pm-shaped foreach disappeared")?;
    let NodeKind::Foreach { variable, list, .. } = &loop_node.kind else {
        return Err("expected foreach".into());
    };
    assert!(
        matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "_")
    );
    assert!(
        matches!(&list.kind, NodeKind::VariableDeclaration { declarator, .. } if declarator == "my")
    );
    assert_eq!(&source[list.location.start..list.location.end], "my @filename = @_");
    Ok(())
}

#[test]
fn existing_implicit_and_explicit_iterator_forms_stay_foreach() -> TestResult {
    for keyword in ["for", "foreach"] {
        for header in [
            "((my @x = (1, 2)))",
            "(1, 2)",
            "my $x (1, 2)",
            "(our @x = (1, 2))",
            "(local @x = (1, 2))",
        ] {
            let source = format!("{keyword} {header} {{ print $_; }}");
            let ast = parse(&source)?;
            assert!(find_foreach(&ast).is_some(), "{source}: {}", ast.to_sexp());
        }
    }
    Ok(())
}

#[test]
fn semicolons_preserve_all_three_c_style_clauses() -> TestResult {
    for keyword in ["for", "foreach"] {
        for initialization in [
            "my $i = 0",
            "$i = 0",
            "my $i = 0, $j = 1",
            "my $i = 0 or die",
            "my $i && $ready",
            "my $i == 1",
            "my $i ? 1 : 2",
            "my $i ** 2",
            "my $i = 2 ** 3",
            "my $i **= 2",
        ] {
            let source = format!("{keyword} ({initialization}; $i < 2; ++$i) {{ print $_; }}");
            let ast = parse(&source)?;
            let NodeKind::Program { statements } = &ast.kind else {
                return Err("expected program".into());
            };
            let NodeKind::For { init, condition, update, body, .. } = &statements[0].kind else {
                return Err(format!("C-style loop changed kind: {}", ast.to_sexp()).into());
            };
            for (clause, expected) in
                [(init, initialization), (condition, "$i < 2"), (update, "++$i")]
            {
                let clause = clause.as_ref().ok_or("C-style clause lost")?;
                assert_eq!(&source[clause.location.start..clause.location.end], expected);
            }
            assert_eq!(&source[body.location.start..body.location.end], "{ print $_; }");
            assert!(find_foreach(&ast).is_none());
        }
        parse(&format!("{keyword} (;;) {{ last; }}"))?;
    }
    Ok(())
}

#[test]
fn incomplete_declaration_operators_still_report_blocking_errors() {
    for keyword in ["for", "foreach"] {
        for expression in ["my $x &&", "my $x ==", "my $x ? 1 :"] {
            let source = format!("{keyword} ({expression}) {{ print $_; }}");
            let mut parser = Parser::new(&source);
            let result = parser.parse();
            assert!(
                result.is_err() || parser.errors().iter().any(|error| error.blocks_clean_parse()),
                "incomplete operator silently accepted: {source}"
            );
        }
    }
}

#[test]
fn missing_c_style_separator_still_reports_a_blocking_error() {
    for keyword in ["for", "foreach"] {
        let source = format!("{keyword} (my $i = 0 $i < 2; ++$i) {{ print $i; }}");
        let mut parser = Parser::new(&source);
        let result = parser.parse();
        assert!(
            result.is_err() || parser.errors().iter().any(|error| error.blocks_clean_parse()),
            "malformed header silently accepted: {source}"
        );
        assert!(
            parser
                .errors()
                .iter()
                .any(|error| error.to_string().contains("Missing ';' after for-loop init")),
            "separator recovery disappeared: {:?}",
            parser.errors()
        );
    }
}

#[test]
fn implicit_foreach_owns_its_continue_block_and_following_statement() -> TestResult {
    for keyword in ["for", "foreach"] {
        for list_text in ["1, 2", "my @values = @_"] {
            for prefix in ["", "# café\r\n"] {
                let body_text = "{ print $_; }";
                let continue_text = "{ print \"done\"; }";
                let loop_text =
                    format!("{keyword} ({list_text}) {body_text} continue {continue_text}");
                let source = format!("{prefix}{loop_text} my $after = 1;");
                let ast = parse(&source)?;
                let NodeKind::Program { statements } = &ast.kind else {
                    return Err("expected program".into());
                };
                assert_eq!(statements.len(), 2, "continuation must belong to the loop");
                let node = &statements[0];
                let NodeKind::Foreach { variable, list, body, continue_block } = &node.kind else {
                    return Err("expected top-level foreach".into());
                };
                assert!(matches!(
                    &variable.kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "_"
                ));
                assert_eq!(variable.location.start, prefix.len());
                assert_eq!(variable.location.end, prefix.len());
                assert_eq!(&source[list.location.start..list.location.end], list_text);
                assert_eq!(&source[body.location.start..body.location.end], body_text);
                let continuation = continue_block.as_deref().ok_or("continue block disappeared")?;
                assert_eq!(
                    &source[continuation.location.start..continuation.location.end],
                    continue_text
                );
                assert_eq!(node.location.start, prefix.len());
                assert_eq!(node.location.end, prefix.len() + loop_text.len());
                let children = node.children();
                assert!(children.into_iter().any(|child| std::ptr::eq(child, continuation)));

                let NodeKind::VariableDeclaration { declarator, variable, initializer, .. } =
                    &statements[1].kind
                else {
                    return Err("following declaration disappeared or changed ownership".into());
                };
                assert_eq!(declarator, "my");
                assert!(matches!(
                    &variable.kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"
                ));
                let initializer = initializer.as_deref().ok_or("following initializer lost")?;
                assert!(matches!(&initializer.kind, NodeKind::Number { value } if value == "1"));
                assert_eq!(&source[initializer.location.start..initializer.location.end], "1");
            }
        }
    }
    Ok(())
}

#[test]
fn explicit_foreach_continuation_remains_attached() -> TestResult {
    for keyword in ["for", "foreach"] {
        let body_text = "{ print $item; }";
        let continue_text = "{ print \"done\"; }";
        let loop_text = format!("{keyword} my $item (1, 2) {body_text} continue {continue_text}");
        let source = format!("{loop_text} my $after = 1;");
        let ast = parse(&source)?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected program".into());
        };
        assert_eq!(statements.len(), 2);
        let NodeKind::Foreach { variable, body, continue_block, .. } = &statements[0].kind else {
            return Err("explicit iterator changed kind".into());
        };
        let NodeKind::VariableDeclaration {
            declarator,
            variable: iterator,
            initializer: None,
            ..
        } = &variable.kind
        else {
            return Err("explicit iterator declaration changed kind".into());
        };
        assert_eq!(declarator, "my");
        assert!(matches!(
            &iterator.kind,
            NodeKind::Variable { sigil, name } if sigil == "$" && name == "item"
        ));
        assert_eq!(&source[iterator.location.start..iterator.location.end], "$item");
        assert_eq!(&source[body.location.start..body.location.end], body_text);
        let continuation = continue_block.as_deref().ok_or("explicit continue block lost")?;
        assert_eq!(&source[continuation.location.start..continuation.location.end], continue_text);
        assert_eq!(statements[0].location.end, loop_text.len());
    }
    Ok(())
}

#[test]
fn c_style_for_continuation_blocks_remain_rejected() {
    for keyword in ["for", "foreach"] {
        let source =
            format!("{keyword} (my $i = 0; $i < 2; ++$i) {{ print $i; }} continue {{ print $i; }}");
        let mut parser = Parser::new(&source);
        let result = parser.parse();
        assert!(
            matches!(result, Err(ParseError::CStyleForContinueBlock { .. })),
            "C-style continuation block did not report its specific error: {source}"
        );
    }
}

#[test]
fn implicit_foreach_leaves_standalone_continue_as_a_following_statement() -> TestResult {
    for keyword in ["for", "foreach"] {
        let body_text = "{ print $_; }";
        let loop_text = format!("{keyword} (1, 2) {body_text}");
        let source = format!("{loop_text} continue; my $after = 1;");
        let ast = parse(&source)?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected program".into());
        };
        assert_eq!(statements.len(), 3);
        let NodeKind::Foreach { continue_block, .. } = &statements[0].kind else {
            return Err("implicit iterator changed kind".into());
        };
        assert!(continue_block.is_none());
        assert_eq!(statements[0].location.end, loop_text.len());
        let NodeKind::VariableDeclaration { declarator, variable, initializer, .. } =
            &statements[2].kind
        else {
            return Err("following declaration changed kind".into());
        };
        assert_eq!(declarator, "my");
        assert!(matches!(
            &variable.kind,
            NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"
        ));
        let initializer = initializer.as_deref().ok_or("following initializer lost")?;
        assert!(matches!(&initializer.kind, NodeKind::Number { value } if value == "1"));
        assert_eq!(&source[initializer.location.start..initializer.location.end], "1");
    }
    Ok(())
}
