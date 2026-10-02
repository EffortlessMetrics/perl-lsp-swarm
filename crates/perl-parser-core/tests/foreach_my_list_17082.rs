//! Perl 5.40.1 accepts declarations in implicit-topic foreach lists, including
//! the `for (my @filename = @_)` idiom in core parent.pm 0.241. The delimiter
//! after the complete list, not its leading `my`, selects foreach vs C-style.

use perl_parser_core::{Node, NodeKind, Parser};

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
        for initialization in ["my $i = 0", "$i = 0", "my $i = 0, $j = 1", "my $i = 0 or die"] {
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
