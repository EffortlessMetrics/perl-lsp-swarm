//! Serde model for the #11411 read-only vim-lsp subject refresh: the bounded
//! observation packet captured by the network observer (or a fixture), and
//! the deterministic review artifact the classifier emits.
//!
//! Boundedness law: every string field carries a hard cap or a closed
//! vocabulary enforced by [`crate::vim_lsp_subject_refresh::validate_packet`],
//! so no raw or unbounded upstream content can enter durable output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Schema identity of the observation packet consumed by the classifier.
pub const OBSERVATION_SCHEMA_VERSION: &str = "vim_lsp_subject_observation.v1";

/// Schema identity of the emitted review artifact.
pub const REFRESH_SCHEMA_VERSION: &str = "vim_lsp_subject_refresh.v1";

/// Drift classes preserved from #11411. Multiple classes may apply to one
/// observation; `instrument_failed` and `unknown_or_conflicting_authority`
/// always fail closed and suppress `no_change`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DriftClass {
    NoChange,
    MetadataOnlyNonSemantic,
    NewUpstreamReleaseOrRefAvailable,
    SelectedRefChangedMissingOrUnreachable,
    SelectedTreeDigestChanged,
    VimHostFloorOrRequiredFeatureChanged,
    PluginLoadOrInstallShapeChanged,
    RegistrationRootOrConfigApiChanged,
    ReadinessDiagnosticsOrLoggingSurfaceChanged,
    CompletionOrSnippetApplicationModelChanged,
    NavigationOrWorkspaceEditActionChanged,
    WorkspaceConfigurationBehaviorChanged,
    TextSyncOrDidChangeObservationSurfaceChanged,
    ServerRestartOrBufferLifecycleSurfaceChanged,
    WorkspaceFolderOrChangeNotificationsBehaviorChanged,
    SelectedPublicApiDeprecatedOrRemoved,
    MaintenanceStateChanged,
    UnknownOrConflictingAuthority,
    InstrumentFailed,
}

impl DriftClass {
    /// Stable token spelling used in artifacts and tests.
    pub fn token(self) -> &'static str {
        match self {
            DriftClass::NoChange => "no_change",
            DriftClass::MetadataOnlyNonSemantic => "metadata_only_non_semantic",
            DriftClass::NewUpstreamReleaseOrRefAvailable => "new_upstream_release_or_ref_available",
            DriftClass::SelectedRefChangedMissingOrUnreachable => {
                "selected_ref_changed_missing_or_unreachable"
            }
            DriftClass::SelectedTreeDigestChanged => "selected_tree_digest_changed",
            DriftClass::VimHostFloorOrRequiredFeatureChanged => {
                "vim_host_floor_or_required_feature_changed"
            }
            DriftClass::PluginLoadOrInstallShapeChanged => "plugin_load_or_install_shape_changed",
            DriftClass::RegistrationRootOrConfigApiChanged => {
                "registration_root_or_config_api_changed"
            }
            DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged => {
                "readiness_diagnostics_or_logging_surface_changed"
            }
            DriftClass::CompletionOrSnippetApplicationModelChanged => {
                "completion_or_snippet_application_model_changed"
            }
            DriftClass::NavigationOrWorkspaceEditActionChanged => {
                "navigation_or_workspace_edit_action_changed"
            }
            DriftClass::WorkspaceConfigurationBehaviorChanged => {
                "workspace_configuration_behavior_changed"
            }
            DriftClass::TextSyncOrDidChangeObservationSurfaceChanged => {
                "text_sync_or_did_change_observation_surface_changed"
            }
            DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged => {
                "server_restart_or_buffer_lifecycle_surface_changed"
            }
            DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged => {
                "workspace_folder_or_change_notifications_behavior_changed"
            }
            DriftClass::SelectedPublicApiDeprecatedOrRemoved => {
                "selected_public_api_deprecated_or_removed"
            }
            DriftClass::MaintenanceStateChanged => "maintenance_state_changed",
            DriftClass::UnknownOrConflictingAuthority => "unknown_or_conflicting_authority",
            DriftClass::InstrumentFailed => "instrument_failed",
        }
    }

    /// Every class, in declaration order, for exhaustive-table tests.
    pub const ALL: &'static [DriftClass] = &[
        DriftClass::NoChange,
        DriftClass::MetadataOnlyNonSemantic,
        DriftClass::NewUpstreamReleaseOrRefAvailable,
        DriftClass::SelectedRefChangedMissingOrUnreachable,
        DriftClass::SelectedTreeDigestChanged,
        DriftClass::VimHostFloorOrRequiredFeatureChanged,
        DriftClass::PluginLoadOrInstallShapeChanged,
        DriftClass::RegistrationRootOrConfigApiChanged,
        DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
        DriftClass::CompletionOrSnippetApplicationModelChanged,
        DriftClass::NavigationOrWorkspaceEditActionChanged,
        DriftClass::WorkspaceConfigurationBehaviorChanged,
        DriftClass::TextSyncOrDidChangeObservationSurfaceChanged,
        DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged,
        DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged,
        DriftClass::SelectedPublicApiDeprecatedOrRemoved,
        DriftClass::MaintenanceStateChanged,
        DriftClass::UnknownOrConflictingAuthority,
        DriftClass::InstrumentFailed,
    ];
}

/// Transport status of one probe. `Failed` means the instrument could not
/// observe; it never means "no drift".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Ok,
    Failed,
}

