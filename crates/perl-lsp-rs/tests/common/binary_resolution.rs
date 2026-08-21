//! Binary resolution logic for finding the Perl LSP executable.
//!
//! Resolution order (fixed for test reliability):
//! 1. PERL_LSP_BIN env var (explicit override)
//! 2. Runtime `CARGO_BIN_EXE_perllsp` (when owned by the product package)
//! 3. Workspace target directory binaries (DEBUG first, then release)
//! 4. PATH lookup
//! 5. `cargo run -p perllsp` fallback

use perl_tdd_support::must;
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
        // Prefer the profile these tests were themselves built with, so a `cargo test
        // --release` run does not silently drive a debug server (or vice versa).
        for profile in active_profile_order() {
            // Deliberately the DEFAULT target dir, not env-aware: a directly
            // executed binary is the configuration #11858 shows dropping the
            // keyword/snippet completion set (cargo-run-shaped launches carry
            // CARGO_* env; a bare binary does not) — until that product bug
            // is fixed, the harness keeps resolving the way CI has always
            // run. The #11848 stall's resolver half stays mitigated by the
            // fail-loud pre-build refusal below instead.
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
            match ensure_perllsp_built(workspace_root) {
                Some(built) => {
                    let mut c = Command::new(built);
                    c.arg("--stdio");
                    v.push(c);
                }
                // A failed pre-build (e.g. the linker-crash family) must fail
                // LOUDLY here: falling through to the `cargo run` candidate
                // below silently compiles inside the initialize deadline and
                // resurfaces as an unexplained handshake stall (#11848 — the
                // captured stderr of one such stall was nothing but rustc
                // warnings from that inline compile). The message carries the
                // build's own error lines because inherited stderr is not
                // captured per-test by libtest.
                None => {
                    must(Err::<Command, _>(
                        "pre-building the perllsp binary failed (error lines above, from the \
                         build's captured output); refusing the cargo-run fallback because it \
                         would compile inside the initialize deadline and stall the handshake \
                         (#11848)",
                    ));
                }
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

/// Cargo profile directory these tests were compiled into.
fn active_profile() -> &'static str {
    if cfg!(debug_assertions) { "debug" } else { "release" }
}

/// Target-directory profiles to probe, most-appropriate first.
fn active_profile_order() -> [&'static str; 2] {
    if cfg!(debug_assertions) { ["debug", "release"] } else { ["release", "debug"] }
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
            // Build the profile these tests were built with. Building debug from a
            // `cargo test --release` run would hand the suite a debug server and
            // quietly change the performance characteristics under measurement.
            let profile = active_profile();
            let mut args = vec!["build", "-q", "-p", "perllsp", "--bin", "perllsp"];
            if profile == "release" {
                args.push("--release");
            }
            // Capture the build's output rather than inheriting it: inherited
            // stderr is NOT captured per-test by libtest, so a failed build's
            // diagnostics never reached the receipt (#11848) — the panic below
            // must be self-contained. The tail is bounded; a full warning
            // stream is noise, the error lines are signal.
            let output =
                Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
                    .args(&args)
                    .current_dir(workspace_root)
                    .output();
            match output {
                Ok(out) if out.status.success() => {
                    // Default target dir, matching the probe above (see the
                    // probe's comment for why this is deliberately NOT
                    // env-aware until #11858 lands).
                    let path =
                        workspace_root.join("target").join(profile).join(perllsp_file_name());
                    if !path.exists() {
                        // A successful build whose binary is not where we
                        // look (a custom CARGO_TARGET_DIR redirected it on
                        // CI) previously returned None SILENTLY and fell to
                        // cargo run — the exact shape of the #11848 stall.
                        // Say where we looked so the next occurrence is
                        // diagnosable in one run.
                        eprintln!(
                            "perl-lsp-rs tests: `cargo build -p perllsp` succeeded but {} is \
                             absent; CARGO_TARGET_DIR={:?}",
                            path.display(),
                            std::env::var_os("CARGO_TARGET_DIR")
                        );
                    }
                    path.exists().then_some(path)
                }
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stderr);
                    let error_lines: Vec<&str> =
                        text.lines().filter(|l| l.contains("error")).collect();
                    let tail = if error_lines.is_empty() {
                        text.lines().rev().take(10).collect::<Vec<_>>().join("\n")
                    } else {
                        error_lines.into_iter().take(10).collect::<Vec<_>>().join("\n")
                    };
                    eprintln!("perl-lsp-rs tests: `cargo build -p perllsp` failed:\n{tail}");
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
