//! Binary resolution logic for finding the Perl LSP executable.
//!
//! Resolution order (fixed for test reliability):
//! 1. PERL_LSP_BIN env var (explicit override)
//! 2. Runtime `CARGO_BIN_EXE_perllsp` (when owned by the product package)
//! 3. Workspace target directory binaries (DEBUG first, then release)
//! 4. PATH lookup
//! 5. `cargo run -p perllsp` fallback

use std::process::Command;

pub(crate) fn resolve_perl_lsp_cmds() -> impl Iterator<Item = Command> {
    // Resolution order (fixed for test reliability):
    // 1. PERL_LSP_BIN env var (explicit override, useful for custom target dirs)
    // 2. Compile-time CARGO_BIN_EXE (guaranteed correct during `cargo test -p perl-lsp-rs`)
    // 3. Runtime CARGO_BIN_EXE_* (fallback for edge cases)
    // 4. Workspace target directory binaries (DEBUG first, then release)
    // 5. PATH lookup
    // 6. cargo run fallback (slow but always works)
    //
    // IMPORTANT: Debug binary is checked BEFORE release to avoid stale release binaries
    // causing test failures. When you run `cargo test -p perl-lsp-rs`, cargo builds debug.
    let mut v: Vec<Command> = Vec::new();

    // 1. Explicit override via PERL_LSP_BIN
    if let Ok(p) = std::env::var("PERL_LSP_BIN") {
        let mut c = Command::new(p);
        c.arg("--stdio");
        v.push(c);
    }

    // 2. Runtime Cargo product-binary path.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_perllsp") {
        let mut c = Command::new(p);
        c.arg("--stdio");
        v.push(c);
    }

    // 4. Try workspace target directory binaries (using absolute paths)
    // IMPORTANT: Debug BEFORE release to avoid stale release binary issues
    // CARGO_MANIFEST_DIR points to the crate directory, we need the workspace root
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let crate_dir = std::path::Path::new(&manifest_dir);
        // Walk up to find workspace root (contains Cargo.toml with [workspace])
        let workspace_root =
            crate_dir.ancestors().find(|p| p.join("Cargo.lock").exists()).unwrap_or(crate_dir);

        // Try DEBUG binary first (this is what `cargo test` builds by default).
        // The executable suffix matters: on Windows the file is `perllsp.exe`, so a
        // suffix-less `exists()` check silently skips a perfectly good local binary
        // and forces the slow `cargo run` fallback below.
        for profile in ["debug", "release"] {
            let binary = workspace_root.join("target").join(profile).join(perllsp_file_name());
            if binary.exists() {
                let mut c = Command::new(&binary);
                c.arg("--stdio");
                v.push(c);
            }
        }

        // The server binary lives in the `perllsp` package, not in `perl-lsp-rs` where
        // these tests live, so `cargo test -p perl-lsp-rs` never builds it. If nothing
        // above resolved, build it ONCE here rather than leaving the `cargo run`
        // fallback to compile inside a per-request timeout it cannot possibly meet.
        if v.is_empty() {
            if let Some(built) = ensure_perllsp_built(workspace_root) {
                let mut c = Command::new(built);
                c.arg("--stdio");
                v.push(c);
            }
        }
    }

    // 4. Try the public command from PATH.
    {
        let mut c = Command::new("perllsp");
        c.arg("--stdio");
        v.push(c);
    }
    // 5. Fallback: use cargo run with debug profile (matches what tests build)
    // This is SLOW because it may need to compile, but always works
    {
        let mut c = Command::new("cargo");
        c.args(["run", "-q", "-p", "perllsp", "--", "--stdio"]);
        v.push(c);
    }

    v.into_iter()
}

/// File name of the server executable, including the platform suffix.
fn perllsp_file_name() -> String {
    format!("perllsp{}", std::env::consts::EXE_SUFFIX)
}

/// Build the `perllsp` binary once per test process and return its path.
///
/// The tests spawn a server owned by a different package, so nothing in
/// `cargo test -p perl-lsp-rs` guarantees it exists. Building it here — before any
/// request deadline starts — keeps the cost out of `initialize`, which previously
/// timed out against an inline `cargo run` that needed roughly a minute to compile.
fn ensure_perllsp_built(workspace_root: &std::path::Path) -> Option<std::path::PathBuf> {
    static BUILT: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let status =
                Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
                    .args(["build", "-q", "-p", "perllsp", "--bin", "perllsp"])
                    .current_dir(workspace_root)
                    .status();
            match status {
                Ok(s) if s.success() => {
                    let path = workspace_root.join("target/debug").join(perllsp_file_name());
                    path.exists().then_some(path)
                }
                Ok(s) => {
                    eprintln!("perl-lsp-rs tests: `cargo build -p perllsp` failed with {s}");
                    None
                }
                Err(e) => {
                    eprintln!("perl-lsp-rs tests: could not run `cargo build -p perllsp`: {e}");
                    None
                }
            }
        })
        .clone()
}
