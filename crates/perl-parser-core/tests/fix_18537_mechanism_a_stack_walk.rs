//! Issue #18537 mechanism A: recursive `contains_heredoc` / `contains_left_shift`
//! walks on left-deep binary trees overflowed small stacks during statement
//! terminator handling. These inputs must complete without process abort.

mod cpan_test_helpers;

use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::must_with;
use std::thread;

const ONE_MIB_STACK: usize = 1024 * 1024;
const DEEP_TERMS: usize = 20_000;

fn build_add_chain(terms: usize) -> String {
    let mut expr = String::from("1");
    for _ in 1..terms {
        expr.push_str("+1");
    }
    format!("my $x = {};", expr)
}

fn assert_ast_clean_iterative(root: &Node) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(
            &node.kind,
            NodeKind::Error { .. }
                | NodeKind::MissingExpression
                | NodeKind::MissingStatement
                | NodeKind::MissingIdentifier
                | NodeKind::MissingBlock
        ) {
            panic!("unexpected AST error node: {}", node.kind.kind_name());
        }
        stack.extend(node.children());
    }
}

fn count_binary_ops_iterative(root: &Node, op: &str) -> usize {
    let mut stack = vec![root];
    let mut count = 0usize;
    while let Some(node) = stack.pop() {
        if matches!(&node.kind, NodeKind::Binary { op: node_op, .. } if node_op == op) {
            count += 1;
        }
        stack.extend(node.children());
    }
    count
}

fn parse_and_validate_on_one_mib_stack(
    source: &str,
    expected_binary_op_count: Option<(&'static str, usize)>,
) -> Result<(), String> {
    let source = source.to_owned();
    let handle = thread::Builder::new()
        .stack_size(ONE_MIB_STACK)
        .spawn(move || {
            let mut parser = Parser::new(&source);
            let ast = parser.parse().map_err(|e| format!("parse returned error: {e:?}"))?;
            if !parser.get_errors().is_empty() {
                return Err(format!("unexpected parser diagnostics: {:?}", parser.get_errors()));
            }
            assert_ast_clean_iterative(&ast);
            if let Some((op, expected)) = expected_binary_op_count {
                let got = count_binary_ops_iterative(&ast, op);
                if got != expected {
                    return Err(format!("expected {expected} `{op}` binaries, got {got}"));
                }
            }
            Ok(())
        })
        .map_err(|e| format!("failed to spawn parser thread: {e}"))?;
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err("parser thread aborted (likely stack overflow)".into()),
    }
}

#[test]
fn deep_add_chain_does_not_abort_on_statement_terminator_walk() {
    must_with(
        parse_and_validate_on_one_mib_stack(
            &build_add_chain(DEEP_TERMS),
            Some(("+", DEEP_TERMS - 1)),
        ),
        "left-deep addition chain must not overflow the stack",
    );
}

#[test]
fn deep_dot_chain_does_not_abort_on_statement_terminator_walk() {
    let mut expr = String::from("'a'");
    for _ in 0..DEEP_TERMS {
        expr.push_str(" . 'b'");
    }
    let source = format!("my $x = {};", expr);
    must_with(
        parse_and_validate_on_one_mib_stack(&source, Some((".", DEEP_TERMS))),
        "left-deep concatenation chain must not overflow the stack",
    );
}

#[test]
fn deep_and_chain_does_not_abort_on_statement_terminator_walk() {
    let mut expr = String::from("1");
    for _ in 0..DEEP_TERMS {
        expr.push_str(" && 1");
    }
    let source = format!("my $x = {};", expr);
    must_with(
        parse_and_validate_on_one_mib_stack(&source, Some(("&&", DEEP_TERMS))),
        "left-deep logical-and chain must not overflow the stack",
    );
}

#[test]
fn deep_identifier_left_shift_and_chain_does_not_abort_on_statement_terminator_walk() {
    let mut expr = String::from("foo << 1");
    for _ in 0..DEEP_TERMS {
        expr.push_str(" && 1");
    }
    let source = format!("my $x = {};", expr);
    must_with(
        parse_and_validate_on_one_mib_stack(&source, Some(("&&", DEEP_TERMS))),
        "identifier-left-shift with deep && chain must not overflow the stack",
    );
}
