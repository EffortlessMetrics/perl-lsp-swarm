//! Dist::Zilla project detection, plugin registry notes, and root candidates.
//!
//! Non-executing, bounded detection for #2980: classify Dist::Zilla projects from
//! `dist.ini`, emit typed source and generated root candidates with freshness and
//! explicit rejections, record reviewed plugin effects from authoring facts, and
//! never run `dzil`, plugins, `Makefile.PL`, or `Build.PL`.
//!
//! This module is a facts producer only. [`crate::environment::builder`] remains
//! the pure compiler that turns hand-fed declarations into
//! [`ProjectEnvironmentSnapshot`] receipts.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::dist_authoring::{plugin_generates_metadata, plugin_registry_identity, DistAuthoringFacts, DistAuthoringSource};
use crate::environment::{
    BuildSystemKind, EnvironmentInputAuthority, EnvironmentLimitation, IncludeEntryRole,
    ProjectRootRole,
};

/// Adapter surface version recorded alongside build-system facts (#2980).
pub const ADAPTER_VERSION: &str = "dist_zilla_roots.v1";

/// Authoring marker for Dist::Zilla.
pub const DIST_INI: &str = "dist.ini";

/// Canonical project source root.
pub const SOURCE_LIB: &str = "lib";

/// Explicit test-helper source root.
pub const TEST_LIB: &str = "t/lib";

/// Dist::Zilla staged build directory (not executed here).
pub const STAGED_BUILD: &str = ".build";

/// Generated module root (`dzil build` output).
pub const BLIB_LIB: &str = "blib/lib";

/// Generated architecture-specific module root.
pub const BLIB_ARCH: &str = "blib/arch";

/// Logical name for an explicitly configured Dist::Zilla driver candidate.
pub const DZIL_LOGICAL_NAME: &str = "dzil";

/// Rejection: PATH-only `dzil` must not become a default candidate.
pub const REASON_DZIL_NOT_CONFIGURED: &str = "dzil_not_explicitly_configured";

/// Rejection: configured `dzil` path is absent on disk.
pub const REASON_CONFIGURED_DZIL_MISSING: &str = "configured_dzil_missing";

/// Versioned reviewed plugin registry for environment effects (#2980).
pub const PLUGIN_REGISTRY_VERSION: &str = "dist_zilla_plugin_registry.v1";

/// Watcher / invalidation inputs aligned with #13640 metadata routes.
pub const WATCHER_INPUTS: &[&str] = &[
    DIST_INI,
    STAGED_BUILD,
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

/// Rejection: no `dist.ini` marker justifies generated roots.
pub const REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM: &str = "generated_root_without_build_system";

/// Rejection: path exists but is not a directory.
pub const REASON_NOT_A_DIRECTORY: &str = "not_a_directory";

/// Freshness of one root candidate relative to authoring and source inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistZillaRootFreshness {
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
pub enum DetectedDistZillaBuildSystem {
    /// No `dist.ini` marker.
    None,
    /// `dist.ini` present.
    DistZilla,
}

impl DetectedDistZillaBuildSystem {
    /// Map to the environment snapshot build-system enum when a family is known.
    #[must_use]
    pub const fn as_build_system_kind(self) -> Option<BuildSystemKind> {
        match self {
            Self::None => None,
            Self::DistZilla => Some(BuildSystemKind::DistZilla),
        }
    }
}

/// Disposition of one root candidate after bounded review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistZillaRootDisposition {
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
pub struct DistZillaRootCandidate {
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
    pub freshness: DistZillaRootFreshness,
    /// Selected or rejected with a typed reason.
    pub disposition: DistZillaRootDisposition,
    /// Producer-local stable order (`blib/arch` before `blib/lib`).
    pub source_order: u32,
    /// When true, this source root outranks a generated mirror of the same module.
    pub source_canonical_over_generated: bool,
}

/// Disposition of an explicitly configured `dzil` tool candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistZillaToolDisposition {
    /// No configured executable was supplied; PATH is not consulted.
    NotConfigured,
    /// Configured executable exists and may be recorded as a candidate only.
    Selected {
        /// Workspace-relative path when under the root, otherwise absolute.
        path: String,
    },
    /// Configured executable was supplied but rejected.
    Rejected {
        /// Stable machine reason.
        reason_code: &'static str,
        /// Human-readable detail without host-specific paths when possible.
        detail: String,
    },
}

