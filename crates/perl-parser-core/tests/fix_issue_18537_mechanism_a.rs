//! Issue #18537 mechanism A: post-parse AST walks in `statements.rs` must not recurse
//! on left-deep binary chains (uncatchable stack overflow on 1 MiB stacks).

use perl_parser_core::Parser;
use std::thread;
use std::time::Duration;

fn deep_binary_chain(op: &str, terms: usize) -> String {
    let expr = format!("{}1", format!("1{op}").repeat(terms.saturating_sub(1)));
    format!("my $x = {expr};")
}

fn parse_on_one_mib_stack(source: String) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            let mut parser = Parser::new(&source);
            let result = parser.parse_with_recovery();
            let _ = tx.send(result.is_ok());
        })
        .expect("spawn parse thread");
    rx.recv_timeout(Duration::from_secs(60)).expect("parse thread did not finish in time")
}

#[test]
fn deep_add_chain_20000_terms_on_one_mib_stack() {
    assert!(parse_on_one_mib_stack(deep_binary_chain("+", 20_000)));
}

#[test]
fn deep_dot_chain_20000_terms_on_one_mib_stack() {
    assert!(parse_on_one_mib_stack(deep_binary_chain(".", 20_000)));
}

#[test]
fn deep_and_chain_20000_terms_on_one_mib_stack() {
    assert!(parse_on_one_mib_stack(deep_binary_chain("&&", 20_000)));
}
