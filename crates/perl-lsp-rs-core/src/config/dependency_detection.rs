//! Marker-based Perl dependency-manager include-path detection.

use std::io;
use std::path::Path;

/// Marker paths are defined once in `super::project_metadata` so watcher
/// invalidation (#13640) classifies exactly the paths this detector probes.
use super::project_metadata::{
    CARMEL_DEV_STATE, CARMEL_ROLLOUT_SENTINEL, CARTON_LOCK, CPANFILE_DECLARATION, CPANFILE_SNAPSHOT,
};

/// Carton and rolled-out Carmel share one install-base layout: a standard
/// ExtUtils::InstallPaths `local/` tree whose single include root is
/// `local/lib/perl5` (#13642 plan S8 receipt §1/§3).
const LOCAL_INSTALL_INCLUDE_PATH: &str = "local/lib/perl5";

/// Outcome of a marker-based include-root decision (#15087).
///
/// `Detected` is a proven presence or absence. `Indeterminate` means at least
/// one probe that can change the current root truth table failed with an
/// error other than `NotFound`, so reconciliation must not retire prior
/// detector-owned roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DependencyIncludeDetection {
    Detected(Vec<String>),
    Indeterminate,
}

/// Three-valued marker observation: a filesystem that cannot answer is not
/// treated as absence (`Path::is_file` collapses every `stat` error to
/// `false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerPresence {
    Present,
    Absent,
    Unknown,
}

impl MarkerPresence {
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Absent, _) | (_, Self::Absent) => Self::Absent,
            (Self::Present, Self::Present) => Self::Present,
            (Self::Present, Self::Unknown)
            | (Self::Unknown, Self::Present)
            | (Self::Unknown, Self::Unknown) => Self::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Present, _) | (_, Self::Present) => Self::Present,
            (Self::Absent, Self::Absent) => Self::Absent,
            (Self::Absent, Self::Unknown)
            | (Self::Unknown, Self::Absent)
            | (Self::Unknown, Self::Unknown) => Self::Unknown,
        }
    }

    fn not(self) -> Self {
        match self {
            Self::Present => Self::Absent,
            Self::Absent => Self::Present,
            Self::Unknown => Self::Unknown,
        }
    }
}

/// Classify a marker path without collapsing non-`NotFound` errors to absence.
///
/// `try_exists` distinguishes confirmed absence from an unreadable probe;
/// `metadata` then decides file versus directory. A path that exists but is
/// not a file is a real answer (`Absent`), matching the previous `is_file`
/// contract for directories.
fn probe_marker_file(path: &Path) -> MarkerPresence {
    match path.try_exists() {
        Ok(false) => return MarkerPresence::Absent,
        Ok(true) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return MarkerPresence::Absent,
        Err(_) => return MarkerPresence::Unknown,
    }

    match path.metadata() {
        Ok(meta) if meta.is_file() => MarkerPresence::Present,
        Ok(_) => MarkerPresence::Absent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => MarkerPresence::Absent,
        Err(_) => MarkerPresence::Unknown,
    }
}

fn resolve_detection(
    declared: MarkerPresence,
    snapshot_locked: MarkerPresence,
    carton_locked: MarkerPresence,
    carmel_dev_state: MarkerPresence,
    carmel_rolled_out: MarkerPresence,
) -> DependencyIncludeDetection {
    // Current root truth table: admit `local/lib/perl5` iff declared &&
    // (carton.lock || (cpanfile.snapshot && !Carmel-dev) || local/.carmel).
    let carton_root = carton_locked.or(snapshot_locked.and(carmel_dev_state.not()));
    match declared.and(carton_root.or(carmel_rolled_out)) {
        MarkerPresence::Present => {
            DependencyIncludeDetection::Detected(vec![LOCAL_INSTALL_INCLUDE_PATH.to_string()])
        }
        MarkerPresence::Absent => DependencyIncludeDetection::Detected(Vec::new()),
        MarkerPresence::Unknown => DependencyIncludeDetection::Indeterminate,
    }
}

/// Detect include-root contribution, preserving probe uncertainty (#15087).
#[must_use]
pub(crate) fn detect_dependency_include_paths_result(
    workspace_root: &Path,
    declaration_present: Option<bool>,
) -> DependencyIncludeDetection {
    let declared = match declaration_present {
        Some(true) => MarkerPresence::Present,
        Some(false) => MarkerPresence::Absent,
        None => probe_marker_file(&workspace_root.join(CPANFILE_DECLARATION)),
    };

    resolve_detection(
        declared,
        probe_marker_file(&workspace_root.join(CPANFILE_SNAPSHOT)),
        probe_marker_file(&workspace_root.join(CARTON_LOCK)),
        probe_marker_file(&workspace_root.join(CARMEL_DEV_STATE)),
        probe_marker_file(&workspace_root.join(CARMEL_ROLLOUT_SENTINEL)),
    )
}

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
///
/// This public `Vec` surface maps an indeterminate probe decision to an empty
/// list so the published signature stays unchanged. Production reconcile uses
/// the crate-internal detection result and keeps prior detector-owned roots
/// when the decision cannot be proven (#15087).
#[must_use]
pub fn detect_dependency_include_paths(workspace_root: &Path) -> Vec<String> {
    detect_dependency_include_paths_with_declaration(workspace_root, None)
}

