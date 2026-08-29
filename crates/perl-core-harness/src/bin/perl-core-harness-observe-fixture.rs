#![expect(
    clippy::print_stderr,
    reason = "The fixture emulates one upstream runner process: its stdout bytes ARE the observation subject and stderr carries its failure diagnostics."
)]

//! Hermetic upstream-runner fixture for the observed-discovery exact-process
//! suite (#12283).
//!
//! The binary stands in for the prepared tree's host Perl executing `t/TEST`:
//! it receives the observation route's argv (`TEST --dumptests <selector
//! arguments>`), walks its own working directory the way upstream `t/TEST`
//! `_find_tests` does (recursive `.t` collection per selector root), and prints
//! the selected rows the way upstream `dump_tests` does (repository-root-
//! relative spellings, one per line, sorted). Drift modes—selected through a
//! `.observe-fixture-mode` marker file in the working directory, so parallel
//! tests with isolated trees never race on shared state—let the suite prove
//! the capture route types honest dispositions instead of guessing.
//!
//! Selection here intentionally lives inside this process: the observation
//! route under test must never expand the target itself.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

/// Drift-mode marker read from the fixture tree's working directory.
const MODE_MARKER: &str = ".observe-fixture-mode";

/// Readiness marker written once the hang mode has flushed its rows, so the
/// test suite can order cancellation strictly after the evidence exists.
const READY_MARKER: &str = ".observe-fixture-ready";

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    if let Some(status) = emulate(&argv) {
        std::process::exit(status);
    }
    eprintln!("usage: perl-core-harness-observe-fixture TEST --dumptests <selector arguments>");
    std::process::exit(64);
}

/// Emulate one upstream `t/TEST --dumptests` invocation. Returns the exit
/// status, or `None` when the argv is not a discovery invocation.
fn emulate(argv: &[String]) -> Option<i32> {
    if argv.first().map(String::as_str) != Some("TEST") || !argv.iter().any(|a| a == "--dumptests")
    {
        return None;
    }
    let selectors = argv
        .iter()
        .skip(1)
        .filter(|argument| **argument != "--dumptests")
        .filter(|argument| !argument.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>();
    let mode = fs::read_to_string(MODE_MARKER)
        .map(|raw| raw.trim().to_string())
        .unwrap_or_else(|_| "select".to_string());
    match run_mode(&mode, &selectors) {
        Ok(status) => Some(status),
        Err(error) => {
            eprintln!("observe-fixture: {error}");
            Some(70)
        }
    }
}

fn run_mode(mode: &str, selectors: &[String]) -> io::Result<i32> {
    match mode {
        "hang" => {
            // A mid-run observation: the member rows are emitted and flushed
            // (signalled through the readiness marker), then the process parks
            // so deadline/cancellation supervision can type the terminal state
            // from real retained evidence.
            let rows = selected_rows(selectors)?;
            write_rows(&rows)?;
            let _ = fs::write(READY_MARKER, "ready\n");
            park_forever();
            Ok(0)
        }
        "empty" => Ok(0),
        "select" => {
            let rows = selected_rows(selectors)?;
            write_rows(&rows)?;
            Ok(0)
        }
        "select_fail" => {
            let rows = selected_rows(selectors)?;
            write_rows(&rows)?;
            Ok(7)
        }
        "duplicate_first" => {
            let mut rows = selected_rows(selectors)?;
            if let Some(first) = rows.first().cloned() {
                rows.insert(1, first);
            }
            write_rows(&rows)?;
            Ok(0)
        }
        "foreign_extra" => {
            let mut rows = selected_rows(selectors)?;
            rows.push("t/comp/foreign_extra.t".to_string());
            write_rows(&rows)?;
            Ok(0)
        }
        "drifted_row" => {
            let mut rows = selected_rows(selectors)?;
            if let Some(first) = rows.first_mut() {
                // Leading-space drift: a row the strict decoder must type as
                // malformed, never trim into an accepted member.
                *first = format!(" {first}");
            }
            write_rows(&rows)?;
            Ok(0)
        }
        "t_relative" => {
            // Emits `t/`-relative rows; the observation route binds the
            // canonical repository frame from the runner route, so these rows
            // must stay out of target instead of re-framing the stream.
            let rows = selected_rows(selectors)?;
            let relative = rows
                .into_iter()
                .map(|row| row.strip_prefix("t/").map(str::to_string).unwrap_or(row))
                .collect::<Vec<_>>();
            write_rows(&relative)?;
            Ok(0)
        }
        "invalid_utf8" => {
            let mut stdout = io::stdout();
            stdout.write_all(b"t/base/\xff.t\n")?;
            stdout.flush()?;
            Ok(0)
        }
        other => Err(io::Error::other(format!("unknown fixture mode {other}"))),
    }
}

fn write_rows(rows: &[String]) -> io::Result<()> {
    let mut stdout = io::stdout();
    for row in rows {
        writeln!(stdout, "{row}")?;
    }
    stdout.flush()
}

/// Collect `.t` rows for the selector roots exactly like the emulated upstream
/// walk: recursive per root, printed as repository-root-relative spellings and
/// sorted, mirroring `_find_tests` plus `dump_tests` at the pinned ref.
fn selected_rows(selectors: &[String]) -> io::Result<Vec<String>> {
    let mut rows = Vec::new();
    for selector in selectors {
        collect_dot_t(Path::new(selector), selector, &mut rows)?;
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn collect_dot_t(dir: &Path, root: &str, rows: &mut Vec<String>) -> io::Result<()> {
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_dot_t(&path, root, rows)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("t") {
            let relative = path
                .strip_prefix(root)
                .map(|rest| rest.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
            rows.push(format!("t/{root}/{relative}"));
        }
    }
    Ok(())
}

fn park_forever() {
    #[expect(
        clippy::duration_suboptimal_units,
        reason = "the stable Duration constructors stop at seconds; the 60-second park is intentional"
    )]
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
