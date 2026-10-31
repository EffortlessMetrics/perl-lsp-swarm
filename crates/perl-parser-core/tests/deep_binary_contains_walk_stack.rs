//! Mechanism A (#18537): post-parse `contains_*` AST walks must not overflow small stacks.

mod cpan_test_helpers;

use perl_parser_core::Parser;

const SMALL_STACK_BYTES: usize = 1024 * 1024;

fn left_deep_add_chain(terms: usize) -> String {
    let mut expr = String::from("1");
    for i in 0..terms.saturating_sub(1) {
        expr.push('+');
        expr.push(char::from(b'0' + (i % 10) as u8));
    }
    format!("my $x = {expr};")
}

fn left_deep_dot_chain(terms: usize) -> String {
    let mut expr = String::from("'a'");
    for _ in 1..terms {
        expr.push('.');
        expr.push('b');
    }
    format!("my $x = {expr};")
}

fn left_deep_and_chain(terms: usize) -> String {
    let mut expr = String::from("1");
    for _ in 1..terms {
        expr.push_str("&&1");
    }
    format!("my $x = {expr};")
}

fn parse_on_small_stack(source: &str) -> Result<(), String> {
    let source = source.to_owned();
    let handle = std::thread::Builder::new()
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || {
            let mut parser = Parser::new(&source);
            parser.parse().map(|_| ())
        })
        .map_err(|error| format!("failed to spawn small-stack worker: {error}"))?;
    match handle.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("parse error (acceptable): {error:?}")),
        Err(_) => Err("small-stack worker aborted (stack overflow)".to_string()),
    }
}

#[test]
fn deep_add_chain_parses_without_stack_overflow_on_one_mib_stack() {
    let source = left_deep_add_chain(20_000);
    parse_on_small_stack(&source).unwrap_or_else(|message| {
        panic!("{message}; source length {}", source.len());
    });
}

#[test]
fn deep_dot_chain_parses_without_stack_overflow_on_one_mib_stack() {
    let source = left_deep_dot_chain(20_000);
    parse_on_small_stack(&source).unwrap_or_else(|message| {
        panic!("{message}; source length {}", source.len());
    });
}

#[test]
fn deep_and_chain_parses_without_stack_overflow_on_one_mib_stack() {
    let source = left_deep_and_chain(20_000);
    parse_on_small_stack(&source).unwrap_or_else(|message| {
        panic!("{message}; source length {}", source.len());
    });
}
