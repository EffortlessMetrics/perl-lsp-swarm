// Build script - panics are idiomatic for failing builds
#![allow(clippy::pedantic)]

use std::error::Error;

/// Run a git command, returning its trimmed stdout only when it actually
/// succeeded and produced something.
///
/// `Command::output()` returns `Ok` whenever the process *ran*, even if it
/// exited non-zero — so a plain `.ok()` treats "fatal: not a git repository"
/// (exit 128, empty stdout) as success and yields an empty string. Checking
/// the status is what makes the caller's fallback reachable.
fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() { None } else { Some(text.to_string()) }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Identify the source revision for `--version` / `--info`.
    //
    // These are reported to users and pasted into bug reports, so the kind is
    // recorded alongside the value rather than guessed at display time. Using
    // `git describe --always` alone cannot support an honest label: it silently
    // degrades from a tag to a bare commit SHA, which is how every untagged
    // build came to print "Git tag: <sha>".
    let (revision, revision_kind) = match git(&["describe", "--tags", "--exact-match"]) {
        Some(tag) => (tag, "tag"),
        None => match git(&["rev-parse", "--short", "HEAD"]) {
            Some(commit) => (commit, "commit"),
            // No git, no .git directory, or a shallow export: a release
            // tarball, a vendored source tree, or `cargo install` from a
            // registry. Say so instead of emitting an empty field.
            None => ("unknown".to_string(), "unknown"),
        },
    };

    println!("cargo:rustc-env=BUILD_REVISION={revision}");
    println!("cargo:rustc-env=BUILD_REVISION_KIND={revision_kind}");

    // Configure check-cfg for test-only cfg attributes
    println!("cargo:rustc-check-cfg=cfg(ci)");

    // Re-run build script when HEAD or refs change, so the revision stays fresh
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
    println!("cargo:rerun-if-changed=../../.git/packed-refs");

    Ok(())
}
