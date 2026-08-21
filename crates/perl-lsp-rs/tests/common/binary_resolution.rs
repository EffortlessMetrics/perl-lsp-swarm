//! Binary resolution logic for finding the Perl LSP executable.
//!
//! Resolution order (fixed for test reliability):
//! 1. PERL_LSP_BIN env var (explicit override)
//! 2. Runtime `CARGO_BIN_EXE_perllsp` (when owned by the product package)
//! 3. Workspace target directory binaries (DEBUG first, then release)
//! 4. PATH lookup
//! 5. `cargo run -p perllsp` fallback

use perl_tdd_support::must;
use std::path::Path;
use std::process::Command;

const BUILD_STDERR_MAX_BYTES: usize = 8 * 1024;

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
            let binary = target_directory(workspace_root).join(profile).join(perllsp_file_name());
            if is_executable_file(&binary) {
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
                Ok(built) => {
                    let mut c = Command::new(built);
                    c.arg("--stdio");
                    v.push(c);
                }
                // A FAILED pre-build (e.g. the linker-crash family) must fail
                // LOUDLY: falling through would silently compile inside the
                // initialize deadline and resurface as an unexplained
                // handshake stall (#11848 — the captured stderr of one such
                // stall was nothing but rustc warnings from that inline
                // compile). The message carries the build's own error lines
                // because inherited stderr is not captured per-test by
                // libtest.
                Err(build_errors) => {
                    must(Err::<Command, _>(format!(
                        "pre-building the perllsp binary failed:\n{build_errors}\nrefusing the \
                         cargo-run fallback because it would compile inside the initialize \
                         deadline and stall the handshake (#11848)",
                    )));
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

fn target_directory(workspace_root: &std::path::Path) -> std::path::PathBuf {
    target_directory_from(std::env::var_os("CARGO_TARGET_DIR"), workspace_root)
}

fn target_directory_from(
    configured: Option<std::ffi::OsString>,
    workspace_root: &std::path::Path,
) -> std::path::PathBuf {
    match configured {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            if path.is_absolute() { path } else { workspace_root.join(path) }
        }
        None => workspace_root.join("target"),
    }
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
fn ensure_perllsp_built(workspace_root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    static BUILT: std::sync::OnceLock<Result<std::path::PathBuf, String>> =
        std::sync::OnceLock::new();
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
                    let path =
                        target_directory(workspace_root).join(profile).join(perllsp_file_name());
                    built_binary_or_refuse(path)
                }
                Ok(out) => {
                    // Self-contained failure text: inherited stderr is not
                    // per-test captured, so the caller's refusal message must
                    // carry the build's own error lines. Error lines are
                    // signal; a full warning stream is noise.
                    let text = String::from_utf8_lossy(&out.stderr);
                    let error_lines: Vec<&str> =
                        text.lines().filter(|l| l.contains("error")).collect();
                    let tail = if error_lines.is_empty() {
                        text.lines().rev().take(10).collect::<Vec<_>>().join("\n")
                    } else {
                        error_lines.into_iter().take(10).collect::<Vec<_>>().join("\n")
                    };
                    Err(format!(
                        "cargo build -p perllsp failed:\n{}",
                        bounded_newest_bytes(tail, BUILD_STDERR_MAX_BYTES)
                    ))
                }
                Err(e) => Err(format!("could not run `cargo build -p perllsp`: {e}")),
            }
        })
        .clone()
}

fn bounded_newest_bytes(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text.drain(..start);
    text
}

fn built_binary_or_refuse(path: std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    if is_executable_file(&path) {
        Ok(path)
    } else {
        Err(format!(
            "cargo build -p perllsp succeeded but candidate binary is not a regular executable: \
             {}; refusing the cargo-run fallback",
            path.display()
        ))
    }
}

fn is_executable_file(path: &Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.file_type().is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{built_binary_or_refuse, target_directory_from};
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn target_directory_resolves_relative_and_absolute_values() {
        let root = Path::new("/workspace");
        assert_eq!(
            target_directory_from(Some(OsString::from(".ci-target")), root),
            root.join(".ci-target")
        );
        assert_eq!(
            target_directory_from(Some(OsString::from("/tmp/target")), root),
            Path::new("/tmp/target")
        );
        assert_eq!(target_directory_from(None, root), root.join("target"));
    }

    #[test]
    fn missing_prebuilt_binary_refuses_cargo_run_fallback() {
        let result = built_binary_or_refuse(Path::new("/definitely/missing/perllsp").to_owned());
        assert!(matches!(result, Err(message) if message.contains("not a regular executable")));
    }

    #[test]
    fn current_executable_is_accepted_as_a_real_binary() {
        let path = perl_test_must::must(std::env::current_exe());
        assert_eq!(built_binary_or_refuse(path.clone()), Ok(path));
    }

    #[cfg(unix)]
    #[test]
    fn non_regular_and_non_executable_paths_refuse_cargo_run_fallback() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir()
            .join(format!("perl-lsp-rs-binary-resolution-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create binary-resolution test directory");
        let directory = root.join("directory");
        let file = root.join("file");
        fs::create_dir(&directory).expect("create directory candidate");
        fs::write(&file, b"not executable").expect("create file candidate");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644))
            .expect("set non-executable permissions");

        assert!(built_binary_or_refuse(directory).is_err());
        assert!(built_binary_or_refuse(file).is_err());

        fs::remove_dir_all(root).expect("remove binary-resolution test directory");
    }
}
