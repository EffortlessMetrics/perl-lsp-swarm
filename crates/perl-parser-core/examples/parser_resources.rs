//! Parse one file repeatedly, separating parser work from loading/projection.
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[path = "../tests/support/resource_counter.rs"]
mod resource_counter;

use perl_ast::{NativeDebugSexpLimits, NativeDebugSexpResult};
use perl_parser_core::Parser;
use resource_counter::{CountingAllocator, measure};
use std::error::Error;
use std::hint::black_box;
use std::time::Instant;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: parser_resources FILE [ITERATIONS] [--oracle]")?;
    let mut iterations = 1usize;
    let mut count_supplied = false;
    let mut oracle = false;
    for argument in args {
        if argument == "--oracle" && !oracle {
            oracle = true;
        } else if !count_supplied {
            iterations = argument.parse()?;
            count_supplied = true;
        } else {
            return Err("usage: parser_resources FILE [ITERATIONS] [--oracle]".into());
        }
    }
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    let source = std::fs::read_to_string(&path)?;
    let mut parse_ns = 0u128;
    let mut drop_ns = 0u128;
    let mut calls = 0u64;
    let mut requested_bytes = 0u64;
    for iteration in 0..iterations {
        let start = Instant::now();
        let (output, work) = measure(|| Parser::new(black_box(&source)).parse_with_recovery());
        parse_ns += start.elapsed().as_nanos();
        calls += work.calls;
        requested_bytes += work.requested_bytes;
        if oracle && iteration == 0 {
            // Debug projection is not AST equality or source fidelity. Require
            // complete traversal and record all ordered kinds/spans separately.
            // Projection runs outside both measurement scopes.
            let mut projection = String::new();
            let rendered =
                output.ast.render_debug_sexp(&mut projection, NativeDebugSexpLimits::unbounded());
            if !matches!(rendered, NativeDebugSexpResult::Complete { .. }) {
                return Err(format!("incomplete oracle projection: {rendered:?}").into());
            }
            let mut nodes = vec![&output.ast];
            let mut spans = Vec::new();
            while let Some(node) = nodes.pop() {
                spans.push((
                    node.kind.kind_name(),
                    node.location.start,
                    node.location.end,
                    node.child_count(),
                ));
                let mut children = node.children();
                children.reverse();
                nodes.extend(children);
            }
            println!(
                "oracle={projection}|{spans:?}|{:?}|{:?}|{:?}",
                output.diagnostics,
                output.stop_cause(),
                output.budget_usage
            );
        }
        let start = Instant::now();
        drop(black_box(output));
        drop_ns += start.elapsed().as_nanos();
    }
    println!(
        "bytes={} iterations={iterations} parse_ns={parse_ns} drop_ns={drop_ns} allocation_calls={calls} requested_bytes={requested_bytes}",
        source.len()
    );
    Ok(())
}
