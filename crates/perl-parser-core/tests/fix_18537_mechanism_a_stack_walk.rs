//! Issue #18537 mechanism A: recursive `contains_heredoc` / `contains_left_shift`
//! walks on left-deep binary trees overflowed small stacks during statement
//! terminator handling. These inputs must complete without process abort.

mod cpan_test_helpers;

use perl_parser_core::Parser;
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

fn parse_on_one_mib_stack(source: &str) -> Result<(), String> {
    let source = source.to_owned();
    let handle = thread::Builder::new()
        .stack_size(ONE_MIB_STACK)
        .spawn(move || {
            let mut parser = Parser::new(&source);
            parser.parse()
        })
        .map_err(|e| format!("failed to spawn parser thread: {e}"))?;
    match handle.join() {
        Ok(Ok(_ast)) => Ok(()),
        Ok(Err(e)) => Err(format!("parse returned error: {e:?}")),
        Err(_) => Err("parser thread aborted (likely stack overflow)".into()),
    }
}

#[test]
fn deep_add_chain_does_not_abort_on_statement_terminator_walk() {
    parse_on_one_mib_stack(&build_add_chain(DEEP_TERMS))
        .expect("left-deep addition chain must not overflow the stack");
}

#[test]
fn deep_dot_chain_does_not_abort_on_statement_terminator_walk() {
    let mut expr = String::from("'a'");
    for _ in 0..DEEP_TERMS {
        expr.push_str(" . 'b'");
    }
    let source = format!("my $x = {};", expr);
    parse_on_one_mib_stack(&source)
        .expect("left-deep concatenation chain must not overflow the stack");
}

#[test]
fn deep_and_chain_does_not_abort_on_statement_terminator_walk() {
    let mut expr = String::from("1");
    for _ in 0..DEEP_TERMS {
        expr.push_str(" && 1");
    }
    let source = format!("my $x = {};", expr);
    parse_on_one_mib_stack(&source)
        .expect("left-deep logical-and chain must not overflow the stack");
}
