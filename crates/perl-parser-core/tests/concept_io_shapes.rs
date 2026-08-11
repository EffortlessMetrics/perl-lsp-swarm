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

#[test]
fn angle_forms_do_not_collapse_into_shift_or_heredoc() -> Result<(), String> {
    let source = concat!(
        "my $stdin = <STDIN>;\n",
        "my $diamond = <>;\n",
        "my $safe_diamond = <<>>;\n",
        "my @files = <*.pl>;\n",
        "my $shifted = $value << 2;\n",
    );
    let ast = parse_clean(source)?;
    let mut stdin_readlines = Vec::new();
    let mut diamonds = 0usize;
    let mut globs = Vec::new();
    let mut shifts = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Readline { filehandle: Some(filehandle) } if filehandle == "STDIN" => {
            if let Some(text) = source_text(source, node) {
                stdin_readlines.push(text);
            }
        }
        NodeKind::Diamond => diamonds += 1,
        NodeKind::Glob { .. } => {
            if let Some(text) = source_text(source, node) {
                globs.push(text);
            }
        }
        NodeKind::Binary { op, .. } if op == "<<" => {
            if let Some(text) = source_text(source, node) {
                shifts.push(text);
            }
        }
        _ => {}
    });

    assert_eq!(stdin_readlines, vec!["<STDIN>"]);
    assert_eq!(diamonds, 2, "<> and <<>> must remain Diamond nodes");
    assert_eq!(globs, vec!["<*.pl>"]);
    assert_eq!(shifts, vec!["$value << 2"]);
    Ok(())
}

#[test]
fn heredoc_opener_remains_distinct_from_diamond() -> Result<(), String> {
    let source = "my $document = <<EOF;\nhello\nEOF\n";
    let ast = parse_clean(source)?;
    let mut heredoc_spans = Vec::new();
    let mut diamonds = 0usize;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Heredoc { delimiter, .. } if delimiter == "EOF" => {
            if let Some(text) = source_text(source, node) {
                heredoc_spans.push(text);
            }
        }
        NodeKind::Diamond => diamonds += 1,
        _ => {}
    });

    assert_eq!(heredoc_spans.len(), 1, "<<EOF must remain a Heredoc");
    assert!(heredoc_spans[0].starts_with("<<EOF"));
    assert!(heredoc_spans[0].contains("hello"));
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
        if let NodeKind::IndirectCall { method, object, args } = &node.kind {
            match method.as_str() {
                "print" if matches!(&object.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh") => {
                    assert_eq!(args.len(), 1, "print scalar handle must retain one output argument");
                    if let Some(text) = source_text(source, node) {
                        scalar_handle_print.push(text);
                    }
                }
                "print" if matches!(&object.kind, NodeKind::Block { .. }) => {
                    assert_eq!(args.len(), 1, "print braced handle must retain one output argument");
                    if let Some(text) = source_text(source, node) {
                        braced_handle_print.push(text);
                    }
                }
                "printf" if matches!(&object.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh") => {
                    assert_eq!(args.len(), 2, "printf must retain format and value arguments");
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
