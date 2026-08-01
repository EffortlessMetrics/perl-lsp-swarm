// Build script - panics are idiomatic for failing builds
#![allow(clippy::pedantic, clippy::panic)]

use std::error::Error;

/// Run `git <args>` and return its trimmed stdout, or `None` when the value
/// cannot be trusted.
///
/// Three distinct failures all have to collapse to `None`, and only one of
/// them is "git is not installed":
///
/// * the `git` binary is missing -- `output()` returns `Err`;
/// * `git` ran but rejected the request -- notably `fatal: not a git
///   repository`, which exits 128 with **empty stdout and a successful
///   spawn**. This is the case for every source-tarball and `cargo install`
///   build, where there is no `.git` directory at all;
/// * `git` succeeded but printed nothing.
///
/// The middle case is the one that matters. `Command::output()` reports
/// success as long as the process *spawned*, so `.ok()` yields `Some` and an
/// empty `stdout` decodes cleanly to `Ok("")`. Without the explicit status and
/// emptiness checks below, that empty string is threaded all the way into
/// `GIT_TAG` and a `cargo install perllsp` user sees a bare `Git tag:` label
/// with nothing after it.
fn git_output(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Get git tag for embedding in version output. Builds without a `.git`
    // directory report "unknown" rather than an empty string.
    let tag = git_output(&["describe", "--tags", "--always"]).unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=GIT_TAG={tag}");

    // Configure check-cfg for test-only cfg attributes
    println!("cargo:rustc-check-cfg=cfg(ci)");

    // Re-run build script when HEAD or refs change, so GIT_TAG stays fresh
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
    println!("cargo:rerun-if-changed=../../.git/packed-refs");

    Ok(())
}
