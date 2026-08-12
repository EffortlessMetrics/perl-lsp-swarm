//! Concept-level parser proofs for Perl I/O-shaped syntax (#6705).
//!
//! These tests pin source disambiguation only. Filesystem, process, ARGV,
//! filehandle, tie, and security semantics remain downstream concerns.

use perl_parser_core::{Node, NodeKind, Parser};

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()))
    }
}

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn source_text(source: &str, node: &Node) -> Option<String> {
    source
        .get(node.location.start..node.location.end)
        .map(str::to_owned)
}

fn subtree_contains(node: &Node, predicate: &impl Fn(&NodeKind) -> bool) -> bool {
    predicate(&node.kind)
        || node
            .children()
            .into_iter()
            .any(|child| subtree_contains(child, predicate))
}

#[test]
fn angle_forms_keep_exact_readline_diamond_glob_and_shift_shapes() -> Result<(), String> {
    let source = concat!(
        "my $stdin = <STDIN>;\n",
        "my $dynamic = <$fh>;\n",
        "my $diamond = <>;\n",
        "my $safe_diamond = <<>>;\n",
        "my @files = <*.pl>;\n",
        "my $left = $value << 2;\n",
        "my $right = $value >> 2;\n",
    );
    let ast = parse_clean(source)?;
    let mut readlines = Vec::new();
    let mut diamonds = Vec::new();
    let mut globs = Vec::new();
    let mut shifts = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Readline { filehandle } => {
            readlines.push((filehandle.clone(), source_text(source, node)));
        }
        NodeKind::Diamond => {
            diamonds.push(source_text(source, node));
        }
        NodeKind::Glob { pattern } => {
            globs.push((pattern.clone(), source_text(source, node)));
        }
        NodeKind::Binary { op, left, right } if matches!(op.as_str(), "<<" | ">>") => {
            shifts.push((
                op.clone(),
                source_text(source, node),
                source_text(source, left),
                source_text(source, right),
                matches!(
                    &left.kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "value"
                ),
                matches!(&right.kind, NodeKind::Number { value } if value == "2"),
            ));
        }
        _ => {}
    });

    assert_eq!(
        readlines,
        vec![
            (Some("STDIN".to_string()), Some("<STDIN>".to_string())),
            (Some("$fh".to_string()), Some("<$fh>".to_string())),
        ],
        "bareword and scalar-held filehandles must remain distinct Readline payloads"
    );
    assert_eq!(
        diamonds,
        vec![Some("<>".to_string()), Some("<<>>".to_string())],
        "ordinary and safe diamond forms share NodeKind but retain exact source geometry"
    );
    assert_eq!(
        globs,
        vec![("*.pl".to_string(), Some("<*.pl>".to_string()))]
    );
    assert_eq!(
        shifts,
        vec![
            (
                "<<".to_string(),
                Some("$value << 2".to_string()),
                Some("$value".to_string()),
                Some("2".to_string()),
                true,
                true,
            ),
            (
                ">>".to_string(),
                Some("$value >> 2".to_string()),
                Some("$value".to_string()),
                Some("2".to_string()),
                true,
                true,
            ),
        ],
        "left and right shifts must retain their operators and owned operands"
    );
    Ok(())
}

