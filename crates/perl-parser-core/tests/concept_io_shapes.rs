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
    let mut stdin_readline = 0usize;
    let mut diamonds = 0usize;
    let mut globs = 0usize;
    let mut shifts = 0usize;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Readline { filehandle: Some(filehandle) } if filehandle == "STDIN" => {
            stdin_readline += 1;
        }
        NodeKind::Diamond => diamonds += 1,
        NodeKind::Glob { .. } => globs += 1,
        NodeKind::Binary { op, .. } if op == "<<" => shifts += 1,
        _ => {}
    });

    assert_eq!(stdin_readline, 1, "<STDIN> must remain a named Readline");
    assert_eq!(diamonds, 2, "<> and <<>> must remain Diamond nodes");
    assert_eq!(globs, 1, "<*.pl> must remain a Glob node");
    assert_eq!(shifts, 1, "$value << 2 must remain a shift expression");
    Ok(())
}

#[test]
fn heredoc_opener_remains_distinct_from_diamond() -> Result<(), String> {
    let source = "my $document = <<EOF;\nhello\nEOF\n";
    let ast = parse_clean(source)?;
    let mut heredocs = 0usize;
    let mut diamonds = 0usize;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Heredoc { delimiter, .. } if delimiter == "EOF" => heredocs += 1,
        NodeKind::Diamond => diamonds += 1,
        _ => {}
    });

    assert_eq!(heredocs, 1, "<<EOF must remain a Heredoc");
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
    let mut scalar_handle_print = 0usize;
    let mut braced_handle_print = 0usize;
    let mut scalar_handle_printf = 0usize;

    walk(&ast, &mut |node| {
        if let NodeKind::IndirectCall { method, object, args } = &node.kind {
            match method.as_str() {
                "print" if matches!(&object.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh") => {
                    assert_eq!(args.len(), 1, "print scalar handle must retain one output argument");
                    scalar_handle_print += 1;
                }
                "print" if matches!(&object.kind, NodeKind::Block { .. }) => {
                    assert_eq!(args.len(), 1, "print braced handle must retain one output argument");
                    braced_handle_print += 1;
                }
                "printf" if matches!(&object.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh") => {
                    assert_eq!(args.len(), 2, "printf must retain format and value arguments");
                    scalar_handle_printf += 1;
                }
                _ => {}
            }
        }
    });

    assert_eq!(scalar_handle_print, 1, "print $fh LIST lost its indirect-filehandle shape");
    assert_eq!(braced_handle_print, 1, "print { $fh } LIST lost its braced-handle shape");
    assert_eq!(scalar_handle_printf, 1, "printf $fh FORMAT, LIST lost its handle boundary");
    Ok(())
}
