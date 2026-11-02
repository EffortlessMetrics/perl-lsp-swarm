//! Criterion throughput for `ExportSymbolExtractor` on large `qw` export lists.
//!
//! Covers the multi-byte delimiter slicing seam exercised by #17988 / #18020; the
//! crate had correctness tests but no repeatable timing surface (#18031 §3).

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::export_analyzer::ExportSymbolExtractor;
use std::hint::black_box;

fn exporter_with_export_ok_parens(symbol_count: usize) -> String {
    let symbols: Vec<String> = (0..symbol_count).map(|i| format!("export_sym_{i}")).collect();
    format!(
        "package Bench::ExportQw::Paren;\n\
         use Exporter 'import';\n\
         our @EXPORT_OK = qw({});\n\
         1;\n",
        symbols.join(" ")
    )
}

fn exporter_with_export_ok_multibyte_delim(symbol_count: usize) -> String {
    let symbols: Vec<String> = (0..symbol_count).map(|i| format!("export_sym_{i}")).collect();
    format!(
        "package Bench::ExportQw::Section;\n\
         use Exporter 'import';\n\
         our @EXPORT_OK = qw§{}§;\n\
         1;\n",
        symbols.join(" ")
    )
}

fn extract_export_info(source: &str) -> usize {
    let mut parser = Parser::new(source);
    let ast = parser.parse().expect("fixture must parse");
    let info = ExportSymbolExtractor::extract(&ast).expect("exporter fixture must extract");
    info.optional_export.len()
}

fn bench_export_qw_extract(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_qw_extract");

    for count in [32_usize, 128, 512] {
        let paren_source = exporter_with_export_ok_parens(count);
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("paren_delimiter", count),
            &paren_source,
            |b, src| {
                b.iter(|| black_box(extract_export_info(src)));
            },
        );

        let section_source = exporter_with_export_ok_multibyte_delim(count);
        group.bench_with_input(
            BenchmarkId::new("section_delimiter", count),
            &section_source,
            |b, src| {
                b.iter(|| black_box(extract_export_info(src)));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_export_qw_extract);
criterion_main!(benches);
