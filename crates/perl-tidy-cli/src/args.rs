use clap::{ArgGroup, Parser};
use serde::Serialize;
use std::path::PathBuf;

/// The standalone command only composes the native formatter and local I/O.
#[derive(Debug, Parser)]
#[command(name = "perl-tidy", version, about = "Format Perl source with the native Rust formatter")]
#[command(group(ArgGroup::new("operation").args(["stdout", "check", "write", "config_effective", "explain"]).multiple(false)))]
pub(crate) struct Args {
    /// Explicit files; directories and implicit recursive scans are not supported.
    #[arg(value_name = "FILE", conflicts_with = "stdin")]
    pub(crate) paths: Vec<PathBuf>,
    /// Read UTF-8 source from stdin.
    #[arg(long, conflicts_with = "write")]
    pub(crate) stdin: bool,
    /// Logical name for stdin in reports. Never authorizes reading or writing this path.
    #[arg(long, requires = "stdin", value_name = "PATH")]
    pub(crate) stdin_filename: Option<PathBuf>,
    /// Emit formatted source to stdout (the default operation).
    #[arg(long)]
    pub(crate) stdout: bool,
    /// Exit successfully only when every selected source is a legitimate native no-change.
    #[arg(long)]
    pub(crate) check: bool,
    /// Plan all files, then replace supported regular files with guarded rollback.
    #[arg(long)]
    pub(crate) write: bool,
    /// Show the exact native configuration selected by this command.
    #[arg(long)]
    pub(crate) config_effective: bool,
    /// Explain the canonical native outcome without emitting formatted source.
    #[arg(long)]
    pub(crate) explain: bool,
    /// Emit one versioned JSON report instead of source or human presentation.
    #[arg(long)]
    pub(crate) json: bool,
    /// Read a complete NativeFormatConfig JSON object. No project discovery is implicit.
    #[arg(long, value_name = "JSON_FILE")]
    pub(crate) config: Option<PathBuf>,
    /// Override native line width.
    #[arg(long)]
    pub(crate) line_width: Option<u32>,
    /// Override native indentation width.
    #[arg(long)]
    pub(crate) indent_width: Option<u32>,
    /// Use tabs for native indentation.
    #[arg(long, conflicts_with = "spaces")]
    pub(crate) tabs: bool,
    /// Use spaces for native indentation.
    #[arg(long, conflicts_with = "tabs")]
    pub(crate) spaces: bool,
    /// Select final-newline policy.
    #[arg(long, value_parser = ["preserve", "insert", "trim"])]
    pub(crate) final_newline: Option<String>,
    /// Select opening-brace placement.
    #[arg(long, value_parser = ["same-line", "next-line"])]
    pub(crate) brace_placement: Option<String>,
    /// Select else/elsif placement.
    #[arg(long, value_parser = ["cuddled", "separate-line"])]
    pub(crate) else_placement: Option<String>,
    /// Select supported control-keyword spacing.
    #[arg(long, value_parser = ["space", "compact"])]
    pub(crate) keyword_spacing: Option<String>,
    /// Select trailing-comma policy.
    #[arg(long, value_parser = ["preserve", "add-when-wrapped"])]
    pub(crate) trailing_comma: Option<String>,
    /// Maximum source bytes per input, including stdin (default 8 MiB).
    #[arg(long, default_value_t = 8 * 1024 * 1024, value_parser = clap::value_parser!(u64).range(1..=64 * 1024 * 1024))]
    pub(crate) max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Operation {
    Stdout,
    Check,
    Write,
    ConfigEffective,
    Explain,
}

impl Args {
    pub(crate) fn operation(&self) -> Operation {
        if self.check {
            Operation::Check
        } else if self.write {
            Operation::Write
        } else if self.config_effective {
            Operation::ConfigEffective
        } else if self.explain {
            Operation::Explain
        } else {
            Operation::Stdout
        }
    }
}
