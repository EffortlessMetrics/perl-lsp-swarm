use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use perl_parser::{Node, ParseError, Parser};
use serde::Serialize;

const LEGACY_SUMMARY_SCHEMA: &str = "perl.parse_summary.legacy.v1";
const LEGACY_SUMMARY_SUBJECT: &str = "native_ast_root_summary";
const LEGACY_SUMMARY_LIMITATIONS: &[&str] = &[
    "root_summary_only",
    "not_native_parse_artifact",
    "legacy_native_ast_sexp_is_not_canonical_tree_sitter_output",
    "parser_terminal_source_identity_and_decode_history_are_not_recorded",
];

#[derive(Default)]
struct TotalStats {
    files_parsed: usize,
    files_failed: usize,
    total_bytes: usize,
    total_time: std::time::Duration,
    total_nodes: usize,
    file_details: Vec<FileStats>,
}

struct FileStats {
    name: String,
    bytes: usize,
    time: std::time::Duration,
    nodes: usize,
    error: bool,
}

impl TotalStats {
    fn new() -> Self {
        Self::default()
    }

    fn add_file(&mut self, name: &str, bytes: usize, time: std::time::Duration, nodes: usize) {
        self.files_parsed += 1;
        self.total_bytes += bytes;
        self.total_time += time;
        self.total_nodes += nodes;
        self.file_details.push(FileStats {
            name: name.to_string(),
            bytes,
            time,
            nodes,
            error: false,
        });
    }

    fn add_error(&mut self, name: &str) {
        self.files_failed += 1;
        self.file_details.push(FileStats {
            name: name.to_string(),
            bytes: 0,
            time: std::time::Duration::ZERO,
            nodes: 0,
            error: true,
        });
    }

    fn print(&self) {
        eprintln!("\n=== Total Statistics ===");
        eprintln!("Files parsed: {}", self.files_parsed);
        eprintln!("Files failed: {}", self.files_failed);
        eprintln!(
            "Total size: {} bytes ({:.2} KB)",
            self.total_bytes,
            self.total_bytes as f64 / 1024.0
        );
        eprintln!("Total time: {:?}", self.total_time);
        eprintln!("Total nodes: {}", self.total_nodes);

        if self.files_parsed > 0 {
            let avg_speed = self.total_bytes as f64 / self.total_time.as_secs_f64() / 1_000_000.0;
            eprintln!("Average speed: {:.2} MB/s", avg_speed);
            eprintln!("Average nodes per file: {}", self.total_nodes / self.files_parsed);
        }

        if self.file_details.len() > 1 && self.file_details.len() <= 20 {
            eprintln!("\n=== File Details ===");
            for stat in &self.file_details {
                if stat.error {
                    eprintln!("{}: FAILED", stat.name);
                } else {
                    eprintln!(
                        "{}: {} bytes, {:?}, {} nodes",
                        stat.name, stat.bytes, stat.time, stat.nodes
                    );
                }
            }
        }
    }
}

#[derive(Debug)]
struct Args {
    inputs: Vec<Input>,
    output_format: OutputFormat,
    show_stats: bool,
    pretty: bool,
    quiet: bool,
    continue_on_error: bool,
}

#[derive(Debug)]
enum Input {
    File(PathBuf),
    Stdin,
}

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    LegacySexp,
    LegacyJson,
    UnstableDebug,
}

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

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut inputs = Vec::new();
        let mut output_format = OutputFormat::LegacySexp;
        let mut show_stats = false;
        let mut pretty = false;
        let mut quiet = false;
        let mut continue_on_error = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "-V" | "--version" => {
                    println!("perl-parse v{}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                "-f" | "--format" => {
                    let format = args.next().ok_or("Missing format argument")?;
                    output_format = match format.as_str() {
                        "sexp" | "s-expression" => OutputFormat::LegacySexp,
                        "json" => OutputFormat::LegacyJson,
                        "debug" => OutputFormat::UnstableDebug,
                        _ => return Err(format!("Unknown format: {}", format)),
                    };
                }
                "-s" | "--stats" => show_stats = true,
                "-p" | "--pretty" => pretty = true,
                "-q" | "--quiet" => quiet = true,
                "-c" | "--continue" => continue_on_error = true,
                "-" => inputs.push(Input::Stdin),
                path if path.starts_with('-') => {
                    return Err(format!("Unknown option: {}", path));
                }
                path => {
                    inputs.push(Input::File(PathBuf::from(path)));
                }
            }
        }

        if inputs.is_empty() {
            inputs.push(Input::Stdin);
        }

        Ok(Args {
            inputs,
            output_format,
            show_stats,
            pretty,
            quiet,
            continue_on_error,
        })
    }
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> &'static str {
    r#"perl-parse - Parse Perl code and render a selected parser projection

USAGE:
    perl-parse [OPTIONS] [FILE...]

ARGS:
    <FILE>...    Path(s) to Perl file(s) to parse (use '-' for stdin)

OPTIONS:
    -h, --help              Print help information
    -V, --version           Print version information
    -f, --format <FORMAT>   Output projection [default: sexp]
                           sexp  Legacy native-AST S-expression; transitional,
                                 not canonical Tree-sitter output
                           json  Versioned legacy root summary;
                                 not NativeParseArtifact
                           debug Unstable human-only Rust Debug output
    -s, --stats             Show parsing statistics
    -p, --pretty            Pretty-print JSON output
    -q, --quiet             Suppress output (useful with --stats)
    -c, --continue          Continue on error when parsing multiple files

EXAMPLES:
    # Render the transitional native-AST S-expression
    perl-parse script.pl

    # Parse from stdin
    echo 'print "Hello"' | perl-parse -

    # Render the versioned legacy JSON root summary with statistics
    perl-parse -f json -s script.pl

    # Parse multiple files, show only stats
    perl-parse -q -s *.pl

    # Pretty-print the versioned legacy JSON root summary
    perl-parse -f json -p script.pl
