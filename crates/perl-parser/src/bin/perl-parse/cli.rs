use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) inputs: Vec<Input>,
    pub(crate) output_format: OutputFormat,
    pub(crate) show_stats: bool,
    pub(crate) pretty: bool,
    pub(crate) quiet: bool,
    pub(crate) continue_on_error: bool,
}

#[derive(Debug)]
pub(crate) enum Input {
    File(PathBuf),
    Stdin,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum OutputFormat {
    LegacySexp,
    LegacyJson,
    UnstableDebug,
}

/// Parsed argv after the program name.
///
/// Help and version are distinct from a runnable request so they can exit as
/// soon as those flags are seen, matching historical argument-order behavior.
#[derive(Debug)]
pub(crate) enum CliRequest {
    Help,
    Version,
    Run(Args),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessStatus {
    Success,
    Failure,
}

impl ProcessStatus {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Failure => 1,
        }
    }
}

/// Parse argv after the program name.
///
/// `--help`/`-h` and `--version`/`-V` return immediately when first seen, so
/// later flags are not interpreted. That matches the historical process-exit
/// argument-order behavior.
pub(crate) fn parse_args<I, S>(args: I) -> Result<CliRequest, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let mut inputs = Vec::new();
    let mut output_format = OutputFormat::LegacySexp;
    let mut show_stats = false;
    let mut pretty = false;
    let mut quiet = false;
    let mut continue_on_error = false;

    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "-h" | "--help" => return Ok(CliRequest::Help),
            "-V" | "--version" => return Ok(CliRequest::Version),
            "-f" | "--format" => {
                let format = args.next().ok_or_else(|| "Missing format argument".to_string())?;
                output_format = match format.as_ref() {
                    "sexp" | "s-expression" => OutputFormat::LegacySexp,
                    "json" => OutputFormat::LegacyJson,
                    "debug" => OutputFormat::UnstableDebug,
                    other => return Err(format!("Unknown format: {other}")),
                };
            }
            "-s" | "--stats" => show_stats = true,
            "-p" | "--pretty" => pretty = true,
            "-q" | "--quiet" => quiet = true,
            "-c" | "--continue" => continue_on_error = true,
            "-" => inputs.push(Input::Stdin),
            path if path.starts_with('-') => {
                return Err(format!("Unknown option: {path}"));
            }
            path => {
                inputs.push(Input::File(PathBuf::from(path)));
            }
        }
    }

    if inputs.is_empty() {
        inputs.push(Input::Stdin);
    }

    Ok(CliRequest::Run(Args {
        inputs,
        output_format,
        show_stats,
        pretty,
        quiet,
        continue_on_error,
    }))
}

pub(crate) fn help_text() -> &'static str {
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

pub(crate) fn write_help(stdout: &mut impl Write) -> io::Result<()> {
    writeln!(stdout, "{}", help_text())
}

pub(crate) fn write_version(stdout: &mut impl Write) -> io::Result<()> {
    writeln!(stdout, "perl-parse v{}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn write_usage_error(stderr: &mut impl Write, error: &str) -> io::Result<()> {
    writeln!(stderr, "Error: {error}")?;
    writeln!(stderr, "Try 'perl-parse --help' for more information.")
}
