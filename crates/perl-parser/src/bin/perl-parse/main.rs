mod cli;
mod report;
mod stats;

use std::fs;
use std::io::{self, Read, Write};
use std::time::Instant;

use perl_parser::{Node, Parser};
use serde::Serialize;

use cli::{
    Args, CliRequest, Input, OutputFormat, ProcessStatus, parse_args, write_help,
    write_usage_error, write_version,
};
use report::write_error;
use stats::TotalStats;

const LEGACY_SUMMARY_SCHEMA: &str = "perl.parse_summary.legacy.v1";
const LEGACY_SUMMARY_SUBJECT: &str = "native_ast_root_summary";
const LEGACY_SUMMARY_LIMITATIONS: &[&str] = &[
    "root_summary_only",
    "not_native_parse_artifact",
    "legacy_native_ast_sexp_is_not_canonical_tree_sitter_output",
    "parser_terminal_source_identity_and_decode_history_are_not_recorded",
];

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ByteRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Serialize)]
struct LegacyParseSummary {
    schema: &'static str,
    subject: &'static str,
    native_root_kind: &'static str,
    root_byte_range: ByteRange,
    node_count: usize,
    legacy_native_ast_sexp: String,
    limitations: &'static [&'static str],
}

/// Run the private CLI against injected writers.
///
/// Write failures return `Err` and are terminal, including under `--continue`.
/// Logical failures (usage, unreadable input without `--continue`, parse or
/// serialization errors) return `Ok(ProcessStatus::Failure)` after the
/// diagnostic has been written. Output-failure status is nonzero; preserving a
/// panic's incidental exit code is not required.
pub(crate) fn execute(
    argv: impl IntoIterator<Item = impl AsRef<str>>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let request = match parse_args(argv) {
        Ok(request) => request,
        Err(error) => {
            write_usage_error(stderr, &error)?;
            return Ok(ProcessStatus::Failure);
        }
    };

    match request {
        CliRequest::Help => {
            write_help(stdout)?;
            Ok(ProcessStatus::Success)
        }
        CliRequest::Version => {
            write_version(stdout)?;
            Ok(ProcessStatus::Success)
        }
        CliRequest::Run(args) => run(args, stdout, stderr),
    }
}

fn run(args: Args, stdout: &mut impl Write, stderr: &mut impl Write) -> io::Result<ProcessStatus> {
    let mut total_stats = TotalStats::new();
    let mut had_error = false;

    for input in &args.inputs {
        let path_str = match input {
            Input::File(path) => path.display().to_string(),
            Input::Stdin => "<stdin>".to_string(),
        };

        if !args.quiet && args.inputs.len() > 1 {
            writeln!(stderr, "=== Parsing {path_str} ===")?;
        }

        let source = match read_input(input) {
            Ok(source) => source,
            Err(error) => {
                writeln!(stderr, "Error reading {path_str}: {error}")?;
                if args.continue_on_error {
                    had_error = true;
                    total_stats.add_error(&path_str);
                    continue;
                }
                return Ok(ProcessStatus::Failure);
            }
        };

        let start = Instant::now();
        let mut parser = Parser::new(&source);
        let result = parser.parse();
        let parse_time = start.elapsed();

        match result {
            Ok(ast) => {
                let node_count = ast.count_nodes();
                if !args.quiet {
                    match render_output(&ast, args.output_format, args.pretty) {
                        Ok(output) => writeln!(stdout, "{output}")?,
                        Err(error) => {
                            writeln!(stderr, "Output serialization error in {path_str}: {error}")?;
                            had_error = true;
                            total_stats.add_error(&path_str);
                            if args.continue_on_error {
                                continue;
                            }
                            return Ok(ProcessStatus::Failure);
                        }
                    }
                }

                total_stats.add_file(&path_str, source.len(), parse_time, node_count);
            }
            Err(error) => {
                if !args.quiet {
                    writeln!(stderr, "\nError in {path_str}:")?;
                    write_error(&error, &source, stderr)?;
                }
                if args.continue_on_error {
                    had_error = true;
                    total_stats.add_error(&path_str);
                } else {
                    return Ok(ProcessStatus::Failure);
                }
            }
        }
    }

    if args.show_stats {
        total_stats.write(stderr)?;
    }

    if had_error { Ok(ProcessStatus::Failure) } else { Ok(ProcessStatus::Success) }
}

fn main() {
    let status = match execute(std::env::args().skip(1), &mut io::stdout(), &mut io::stderr()) {
        Ok(status) => status,
        Err(_) => {
            // A failed stdout/stderr sink must not receive another write. Map
            // the I/O error to a terminal nonzero status at the process boundary.
            ProcessStatus::Failure
        }
    };
    if status != ProcessStatus::Success {
        std::process::exit(status.code());
    }
}