#[test]
fn heredoc_opener_keeps_exact_payload_and_body_span_without_diamond() -> Result<(), String> {
    let source = "my $document = <<EOF;\nhello\nEOF\n";
    let ast = parse_clean(source)?;
    let mut heredocs = Vec::new();
    let mut diamonds = 0usize;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Heredoc {
            delimiter,
            content,
            interpolated,
            indented,
            command,
            body_span,
        } => {
            let body_geometry = body_span.as_ref().map(|span| {
                (
                    span.start,
                    span.end,
                    source.get(span.start..span.end).map(str::to_owned),
                )
            });
            heredocs.push((
                delimiter.clone(),
                content.clone(),
                *interpolated,
                *indented,
                *command,
                source_text(source, node),
                body_geometry,
            ));
        }
        NodeKind::Diamond => diamonds += 1,
        _ => {}
    });

    assert_eq!(heredocs.len(), 1, "<<EOF must produce exactly one Heredoc node");
    let (delimiter, content, interpolated, indented, command, opener, body) = &heredocs[0];
    assert_eq!(delimiter, "EOF");
    assert_eq!(content, "hello");
    assert!(*interpolated, "bare heredoc delimiters permit interpolation");
    assert!(!*indented, "plain <<EOF must not be marked as an indented heredoc");
    assert!(!*command, "plain <<EOF must not be marked as command execution");
    assert!(
        opener.as_deref().is_some_and(|text| text.starts_with("<<EOF")),
        "heredoc declaration span must begin at its opener"
    );
    assert_eq!(
        body,
        &Some((22, 27, Some("hello".to_string()))),
        "body_span must retain the exact body bytes represented separately from content"
    );
    assert_eq!(diamonds, 0, "heredoc syntax must not fabricate a Diamond node");
    Ok(())
}

#[test]
fn indirect_filehandle_forms_keep_handle_and_output_list_boundaries() -> Result<(), String> {
    let source = concat!(
        "print $fh \"hello\";\n",
        "print { $fh } \"hello\";\n",
        "printf $fh \"%s\", $value;\n",
    );
    let ast = parse_clean(source)?;
    let mut scalar_handle_print = Vec::new();
    let mut braced_handle_print = Vec::new();
    let mut scalar_handle_printf = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::IndirectCall {
            method,
            object,
            args,
        } = &node.kind
        {
            match method.as_str() {
                "print"
                    if matches!(
                        &object.kind,
                        NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh"
                    ) =>
                {
                    assert_eq!(source_text(source, object).as_deref(), Some("$fh"));
                    assert_eq!(
                        args.len(),
                        1,
                        "print scalar handle must retain one output argument"
                    );
                    assert!(matches!(&args[0].kind, NodeKind::String { .. }));
                    assert_eq!(
                        source_text(source, &args[0]).as_deref(),
                        Some("\"hello\"")
                    );
                    if let Some(text) = source_text(source, node) {
                        scalar_handle_print.push(text);
                    }
                }
                "print" if matches!(&object.kind, NodeKind::Block { .. }) => {
                    assert_eq!(source_text(source, object).as_deref(), Some("{ $fh }"));
                    assert!(
                        subtree_contains(object, &|kind| matches!(
                            kind,
                            NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh"
                        )),
                        "braced print handle lost the $fh expression"
                    );
                    assert_eq!(
                        args.len(),
                        1,
                        "print braced handle must retain one output argument"
                    );
                    assert!(matches!(&args[0].kind, NodeKind::String { .. }));
                    assert_eq!(
                        source_text(source, &args[0]).as_deref(),
                        Some("\"hello\"")
                    );
                    if let Some(text) = source_text(source, node) {
                        braced_handle_print.push(text);
                    }
                }
                "printf"
                    if matches!(
                        &object.kind,
                        NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh"
                    ) =>
                {
                    assert_eq!(source_text(source, object).as_deref(), Some("$fh"));
                    assert_eq!(
                        args.len(),
                        2,
                        "printf must retain format and value arguments"
                    );
                    assert!(matches!(&args[0].kind, NodeKind::String { .. }));
                    assert_eq!(source_text(source, &args[0]).as_deref(), Some("\"%s\""));
                    assert!(matches!(
                        &args[1].kind,
                        NodeKind::Variable { sigil, name } if sigil == "$" && name == "value"
                    ));
                    assert_eq!(source_text(source, &args[1]).as_deref(), Some("$value"));
                    if let Some(text) = source_text(source, node) {
                        scalar_handle_printf.push(text);
                    }
                }
                _ => {}
            }
        }
    });

    assert_eq!(scalar_handle_print, vec!["print $fh \"hello\""]);
    assert_eq!(braced_handle_print, vec!["print { $fh } \"hello\""]);
    assert_eq!(scalar_handle_printf, vec!["printf $fh \"%s\", $value"]);
    Ok(())
}
