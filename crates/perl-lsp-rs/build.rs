// Build script - panics are idiomatic for failing builds
#![allow(clippy::pedantic, clippy::panic)]

use std::error::Error;
use std::process::Command;

/// Run `git` and return trimmed stdout only when the command actually produced a value.
///
/// `Command::output()` succeeding means the process *spawned*, not that it worked.
/// Outside a git checkout `git describe` exits 128 with empty stdout, so the exit
/// status and the emptiness of stdout both have to be checked before the output is
/// treated as a real git description.
fn git_value(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Get git tag for embedding in version output. Source tarballs and
    // `cargo install` builds have no reachable checkout and report "unknown".
    let tag = git_value(&["describe", "--tags", "--always"]).unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=GIT_TAG={tag}");

    // Configure check-cfg for test-only cfg attributes
    println!("cargo:rustc-check-cfg=cfg(ci)");

    // Re-run build script when HEAD or refs change, so GIT_TAG stays fresh
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
    println!("cargo:rerun-if-changed=../../.git/packed-refs");

    Ok(())
}
