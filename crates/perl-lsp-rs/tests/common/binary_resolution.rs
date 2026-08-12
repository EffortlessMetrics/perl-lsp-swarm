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

        // Try DEBUG binary first (this is what `cargo test` builds by default)
        let debug_binary = workspace_root.join("target/debug/perllsp");
        if debug_binary.exists() {
            let mut c = Command::new(&debug_binary);
            c.arg("--stdio");
            v.push(c);
        }

        let release_binary = workspace_root.join("target/release/perllsp");
        if release_binary.exists() {
            let mut c = Command::new(&release_binary);
            c.arg("--stdio");
            v.push(c);
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
