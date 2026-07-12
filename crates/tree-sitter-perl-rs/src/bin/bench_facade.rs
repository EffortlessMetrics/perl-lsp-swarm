//! Benchmark binary for the `tree-sitter-perl-rs` facade.
//!
//! Measures wall-clock parse time through the tree-sitter-style ergonomic
//! facade that wraps the v3 native Rust parser (`perl-parser-core`).
//!
//! # Output format
//!
//! ```text
//! status=success error=false duration_us=N
//! ```
//!
//! This matches the output format of `bench_parser_c` and `perl-parser-bench`
//! so that `perl-ci-hygiene`'s `quick-bench` command can consume all three with
//! the same parsing logic.
//!
//! # Usage
//!
//! ```text
//! bench_facade <file>
//! bench_facade <file> --incremental
//! ```
// Benchmark binary — println!/eprintln! are used for structured output consumed by bench harness.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::env;
use std::fs;
use std::time::Instant;

use perl_position_tracking::Position;
use tree_sitter_perl_rs::{InputEdit, Parser};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: bench_facade <file>");
        std::process::exit(1);
    }
    let file_path = &args[1];
    let incremental_mode = args.get(2).is_some_and(|arg| arg == "--incremental");
    let code = fs::read_to_string(file_path).unwrap_or_else(|e| {
        eprintln!("Failed to read file: {}", e);
        std::process::exit(1);
    });

    let mut parser = Parser::new();
    let start = Instant::now();
    let result = parser.parse_detailed(&code);
    let duration = start.elapsed().as_micros();
    let has_error = result.has_error();

    match result.tree {
        Some(mut old_tree) => {
            if !incremental_mode {
                println!("status=success error={} duration_us={}", has_error, duration);
                return;
            }

            let Some((new_source, edit)) = benchmark_edit(&code) else {
                println!(
                    "status=success error={} duration_us={} incremental=skipped",
                    has_error, duration
                );
                return;
            };
            old_tree.edit(&edit);
            let incremental_start = Instant::now();
            let Some(incremental) = parser.parse_with_old_tree(&new_source, &old_tree) else {
                println!("status=failure error=true duration_us={} incremental=failed", duration);
                std::process::exit(1);
            };
            let incremental_duration = incremental_start.elapsed().as_micros();
            let Some(metrics) = incremental.incremental_metrics() else {
                println!(
                    "status=failure error=true duration_us={} incremental=unmeasured",
                    duration
                );
                std::process::exit(1);
            };
            println!(
                "status=success error={} duration_us={} incremental_duration_us={} ast_nodes_reused={} ast_nodes_reparsed={} tokens_reused={} tokens_relexed={}",
                has_error,
                duration,
                incremental_duration,
                metrics.ast_nodes_reused,
                metrics.ast_nodes_reparsed,
                metrics.tokens_reused,
                metrics.tokens_relexed,
            );
        }
        None => {
            println!("status=failure error=true duration_us={}", duration);
            std::process::exit(1);
        }
    }
}

fn benchmark_edit(source: &str) -> Option<(String, InputEdit)> {
    let (_dollar, name_byte) = source.char_indices().find_map(|(index, ch)| {
        (ch == '$').then(|| {
            source[index + 1..]
                .char_indices()
                .next()
                .filter(|(_, next)| next.is_ascii_alphabetic())
                .map(|(offset, _)| (index, index + 1 + offset))
        })
    })??;
    let replacement = if source.as_bytes().get(name_byte) == Some(&b'_') { 'a' } else { '_' };
    let mut new_source = source.to_owned();
    new_source.replace_range(name_byte..name_byte + 1, &replacement.to_string());
    let start = position(source, name_byte);
    let end = position(source, name_byte + 1);
    let edit = InputEdit::new(name_byte, name_byte + 1, name_byte + 1, start, end, end);
    Some((new_source, edit))
}

fn position(source: &str, byte: usize) -> Position {
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let column = prefix.rsplit('\n').next().map_or(prefix.len(), str::len) as u32;
    Position::new(byte, line, column)
}
