//! ExtUtils::MakeMaker / Module::Build project detection and root candidates.
//!
//! Non-executing, bounded detection for the #2981 adapter leaf #13636: classify
//! EUMM and Module::Build projects from marker files, emit typed source and
//! generated root candidates with freshness and explicit rejections, and never
//! run `Makefile.PL`, `Build.PL`, `make`, or `./Build`.
//!
//! This module is a facts producer only. [`crate::environment::builder`] remains
//! the pure compiler that turns hand-fed declarations into
//! [`ProjectEnvironmentSnapshot`] receipts.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::environment::{
    BuildSystemKind, EnvironmentInputAuthority, EnvironmentLimitation, IncludeEntryRole,
    ProjectRootRole,
};

/// Adapter surface version recorded alongside build-system facts (#2981).
pub const ADAPTER_VERSION: &str = "eumm_mb_roots.v1";

/// Authoring marker for ExtUtils::MakeMaker.
pub const MAKEFILE_PL: &str = "Makefile.PL";

/// Authoring marker for Module::Build.
pub const BUILD_PL: &str = "Build.PL";

/// Canonical project source root.
pub const SOURCE_LIB: &str = "lib";

/// Explicit test-helper source root.
pub const TEST_LIB: &str = "t/lib";

/// Generated module root (`make` / `./Build` output).
pub const BLIB_LIB: &str = "blib/lib";

/// Generated architecture-specific module root.
pub const BLIB_ARCH: &str = "blib/arch";

/// Generated `Makefile` marker (post-`Makefile.PL`, not executed here).
pub const GENERATED_MAKEFILE: &str = "Makefile";

/// Generated `Build` script marker (post-`Build.PL`, not executed here).
pub const GENERATED_BUILD: &str = "Build";

/// Watcher / invalidation inputs aligned with #13640 metadata routes.
pub const WATCHER_INPUTS: &[&str] = &[
    MAKEFILE_PL,
    BUILD_PL,
    GENERATED_MAKEFILE,
    GENERATED_BUILD,
    "META.json",
    "META.yml",
    "MYMETA.json",
    "MYMETA.yml",
    SOURCE_LIB,
    TEST_LIB,
    BLIB_LIB,
    BLIB_ARCH,
];

/// Rejection: path resolves outside the workspace root.
pub const REASON_ROOT_ESCAPES_WORKSPACE: &str = "root_escapes_workspace";

/// Rejection: expected directory is absent.
pub const REASON_ROOT_MISSING: &str = "root_missing";

/// Rejection: generated tree is older than authoring or source inputs.
pub const REASON_GENERATED_ROOT_STALE: &str = "generated_root_stale";

/// Rejection: no EUMM/MB authoring marker justifies generated roots.
pub const REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM: &str = "generated_root_without_build_system";

/// Rejection: path exists but is not a directory.
pub const REASON_NOT_A_DIRECTORY: &str = "not_a_directory";

/// Freshness of one root candidate relative to authoring and source inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EummMbRootFreshness {
    /// Present and not stale relative to bounded freshness inputs.
    Current,
    /// Present but older than a reviewed freshness anchor.
    Stale,
    /// Expected path is absent.
    Missing,
    /// Freshness does not apply (for example a source root with no generated mirror).
    NotApplicable,
}

/// Reviewed build-system family detected from marker files only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedEummMbBuildSystem {
    /// No `Makefile.PL` or `Build.PL` marker.
    None,
    /// `Makefile.PL` only.
    ExtUtilsMakeMaker,
    /// `Build.PL` only, or both markers with Module::Build taking precedence.
    ModuleBuild,
}

impl DetectedEummMbBuildSystem {
    /// Map to the environment snapshot build-system enum when a family is known.
    #[must_use]
    pub const fn as_build_system_kind(self) -> Option<BuildSystemKind> {
        match self {
            Self::None => None,
            Self::ExtUtilsMakeMaker => Some(BuildSystemKind::ExtUtilsMakeMaker),
            Self::ModuleBuild => Some(BuildSystemKind::ModuleBuild),
        }
    }
}

/// Disposition of one root candidate after bounded review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EummMbRootDisposition {
    /// Root may participate in environment facts for its role.
    Selected,
    /// Root is visible but must not be treated as an active candidate.
    Rejected {
        /// Stable machine reason.
        reason_code: &'static str,
        /// Human-readable detail without host-specific paths when possible.
        detail: String,
    },
}

