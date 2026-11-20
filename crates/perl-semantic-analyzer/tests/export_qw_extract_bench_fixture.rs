//! Fixture contract for `export_qw_benchmark`: large `qw` lists must round-trip
//! through `ExportSymbolExtractor` without losing symbols (#18031 §3).

#![deny(clippy::map_err_ignore)]

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::export_analyzer::ExportSymbolExtractor;
use perl_tdd_support::{must, must_some_with};
use std::error::Error;

fn exporter_source(delim: &str, symbol_count: usize) -> Result<String, Box<dyn Error>> {
    let symbols: Vec<String> = (0..symbol_count).map(|i| format!("sym_{i}")).collect();
    let body = symbols.join(" ");
    match delim {
        "paren" => Ok(format!(
            "package Bench::Fixture;\nuse Exporter 'import';\nour @EXPORT_OK = qw({body});\n1;\n"
        )),
        "section" => Ok(format!(
            "package Bench::Fixture;\nuse Exporter 'import';\nour @EXPORT_OK = qw§{body}§;\n1;\n"
        )),
        other => Err(format!("unknown delimiter fixture {other}").into()),
    }
}

fn extract_optional_export_count(source: &str) -> Result<usize, Box<dyn Error>> {
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let info = ExportSymbolExtractor::extract(&ast)
        .ok_or_else(|| "expected Exporter-based module".to_string())?;
    Ok(info.optional_export.len())
}

#[test]
fn large_paren_qw_export_ok_yields_every_symbol() -> Result<(), Box<dyn Error>> {
    let source = exporter_source("paren", 512)?;
    assert_eq!(extract_optional_export_count(&source)?, 512);
    let mut parser = Parser::new(&source);
    let ast = must(parser.parse());
    let info = must_some_with(ExportSymbolExtractor::extract(&ast), "extract large paren fixture");
    assert!(info.optional_export.contains("sym_0"));
    assert!(info.optional_export.contains("sym_511"));
    Ok(())
}

#[test]
fn large_section_delimiter_qw_export_ok_yields_every_symbol() -> Result<(), Box<dyn Error>> {
    let source = exporter_source("section", 256)?;
    assert_eq!(extract_optional_export_count(&source)?, 256);
    Ok(())
}
