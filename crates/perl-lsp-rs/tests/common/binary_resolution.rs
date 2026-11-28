//! Binary resolution logic for finding the Perl LSP executable.
//!
//! Resolution order (fixed for test reliability):
//! 1. PERL_LSP_BIN env var (explicit override)
//! 2. Runtime `CARGO_BIN_EXE_perllsp` (when owned by the product package)
//! 3. Workspace target directory: the ACTIVE profile's artifact, made
//!    current through a bounded pre-build before any deadline starts
//! 4. PATH lookup
//!
//! There is deliberately NO `cargo run` candidate: a cargo invocation inside
//! the handshake deadline IS the #11848 stall family — it must wait on the
//! build-directory lock under suite contention and can compile for minutes.
//! Every non-env candidate is either an explicit executable or the product of
//! a pre-build performed before any deadline starts; when nothing resolves or
//! spawns, the harness fails loudly instead of silently degrading into cargo.
//!
//! Freshness is part of the contract (P2 review on #11905, staleness repair
//! in #11946): only a current ACTIVE-profile artifact describes this source
//! tree. Existence alone proves nothing — an executable left behind by an
//! earlier checkout (either profile) predates the reviewed source, so the
//! target-directory candidate ALWAYS routes through the pre-build, letting
//! cargo's fingerprints decide whether anything needs rebuilding. Cargo is
//! the freshness oracle; this file does not reinvent one from mtimes.

use perl_tdd_support::must;
use std::path::Path;
use std::process::Command;

const BUILD_STDERR_MAX_BYTES: usize = 8 * 1024;

pub(crate) fn resolve_perl_lsp_cmds() -> impl Iterator<Item = Command> {
    // Resolution order (fixed for test reliability):
    // 1. Explicit override via PERL_LSP_BIN
    // 2. Compile-time CARGO_BIN_EXE (guaranteed correct during `cargo test -p perl-lsp-rs`)
    // 3. Runtime CARGO_BIN_EXE_* (fallback for edge cases)
    // 4. Workspace target directory: the ACTIVE-profile artifact, made
    //    current through the pre-build
    // 5. PATH lookup
    //
    // IMPORTANT: only a current active profile's binary is acceptable here.
    // Any executable that predates the reviewed source — either profile —
    // would silently test stale code (P2 review on #11905, #11946).
    let mut v: Vec<Command> = Vec::new();

    // Explicit candidates are authoritative when they point to an existing
    // executable. Returning here is important: probing the target directory
    // can pre-build the product even though the caller already supplied a
    // usable binary, recreating the #11848 deadline risk.
    if let Some(explicit) = resolve_explicit_candidates(
        std::env::var_os("PERL_LSP_BIN"),
        std::env::var_os("CARGO_BIN_EXE_perllsp"),
    ) {
        return explicit.into_iter();
    }

    // 3. Workspace target directory (using absolute paths) — the ACTIVE
    //    profile's artifact, made current before any deadline starts.
    //    Existence alone is never acceptance: an artifact that predates this
    //    source tree would silently test stale code (#11946), so the
    //    candidate always comes from the pre-build, whose cargo fingerprints
    //    rebuild exactly when the artifact is out of date. An opposite-
    //    profile executable found beside the build is carried only as
    //    refusal-diagnostics context — it is never a spawn candidate.
    if let Some(workspace_root) = workspace_root_from_manifest() {
        let found_opposite = opposite_profile_artifact(&target_directory(&workspace_root));
        match ensure_perllsp_built(&workspace_root) {
            Ok(built) => {
                let mut c = Command::new(built);
                c.arg("--stdio");
                v.push(c);
            }
            Err(build_errors) => {
                must(Err::<Command, _>(refuse_after_failed_build(
                    &build_errors,
                    found_opposite.as_deref(),
                )));
            }
        }
    }

    // 4. Try the public command from PATH — the last candidate. An installed
    //    perllsp is an existing executable, so spawning it is bounded, unlike
    //    a cargo invocation; there is deliberately no cargo-run tail (#11848).
    {
        let mut c = Command::new("perllsp");
        c.arg("--stdio");
        v.push(c);
    }

    v.into_iter()
}

pub(crate) fn resolve_explicit_candidates(
    perl_lsp_bin: Option<std::ffi::OsString>,
    cargo_bin_exe: Option<std::ffi::OsString>,
) -> Option<Vec<Command>> {
    [("PERL_LSP_BIN", perl_lsp_bin), ("CARGO_BIN_EXE_perllsp", cargo_bin_exe)].into_iter().find_map(
        |(source, path)| {
            path.and_then(|path| {
                existing_candidate_command(path, source).map(|command| vec![command])
            })
        },
    )
}