/// One typed root/include candidate with provenance and freshness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EummMbRootCandidate {
    /// Workspace-relative POSIX-style path (`lib`, `blib/lib`, …).
    pub relative_path: String,
    /// Project-root role in the environment model.
    pub project_role: ProjectRootRole,
    /// Include-root role when this path participates in `@INC` planning.
    pub include_role: IncludeEntryRole,
    /// Authority class for the candidate.
    pub authority: EnvironmentInputAuthority,
    /// Whether the path exists as a directory at detection time.
    pub exists: bool,
    /// Freshness relative to authoring/source anchors.
    pub freshness: EummMbRootFreshness,
    /// Selected or rejected with a typed reason.
    pub disposition: EummMbRootDisposition,
    /// Producer-local stable order (`blib/arch` before `blib/lib`).
    pub source_order: u32,
    /// When true, this source root outranks a generated mirror of the same module.
    pub source_canonical_over_generated: bool,
}

/// Bounded EUMM / Module::Build detection for one workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EummMbDetection {
    /// `Makefile.PL` marker present.
    pub makefile_pl_present: bool,
    /// `Build.PL` marker present.
    pub build_pl_present: bool,
    /// Reviewed build-system classification.
    pub build_system: DetectedEummMbBuildSystem,
    /// Ordered root candidates (selected and rejected).
    pub roots: Vec<EummMbRootCandidate>,
    /// Explicit limitations (for example unreadable freshness metadata).
    pub limitations: Vec<EnvironmentLimitation>,
}

/// Detect EUMM / Module::Build markers and root candidates for `workspace_root`.
///
/// Bounded and non-executing: marker existence checks, directory metadata, and
/// a capped walk of `lib/` for freshness comparison only. No build scripts are
/// executed and no subprocess is spawned.
#[must_use]
pub fn detect_eumm_mb(workspace_root: &Path) -> EummMbDetection {
    let makefile_pl_present = is_marker_file(&workspace_root.join(MAKEFILE_PL));
    let build_pl_present = is_marker_file(&workspace_root.join(BUILD_PL));

    let build_system = classify_build_system(makefile_pl_present, build_pl_present);

    let mut limitations = Vec::new();
    let freshness_anchors =
        collect_freshness_anchors(workspace_root, makefile_pl_present, build_pl_present);

    let mut roots = Vec::new();
    roots.push(evaluate_source_root(
        workspace_root,
        SOURCE_LIB,
        ProjectRootRole::Source,
        IncludeEntryRole::WorkspaceConfigured,
        0,
        true,
        &freshness_anchors,
    ));
    roots.push(evaluate_source_root(
        workspace_root,
        TEST_LIB,
        ProjectRootRole::Test,
        IncludeEntryRole::Other,
        1,
        false,
        &freshness_anchors,
    ));

    if matches!(build_system, DetectedEummMbBuildSystem::None) {
        for (relative, role, order) in [
            (BLIB_ARCH, IncludeEntryRole::BlibArch, 2_u32),
            (BLIB_LIB, IncludeEntryRole::BlibLib, 3_u32),
        ] {
            roots.push(reject_generated_without_build_system(
                workspace_root,
                relative,
                role,
                order,
            ));
        }
    } else {
        roots.push(evaluate_generated_root(
            workspace_root,
            BLIB_ARCH,
            IncludeEntryRole::BlibArch,
            2,
            &freshness_anchors,
            &mut limitations,
        ));
        roots.push(evaluate_generated_root(
            workspace_root,
            BLIB_LIB,
            IncludeEntryRole::BlibLib,
            3,
            &freshness_anchors,
            &mut limitations,
        ));
    }

    EummMbDetection { makefile_pl_present, build_pl_present, build_system, roots, limitations }
}

fn classify_build_system(
    makefile_pl_present: bool,
    build_pl_present: bool,
) -> DetectedEummMbBuildSystem {
    match (makefile_pl_present, build_pl_present) {
        (false, false) => DetectedEummMbBuildSystem::None,
        (true, false) => DetectedEummMbBuildSystem::ExtUtilsMakeMaker,
        (false, true) => DetectedEummMbBuildSystem::ModuleBuild,
        (true, true) => DetectedEummMbBuildSystem::ModuleBuild,
    }
}

struct FreshnessAnchors {
    authoring_latest: Option<SystemTime>,
    source_lib_latest: Option<SystemTime>,
}

fn collect_freshness_anchors(
    workspace_root: &Path,
    makefile_pl_present: bool,
    build_pl_present: bool,
) -> FreshnessAnchors {
    let mut authoring_latest = None;
    if makefile_pl_present {
        merge_latest(&mut authoring_latest, file_mtime(&workspace_root.join(MAKEFILE_PL)));
    }
    if build_pl_present {
        merge_latest(&mut authoring_latest, file_mtime(&workspace_root.join(BUILD_PL)));
    }
    let source_lib_latest = directory_latest_mtime_bounded(&workspace_root.join(SOURCE_LIB), 4096);
    FreshnessAnchors { authoring_latest, source_lib_latest }
}