"#
}

fn main() {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("Try 'perl-parse --help' for more information.");
            std::process::exit(1);
        }
    };

    let mut total_stats = TotalStats::new();
    let mut had_error = false;

    for input in &args.inputs {
        let path_str = match input {
            Input::File(path) => path.display().to_string(),
            Input::Stdin => "<stdin>".to_string(),
        };

        if !args.quiet && args.inputs.len() > 1 {
            eprintln!("=== Parsing {} ===", path_str);
        }

        let source = match read_input(input) {
            Ok(source) => source,
            Err(e) => {
                eprintln!("Error reading {}: {}", path_str, e);
                if args.continue_on_error {
                    had_error = true;
                    continue;
                } else {
                    std::process::exit(1);
                }
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
                        Ok(output) => println!("{output}"),
                        Err(error) => {
                            eprintln!("Output serialization error in {}: {}", path_str, error);
                            had_error = true;
                            total_stats.add_error(&path_str);
                            if args.continue_on_error {
                                continue;
                            }
                            std::process::exit(1);
                        }
                    }
                }

                total_stats.add_file(&path_str, source.len(), parse_time, node_count);
            }
            Err(e) => {
                if !args.quiet {
                    eprintln!("\nError in {}:", path_str);
                    print_error(&e, &source);
                }
                if args.continue_on_error {
                    had_error = true;
                    total_stats.add_error(&path_str);
                } else {
                    std::process::exit(1);
                }
            }
        }
    }

    if args.show_stats {
        total_stats.print();
    }

    if had_error {
        std::process::exit(1);
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
        root_byte_range: ByteRange {
            start: ast.location.start,
            end: ast.location.end,
        },
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

fn print_error(error: &ParseError, source: &str) {
    let mut stderr = io::stderr();

    match error {
        ParseError::UnexpectedToken {
            expected,
            found,
            location,
        } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(
                stderr,
                "Parse error: Unexpected token at line {}, column {}",
                line, col
            )
            .ok();
            writeln!(stderr, "  Expected: {}", expected).ok();
            writeln!(stderr, "  Found: {}", found).ok();
            print_error_context(source, *location, &mut stderr);
        }
        ParseError::UnexpectedEof => {
            writeln!(stderr, "Parse error: Unexpected end of input").ok();
            if !source.is_empty() {
                print_error_context(source, source.len() - 1, &mut stderr);
            }
        }
        ParseError::SyntaxError { message, location } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(
                stderr,
                "Parse error: {} at line {}, column {}",
                message, line, col
            )
            .ok();
            print_error_context(source, *location, &mut stderr);
        }
        ParseError::Advisory { message, location } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(
                stderr,
                "Parse advisory: {} at line {}, column {}",
                message, line, col
            )
            .ok();
            print_error_context(source, *location, &mut stderr);
        }
        ParseError::InvalidNumber { literal } => {
            writeln!(stderr, "Parse error: Invalid number literal: {}", literal).ok();
        }
        ParseError::InvalidString => {
            writeln!(stderr, "Parse error: Invalid string literal").ok();
        }
        ParseError::UnclosedDelimiter { delimiter } => {
            writeln!(stderr, "Parse error: Unclosed delimiter: {}", delimiter).ok();
        }
        ParseError::InvalidRegex { message } => {
            writeln!(stderr, "Parse error: Invalid regex: {}", message).ok();
        }
        ParseError::LexerError { message } => {
            writeln!(stderr, "Parse error: Lexer error: {}", message).ok();
        }
        ParseError::RecursionLimit => {
            writeln!(stderr, "Parse error: Maximum recursion depth exceeded").ok();
        }
        ParseError::NestingTooDeep {
            depth,
            max_depth,
        } => {
            writeln!(
                stderr,
                "Parse error: Nesting too deep ({} > {})",
                depth, max_depth
            )
            .ok();
        }
        ParseError::Cancelled => {
            writeln!(stderr, "Parse error: Parsing cancelled").ok();
        }
        ParseError::Recovered {
            site,
            kind,
            location,
        } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(
                stderr,
                "Parse recovery: {:?} at {:?} (line {}, column {})",
                kind, site, line, col
            )
            .ok();
            print_error_context(source, *location, &mut stderr);
        }
        // Forward-compatible fallback for future variants (#2898)
        _ => {
            writeln!(stderr, "Parse error: {}", error).ok();
        }
    }
}

fn position_to_line_col(source: &str, position: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;

    for (i, ch) in source.chars().enumerate() {
        if i >= position {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}

fn print_error_context(source: &str, position: usize, stderr: &mut io::Stderr) {
    let lines: Vec<&str> = source.lines().collect();
    let (line_num, col_num) = position_to_line_col(source, position);

    if line_num > 0 && line_num <= lines.len() {
        writeln!(stderr).ok();

        // Show previous line if available
        if line_num > 1 {
            writeln!(stderr, "  {} | {}", line_num - 1, lines[line_num - 2]).ok();
        }

        // Show error line
        writeln!(stderr, "  {} | {}", line_num, lines[line_num - 1]).ok();

        // Show error pointer
        write!(stderr, "  {} | ", " ".repeat(line_num.to_string().len())).ok();
        writeln!(stderr, "{}^", " ".repeat(col_num - 1)).ok();

        // Show next line if available
        if line_num < lines.len() {
            writeln!(stderr, "  {} | {}", line_num + 1, lines[line_num]).ok();
        }
    }
}

#[cfg(test)]
#[path = "perl-parse-tests.rs"]
mod tests;
