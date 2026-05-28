//! Contract test for the executable LSP 3.18 claim guard.

use std::{error::Error, path::Path, path::PathBuf};

use assert_cmd::Command;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[test]
fn check_lsp_318_claims_accepts_current_contract() -> TestResult {
    let root = repo_root()?;

    Command::cargo_bin("xtask")?.current_dir(root).arg("check-lsp-318-claims").assert().success();

    Ok(())
}

fn repo_root() -> TestResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest must be nested under repo root".into())
}