/// Reviewed plugin registry note derived from authoring facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistZillaPluginRegistryNote {
    /// Plugin or bundle heading from `dist.ini`.
    pub plugin: String,
    /// Registry classification code.
    pub effect: &'static str,
    /// Human-readable detail for limitations or supported facts.
    pub detail: String,
}

/// Bounded Dist::Zilla detection for one workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistZillaDetection {
    /// `dist.ini` marker present.
    pub dist_ini_present: bool,
    /// Reviewed build-system classification.
    pub build_system: DetectedDistZillaBuildSystem,
    /// Ordered root candidates (selected and rejected).
    pub roots: Vec<DistZillaRootCandidate>,
    /// Explicit limitations (for example unreadable freshness metadata).
    pub limitations: Vec<EnvironmentLimitation>,
    /// Explicitly configured `dzil` candidate disposition (never PATH-derived).
    pub dzil_tool: DistZillaToolDisposition,
    /// Reviewed plugin registry notes from authoring facts, when supplied.
    pub plugin_notes: Vec<DistZillaPluginRegistryNote>,
}

/// Options for [`detect_dist_zilla`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistZillaDetectOptions<'a> {
    /// Explicit `dzil` executable path from project/user configuration only.
    pub configured_dzil: Option<&'a Path>,
}

/// Detect Dist::Zilla markers, roots, and optional configured tool candidates.
///
/// Bounded and non-executing: marker existence checks, directory metadata, and
/// a capped walk of `lib/` for freshness comparison only. No `dzil` invocation,
/// plugin execution, or subprocess is spawned.
#[must_use]
pub fn detect_dist_zilla(
    workspace_root: &Path,
    options: DistZillaDetectOptions<'_>,
) -> DistZillaDetection {
    detect_dist_zilla_with_plugins(workspace_root, options, &[])
}

/// Like [`detect_dist_zilla`] but projects reviewed plugin headings from authoring facts.
#[must_use]
pub fn detect_dist_zilla_with_plugins(
    workspace_root: &Path,
    options: DistZillaDetectOptions<'_>,
    plugin_headings: &[String],
) -> DistZillaDetection {
    let dist_ini_present = is_marker_file(&workspace_root.join(DIST_INI));
    let build_system = if dist_ini_present {
        DetectedDistZillaBuildSystem::DistZilla
    } else {
        DetectedDistZillaBuildSystem::None
    };

    let mut limitations = Vec::new();
    let freshness_anchors = collect_freshness_anchors(workspace_root, dist_ini_present);

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

    if matches!(build_system, DetectedDistZillaBuildSystem::None) {
        for (relative, role, order) in [
            (STAGED_BUILD, IncludeEntryRole::Other, 2_u32),
            (BLIB_ARCH, IncludeEntryRole::BlibArch, 3_u32),
            (BLIB_LIB, IncludeEntryRole::BlibLib, 4_u32),
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
            STAGED_BUILD,
            ProjectRootRole::Build,
            IncludeEntryRole::Other,
            2,
            &freshness_anchors,
            &mut limitations,
        ));
        roots.push(evaluate_generated_root(
            workspace_root,
            BLIB_ARCH,
            ProjectRootRole::Generated,
            IncludeEntryRole::BlibArch,
            3,
            &freshness_anchors,
            &mut limitations,
        ));
        roots.push(evaluate_generated_root(
            workspace_root,
            BLIB_LIB,
            ProjectRootRole::Generated,
            IncludeEntryRole::BlibLib,
            4,
            &freshness_anchors,
            &mut limitations,
        ));
    }

    let dzil_tool = evaluate_configured_dzil(workspace_root, options.configured_dzil);
    let plugin_notes =
        plugin_headings.iter().map(|heading| classify_plugin_heading(heading)).collect();

    DistZillaDetection {
        dist_ini_present,
        build_system,
        roots,
        limitations,
        dzil_tool,
        plugin_notes,
    }
}

struct FreshnessAnchors {
    authoring_latest: Option<SystemTime>,
    source_lib_latest: Option<SystemTime>,
}

fn collect_freshness_anchors(workspace_root: &Path, dist_ini_present: bool) -> FreshnessAnchors {
    let mut authoring_latest = None;
    if dist_ini_present {
        merge_latest(&mut authoring_latest, file_mtime(&workspace_root.join(DIST_INI)));
    }
    let source_lib_latest = directory_latest_mtime_bounded(&workspace_root.join(SOURCE_LIB), 4096);
    FreshnessAnchors { authoring_latest, source_lib_latest }
}

