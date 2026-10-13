//! CWD-independence proof for the activation generator (#9204).
//!
//! This lives in its own integration-test binary on purpose. The proof has to
//! move the process working directory, which is global state shared by every
//! test running concurrently in the same binary. Isolating it here means it
//! cannot destabilize the negative controls in `activation.rs`, and a future
//! test there that resolves a relative path cannot flake against it.
//!
//! `#[serial]` is the repository's #1269 contract for a parallel-unsafe test.
//! Its own binary already means nothing races it today, so the annotation is
//! belt-and-braces — but the ratchet asks for the annotation rather than a row
//! in `ci/serial_test_identities.json`, and that registry is a debt ledger
//! being drained, not a place to add to.

use serial_test::serial;
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::activation;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

#[test]
#[serial]
fn generation_is_deterministic_across_runs_and_process_cwd() -> TestResult {
    let root = repo_root();
    let first = activation::generate(&root).map_err(|error| error.to_string())?;
    let second = activation::generate(&root).map_err(|error| error.to_string())?;
    assert_eq!(first, second, "two in-process generations must be structurally identical");
    assert_eq!(first.to_bytes()?, second.to_bytes()?);

    // Negative control: an implementation that (incorrectly) reads through a
    // relative path or `env::current_dir()` would produce different bytes,
    // or fail outright, once the process CWD differs from the repo root.
    let original_cwd = std::env::current_dir()?;
    let scratch = std::env::temp_dir().join(format!(
        "activation-cwd-independence-{}-{}",
        std::process::id(),
        first.rows.len()
    ));
    std::fs::create_dir_all(&scratch)?;
    std::env::set_current_dir(&scratch)?;
    let third = activation::generate(&root);
    let restore_result = std::env::set_current_dir(&original_cwd);
    let _ = std::fs::remove_dir_all(&scratch);
    restore_result?;
    let third = third.map_err(|error| error.to_string())?;

    assert_eq!(first, third, "generation must not depend on process CWD");
    assert_eq!(first.to_bytes()?, third.to_bytes()?);
    Ok(())
}
