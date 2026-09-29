//! Classify one exact `main` push event without mutating repository state.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use xtask::main_history_event::{PushEvent, classify_push_event};

#[derive(Debug, Parser)]
#[command(
    about = "Classify exact push-event movement on a protected ref without inferring history loss from an incomplete graph"
)]
struct Arguments {
    /// Repository worktree to inspect.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Commit the ref pointed at before the push (`github.event.before`).
    #[arg(long)]
    before: String,
    /// Commit the ref points at after the push (`github.sha`).
    #[arg(long)]
    after: String,
    /// Fully qualified ref the push targeted (`github.ref`).
    #[arg(long = "ref")]
    reference: String,
    /// GitHub's `forced` push-payload flag.
    #[arg(long = "event-forced", default_value_t = false)]
    event_forced: bool,
    /// GitHub's `created` push-payload flag.
    #[arg(long = "event-created", default_value_t = false)]
    event_created: bool,
    /// GitHub's `deleted` push-payload flag.
    #[arg(long = "event-deleted", default_value_t = false)]
    event_deleted: bool,
    /// Write the canonical pretty JSON receipt to this path.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Emit canonical pretty JSON on stdout instead of the human projection.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let event = PushEvent {
        reference: &arguments.reference,
        before: &arguments.before,
        after: &arguments.after,
        forced: arguments.event_forced,
        created: arguments.event_created,
        deleted: arguments.event_deleted,
    };
    let receipt = classify_push_event(&arguments.repo, &event);

    let json = match serde_json::to_string_pretty(&receipt) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("main-history-event serialization failure: {error}");
            return ExitCode::from(4);
        }
    };

    // The receipt is written before the process reports a blocking verdict, so a
    // red detector still uploads the evidence that explains why it is red.
    if let Some(output) = arguments.output.as_ref() {
        if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty())
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("main-history-event could not create {}: {error}", parent.display());
            return ExitCode::from(4);
        }
        if let Err(error) = fs::write(output, format!("{json}\n")) {
            eprintln!("main-history-event could not write {}: {error}", output.display());
            return ExitCode::from(4);
        }
    }

    if arguments.json {
        println!("{json}");
    } else {
        print!("{}", receipt.render_human());
    }
    ExitCode::from(receipt.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_push_event_subject() -> anyhow::Result<()> {
        let arguments = Arguments::try_parse_from([
            "main-history-event",
            "--repo",
            ".",
            "--before",
            "1111111111111111111111111111111111111111",
            "--after",
            "2222222222222222222222222222222222222222",
            "--ref",
            "refs/heads/main",
            "--event-forced",
            "--output",
            "target/history/main-event.json",
        ])?;

        assert_eq!(arguments.repo, PathBuf::from("."));
        assert_eq!(arguments.before, "1111111111111111111111111111111111111111");
        assert_eq!(arguments.after, "2222222222222222222222222222222222222222");
        assert_eq!(arguments.reference, "refs/heads/main");
        assert!(arguments.event_forced);
        assert!(!arguments.event_created);
        assert!(!arguments.event_deleted);
        assert_eq!(arguments.output, Some(PathBuf::from("target/history/main-event.json")));
        Ok(())
    }

    #[test]
    fn defaults_event_flags_to_false_and_requires_the_exact_subject() {
        let arguments = Arguments::try_parse_from([
            "main-history-event",
            "--before",
            "HEAD~1",
            "--after",
            "HEAD",
            "--ref",
            "refs/heads/main",
        ]);
        assert!(arguments.is_ok_and(|arguments| {
            !arguments.event_forced && !arguments.event_created && !arguments.event_deleted
        }));

        assert!(Arguments::try_parse_from(["main-history-event", "--after", "HEAD"]).is_err());
        assert!(Arguments::try_parse_from(["main-history-event", "--before", "HEAD"]).is_err());
        assert!(
            Arguments::try_parse_from([
                "main-history-event",
                "--before",
                "HEAD~1",
                "--after",
                "HEAD"
            ])
            .is_err()
        );
    }
}
