//! Scalar output handles must keep heredoc messages on the ordinary call route.

use perl_parser_core::{Node, NodeKind, Parser};

fn clean_ast(source: &str) -> Node {
    let output = Parser::new(source).parse_with_recovery();
    assert!(output.diagnostics.is_empty(), "{source:?}: {:?}", output.diagnostics);
    output.ast
}

fn indirect_calls<'a>(node: &'a Node, calls: &mut Vec<&'a Node>) {
    if matches!(node.kind, NodeKind::IndirectCall { .. }) {
        calls.push(node);
    }
    for child in node.children() {
        indirect_calls(child, calls);
    }
}

fn regular_print(node: &Node) -> Option<&[Node]> {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "print"
    {
        return Some(args);
    }
    node.children().into_iter().find_map(regular_print)
}

fn contains_left_shift(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Binary { op, .. } if op == "<<")
        || node.children().into_iter().any(contains_left_shift)
}

#[test]
fn scalar_output_handles_preserve_heredoc_messages_and_following_statements() {
    for method in ["print", "printf", "say"] {
        for prefix in ["", "my $ok = "] {
            let source = format!("{prefix}{method} $fh <<'END';\nhello\nEND\nmy $after = 1;\n");
            let ast = clean_ast(&source);
            let mut calls = Vec::new();
            indirect_calls(&ast, &mut calls);
            assert_eq!(calls.len(), 1, "{source:?}: {}", ast.to_sexp());
            let NodeKind::IndirectCall { method: actual, object, args } = &calls[0].kind else {
                unreachable!("collected only indirect calls");
            };
            assert_eq!(actual, method);
            assert!(matches!(&object.kind, NodeKind::Variable { sigil, name }
                if sigil == "$" && name == "fh"));
            assert_eq!(args.len(), 1);
            assert!(
                matches!(&args[0].kind,
                NodeKind::Heredoc { delimiter, content, body_span: Some(span), .. }
                if delimiter == "END" && content == "hello"
                    && source.get(span.start..span.end) == Some("hello")),
                "heredoc message lost its body: {:?}",
                args[0]
            );
            let NodeKind::Program { statements } = &ast.kind else {
                unreachable!("parser returns a program");
            };
            assert_eq!(statements.len(), 2, "following statement lost: {}", ast.to_sexp());
            assert!(ast.to_sexp().contains("after"));
        }
    }
}

#[test]
fn comma_separated_heredoc_stays_an_ordinary_print_list() -> Result<(), String> {
    let ast = clean_ast("print $fh, <<'END';\nhello\nEND\n");
    let mut calls = Vec::new();
    indirect_calls(&ast, &mut calls);
    assert!(calls.is_empty(), "{}", ast.to_sexp());
    let args = regular_print(&ast).ok_or_else(|| format!("print call lost: {}", ast.to_sexp()))?;
    assert_eq!(args.len(), 2);
    assert!(matches!(&args[0].kind, NodeKind::Variable { sigil, name }
        if sigil == "$" && name == "fh"));
    assert!(matches!(&args[1].kind, NodeKind::Heredoc { content, .. } if content == "hello"));
    Ok(())
}

#[test]
fn shift_operands_do_not_become_filehandle_messages() {
    for source in ["print $fh << 2;", "my $n = $fh << 2;", "print($fh << 'END');"] {
        let ast = clean_ast(source);
        let mut calls = Vec::new();
        indirect_calls(&ast, &mut calls);
        assert!(calls.is_empty(), "{source:?}: {}", ast.to_sexp());
        assert!(contains_left_shift(&ast), "shift lost: {}", ast.to_sexp());
    }
}

#[test]
fn malformed_output_and_later_residue_remain_blocking() {
    for source in [
        "print $fh <<'END';\nhello\n",
        "print $fh 1 2;",
        "print $fh <<'END';\nhello\nEND\nmy $x = 1 print 'bad';",
    ] {
        let output = Parser::new(source).parse_with_recovery();
        assert!(
            output.diagnostics.iter().any(|error| error.blocks_clean_parse()),
            "malformed input returned clean: {source:?}"
        );
    }
}
