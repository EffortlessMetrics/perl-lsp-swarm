#![expect(
    clippy::expect_used,
    reason = "test-only resolution barriers must fail loudly, not degrade into the cargo fallback"
)]

use crate::common::binary_resolution::{
    active_profile_order, built_binary_or_refuse, opposite_profile_artifact, perllsp_file_name,
    probed_binary_paths, refuse_after_failed_build, resolve_explicit_candidates,
    resolve_perl_lsp_cmds, target_directory_from,
};
use perl_tdd_support::must;

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
    /// it. The leftover is collected purely as refusal-diagnostics context;
    /// the spawn candidate always comes from the freshness pre-build.
    #[test]
    fn stale_opposite_profile_artifact_is_refused_not_reused() {
        let root = planted_artifact_workspace("stale-opposite-refused");
        let target_dir = root.join("target");
        let stale = plant_fake_perllsp(&target_dir, super::active_profile_order()[1]);

        let leftover = super::opposite_profile_artifact(&target_dir);
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

    /// The #11946 regression surface: the resolver must not contain an
    /// existence-based reuse path for the active-profile artifact, because
    /// an artifact left by an earlier checkout predates the reviewed source.
    /// With the reuse branch removed, every target-directory candidate comes
    /// from `ensure_perllsp_built`, so this pins the diagnostics contract
    /// that replaced probing and keeps the refusal text honest when the
    /// pre-build fails beside a planted leftover.
    #[test]
    fn failed_prebuild_names_a_planted_leftover_without_reusing_it() {
        let root = planted_artifact_workspace("prebuild-refuses-leftover");
        let target_dir = root.join("target");
        plant_fake_perllsp(&target_dir, super::active_profile_order()[0]);
        let stale = plant_fake_perllsp(&target_dir, super::active_profile_order()[1]);

        let leftover = super::opposite_profile_artifact(&target_dir);
        assert_eq!(leftover.as_deref(), Some(stale.as_path()));
        let refusal =
            super::refuse_after_failed_build("cargo build -p perllsp failed", leftover.as_deref());
        assert!(refusal.contains(&stale.display().to_string()), "{refusal}");
        assert!(refusal.contains("silently test stale code"), "{refusal}");
        assert!(refusal.contains("#11848"), "{refusal}");

        must(
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("remove synthetic workspace {}: {e}", root.display())),
        );
    }

    /// A bare target directory contributes no opposite-profile diagnostics,
    /// and the refusal text carries no opposite-profile clause when there is
    /// nothing to refuse.
    #[test]
    fn empty_target_directory_requires_a_build_without_opposite_context() {
        let root = planted_artifact_workspace("empty-requires-build");
        let target_dir = root.join("target");

        let found_opposite = super::opposite_profile_artifact(&target_dir);
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