fn evaluate_source_root(
    workspace_root: &Path,
    relative: &str,
    project_role: ProjectRootRole,
    include_role: IncludeEntryRole,
    source_order: u32,
    source_canonical_over_generated: bool,
    _anchors: &FreshnessAnchors,
) -> EummMbRootCandidate {
    let path = workspace_root.join(relative);
    match bounded_directory_state(workspace_root, &path) {
        DirectoryState::Missing => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: false,
            freshness: EummMbRootFreshness::Missing,
            disposition: EummMbRootDisposition::Rejected {
                reason_code: REASON_ROOT_MISSING,
                detail: format!("expected source root `{relative}` is absent"),
            },
            source_order,
            source_canonical_over_generated,
        },
        DirectoryState::EscapesWorkspace => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: false,
            freshness: EummMbRootFreshness::Missing,
            disposition: EummMbRootDisposition::Rejected {
                reason_code: REASON_ROOT_ESCAPES_WORKSPACE,
                detail: format!("source root `{relative}` resolves outside the workspace"),
            },
            source_order,
            source_canonical_over_generated,
        },
        DirectoryState::NotDirectory => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: false,
            freshness: EummMbRootFreshness::Missing,
            disposition: EummMbRootDisposition::Rejected {
                reason_code: REASON_NOT_A_DIRECTORY,
                detail: format!("path `{relative}` exists but is not a directory"),
            },
            source_order,
            source_canonical_over_generated,
        },
        DirectoryState::Present => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: true,
            freshness: EummMbRootFreshness::NotApplicable,
            disposition: EummMbRootDisposition::Selected,
            source_order,
            source_canonical_over_generated,
        },
    }
}

fn reject_generated_without_build_system(
    workspace_root: &Path,
    relative: &str,
    include_role: IncludeEntryRole,
    source_order: u32,
) -> EummMbRootCandidate {
    let path = workspace_root.join(relative);
    let exists = matches!(bounded_directory_state(workspace_root, &path), DirectoryState::Present,);
    EummMbRootCandidate {
        relative_path: relative.to_string(),
        project_role: ProjectRootRole::Generated,
        include_role,
        authority: EnvironmentInputAuthority::BuildMetadata,
        exists,
        freshness: EummMbRootFreshness::NotApplicable,
        disposition: EummMbRootDisposition::Rejected {
            reason_code: REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM,
            detail: format!(
                "generated root `{relative}` is not admitted without a Makefile.PL or Build.PL marker"
            ),
        },
        source_order,
        source_canonical_over_generated: false,
    }
}

fn evaluate_generated_root(
    workspace_root: &Path,
    relative: &str,
    include_role: IncludeEntryRole,
    source_order: u32,
    anchors: &FreshnessAnchors,
    limitations: &mut Vec<EnvironmentLimitation>,
) -> EummMbRootCandidate {
    let path = workspace_root.join(relative);
    match bounded_directory_state(workspace_root, &path) {
        DirectoryState::Missing => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role: ProjectRootRole::Generated,
            include_role,
            authority: EnvironmentInputAuthority::BuildMetadata,
            exists: false,
            freshness: EummMbRootFreshness::Missing,
            disposition: EummMbRootDisposition::Rejected {
                reason_code: REASON_ROOT_MISSING,
                detail: format!("generated root `{relative}` is absent"),
            },
            source_order,
            source_canonical_over_generated: false,
        },
        DirectoryState::EscapesWorkspace => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role: ProjectRootRole::Generated,
            include_role,
            authority: EnvironmentInputAuthority::BuildMetadata,
            exists: false,
            freshness: EummMbRootFreshness::Missing,
            disposition: EummMbRootDisposition::Rejected {
                reason_code: REASON_ROOT_ESCAPES_WORKSPACE,
                detail: format!("generated root `{relative}` resolves outside the workspace"),
            },
            source_order,
            source_canonical_over_generated: false,
        },
        DirectoryState::NotDirectory => EummMbRootCandidate {
            relative_path: relative.to_string(),
            project_role: ProjectRootRole::Generated,
            include_role,
            authority: EnvironmentInputAuthority::BuildMetadata,
            exists: false,
            freshness: EummMbRootFreshness::Missing,
            disposition: EummMbRootDisposition::Rejected {
                reason_code: REASON_NOT_A_DIRECTORY,
                detail: format!("path `{relative}` exists but is not a directory"),
            },
            source_order,
            source_canonical_over_generated: false,
        },
        DirectoryState::Present => {
            let generated_latest =
                directory_latest_mtime_bounded(&path, 4096).or_else(|| file_mtime(&path));
            let freshness = classify_generated_freshness(generated_latest, anchors, limitations);
            let disposition = match freshness {
                EummMbRootFreshness::Current => EummMbRootDisposition::Selected,
                EummMbRootFreshness::Stale => EummMbRootDisposition::Rejected {
                    reason_code: REASON_GENERATED_ROOT_STALE,
                    detail: format!(
                        "generated root `{relative}` is stale relative to authoring or source inputs"
                    ),
                },
                EummMbRootFreshness::Missing | EummMbRootFreshness::NotApplicable => {
                    EummMbRootDisposition::Rejected {
                        reason_code: REASON_ROOT_MISSING,
                        detail: format!(
                            "generated root `{relative}` could not be classified as current"
                        ),
                    }
                }
            };
            EummMbRootCandidate {
                relative_path: relative.to_string(),
                project_role: ProjectRootRole::Generated,
                include_role,
                authority: EnvironmentInputAuthority::BuildMetadata,
                exists: true,
                freshness,
                disposition,
                source_order,
                source_canonical_over_generated: false,
            }
        }
    }
}