fn evaluate_configured_dzil(
    workspace_root: &Path,
    configured: Option<&Path>,
) -> DistZillaToolDisposition {
    let Some(configured) = configured else {
        return DistZillaToolDisposition::NotConfigured;
    };
    let path = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        workspace_root.join(configured)
    };
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => {
            DistZillaToolDisposition::Selected { path: path_to_report(workspace_root, &path) }
        }
        Ok(_) => DistZillaToolDisposition::Rejected {
            reason_code: REASON_CONFIGURED_DZIL_MISSING,
            detail: "configured dzil path exists but is not a regular file".to_string(),
        },
        Err(_) => DistZillaToolDisposition::Rejected {
            reason_code: REASON_CONFIGURED_DZIL_MISSING,
            detail: "configured dzil executable is absent".to_string(),
        },
    }
}

fn path_to_report(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Classify one plugin heading using the versioned registry.
#[must_use]
pub fn classify_plugin_heading(plugin: &str) -> DistZillaPluginRegistryNote {
    let normalized = plugin.trim();
    if normalized.starts_with('@') {
        return DistZillaPluginRegistryNote {
            plugin: normalized.to_string(),
            effect: "bundle_recorded_not_expanded",
            detail: format!("Dist::Zilla bundle `{normalized}` is recorded, not expanded"),
        };
    }
    if plugin_generates_metadata(normalized) {
        return DistZillaPluginRegistryNote {
            plugin: normalized.to_string(),
            effect: "plugin_generated_metadata",
            detail: format!(
                "Dist::Zilla plugin `{normalized}` can generate metadata and is not executed"
            ),
        };
    }
    let identity = plugin_registry_identity(normalized);
    if plugin_eq(identity, "MakeMaker") || identity.ends_with("::MakeMaker") {
        return DistZillaPluginRegistryNote {
            plugin: normalized.to_string(),
            effect: "declares_generated_eumm",
            detail: "plugin declares ExtUtils::MakeMaker as generated installer".to_string(),
        };
    }
    if plugin_eq(identity, "ModuleBuild")
        || plugin_eq(identity, "ModuleBuildTiny")
        || identity.contains("ModuleBuild")
    {
        return DistZillaPluginRegistryNote {
            plugin: normalized.to_string(),
            effect: "declares_generated_module_build",
            detail: "plugin declares Module::Build as generated installer".to_string(),
        };
    }
    if plugin_eq(identity, "Lib") || identity.ends_with("::Lib") {
        return DistZillaPluginRegistryNote {
            plugin: normalized.to_string(),
            effect: "declares_source_layout",
            detail: "Lib gatherer may declare additional lib paths; paths are not inferred here"
                .to_string(),
        };
    }
    DistZillaPluginRegistryNote {
        plugin: normalized.to_string(),
        effect: "plugin_identity_only",
        detail: "plugin identity recorded; behavior is not inferred from the name alone"
            .to_string(),
    }
}

fn plugin_eq(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
        || left.rsplit("::").next().is_some_and(|tail| tail.eq_ignore_ascii_case(right))
}

fn evaluate_source_root(
    workspace_root: &Path,
    relative: &str,
    project_role: ProjectRootRole,
    include_role: IncludeEntryRole,
    source_order: u32,
    source_canonical_over_generated: bool,
    _anchors: &FreshnessAnchors,
) -> DistZillaRootCandidate {
    let path = workspace_root.join(relative);
    match bounded_directory_state(workspace_root, &path) {
        DirectoryState::Missing => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: false,
            freshness: DistZillaRootFreshness::Missing,
            disposition: DistZillaRootDisposition::Rejected {
                reason_code: REASON_ROOT_MISSING,
                detail: format!("expected source root `{relative}` is absent"),
            },
            source_order,
            source_canonical_over_generated,
        },
        DirectoryState::EscapesWorkspace => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: false,
            freshness: DistZillaRootFreshness::Missing,
            disposition: DistZillaRootDisposition::Rejected {
                reason_code: REASON_ROOT_ESCAPES_WORKSPACE,
                detail: format!("source root `{relative}` resolves outside the workspace"),
            },
            source_order,
            source_canonical_over_generated,
        },
        DirectoryState::NotDirectory => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: false,
            freshness: DistZillaRootFreshness::Missing,
            disposition: DistZillaRootDisposition::Rejected {
                reason_code: REASON_NOT_A_DIRECTORY,
                detail: format!("path `{relative}` exists but is not a directory"),
            },
            source_order,
            source_canonical_over_generated,
        },
        DirectoryState::Present => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::WorkspaceConvention,
            exists: true,
            freshness: DistZillaRootFreshness::NotApplicable,
            disposition: DistZillaRootDisposition::Selected,
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
) -> DistZillaRootCandidate {
    let path = workspace_root.join(relative);
    let exists = matches!(bounded_directory_state(workspace_root, &path), DirectoryState::Present,);
    DistZillaRootCandidate {
        relative_path: relative.to_string(),
        project_role: ProjectRootRole::Generated,
        include_role,
        authority: EnvironmentInputAuthority::BuildMetadata,
        exists,
        freshness: DistZillaRootFreshness::NotApplicable,
        disposition: DistZillaRootDisposition::Rejected {
            reason_code: REASON_GENERATED_ROOT_WITHOUT_BUILD_SYSTEM,
            detail: format!(
                "generated root `{relative}` is not admitted without a dist.ini marker"
            ),
        },
        source_order,
        source_canonical_over_generated: false,
    }
}

