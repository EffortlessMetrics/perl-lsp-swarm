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
//! bench_facade <file> --profile
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
    let profile_mode = args.get(2).is_some_and(|arg| arg == "--profile");
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
            if profile_mode {
                profile_tree(&old_tree, &code, has_error, duration);
                return;
            }
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
            let fresh_start = Instant::now();
            let mut fresh_parser = Parser::new();
            let fresh_outcome = fresh_parser.parse_detailed(&new_source);
            let fresh_duration = fresh_start.elapsed().as_micros();
            let Some(fresh_tree) = fresh_outcome.tree else {
                println!(
                    "status=failure error=true duration_us={} incremental=failed fresh=failed",
                    duration
                );
                std::process::exit(1);
            };
            if incremental.root_node().to_sexp() != fresh_tree.root_node().to_sexp() {
                println!(
                    "status=failure error=true duration_us={} incremental=non_equivalent fresh_edit_duration_us={}",
                    duration, fresh_duration
                );
                std::process::exit(1);
            }
            let Some(metrics) = incremental.incremental_metrics() else {
                println!(
                    "status=failure error=true duration_us={} incremental=unmeasured",
                    duration
                );
                std::process::exit(1);
            };
            println!(
                "status=success error={} duration_us={} incremental_duration_us={} fresh_edit_duration_us={} equivalent=true ast_nodes_reused={} ast_nodes_reparsed={} tokens_reused={} tokens_relexed={}",
                has_error,
                duration,
                incremental_duration,
                fresh_duration,
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

fn profile_tree(tree: &tree_sitter_perl_rs::Tree, source: &str, has_error: bool, parse_us: u128) {
    let traversal_start = Instant::now();
    let mut node_count = 0usize;
    visit_nodes(tree.root_node(), &mut |_node| {
        node_count = node_count.saturating_add(1);
    });
    let traversal_us = traversal_start.elapsed().as_micros();

    let kind_start = Instant::now();
    let mut kind_bytes = 0usize;
    visit_nodes(tree.root_node(), &mut |node| {
        kind_bytes = kind_bytes.saturating_add(node.kind().len());
    });
    let kind_us = kind_start.elapsed().as_micros();

    let position_start = Instant::now();
    let mut position_sum = 0usize;
    visit_nodes(tree.root_node(), &mut |node| {
        let start = node.start_position();
        let end = node.end_position();
        position_sum = position_sum
            .saturating_add(start.row)
            .saturating_add(start.column)
            .saturating_add(end.row)
            .saturating_add(end.column);
    });
    let position_us = position_start.elapsed().as_micros();

    let overlay_start = Instant::now();
    let overlay = tree.semantic_overlay();
    let mut overlay_queries = 0usize;
    let offsets = [0, source.len() / 2, source.len()];
    for offset in offsets {
        let _ = std::hint::black_box(overlay.definition_at_offset(offset));
        let _ = std::hint::black_box(overlay.visible_imports_at_offset(offset));
        let _ = std::hint::black_box(overlay.pragma_state_at_offset(offset));
        overlay_queries = overlay_queries.saturating_add(3);
    }
    let overlay_us = overlay_start.elapsed().as_micros();

    println!(
        "status=success error={} profile=true duration_us={} node_count={} traversal_duration_us={} kind_lookup_duration_us={} position_duration_us={} overlay_duration_us={} overlay_queries={} checksum={}",
        has_error,
        parse_us,
        node_count,
        traversal_us,
        kind_us,
        position_us,
        overlay_us,
        overlay_queries,
        kind_bytes.saturating_add(position_sum),
    );
}

fn visit_nodes(
    node: tree_sitter_perl_rs::Node<'_>,
    visit: &mut impl FnMut(tree_sitter_perl_rs::Node<'_>),
) {
    visit(node);
    for child in node.children() {
        visit_nodes(child, visit);
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