fn classify_generated_freshness(
    generated_mtime: Option<SystemTime>,
    anchors: &FreshnessAnchors,
    limitations: &mut Vec<EnvironmentLimitation>,
) -> EummMbRootFreshness {
    let Some(generated_mtime) = generated_mtime else {
        limitations.push(EnvironmentLimitation {
            code: "generated_root_mtime_unavailable".to_string(),
            detail:
                "could not read modification time for a generated root; treating freshness as stale"
                    .to_string(),
            input_id: None,
        });
        return EummMbRootFreshness::Stale;
    };

    if anchors.authoring_latest.is_some_and(|anchor| generated_mtime < anchor) {
        return EummMbRootFreshness::Stale;
    }
    if anchors.source_lib_latest.is_some_and(|anchor| generated_mtime < anchor) {
        return EummMbRootFreshness::Stale;
    }
    EummMbRootFreshness::Current
}

enum DirectoryState {
    Missing,
    EscapesWorkspace,
    NotDirectory,
    Present,
}

fn bounded_directory_state(workspace_root: &Path, path: &Path) -> DirectoryState {
    if path_exists_outside_workspace(workspace_root, path) {
        return DirectoryState::EscapesWorkspace;
    }
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return DirectoryState::Missing,
    };
    if !metadata.is_dir() {
        return DirectoryState::NotDirectory;
    }
    DirectoryState::Present
}

fn path_exists_outside_workspace(workspace_root: &Path, path: &Path) -> bool {
    let root = lexical_normalize(workspace_root);
    let Ok(relative) = path.strip_prefix(workspace_root) else {
        return !lexical_normalize(path).starts_with(root.as_path());
    };

    let mut current = workspace_root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                current.push(name);
                if std::fs::symlink_metadata(&current)
                    .ok()
                    .is_some_and(|meta| meta.file_type().is_symlink())
                {
                    let Ok(link_target) = std::fs::read_link(&current) else {
                        return true;
                    };
                    current = resolve_symlink_target(&current, &link_target);
                    if !lexical_normalize(&current).starts_with(root.as_path()) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    !lexical_normalize(&current).starts_with(root.as_path())
}

fn resolve_symlink_target(link_path: &Path, link_target: &Path) -> PathBuf {
    if link_target.is_absolute() {
        lexical_normalize(link_target)
    } else {
        lexical_normalize(&link_path.parent().unwrap_or(link_path).join(link_target))
    }
}

fn is_marker_file(path: &Path) -> bool {
    std::fs::metadata(path).ok().is_some_and(|meta| meta.is_file())
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

fn merge_latest(target: &mut Option<SystemTime>, candidate: Option<SystemTime>) {
    match (target.as_ref(), candidate) {
        (_, None) => {}
        (None, Some(value)) => *target = Some(value),
        (Some(current), Some(value)) if value > *current => *target = Some(value),
        _ => {}
    }
}

fn directory_latest_mtime_bounded(root: &Path, max_entries: usize) -> Option<SystemTime> {
    let mut latest = file_mtime(root);
    let entries = std::fs::read_dir(root).ok()?;
    let mut seen = 0_usize;
    for entry in entries.flatten() {
        seen += 1;
        if seen > max_entries {
            break;
        }
        let path = entry.path();
        let entry_meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if entry_meta.file_type().is_symlink() {
            merge_latest(&mut latest, file_mtime(&path));
            continue;
        }
        if entry_meta.is_dir() {
            merge_latest(
                &mut latest,
                directory_latest_mtime_bounded(&path, max_entries.saturating_sub(seen)),
            );
        } else {
            merge_latest(&mut latest, file_mtime(&path));
        }
    }
    latest
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