fn render_output(
    ast: &Node,
    output_format: OutputFormat,
    pretty: bool,
) -> Result<String, serde_json::Error> {
    match output_format {
        OutputFormat::LegacySexp => Ok(ast.to_sexp()),
        OutputFormat::LegacyJson => {
            let summary = legacy_parse_summary(ast);
            if pretty {
                serde_json::to_string_pretty(&summary)
            } else {
                serde_json::to_string(&summary)
            }
        }
        OutputFormat::UnstableDebug => Ok(format!("{ast:#?}")),
    }
}

fn legacy_parse_summary(ast: &Node) -> LegacyParseSummary {
    LegacyParseSummary {
        schema: LEGACY_SUMMARY_SCHEMA,
        subject: LEGACY_SUMMARY_SUBJECT,
        native_root_kind: ast.kind.kind_name(),
        root_byte_range: ByteRange { start: ast.location.start, end: ast.location.end },
        node_count: ast.count_nodes(),
        legacy_native_ast_sexp: ast.to_sexp(),
        limitations: LEGACY_SUMMARY_LIMITATIONS,
    }
}

fn read_input(input: &Input) -> io::Result<String> {
    match input {
        Input::File(path) => read_source_bytes(fs::read(path)?),
        Input::Stdin => {
            let mut buffer = Vec::new();
            io::stdin().read_to_end(&mut buffer)?;
            read_source_bytes(buffer)
        }
    }
}

fn read_source_bytes(bytes: Vec<u8>) -> io::Result<String> {
    if let Some(decoded) = decode_utf16_with_bom(&bytes) {
        return Ok(decoded);
    }

    match String::from_utf8(bytes) {
        Ok(source) => Ok(repair_common_mojibake(source)),
        Err(err) => {
            let raw = err.into_bytes();
            let mut decoded = String::with_capacity(raw.len());
            for byte in raw {
                decoded.push(decode_byte_as_windows_1252(byte));
            }
            Ok(decoded)
        }
    }
}

fn decode_utf16_with_bom(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }

    let little_endian = if bytes.starts_with(&[0xFF, 0xFE]) {
        true
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        false
    } else {
        return None;
    };

    let mut words = Vec::with_capacity((bytes.len().saturating_sub(2)) / 2);
    let mut index = 2usize;
    while index + 1 < bytes.len() {
        let word = if little_endian {
            u16::from_le_bytes([bytes[index], bytes[index + 1]])
        } else {
            u16::from_be_bytes([bytes[index], bytes[index + 1]])
        };
        words.push(word);
        index += 2;
    }

    Some(String::from_utf16_lossy(&words))
}

fn repair_common_mojibake(source: String) -> String {
    if mojibake_score(&source) == 0 {
        return source;
    }

    let mut latin1_bytes = Vec::with_capacity(source.len());
    for ch in source.chars() {
        let codepoint = u32::from(ch);
        if codepoint > u32::from(u8::MAX) {
            return source;
        }
        latin1_bytes.push(codepoint as u8);
    }

    match String::from_utf8(latin1_bytes) {
        Ok(repaired) if mojibake_score(&repaired) < mojibake_score(&source) => repaired,
        _ => source,
    }
}

fn mojibake_score(text: &str) -> usize {
    // Common mojibake marker characters produced by decoding UTF-8 as Latin-1/CP-1252.
    const MARKERS: [char; 4] = ['Ã', 'Â', 'â', '\u{FFFD}'];
    text.chars().filter(|ch| MARKERS.contains(ch)).count()
}

fn decode_byte_as_windows_1252(byte: u8) -> char {
    match byte {
        0x80 => '\u{20AC}', // €
        0x82 => '\u{201A}', // ‚
        0x83 => '\u{0192}', // ƒ
        0x84 => '\u{201E}', // „
        0x85 => '\u{2026}', // …
        0x86 => '\u{2020}', // †
        0x87 => '\u{2021}', // ‡
        0x88 => '\u{02C6}', // ˆ
        0x89 => '\u{2030}', // ‰
        0x8A => '\u{0160}', // Š
        0x8B => '\u{2039}', // ‹
        0x8C => '\u{0152}', // Œ
        0x8E => '\u{017D}', // Ž
        0x91 => '\u{2018}', // ‘
        0x92 => '\u{2019}', // ’
        0x93 => '\u{201C}', // “
        0x94 => '\u{201D}', // ”
        0x95 => '\u{2022}', // •
        0x96 => '\u{2013}', // –
        0x97 => '\u{2014}', // —
        0x98 => '\u{02DC}', // ˜
        0x99 => '\u{2122}', // ™
        0x9A => '\u{0161}', // š
        0x9B => '\u{203A}', // ›
        0x9C => '\u{0153}', // œ
        0x9E => '\u{017E}', // ž
        0x9F => '\u{0178}', // Ÿ
        _ => char::from(byte),
    }
}

#[cfg(test)]
mod tests;