/// Evidence recorded for every probe: the exact method (git command or URL)
/// and its bounded result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeEvidence {
    pub id: String,
    pub method: String,
    pub status: ProbeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `git ls-remote` observation of the upstream refscape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefsProbe {
    pub method: String,
    pub status: ProbeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One observed file identity inside a fetched tree. `commit` records the
/// commit the bytes were read from; classification rejects packets whose
/// file bytes are attributed to any commit other than the probe's head, so
/// another ref's facts cannot be applied to the selected subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedFile {
    pub commit: String,
    pub path: String,
    pub present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_blob_sha1: Option<String>,
}

/// Depth-1 fetch observation of the pinned `selected_commit` itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedCommitProbe {
    pub method: String,
    pub status: ProbeStatus,
    pub requested_commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_tree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_author_date: Option<String>,
    #[serde(default)]
    pub entry_file_blobs: Vec<ObservedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The theoretical editor floor sentence parsed out of the observed
/// `doc/vim-lsp.txt`. `parsed == false` while the file exists means the
/// recorded authority could not be re-read: unknown, never "no change".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloorObservation {
    pub parsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neovim_minimum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vim_minimum: Option<String>,
}

/// One needle lookup inside the observed upstream tree, keyed by the
/// inventory surface that cites it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceFinding {
    pub surface: String,
    pub file: String,
    pub needle: String,
    pub found: bool,
}

/// Observation of the current upstream head tree: bounded file identities,
/// the parsed floor sentence, plugin global defaults, maintenance markers,
/// the recorded capability note, and every public-surface needle result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadTreeProbe {
    pub method: String,
    pub status: ProbeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub files: Vec<ObservedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<FloorObservation>,
    #[serde(default)]
    pub plugin_defaults: BTreeMap<String, String>,
    /// Whether the plugin once-guard (`g:lsp_loaded`) still exists in the
    /// observed `plugin/lsp.vim`. `None` when the entry file is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_guard_present: Option<bool>,
    #[serde(default)]
    pub maintenance_markers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet_note_present: Option<bool>,
    #[serde(default)]
    pub surface_findings: Vec<SurfaceFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The bounded, transport-only observation packet. Carries digests, ref
/// names, and needle booleans — never file content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationPacket {
    pub schema_version: String,
    pub observed_at_utc: String,
    pub upstream_repository: String,
    pub refs_probe: RefsProbe,
    pub pinned_commit_probe: PinnedCommitProbe,
    pub head_tree_probe: HeadTreeProbe,
}

// ---------------------------------------------------------------------------
// Artifact model
// ---------------------------------------------------------------------------

/// Pinned-subject identity carried into the artifact as the "before" side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSubjectBefore {
    pub manifest_path: String,
    pub repository: String,
    pub selected_commit: String,
    pub tree_digest: String,
    pub latest_release_tag: String,
    pub vim_theoretical_minimum: String,
    pub neovim_theoretical_minimum: String,
    pub entry_files: Vec<EntryFileBefore>,
}

/// One pinned entry-file identity from the subject manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFileBefore {
    pub path: String,
    pub git_blob_sha1: String,
}

/// Release/currentness comparison, every field carrying its probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseObservation {
    pub pinned_recorded_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_observed_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer_release_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_matches_pin: Option<bool>,
    pub probe_id: String,
}

/// Prerequisite comparison. Observed floors are upstream theoretical
/// metadata only; they never become a tested support floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrerequisiteObservation {
    pub vim_before: String,
    pub neovim_before: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vim_observed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neovim_observed: Option<String>,
    pub probe_id: String,
}

/// One public-surface needle comparison row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceObservationRow {
    pub surface: String,
    pub file: String,
    pub needle: String,
    pub found_observed: bool,
    pub drift_class_if_absent: DriftClass,
}

/// A drift classification with its evidence: which probe produced it and
/// a bounded detail line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedDrift {
    pub class: DriftClass,
    pub evidence_probe_ids: Vec<String>,
    pub detail: String,
}

/// An explicit positive finding: this probe was checked and observed no
/// drift. Zero drift is reported as these rows, never as absent output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositiveFinding {
    pub probe_id: String,
    pub check: String,
    pub result: String,
}

/// One deterministic impact row: which authority or evidence family the
/// class touches, and with which bounded action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactEntry {
    pub item: String,
    pub action: String,
    pub reason: String,
    pub triggered_by: DriftClass,
}

/// A bounded proposed change to a #11369 manifest field. Proposal only:
/// the tool never applies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedSubjectFieldChange {
    pub field: String,
    pub current: String,
    pub observed: String,
}

/// The bounded deterministic review artifact (#11411 output packet).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefreshArtifact {
    pub schema_version: String,
    pub observed_at_utc: String,
    pub advisory_boundary: Vec<String>,
    pub selected_subject_before: SelectedSubjectBefore,
    pub probes: Vec<ProbeEvidence>,
    pub release_observation: ReleaseObservation,
    pub prerequisite_observation: PrerequisiteObservation,
    pub public_surface_observation: Vec<SurfaceObservationRow>,
    pub drift_classes: Vec<ClassifiedDrift>,
    pub positive_findings: Vec<PositiveFinding>,
    pub impacted_evidence: Vec<ImpactEntry>,
    pub recommended_disposition: Vec<String>,
    pub proposed_subject_field_changes: Vec<ProposedSubjectFieldChange>,
    pub required_evidence_refresh_set: Vec<String>,
}
