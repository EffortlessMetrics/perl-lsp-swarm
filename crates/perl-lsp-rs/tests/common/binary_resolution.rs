//! Binary resolution logic for finding the Perl LSP executable.
//!
//! Resolution order (fixed for test reliability):
//! 1. PERL_LSP_BIN env var (explicit override)
//! 2. Runtime `CARGO_BIN_EXE_perllsp` (when owned by the product package)
//! 3. Workspace target directory: ONLY the active profile's artifact,
//!    honoring CARGO_TARGET_DIR, with a bounded pre-build on miss; an
//!    opposite-profile leftover is refused, never silently reused
//! 4. PATH lookup
//!
//! There is deliberately NO `cargo run` candidate: a cargo invocation inside
//! the handshake deadline IS the #11848 stall family — it must wait on the
//! build-directory lock under suite contention and can compile for minutes.
//! Every non-env candidate is either an existing executable or a pre-build
//! performed before any deadline starts; when nothing resolves or spawns,
//! the harness fails loudly instead of silently degrading into cargo.
//!
//! Freshness is part of the contract (P2 review on #11905): only the ACTIVE
//! profile's artifact describes this source tree. An executable left behind
//! by the opposite profile — e.g. a stale `target/release/perllsp` beside a
//! missing debug one — predates the reviewed source, so accepting it would
//! skip the pre-build and make these tests silently exercise stale code.

use perl_tdd_support::must;
use std::path::Path;
use std::process::Command;

const BUILD_STDERR_MAX_BYTES: usize = 8 * 1024;

