//! Inspect one exact Git base/head relationship without mutating repository state.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;
use xtask::git_ancestry::classify_ancestry;

#[derive(Debug, Parser)]
#[command(about = "Classify Git ancestry without treating shallow absence as unrelated history")]
struct Arguments {
    /// Repository worktree to inspect.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Base revision whose relationship to --head is being classified.
    #[arg(long)]
    base: String,
    /// Head revision whose relationship to --base is being classified.
    #[arg(long)]
    head: String,
    /// Emit canonical pretty JSON instead of the human projection.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let receipt = classify_ancestry(&arguments.repo, &arguments.base, &arguments.head);
    if arguments.json {
        match serde_json::to_string_pretty(&receipt) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("git-ancestry serialization failure: {error}");
                return ExitCode::from(4);
            }
        }
    } else {
        print!("{}", receipt.render_human());
    }
    ExitCode::from(receipt.disposition.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_subject_and_json_mode() -> anyhow::Result<()> {
        let arguments = Arguments::try_parse_from([
            "git-ancestry",
            "--repo",
            ".",
            "--base",
            "origin/main",
            "--head",
            "HEAD",
            "--json",
        ])?;

        assert_eq!(arguments.repo, PathBuf::from("."));
        assert_eq!(arguments.base, "origin/main");
        assert_eq!(arguments.head, "HEAD");
        assert!(arguments.json);
        Ok(())
    }

    #[test]
    fn rejects_missing_base_or_head() {
        assert!(Arguments::try_parse_from(["git-ancestry", "--base", "HEAD"]).is_err());
        assert!(Arguments::try_parse_from(["git-ancestry", "--head", "HEAD"]).is_err());
    }
}
