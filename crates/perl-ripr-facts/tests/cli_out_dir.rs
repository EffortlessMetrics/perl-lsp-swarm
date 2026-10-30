//! #17263 binary-level proof: a directory-valued `--out` fails fast with the
//! directory diagnostic on real stderr — before the packet build AND before
//! diff I/O.
//!
//! The `cli.rs` unit tests pin the wrapper's exit code and the message string,
//! but they cannot observe stderr or entry-point ordering: the fail-fast unit
//! test passes even under late rejection, and the message test never sees the
//! wrapper's stderr. These spawn the built `perl-ripr-facts` binary and assert
//! on process exit code plus actual stderr bytes, with negative assertions
//! that kill the wrong-order mutants (a schema error or a diff-read error
//! appearing instead of the directory diagnostic).

use perl_tdd_support::must_with;
use std::process::Command;

/// The standalone exporter binary under test (`[[bin]] name =
/// "perl-ripr-facts"`, `ripr-facts` subcommand). Cargo builds it before
/// compiling this integration target and publishes its path here.
const EXPORTER: &str = env!("CARGO_BIN_EXE_perl-ripr-facts");

/// Unique scratch base per test so parallel `cargo test` threads never share a
/// directory. Relative to the crate dir (the integration-test CWD), matching
/// the existing `target/...` fixture convention.
fn scratch_base(name: &str) -> String {
    format!("target/cli-out-dir-{}-{name}", std::process::id())
}

/// Create `base/outdir` (the directory-valued `--out`) and return both paths.
fn scratch_out_dir(name: &str) -> (String, String) {
    let base = scratch_base(name);
    let out_dir = format!("{base}/outdir");
    let _ = std::fs::remove_dir_all(&base);
    must_with(std::fs::create_dir_all(&out_dir), format!("create scratch directory {out_dir}"));
    (base, out_dir)
}

fn run_exporter(args: &[&str]) -> std::process::Output {
    must_with(
        Command::new(EXPORTER).args(args).output(),
        format!("spawn perl-ripr-facts with args {args:?}"),
    )
}

fn stderr_of(output: &std::process::Output) -> String {
    must_with(String::from_utf8(output.stderr.clone()), "exporter stderr must be UTF-8")
}

#[test]
fn directory_out_beats_schema_validation_on_stderr() -> Result<(), Box<dyn std::error::Error>> {
    // Out-check precedes the packet build: a bogus `--schema` must NOT surface
    // when `--out` names a directory — stderr must carry the directory
    // diagnostic, not the schema rejection.
    let (base, out_dir) = scratch_out_dir("schema");
    let output =
        run_exporter(&["ripr-facts", "--schema", "bogus", "--root", ".", "--out", &out_dir]);
    assert_eq!(output.status.code(), Some(1), "a directory --out must exit 1");
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("existing directory"),
        "stderr must name the directory condition; got: {stderr}"
    );
    assert!(
        !stderr.contains("unsupported schema"),
        "the out check must precede schema validation; got: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}

#[test]
fn directory_out_beats_unreadable_diff_on_stderr() -> Result<(), Box<dyn std::error::Error>> {
    // Out-check precedes diff I/O in `run_cli`: an unreadable `--diff` must
    // NOT mask a directory-valued `--out`.
    let (base, out_dir) = scratch_out_dir("diff");
    let missing_diff = format!("{base}/missing.patch");
    assert!(
        !std::path::Path::new(&missing_diff).exists(),
        "fixture setup must leave the diff path missing"
    );
    let output = run_exporter(&[
        "ripr-facts",
        "--schema",
        "ripr-perl-facts-v1",
        "--root",
        ".",
        "--diff",
        &missing_diff,
        "--out",
        &out_dir,
    ]);
    assert_eq!(output.status.code(), Some(1), "a directory --out must exit 1");
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("existing directory"),
        "stderr must name the directory condition; got: {stderr}"
    );
    assert!(
        !stderr.contains("failed to read diff"),
        "the out check must precede diff I/O; got: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}