/// As [`detect_dependency_include_paths`], with an authoritative override for
/// whether the `cpanfile` declaration exists (#13640).
///
/// The declaration gate is normally an on-disk probe, but an open editor
/// buffer is the authority for its document (#8041) and outlives an external
/// delete of the backing file. `declaration_present` carries that captured
/// state; `None` falls back to the filesystem. Only the declaration gate is
/// overridable — the install-root markers are genuine filesystem state and are
/// always probed on disk. A supplied `Some(...)` is never replaced by a disk
/// probe of `cpanfile`.
#[must_use]
pub fn detect_dependency_include_paths_with_declaration(
    workspace_root: &Path,
    declaration_present: Option<bool>,
) -> Vec<String> {
    match detect_dependency_include_paths_result(workspace_root, declaration_present) {
        DependencyIncludeDetection::Detected(paths) => paths,
        DependencyIncludeDetection::Indeterminate => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::detect_dependency_include_paths;
    use super::{
        CARMEL_DEV_STATE, CARMEL_ROLLOUT_SENTINEL, DependencyIncludeDetection,
        LOCAL_INSTALL_INCLUDE_PATH, MarkerPresence, detect_dependency_include_paths_result,
        detect_dependency_include_paths_with_declaration, probe_marker_file, resolve_detection,
    };

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

    #[test]
    fn probe_classifies_not_found_and_non_file_as_absent() -> Result<(), Box<dyn std::error::Error>>
    {
        let workspace = tempfile::tempdir()?;
        assert_eq!(
            probe_marker_file(&workspace.path().join("carton.lock")),
            MarkerPresence::Absent
        );

        std::fs::create_dir_all(workspace.path().join("local"))?;
        std::fs::create_dir(workspace.path().join(CARMEL_ROLLOUT_SENTINEL))?;
        assert_eq!(
            probe_marker_file(&workspace.path().join(CARMEL_ROLLOUT_SENTINEL)),
            MarkerPresence::Absent,
            "a directory standing where a marker file would go is a real absence"
        );
        Ok(())
    }

    #[test]
    fn supplied_declaration_authority_is_not_replaced_by_disk()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;

        assert_eq!(
            detect_dependency_include_paths_with_declaration(workspace.path(), Some(true)),
            vec![LOCAL_INSTALL_INCLUDE_PATH],
            "Some(true) must keep gating the Carton root even with no on-disk cpanfile"
        );
        assert!(
            detect_dependency_include_paths_with_declaration(workspace.path(), Some(false))
                .is_empty(),
            "Some(false) must refuse the root even after a later disk cpanfile appears"
        );

        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        assert!(
            detect_dependency_include_paths_with_declaration(workspace.path(), Some(false))
                .is_empty(),
            "a supplied declaration override is never replaced by a disk probe"
        );
        assert_eq!(
            detect_dependency_include_paths_with_declaration(workspace.path(), Some(true)),
            vec![LOCAL_INSTALL_INCLUDE_PATH]
        );
        Ok(())
    }

    #[test]
    fn decision_algebra_keeps_dominating_answers_and_flags_real_ambiguity() {
        use MarkerPresence::{Absent, Present, Unknown};

        assert_eq!(
            resolve_detection(Present, Unknown, Present, Unknown, Unknown),
            DependencyIncludeDetection::Detected(vec![LOCAL_INSTALL_INCLUDE_PATH.to_string()]),
            "an explicit carton.lock establishes the root even if other probes are unknown"
        );
        assert_eq!(
            resolve_detection(Present, Absent, Absent, Unknown, Present),
            DependencyIncludeDetection::Detected(vec![LOCAL_INSTALL_INCLUDE_PATH.to_string()]),
            "a present rollout sentinel establishes the same root"
        );
        assert_eq!(
            resolve_detection(Present, Present, Absent, Unknown, Absent),
            DependencyIncludeDetection::Indeterminate,
            "snapshot plus unknown Carmel-dev is the Carton-versus-dev ambiguity"
        );
        assert_eq!(
            resolve_detection(Present, Absent, Absent, Unknown, Absent),
            DependencyIncludeDetection::Detected(Vec::new()),
            "an unknown Carmel-dev marker cannot create a root by itself"
        );
        assert_eq!(
            resolve_detection(Unknown, Absent, Absent, Absent, Absent),
            DependencyIncludeDetection::Detected(Vec::new()),
            "unknown declaration is irrelevant when no root-establishing marker is present"
        );
        assert_eq!(
            resolve_detection(Unknown, Absent, Present, Absent, Absent),
            DependencyIncludeDetection::Indeterminate,
            "unknown declaration plus a present lock cannot prove presence or absence"
        );
    }

    /// Unix-only: a path whose parent component is a regular file yields
    /// `ENOTDIR`, which `std` does not map to `NotFound`. Windows reports the
    /// same construction as `ERROR_PATH_NOT_FOUND` → `NotFound`, so it cannot
    /// discriminate this classifier. Permission bits are also used below when
    /// they actually constrain the runner; `ENOTDIR` remains the reliable
    /// non-`NotFound` construction (#14812).
    #[cfg(unix)]
    #[test]
    fn non_not_found_probe_error_is_unknown_not_absent() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let not_a_directory = workspace.path().join("regular-file");
        std::fs::write(&not_a_directory, "")?;
        assert_eq!(
            probe_marker_file(&not_a_directory.join("cpanfile")),
            MarkerPresence::Unknown,
            "ENOTDIR must not collapse to absence"
        );

        assert_eq!(
            detect_dependency_include_paths_result(&not_a_directory, None),
            DependencyIncludeDetection::Indeterminate,
            "all five probes unreadable can hide a present carton.lock"
        );
        assert_eq!(
            detect_dependency_include_paths_result(&not_a_directory, Some(false)),
            DependencyIncludeDetection::Detected(Vec::new()),
            "Some(false) still proves absence even when disk probes cannot answer"
        );
        Ok(())
    }
}
