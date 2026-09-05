//! Marker-based Perl dependency-manager include-path detection.

use std::path::Path;

/// Marker paths are defined once in `super::project_metadata` so watcher
/// invalidation (#13640) classifies exactly the paths this detector probes.
use super::project_metadata::{
    CARMEL_DEV_STATE, CARMEL_ROLLOUT_SENTINEL, CARTON_LOCK, CPANFILE_SNAPSHOT,
};

/// Carton and rolled-out Carmel share one install-base layout: a standard
/// ExtUtils::InstallPaths `local/` tree whose single include root is
/// `local/lib/perl5` (#13642 plan S8 receipt §1/§3).
const LOCAL_INSTALL_INCLUDE_PATH: &str = "local/lib/perl5";

/// Detect include paths produced by Carton and Carmel in a workspace root.
///
/// The project is the directory holding `cpanfile` (the declaration; never
/// installed-root evidence by itself). One relative root is produced:
/// `local/lib/perl5` — when Carton markers (`carton.lock`, or a
/// `cpanfile.snapshot` is present without the positive Carmel dev marker, or
/// when the `local/.carmel` rollout sentinel marks the tree as Carmel rollout
/// output. The positive `.carmel/MySetup.pm` marker identifies canonical
/// Carmel dev mode, which has no project-local root.
///
/// A `cpanfile.snapshot` is written in the shared Carton format with no
/// producer field, but `.carmel/MySetup.pm` is a positive Carmel dev marker.
/// In that canonical shape the snapshot is Carmel's own dev-mode lock and
/// must not admit a project-local root. An explicit `carton.lock` still
/// identifies Carton when both managers are present.
///
/// These are runtime-derived roots (#4998): their safety class is "bounded,
/// hard-coded, workspace-relative literals" and must stay that way. They are
/// appended to the effective include-path projection only after
/// resource-scope validation conventions; a future metadata-derived absolute
/// root must enter through an explicitly classified source instead
/// (configuration observation train, #10817).
#[must_use]
pub fn detect_dependency_include_paths(workspace_root: &Path) -> Vec<String> {
    if !workspace_root.join("cpanfile").is_file() {
        return Vec::new();
    }

    let snapshot_locked = workspace_root.join(CPANFILE_SNAPSHOT).is_file();
    let carton_locked = workspace_root.join(CARTON_LOCK).is_file();
    let carmel_dev_state = workspace_root.join(CARMEL_DEV_STATE).is_file();
    let carmel_rolled_out = workspace_root.join(CARMEL_ROLLOUT_SENTINEL).is_file();

    // A `cpanfile.snapshot` shares Carton's format with no producer field.
    // The positive Carmel dev marker identifies the canonical Carmel shape;
    // only an explicit Carton lock can establish a Carton root alongside it.
    let carton_root = carton_locked || (snapshot_locked && !carmel_dev_state);

    if carton_root || carmel_rolled_out {
        vec![LOCAL_INSTALL_INCLUDE_PATH.to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::detect_dependency_include_paths;
    use super::{CARMEL_DEV_STATE, CARMEL_ROLLOUT_SENTINEL, LOCAL_INSTALL_INCLUDE_PATH};

    #[test]
    fn detects_carton_from_lockfile() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;

        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    #[test]
    fn detects_carton_from_cpanfile_snapshot() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("cpanfile.snapshot"), "snapshot\n")?;

        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    /// Migrated Carmel detection (#13642 §2/§3): the post-rollout root is the
    /// shared install-base literal, admitted only through the `local/.carmel`
    /// sentinel that `carmel rollout` touches.
    #[test]
    fn detects_carmel_rollout_from_sentinel() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(workspace.path().join("local"))?;
        std::fs::write(workspace.path().join(CARMEL_ROLLOUT_SENTINEL), "")?;
        std::fs::create_dir_all(workspace.path().join(LOCAL_INSTALL_INCLUDE_PATH))?;

        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    #[test]
    fn rollout_directory_does_not_add_include_path() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(workspace.path().join(CARMEL_ROLLOUT_SENTINEL))?;
        assert!(detect_dependency_include_paths(workspace.path()).is_empty());
        Ok(())
    }

    /// Dev-mode Carmel has no project-local root: the detector must return
    /// nothing, and the out-of-workspace artifact roots are owned by the
    /// environment detection seam, never by this relative-literal detector.
    #[test]
    fn carmel_dev_mode_has_no_project_local_root() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(workspace.path().join(".carmel"))?;
        std::fs::write(
            workspace.path().join(CARMEL_DEV_STATE),
            "our %environment = ('inc' => [], 'base' => '/x');\n",
        )?;

        assert!(detect_dependency_include_paths(workspace.path()).is_empty());
        Ok(())
    }

    /// Canonical Carmel dev mode has no project-local root: the snapshot is
    /// Carmel's own shared-format lock. An explicit Carton lock still wins.
    #[test]
    fn carmel_dev_mode_with_its_own_snapshot_has_no_project_local_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(
            workspace.path().join("cpanfile.snapshot"),
            "# carton snapshot format: version 1.0\n",
        )?;
        std::fs::create_dir_all(workspace.path().join(".carmel"))?;
        std::fs::write(
            workspace.path().join(CARMEL_DEV_STATE),
            "our %environment = ('inc' => [], 'base' => '/x');\n",
        )?;

        assert!(detect_dependency_include_paths(workspace.path()).is_empty());
        Ok(())
    }

    /// An explicit `carton.lock` still identifies Carton even next to Carmel
    /// dev state: both managers present keep the one shared install root.
    #[test]
    fn explicit_carton_lock_wins_over_carmel_dev_state() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(
            workspace.path().join("cpanfile.snapshot"),
            "# carton snapshot format: version 1.0\n",
        )?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        std::fs::create_dir_all(workspace.path().join(".carmel"))?;
        std::fs::write(workspace.path().join(CARMEL_DEV_STATE), "1;\n")?;

        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    /// Contract §1: `vendor/cache` is Carmel's tarball download directory,
    /// never a library root. Nothing about a `vendor/cache` tree may enter
    /// this detector's output, so a future plausible-but-baseless vendor
    /// path cannot re-enter silently.
    #[test]
    fn vendor_cache_alone_is_not_a_root() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(workspace.path().join("vendor/cache"))?;
        std::fs::write(workspace.path().join("vendor/cache/JSON-PP-4.16.tar.gz"), "tarball\n")?;

        assert!(detect_dependency_include_paths(workspace.path()).is_empty());
        Ok(())
    }

    /// Migrated mislabel (#13642 §2): Carmel's source has no `vendor/lib/perl5`
    /// anywhere (`vendor/cache` is a tarball dir), so an unrelated vendor tree
    /// no longer claims Carmel.
    #[test]
    fn vendor_root_no_longer_claims_carmel() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(workspace.path().join("vendor/lib/perl5"))?;

        assert!(detect_dependency_include_paths(workspace.path()).is_empty());
        Ok(())
    }

    /// Both managers present: one shared install root, never duplicated, and
    /// no second Carmel-specific path.
    #[test]
    fn both_managers_yield_single_install_root() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        std::fs::create_dir_all(workspace.path().join(".carmel"))?;
        std::fs::write(workspace.path().join(CARMEL_DEV_STATE), "1;\n")?;

        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    #[test]
    fn requires_expected_markers() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        assert!(detect_dependency_include_paths(workspace.path()).is_empty());

        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );

        std::fs::remove_file(workspace.path().join("carton.lock"))?;
        assert!(detect_dependency_include_paths(workspace.path()).is_empty());

        std::fs::create_dir_all(workspace.path().join("local"))?;
        std::fs::write(workspace.path().join(CARMEL_ROLLOUT_SENTINEL), "")?;
        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    /// Recurrence gate (#4998): runtime-derived dependency roots must stay
    /// relative workspace-contained literals. If a future change makes this
    /// detector return absolute or traversal-capable paths from project
    /// metadata (such as Carmel's embedded out-of-workspace artifact roots),
    /// they would inherit resource-scope validation by convention;
    /// that writer must instead be explicitly classified first.
    #[test]
    fn detected_roots_stay_relative_and_workspace_contained()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        std::fs::create_dir_all(workspace.path().join("local"))?;
        std::fs::write(workspace.path().join(CARMEL_ROLLOUT_SENTINEL), "")?;

        for path in detect_dependency_include_paths(workspace.path()) {
            let candidate = std::path::Path::new(&path);
            assert!(!candidate.is_absolute(), "runtime-derived root must not be absolute: {path}");
            assert!(
                !candidate.components().any(|c| c == std::path::Component::ParentDir),
                "runtime-derived root must not traverse: {path}"
            );
            assert!(
                workspace.path().join(&path).starts_with(workspace.path()),
                "runtime-derived root must stay inside the workspace: {path}"
            );
        }
        Ok(())
    }
}