fn evaluate_generated_root(
    workspace_root: &Path,
    relative: &str,
    project_role: ProjectRootRole,
    include_role: IncludeEntryRole,
    source_order: u32,
    anchors: &FreshnessAnchors,
    limitations: &mut Vec<EnvironmentLimitation>,
) -> DistZillaRootCandidate {
    let path = workspace_root.join(relative);
    match bounded_directory_state(workspace_root, &path) {
        DirectoryState::Missing => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::BuildMetadata,
            exists: false,
            freshness: DistZillaRootFreshness::Missing,
            disposition: DistZillaRootDisposition::Rejected {
                reason_code: REASON_ROOT_MISSING,
                detail: format!("generated root `{relative}` is absent"),
            },
            source_order,
            source_canonical_over_generated: false,
        },
        DirectoryState::EscapesWorkspace => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::BuildMetadata,
            exists: false,
            freshness: DistZillaRootFreshness::Missing,
            disposition: DistZillaRootDisposition::Rejected {
                reason_code: REASON_ROOT_ESCAPES_WORKSPACE,
                detail: format!("generated root `{relative}` resolves outside the workspace"),
            },
            source_order,
            source_canonical_over_generated: false,
        },
        DirectoryState::NotDirectory => DistZillaRootCandidate {
            relative_path: relative.to_string(),
            project_role,
            include_role,
            authority: EnvironmentInputAuthority::BuildMetadata,
            exists: false,
            freshness: DistZillaRootFreshness::Missing,
            disposition: DistZillaRootDisposition::Rejected {
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
                DistZillaRootFreshness::Current => DistZillaRootDisposition::Selected,
                DistZillaRootFreshness::Stale => DistZillaRootDisposition::Rejected {
                    reason_code: REASON_GENERATED_ROOT_STALE,
                    detail: format!(
                        "generated root `{relative}` is stale relative to authoring or source inputs"
                    ),
                },
                DistZillaRootFreshness::Missing | DistZillaRootFreshness::NotApplicable => {
                    DistZillaRootDisposition::Rejected {
                        reason_code: REASON_ROOT_MISSING,
                        detail: format!(
                            "generated root `{relative}` could not be classified as current"
                        ),
                    }
                }
            };
            DistZillaRootCandidate {
                relative_path: relative.to_string(),
                project_role,
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
) -> DistZillaRootFreshness {
    let Some(generated_mtime) = generated_mtime else {
        limitations.push(EnvironmentLimitation {
            code: "generated_root_mtime_unavailable".to_string(),
            detail:
                "could not read modification time for a generated root; treating freshness as stale"
                    .to_string(),
            input_id: None,
        });
        return DistZillaRootFreshness::Stale;
    };

    if anchors.authoring_latest.is_some_and(|anchor| generated_mtime < anchor) {
        return DistZillaRootFreshness::Stale;
    }
    if anchors.source_lib_latest.is_some_and(|anchor| generated_mtime < anchor) {
        return DistZillaRootFreshness::Stale;
    }
    DistZillaRootFreshness::Current
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

/// Project reviewed plugin headings from landed authoring facts (#7177).
#[must_use]
pub fn plugin_notes_from_authoring(facts: &DistAuthoringFacts) -> Vec<DistZillaPluginRegistryNote> {
    if facts.source != DistAuthoringSource::DistIni {
        return Vec::new();
    }
    facts.plugins.iter().map(|plugin| classify_plugin_heading(plugin)).collect()
}