fn existing_candidate_command(path: std::ffi::OsString, source: &str) -> Option<Command> {
    let path = std::path::PathBuf::from(path);
    if !is_executable_file(&path) {
        eprintln!(
            "skipping {source} candidate {}: path must exist and name a regular executable",
            path.display()
        );
        return None;
    }
    let mut command = Command::new(path);
    command.arg("--stdio");
    Some(command)
}

/// Executable opposite-profile leftover in the target directory, if any.
///
/// Purely refusal-diagnostics context (#11905): it is never a spawn
/// candidate. The active profile needs no probe — its candidate always comes
/// from the freshness pre-build (#11946).
pub(crate) fn opposite_profile_artifact(
    target_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let [_active, opposite] = active_profile_order();
    let opposite_binary = target_dir.join(opposite).join(perllsp_file_name());
    is_executable_file(&opposite_binary).then_some(opposite_binary)
}

/// Refusal text after a failed pre-build. Keeps the #11848 cargo-run
/// refusal and, when an opposite-profile artifact exists, names that exact
/// path plus why reusing it would lie about what these tests exercise.
pub(crate) fn refuse_after_failed_build(
    build_errors: &str,
    refused_opposite: Option<&std::path::Path>,
) -> String {
    let opposite_note = match refused_opposite {
        Some(path) => format!(
            "\nan executable opposite-profile artifact was found at {} but refused: it was \
             built from different source, and running it would silently test stale code",
            path.display()
        ),
        None => String::new(),
    };
    format!(
        "pre-building the perllsp binary failed:\n{build_errors}{opposite_note}\nrefusing the \
         cargo-run fallback because it would compile inside the initialize deadline and stall \
         the handshake (#11848)",
    )
}

/// File name of the server executable, including the platform suffix.
pub(crate) fn perllsp_file_name() -> String {
    format!("perllsp{}", std::env::consts::EXE_SUFFIX)
}

/// Workspace root inferred from `CARGO_MANIFEST_DIR` — the nearest ancestor
/// holding `Cargo.lock`.
fn workspace_root_from_manifest() -> Option<std::path::PathBuf> {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")?;
    let crate_dir = std::path::Path::new(&manifest_dir);
    Some(
        crate_dir
            .ancestors()
            .find(|p| p.join("Cargo.lock").exists())
            .unwrap_or(crate_dir)
            .to_path_buf(),
    )
}

/// Absolute artifact paths the resolver considers, in profile order.
///
/// The no-candidate spawn diagnostics print exactly these paths so an
/// operator sees where the resolver looks — including a custom
/// CARGO_TARGET_DIR, the exact condition that hid the binary behind the
/// #11848 stalls. Keeping this as the single authority prevents the
/// diagnostics from drifting back to hardcoded `workspace/target` paths.
pub(crate) fn probed_binary_paths() -> Vec<std::path::PathBuf> {
    let Some(workspace_root) = workspace_root_from_manifest() else {
        return Vec::new();
    };
    active_profile_order()
        .iter()
        .map(|profile| target_directory(&workspace_root).join(profile).join(perllsp_file_name()))
        .collect()
}

fn target_directory(workspace_root: &std::path::Path) -> std::path::PathBuf {
    target_directory_from(std::env::var_os("CARGO_TARGET_DIR"), workspace_root)
}

pub(crate) fn target_directory_from(
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
pub(crate) fn active_profile_order() -> [&'static str; 2] {
    if cfg!(debug_assertions) { ["debug", "release"] } else { ["release", "debug"] }
}

/// Build (or freshness-verify) the `perllsp` binary once per test process.
///
/// The tests spawn a server owned by a different package, so nothing in
/// `cargo test -p perl-lsp-rs` guarantees it exists or is current. Routing
/// every target-directory candidate through this pre-build — before any
/// request deadline starts — keeps the cost out of `initialize`, which
/// previously timed out against an inline `cargo run` that needed roughly a
/// minute to compile, and lets cargo's fingerprints rebuild exactly when an
/// existing artifact predates this source tree (#11946). Existence-based
/// reuse would silently test stale code and is deliberately impossible.
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

pub(crate) fn built_binary_or_refuse(
    path: std::path::PathBuf,
) -> Result<std::path::PathBuf, String> {
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
