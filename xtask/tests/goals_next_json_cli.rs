//! End-to-end CLI regression for #3692 defect 1: `cargo xtask goals next
//! --json` must ALWAYS emit parseable JSON on stdout and exit nonzero on
//! any internal error — never an unstructured `color_eyre` dump on
//! stderr.
//!
//! The in-crate unit tests in `goals/mod.rs` (`json_error_output_is_parseable_and_names_the_failure`,
//! `next_with_json_never_returns_err_to_the_caller`) can only exercise
//! `render_output`/`render_json_error` directly — the json=true branch of
//! `next()` itself calls `std::process::exit(1)`, which cannot be invoked
//! in-process without killing the test runner, so nothing in the unit
//! suite actually proves `next()` is wired to print JSON-then-exit rather
//! than propagate `Err` to its caller (a regression here would previously
//! have surfaced as an unstructured stderr dump with a non-JSON stdout,
//! exactly the #3692 defect 1 symptom). This test drives the actual
//! compiled binary end-to-end so that real wiring is exercised.
use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn goals_next_json_prints_parseable_json_and_exits_1_on_internal_error() -> Result<()> {
    // A missing --fixture file is the same cheap, network-free way to
    // force an internal `Err` that the in-crate unit tests use (see
    // `render_output_surfaces_an_err_when_the_fixture_path_does_not_exist`
    // in `goals/mod.rs`) — no `gh auth`/network dependency, and it fails
    // deep inside `build_snapshot` (well past program/manifest
    // resolution), matching the "any internal Err" scope of the fix.
    let assert = cargo_bin_cmd!("xtask")
        .args(["goals", "next", "--json", "--fixture", "definitely/does/not/exist/prs.json"])
        .assert()
        .failure()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone())?;
    let stderr = String::from_utf8(assert.get_output().stderr.clone())?;

    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|e| {
        anyhow::anyhow!(
            "stdout must be parseable JSON for a --json caller, got parse error {e}\nstdout: {stdout:?}\nstderr: {stderr:?}"
        )
    })?;
    let error_field = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected a string \"error\" field, got {parsed}"))?;
    assert!(!error_field.is_empty(), "expected a non-empty error message, got {parsed}");
    assert!(
        parsed.get("error_chain").is_some_and(|v| v.is_array()),
        "expected an \"error_chain\" array field, got {parsed}"
    );

    // The defining symptom of #3692 defect 1: a --json caller must never
    // see an unstructured color_eyre dump on stderr.
    assert!(
        stderr.trim().is_empty(),
        "a --json caller must never see anything on stderr, got: {stderr:?}"
    );

    Ok(())
}
