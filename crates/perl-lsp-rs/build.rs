// Build script - panics are idiomatic for failing builds
#![allow(clippy::pedantic, clippy::panic)]

use std::error::Error;
use std::path::PathBuf;

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
    let text = match String::from_utf8(output.stdout) {
        Ok(text) => text,
        Err(_) => {
            // A byte-valued symbolic branch cannot be named by a UTF-8 Cargo
            // watcher either. Keep the existing label fallback, but rerun it.
            println!("cargo:rerun-if-changed=build.rs/unrepresentable-git-output");
            return None;
        }
    };
    let text = text.trim();
    if text.is_empty() { None } else { Some(text.to_string()) }
}

/// Resolve metadata through Git so linked worktrees use their own HEAD and
/// their common refs. Preserve path whitespace rather than trimming it.
fn git_path(args: &[&str]) -> Option<PathBuf> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok();
    let path = text
        .as_deref()
        .map(|text| text.strip_suffix('\n').unwrap_or(text))
        .filter(|text| !text.contains(['\n', '\r']));
    if let Some(path) = path {
        let path = PathBuf::from(path);
        if path.is_symlink() {
            // Cargo follows a leaf symlink's target timestamp, so replacing
            // HEAD with a link to an older ref can otherwise remain fresh.
            println!("cargo:rerun-if-changed=build.rs/symlink-git-metadata");
        }
        Some(path)
    } else {
        // Cargo's line-oriented UTF-8 watcher protocol cannot name these
        // metadata paths. This path below a regular source file cannot exist:
        // stay conservatively dirty rather than retaining a stale revision.
        println!("cargo:rerun-if-changed=build.rs/unrepresentable-git-path");
        None
    }
}

fn track_revision_inputs() {
    // Missing rerun-if-changed paths are always dirty in Cargo. Use that only
    // for inputs we cannot watch accurately, not optional ordinary ref files.
    println!("cargo:rerun-if-changed=build.rs");
    for name in [
        "PATH",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if let Some(root) = git_path(&["rev-parse", "--show-toplevel"]) {
        let entry = root.join(".git");
        if entry.is_file() {
            // Observe linked/separate-git-dir pointer changes without scanning
            // ordinary repositories' entire Git administration directory.
            println!("cargo:rerun-if-changed={}", entry.display());
        }
    } else {
        // Discovery can appear later without changing source or environment.
        println!("cargo:rerun-if-changed=build.rs/git-discovery-unavailable");
    }
    // Files-layout watchers cannot observe a reftable database or namespaced
    // refs. Preserve label freshness through conservative reruns in those cases.
    if git(&["config", "--get", "extensions.refStorage"]).is_some_and(|format| format != "files")
        || std::env::var_os("GIT_NAMESPACE").is_some_and(|namespace| !namespace.is_empty())
    {
        println!("cargo:rerun-if-changed=build.rs/unsupported-git-ref-layout");
    }
    for name in ["HEAD", "packed-refs", "config", "config.worktree"] {
        if let Some(path) =
            git_path(&["rev-parse", "--git-path", name]).filter(|path| path.exists())
        {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // Watch tag creation/deletion and the current branch, including a loose
    // ref appearing after it was packed. An existing ancestor detects creation
    // without declaring a missing path permanently dirty. Other branches and
    // remote refs do not affect the reported source revision.
    let mut names = vec!["refs/tags".to_string()];
    if git(&["config", "--bool", "--get", "extensions.worktreeConfig"])
        .is_some_and(|enabled| enabled == "true")
        && git_path(&["rev-parse", "--git-path", "config.worktree"])
            .is_none_or(|path| !path.exists())
    {
        // An enabled but absent config must be sampled until it appears.
        // Avoid a recursive watch of index/logs/objects just for creation.
        println!("cargo:rerun-if-changed=build.rs/worktree-config-unavailable");
    }
    let mut reference = "HEAD".to_string();
    for depth in 0..16 {
        let Some(next) = git(&["symbolic-ref", "--no-recurse", "-q", &reference]) else {
            break;
        };
        if names.contains(&next) {
            println!("cargo:rerun-if-changed=build.rs/cyclic-symbolic-ref");
            break;
        }
        names.push(next.clone());
        reference = next;
        if depth == 15 {
            println!("cargo:rerun-if-changed=build.rs/deep-symbolic-ref");
        }
    }
    for name in names {
        if let Some(mut path) = git_path(&["rev-parse", "--git-path", &name]) {
            while !path.exists() {
                if !path.pop() {
                    break;
                }
            }
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
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

    track_revision_inputs();

    Ok(())
}
