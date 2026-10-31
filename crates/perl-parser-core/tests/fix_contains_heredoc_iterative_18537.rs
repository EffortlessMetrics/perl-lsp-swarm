// Issue #18537 mechanism A: recursive contains_heredoc / contains_left_shift walks
// overflowed a 1 MiB stack on left-deep binary expression trees.

mod cpan_test_helpers;

use std::thread;

use cpan_test_helpers::parse;
use perl_parser_core::Parser;

const SMALL_STACK: usize = 1024 * 1024;
const DEEP_BINARY_TERMS: usize = 20_000;

fn build_left_deep_chain(separator: &str, terms: usize) -> String {
    let mut source = String::from("my $x = 1");
    for _ in 0..terms {
        source.push_str(separator);
        source.push('1');
    }
    source.push_str(";\n");
    source
}

fn parse_on_small_stack(source: &str) -> Result<(), String> {
    let source = source.to_string();
    let handle = thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let mut parser = Parser::new(&source);
            parser.parse().map(|_| ()).map_err(|e| format!("{e:?}"))
        })
        .map_err(|e| format!("spawn failed: {e}"))?;
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err("thread panicked or aborted (stack overflow)".into()),
    }
}

#[test]
fn deep_plus_chain_parses_on_one_mib_stack() {
    let source = build_left_deep_chain("+", DEEP_BINARY_TERMS);
    parse_on_small_stack(&source).expect("deep + chain must not abort on a 1 MiB stack");
}

#[test]
fn deep_dot_chain_parses_on_one_mib_stack() {
    let source = build_left_deep_chain(".", DEEP_BINARY_TERMS);
    parse_on_small_stack(&source).expect("deep . chain must not abort on a 1 MiB stack");
}

#[test]
fn deep_and_chain_parses_on_one_mib_stack() {
    let source = build_left_deep_chain("&&", DEEP_BINARY_TERMS);
    parse_on_small_stack(&source).expect("deep && chain must not abort on a 1 MiB stack");
}

#[test]
fn heredoc_recovery_fixture_still_parses_cleanly() {
    let source = r#"print <<'EOF';
body
EOF
"#;
    parse(source);
}