pub(crate) fn resolve_perl_lsp_cmds() -> impl Iterator<Item = Command> {
    // Resolution order (fixed for test reliability):
    // 1. Explicit override via PERL_LSP_BIN
    // 2. Compile-time CARGO_BIN_EXE (guaranteed correct during `cargo test -p perl-lsp-rs`)
    // 3. Runtime CARGO_BIN_EXE_* (fallback for edge cases)
    // 4. Workspace target directory: ACTIVE-profile artifact only
    // 5. PATH lookup
    //
    // IMPORTANT: only the active profile's binary is acceptable here. An
    // opposite-profile executable may predate the reviewed source; reusing it
    // would silently test stale code (P2 review on #11905).
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

    // 3. Workspace target directory (using absolute paths) — active profile
    //    only, per the freshness contract above.
    if let Some(workspace_root) = workspace_root_from_manifest() {
        match probe_target_artifacts(&workspace_root) {
            TargetArtifactProbe::ReuseActiveProfile(binary) => {
                let mut c = Command::new(binary);
                c.arg("--stdio");
                v.push(c);
            }
            TargetArtifactProbe::MustBuild { found_opposite } => {
                // The server binary lives in the `perllsp` package, not in
                // `perl-lsp-rs` where these tests live, so `cargo test -p
                // perl-lsp-rs` never builds it. Build it ONCE here — before any
                // request deadline starts, because no cargo invocation may run
                // inside one (#11848). A failed build refuses loudly, naming
                // any opposite-profile artifact that was found and why it was
                // refused instead of reused.
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

fn resolve_explicit_candidates(
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

/// What probing the workspace target directory may contribute to the suite.
///
/// The freshness boundary from the P2 review on #11905: only the ACTIVE
/// profile's artifact (debug for a normal test build) describes this source
/// tree. An opposite-profile executable left by an earlier checkout would
/// otherwise be accepted while the required pre-build was skipped, and the
/// suite would silently exercise stale code.
#[derive(Debug)]
enum TargetArtifactProbe {
    /// Executable present for the active profile; safe to reuse.
    ReuseActiveProfile(std::path::PathBuf),
    /// No active-profile artifact. The suite must pre-build one before any
    /// request deadline (#11848); `found_opposite` carries an executable
    /// opposite-profile leftover purely as refusal-diagnostics context —
    /// it is never a spawn candidate.
    MustBuild { found_opposite: Option<std::path::PathBuf> },
}

fn probe_target_artifacts(workspace_root: &std::path::Path) -> TargetArtifactProbe {
    probe_target_artifacts_at(&target_directory(workspace_root))
}

fn probe_target_artifacts_at(target_dir: &std::path::Path) -> TargetArtifactProbe {
    let [active, opposite] = active_profile_order();
    let active_binary = target_dir.join(active).join(perllsp_file_name());
    if is_executable_file(&active_binary) {
        return TargetArtifactProbe::ReuseActiveProfile(active_binary);
    }
    let opposite_binary = target_dir.join(opposite).join(perllsp_file_name());
    let found_opposite = is_executable_file(&opposite_binary).then_some(opposite_binary);
    TargetArtifactProbe::MustBuild { found_opposite }
}

/// Refusal text after a failed pre-build. Keeps the #11848 cargo-run
/// refusal and, when an opposite-profile artifact exists, names that exact
/// path plus why reusing it would lie about what these tests exercise.
fn refuse_after_failed_build(
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
fn perllsp_file_name() -> String {
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

/// Absolute paths the resolver probes for a prebuilt server binary, in
/// resolution order.
///
/// The no-candidate spawn diagnostics print exactly these paths so an
/// operator sees what the resolver saw — including a custom CARGO_TARGET_DIR,
/// the exact condition that hid the binary behind the #11848 stalls. Keeping
/// this as the single authority prevents the diagnostics from drifting back
/// to hardcoded `workspace/target` paths.
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
    use super::{built_binary_or_refuse, must, resolve_explicit_candidates, target_directory_from};
    use std::ffi::{OsStr, OsString};
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

    /// True when the command program names cargo itself, whatever spelling or
    /// absolute location — classified by executable basename/file-stem so an
    /// absolute cargo path cannot slip past a string-prefix check (#11848).
    fn is_cargo_invocation(program: &std::ffi::OsStr) -> bool {
        Path::new(program).file_stem().is_some_and(|stem| stem.eq_ignore_ascii_case("cargo"))
    }

    #[test]
    fn cargo_classifier_uses_the_executable_stem_not_a_string_prefix() {
        for cargo_spelling in
            ["cargo", "cargo.exe", "CARGO.EXE", "./cargo", "../bin/cargo", "/usr/local/bin/cargo"]
        {
            assert!(
                is_cargo_invocation(OsStr::new(cargo_spelling)),
                "`{cargo_spelling}` must classify as cargo"
            );
        }
        for other_program in ["perllsp", "perllsp.exe", "cargocult", "/usr/local/bin/perllsp"] {
            assert!(
                !is_cargo_invocation(OsStr::new(other_program)),
                "`{other_program}` must not classify as cargo"
            );
        }
        #[cfg(windows)]
        assert!(is_cargo_invocation(OsStr::new(r"C:\Users\x\.cargo\bin\cargo.exe")));
    }

    /// The #11848 stall family was a `cargo run` candidate compiling inside
    /// the initialize deadline; its refusal contract only holds if no spawn
    /// path can ever reach cargo. This guards the structural invariant: every
    /// yielded candidate is a concrete perllsp executable, never a cargo
    /// invocation that would take the build-directory lock or compile under
    /// the handshake timeout. Classification is by executable stem so absolute
    /// cargo installations are caught too.
    #[test]
    fn resolution_never_offers_a_cargo_candidate() {
        for cmd in super::resolve_perl_lsp_cmds() {
            let program = cmd.get_program();
            assert!(
                !is_cargo_invocation(program),
                "candidate `{}` would compile or block on the build lock inside the \
                 initialize deadline (#11848)",
                program.to_string_lossy()
            );
        }
    }

    /// The probed paths shown in spawn-failure diagnostics must be exactly
    /// what the resolver probes — including the effective CARGO_TARGET_DIR —
    /// so the next stall receipt cannot be misled by stale hardcoded paths.
    #[test]
    fn probed_paths_carry_the_executable_suffix_and_profile_order() {
        let paths = super::probed_binary_paths();
        assert!(!paths.is_empty(), "CARGO_MANIFEST_DIR is set under cargo test");
        assert!(
            paths.iter().all(|p| p
                .to_string_lossy()
                .ends_with(&format!("perllsp{}", std::env::consts::EXE_SUFFIX))),
            "every probed path must name the platform-suffixed binary: {paths:?}"
        );
        assert_eq!(paths.len(), 2, "debug and release profiles are both probed");
    }

    #[test]
    fn current_executable_is_accepted_as_a_real_binary() {
        let path = must(std::env::current_exe().map_err(|e| format!("resolve current exe: {e}")));
        assert_eq!(built_binary_or_refuse(path.clone()), Ok(path));
    }

    #[test]
    fn valid_explicit_candidate_short_circuits_lower_priority_resolution() {
        let root = planted_artifact_workspace("explicit-short-circuit");
        let explicit = plant_fake_perllsp(&root, "explicit");

        let candidates = resolve_explicit_candidates(
            Some(explicit.clone().into_os_string()),
            Some(OsString::from("missing-cargo-candidate")),
        )
        .expect("valid explicit candidate must resolve");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].get_program(), explicit.as_os_str());

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
    }

    #[test]
    fn invalid_explicit_candidate_is_skipped_for_a_valid_lower_priority_candidate() {
        let root = planted_artifact_workspace("invalid-explicit-fallback");
        let fallback = plant_fake_perllsp(&root, "fallback");

        let candidates = resolve_explicit_candidates(
            Some(root.join("missing-explicit").into_os_string()),
            Some(fallback.clone().into_os_string()),
        )
        .expect("valid lower-priority candidate must resolve");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].get_program(), fallback.as_os_str());

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_executable_explicit_candidate_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let root = planted_artifact_workspace("non-executable-explicit");
        let path = root.join("perllsp");
        std::fs::write(&path, b"not executable").expect("write explicit candidate");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("make explicit candidate non-executable");

        assert!(resolve_explicit_candidates(Some(path.into_os_string()), None).is_none());

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
    }

    #[test]
    fn missing_relative_explicit_candidate_is_rejected() {
        assert!(resolve_explicit_candidates(
            Some(OsString::from("./definitely-missing-perllsp")),
            None,
        )
        .is_none());
    }

    /// Synthetic workspace root for target-directory probes: isolated under
    /// the temp dir so tests never read or mutate the real build products.
    fn planted_artifact_workspace(test_name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir()
            .join(format!("perl-lsp-rs-target-probe-{}-{test_name}", std::process::id()));
        must(
            std::fs::create_dir_all(&root)
                .map_err(|e| format!("create synthetic workspace root {}: {e}", root.display())),
        );
        root
    }

    /// Plant a fake executable server binary at
    /// `<root>/target/<profile>/perllsp<suffix>` and return its path. The
    /// bytes are irrelevant: acceptance checks executability, and on this
    /// platform that is regular-file-ness — exactly what let stale leftovers
    /// slip through before the #11905 repair.
    fn plant_fake_perllsp(target_dir: &Path, profile: &str) -> std::path::PathBuf {
        let profile_dir = target_dir.join(profile);
        must(
            std::fs::create_dir_all(&profile_dir)
                .map_err(|e| format!("create {}: {e}", profile_dir.display())),
        );
        let binary = profile_dir.join(super::perllsp_file_name());
        must(
            std::fs::write(&binary, b"leftover from an earlier checkout")
                .map_err(|e| format!("write fake perllsp artifact {}: {e}", binary.display())),
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            must(
                std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).map_err(
                    |e| format!("make fake perllsp artifact executable {}: {e}", binary.display()),
                ),
            );
        }
        binary
    }

    /// The #11905 P2 regression: a normal debug run whose target directory
    /// retains only an opposite-profile (release) executable must NOT reuse
    /// it. The probe demands a pre-build instead, keeping the leftover purely
    /// as context for a loud, truthful refusal if that build fails.
    #[test]
    fn stale_opposite_profile_artifact_is_refused_not_reused() {
        let root = planted_artifact_workspace("stale-opposite-refused");
        let target_dir = root.join("target");
        let stale = plant_fake_perllsp(&target_dir, super::active_profile_order()[1]);

        let leftover = match super::probe_target_artifacts_at(&target_dir) {
            super::TargetArtifactProbe::MustBuild { found_opposite } => found_opposite,
            super::TargetArtifactProbe::ReuseActiveProfile(path) => {
                must(Err::<Option<std::path::PathBuf>, _>(format!(
                    "opposite-profile leftover {} must never be reused as a candidate",
                    path.display()
                )))
            }
        };
        assert_eq!(leftover.as_deref(), Some(stale.as_path()));
        let refusal = super::refuse_after_failed_build("linker failed", leftover.as_deref());
        assert!(refusal.contains(&stale.display().to_string()), "{refusal}");
        assert!(refusal.contains("silently test stale code"), "{refusal}");
        assert!(refusal.contains("#11848"), "{refusal}");

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
    }

    /// When the active profile's own artifact exists, it wins regardless of
    /// any opposite-profile leftover; the probe never consults the latter.
    #[test]
    fn active_profile_artifact_is_preferred_over_an_opposite_profile_leftover() {
        let root = planted_artifact_workspace("active-preferred");
        let target_dir = root.join("target");
        let active = plant_fake_perllsp(&target_dir, super::active_profile_order()[0]);
        plant_fake_perllsp(&target_dir, super::active_profile_order()[1]);

        let reused = match super::probe_target_artifacts_at(&target_dir) {
            super::TargetArtifactProbe::ReuseActiveProfile(path) => path,
            super::TargetArtifactProbe::MustBuild { .. } => must(Err::<std::path::PathBuf, _>(
                "an existing active-profile artifact must not trigger a rebuild".to_string(),
            )),
        };
        assert_eq!(reused, active);

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
    }

    /// A bare target directory demands a build, and the refusal text carries
    /// no opposite-profile clause when there is nothing to refuse.
    #[test]
    fn empty_target_directory_requires_a_build_without_opposite_context() {
        let root = planted_artifact_workspace("empty-requires-build");
        let target_dir = root.join("target");

        let found_opposite = match super::probe_target_artifacts_at(&target_dir) {
            super::TargetArtifactProbe::MustBuild { found_opposite } => found_opposite,
            super::TargetArtifactProbe::ReuseActiveProfile(path) => {
                must(Err::<Option<std::path::PathBuf>, _>(format!(
                    "empty target directory must require a clean build, got reuse of {}",
                    path.display()
                )))
            }
        };
        assert_eq!(found_opposite, None);
        let refusal = super::refuse_after_failed_build("boom", None);
        assert!(refusal.contains("#11848"), "{refusal}");
        assert!(!refusal.contains("opposite-profile artifact"), "{refusal}");

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
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
