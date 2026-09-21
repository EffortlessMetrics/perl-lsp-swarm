//! #16214: the CP00 check surface must be able to tell a real NOT_PROVEN
//! verdict apart from a crashed validator. A verdict exits 3 with a postured
//! structured receipt and per-relation annotations on the error channel; an
//! instrument crash exits 4 with `INSTRUMENT_FAILURE` on the error channel
//! and never produces a verdict receipt.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

const BIN: &str = env!("CARGO_BIN_EXE_semantic-close-containment");

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_validator(args: &[&str]) -> std::io::Result<Run> {
    let output = Command::new(BIN).args(args).output()?;
    Ok(Run {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn write_fixture(dir: &Path, name: &str, contents: &str) -> std::io::Result<PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, contents)?;
    Ok(path)
}

/// Offline fixture whose PR closes an issue the fixture deliberately omits,
/// so the relation resolves to a row-level `NOT_PROVEN_GITHUB` verdict.
const NOT_PROVEN_FIXTURE: &str = r#"{
  "schema_version": "semantic_close_containment_fixture.v1",
  "provenance": {
    "captured_at": "2026-09-20T00:00:00Z",
    "sources": ["https://github.com/effortlessmetrics/perl-lsp-swarm/issues/16214"],
    "subject_shas": [""],
    "boundary": "offline proof fixture; the referenced issue subject is deliberately omitted so the relation resolves to NOT_PROVEN_GITHUB (#16214)"
  },
  "repository": "effortlessmetrics/perl-lsp-swarm",
  "pull_request": {
    "number": 16214,
    "title": "typed exit contract proof",
    "body": "Closes #16214\n"
  },
  "issues": [],
  "expected": {
    "aggregate_code": "NOT_PROVEN_GITHUB",
    "rows": [
      {
        "repository": "effortlessmetrics/perl-lsp-swarm",
        "issue_number": 16214,
        "code": "NOT_PROVEN_GITHUB"
      }
    ]
  }
}"#;

#[test]
fn real_not_proven_verdict_exits_three_with_postured_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let fixture = write_fixture(dir.path(), "not-proven.json", NOT_PROVEN_FIXTURE)?;
    let fixture_path = fixture.to_str().ok_or("non-utf-8 fixture path")?;
    let run = run_validator(&["--fixture", fixture_path, "--format", "json"])?;

    assert_eq!(run.code, Some(3), "a real NOT_PROVEN verdict keeps exit 3");
    assert!(
        run.stdout.contains("\"posture\": \"NOT_PROVEN\""),
        "receipt carries the typed posture field: {}",
        run.stdout
    );
    assert!(run.stdout.contains("\"aggregate_code\": \"NOT_PROVEN_GITHUB\""));
    assert!(
        run.stderr.contains(
            "::error::NOT_PROVEN effortlessmetrics/perl-lsp-swarm#16214: \
             terminal relation could not be checked against its issue subject"
        ),
        "per-relation annotation reaches the error channel: {}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("INSTRUMENT_FAILURE semantic-close-containment"),
        "a verdict is never labelled an instrument failure: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn injected_instrument_crash_exits_four_without_a_verdict() -> Result<(), Box<dyn std::error::Error>>
{
    let run = run_validator(&["--event", "definitely-missing-event-payload-16214.json"])?;

    assert_eq!(run.code, Some(4), "an instrument crash uses the distinct code");
    assert!(
        run.stderr.contains("INSTRUMENT_FAILURE semantic-close-containment"),
        "crash reason reaches the error channel: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("::error::INSTRUMENT_FAILURE semantic-close-containment"),
        "crash reaches the annotations channel: {}",
        run.stderr
    );
    assert!(
        !run.stdout.contains("\"posture\""),
        "no verdict receipt exists for a crash: {}",
        run.stdout
    );
    Ok(())
}

#[test]
fn malformed_fixture_is_an_instrument_crash_not_a_verdict() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempdir()?;
    let fixture = write_fixture(dir.path(), "broken.json", "{\"schema_version\": ")?;
    let fixture_path = fixture.to_str().ok_or("non-utf-8 fixture path")?;
    let run = run_validator(&["--fixture", fixture_path])?;

    assert_eq!(run.code, Some(4));
    assert!(run.stderr.contains("INSTRUMENT_FAILURE semantic-close-containment"));
    assert!(!run.stdout.contains("\"posture\""));
    Ok(())
}
