//! Standalone native perltidy helper: formats Perl source from stdin to
//! stdout using the native engine, as a separate build artifact so the
//! size-capped perllsp binary does not link the formatting engine.
//!
//! Usage: `perllsp-perltidy < input.pl > output.pl`
//! Exit codes: 0 = formatted (or already formatted), 1 = read/write error.

use perl_lsp_perltidy::FormatConfig;
use perl_lsp_perltidy::native::{NativeFormatter, PerlFormatter};
use std::io::{Read, Write};

fn main() -> std::process::ExitCode {
    let mut source = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut source) {
        eprintln!("perllsp-perltidy: failed to read stdin: {error}");
        return std::process::ExitCode::from(1);
    }
    let result = NativeFormatter::new().format_document(&source, &FormatConfig::default());
    for diagnostic in &result.diagnostics {
        eprintln!("perllsp-perltidy: {}", diagnostic.message);
    }
    if std::io::stdout().write_all(result.formatted.as_bytes()).is_err() {
        return std::process::ExitCode::from(1);
    }
    std::process::ExitCode::SUCCESS
}
