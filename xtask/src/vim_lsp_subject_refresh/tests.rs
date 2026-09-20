//! Discriminating fixtures for the #11411 drift classification.
//!
//! Positive proof: one fixture per drift class — each class fires on a
//! packet carrying exactly its signature and nothing else — plus the
//! all-green packet yielding an explicit `no_change` with positive
//! findings, and the upstream-moved-but-semantically-identical packet
//! yielding `metadata_only_non_semantic` without invalidating anything.
//!
//! Negative controls (the #11411 list): floating master never replaces the
//! pin; a new release never promotes anything; another ref's facts cannot
//! be attributed to the observed subject; a theoretical floor change never
//! invalidates directly tested rows; metadata-only drift never invalidates
//! host evidence; instrument failure is never reported as no drift;
//! conflicting probes fail closed; boundedness caps reject oversized
//! content; the proposal writer refuses the authority tree and leaves the
//! landed pin byte-identical.

use std::collections::BTreeMap;

use crate::vim_lsp_subject_refresh::classify::{PinnedSubject, classify};
use crate::vim_lsp_subject_refresh::model::{
    DriftClass, FloorObservation, HeadTreeProbe, ObservationPacket, ObservedFile,
    PinnedCommitProbe, ProbeStatus, RefreshArtifact, RefsProbe, SurfaceFinding,
};
use crate::vim_lsp_subject_refresh::probe_table::{
    EXPECTED_PLUGIN_DEFAULTS, FILE_DOC, FILE_README, SURFACE_PROBES,
};

const PINNED_COMMIT: &str = "e10d186452743beb7b43d2b3427020832f930c2b";
const PINNED_TREE: &str = "dd24cb8e10096c82766143c9fd058105637d72dc";
const MOVED_MASTER: &str = "f11e297563854cfc8c54e3c458113c924a041d3c";
const PINNED_TAG: &str = "v0.1.4";

fn entry_files() -> Vec<(String, String)> {
    [
        ("plugin/lsp.vim", "897466bcffae3f8e9c1c039f50aa927edad05f65"),
        ("autoload/lsp.vim", "466080ee3a3be84ace86b5c46bdff3665d2ced6d"),
        ("autoload/lsp/capabilities.vim", "119e6f0ee4d6bb38b168c8ced333b6f27c5cdc64"),
        ("autoload/lsp/utils.vim", "eb6d5baaa1b0780099e92aa445e6a2573cfdf97f"),
        ("autoload/lsp/utils/text_edit.vim", "86558b6ac191cfbcf43e157ad068e0243bb3cf70"),
        ("autoload/lsp/utils/workspace_config.vim", "0304f29fb02cdcb3d9605413c72cabd5ff84b66f"),
        ("autoload/lsp/utils/workspace_edit.vim", "5dafd26ca2079179410b341e6a6e15651936370b"),
        ("autoload/lsp/omni.vim", "931b3459476eb03eee7cbfda868141c774cab983"),
    ]
    .into_iter()
    .map(|(path, blob)| (path.to_string(), blob.to_string()))
    .collect()
}

fn pinned() -> PinnedSubject {
    PinnedSubject {
        repository: "https://github.com/prabirshrestha/vim-lsp".to_string(),
        selected_commit: PINNED_COMMIT.to_string(),
        tree_digest: PINNED_TREE.to_string(),
        latest_release_tag: PINNED_TAG.to_string(),
        vim_theoretical_minimum: "8.1.1035".to_string(),
        neovim_theoretical_minimum: "0.3".to_string(),
        entry_files: entry_files(),
    }
}

fn probed_paths() -> Vec<String> {
    let mut paths: Vec<String> =
        SURFACE_PROBES.iter().map(|probe| probe.file.to_string()).collect();
    paths.extend(entry_files().into_iter().map(|(path, _)| path));
    paths.push(FILE_DOC.to_string());
    paths.push(FILE_README.to_string());
    paths.sort();
    paths.dedup();
    paths
}

/// The all-green observation packet: upstream world identical to the pin.
fn unchanged_packet() -> ObservationPacket {
    let commit = PINNED_COMMIT.to_string();
    let refs_probe = RefsProbe {
        method: "git ls-remote https://github.com/prabirshrestha/vim-lsp.git HEAD refs/heads/master refs/tags/*".to_string(),
        status: ProbeStatus::Ok,
        head: Some(PINNED_COMMIT.to_string()),
        master: Some(PINNED_COMMIT.to_string()),
        tags: vec![PINNED_TAG.to_string()],
        error: None,
    };
    let pinned_commit_probe = PinnedCommitProbe {
        method: format!("git fetch --depth 1 <url> {PINNED_COMMIT}"),
        status: ProbeStatus::Ok,
        requested_commit: PINNED_COMMIT.to_string(),
        resolved_commit: Some(PINNED_COMMIT.to_string()),
        resolved_tree: Some(PINNED_TREE.to_string()),
        commit_subject: Some("Fix respond to client/registerCapability".to_string()),
        commit_author_date: Some("2026-08-10T16:38:59+09:00".to_string()),
        entry_file_blobs: entry_files()
            .into_iter()
            .map(|(path, blob)| ObservedFile {
                commit: PINNED_COMMIT.to_string(),
                path,
                present: true,
                git_blob_sha1: Some(blob),
            })
            .collect(),
        error: None,
    };
    let head_tree_probe = HeadTreeProbe {
        method: "git fetch --depth 1 <url> <observed-master>".to_string(),
        status: ProbeStatus::Ok,
        commit: Some(PINNED_COMMIT.to_string()),
        files: probed_paths()
            .into_iter()
            .map(|path| {
                let blob = entry_files()
                    .into_iter()
                    .find(|(entry, _)| *entry == path)
                    .map(|(_, blob)| blob)
                    .unwrap_or_else(|| "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
                ObservedFile {
                    commit: commit.clone(),
                    path,
                    present: true,
                    git_blob_sha1: Some(blob),
                }
            })
            .collect(),
        floor: Some(FloorObservation {
            parsed: true,
            neovim_minimum: Some("0.3".to_string()),
            vim_minimum: Some("8.1.1035".to_string()),
        }),
        plugin_defaults: EXPECTED_PLUGIN_DEFAULTS
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect::<BTreeMap<_, _>>(),
        load_guard_present: Some(true),
        maintenance_markers: Vec::new(),
        snippet_note_present: Some(true),
        surface_findings: SURFACE_PROBES
            .iter()
            .map(|probe| SurfaceFinding {
                surface: probe.surface.to_string(),
                file: probe.file.to_string(),
                needle: probe.needle.to_string(),
                found: true,
            })
            .collect(),
        error: None,
    };
    ObservationPacket {
        schema_version: "vim_lsp_subject_observation.v1".to_string(),
        observed_at_utc: "2026-08-24T12:00:00+00:00".to_string(),
        upstream_repository: "https://github.com/prabirshrestha/vim-lsp".to_string(),
        refs_probe,
        pinned_commit_probe,
        head_tree_probe,
    }
}

fn run(packet: &ObservationPacket) -> RefreshArtifact {
    classify(packet, &pinned()).expect("classification of a valid packet succeeds")
}

fn classes(artifact: &RefreshArtifact) -> Vec<DriftClass> {
    artifact.drift_classes.iter().map(|entry| entry.class).collect()
}

fn assert_fires_only(mutated: ObservationPacket, expected: DriftClass) -> RefreshArtifact {
    let artifact = run(&mutated);
    let fired = classes(&artifact);
    assert!(
        fired.contains(&expected),
        "expected {expected:?} to fire; got {fired:?} — detail: {:?}",
        artifact
            .drift_classes
            .iter()
            .map(|entry| (entry.class, entry.detail.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        !fired.contains(&DriftClass::NoChange),
        "a drift signature must suppress no_change; got {fired:?}"
    );
    artifact
}

/// Break one surface-needle finding by (surface, needle).
fn break_needle(packet: &mut ObservationPacket, surface: &str, needle: &str) {
    for finding in &mut packet.head_tree_probe.surface_findings {
        if finding.surface == surface && finding.needle == needle {
            finding.found = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Positive: zero drift is an explicit finding, never absent output
// ---------------------------------------------------------------------------

#[test]
fn all_green_packet_classifies_no_change_with_explicit_positive_findings() {
    let artifact = run(&unchanged_packet());
    assert_eq!(classes(&artifact), vec![DriftClass::NoChange]);
    assert!(
        !artifact.positive_findings.is_empty(),
        "zero drift must be carried by explicit positive findings"
    );
    assert!(artifact.positive_findings.iter().all(|finding| finding.result == "checked_no_drift"));
    // Every class entry and every finding carries evidence.
    for entry in &artifact.drift_classes {
        assert!(
            !entry.evidence_probe_ids.is_empty(),
            "class {} lacks evidence",
            entry.class.token()
        );
    }
    for finding in &artifact.positive_findings {
        assert!(!finding.check.is_empty());
    }
    // Narrow impact: no invalidation, retain-pin disposition.
    assert_eq!(artifact.impacted_evidence.len(), 1);
    assert_eq!(artifact.impacted_evidence[0].action, "no_invalidation");
    assert_eq!(artifact.recommended_disposition, vec!["retain_pin"]);
    assert!(artifact.proposed_subject_field_changes.is_empty());
    // Artifact serializes within boundedness rules.
    crate::vim_lsp_subject_refresh::validate_artifact_boundedness(&artifact)
        .expect("all-green artifact is bounded");
}

#[test]
fn upstream_moved_with_identical_semantics_is_metadata_only_and_invalidates_nothing() {
    let mut packet = unchanged_packet();
    packet.refs_probe.master = Some(MOVED_MASTER.to_string());
    packet.refs_probe.head = Some(MOVED_MASTER.to_string());
    packet.head_tree_probe.commit = Some(MOVED_MASTER.to_string());
    for file in &mut packet.head_tree_probe.files {
        file.commit = MOVED_MASTER.to_string();
    }
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert!(fired.contains(&DriftClass::MetadataOnlyNonSemantic), "got {fired:?}");
    assert!(fired.contains(&DriftClass::NewUpstreamReleaseOrRefAvailable), "got {fired:?}");
    assert!(!fired.contains(&DriftClass::NoChange));
    for impact in &artifact.impacted_evidence {
        assert_ne!(
            impact.action, "invalidate_and_rerun",
            "metadata-only drift must not invalidate host evidence"
        );
    }
    assert!(fired.contains(&DriftClass::NewUpstreamReleaseOrRefAvailable));
}

// ---------------------------------------------------------------------------
// One discriminating fixture per drift class
// ---------------------------------------------------------------------------

#[test]
fn newer_release_tag_classifies_new_upstream_release_and_only_recommends() {
    let mut packet = unchanged_packet();
    packet.refs_probe.tags = vec![PINNED_TAG.to_string(), "v0.2.0".to_string()];
    let artifact = assert_fires_only(packet, DriftClass::NewUpstreamReleaseOrRefAvailable);
    // Negative control: availability is a recommendation, never promotion.
    assert!(artifact.recommended_disposition.contains(&"open_reviewed_pin_update".to_string()));
    assert!(
        !artifact.recommended_disposition.contains(&"refresh_subject_authority_11369".to_string())
    );
    // The proposal carries the field change review-only.
    assert!(
        artifact
            .proposed_subject_field_changes
            .iter()
            .any(|change| change.field == "upstream.latest_release_tag_observation.tag"
                && change.observed == "v0.2.0")
    );
}

#[test]
fn unfetchable_pinned_commit_classifies_selected_ref_missing_or_unreachable() {
    let mut packet = unchanged_packet();
    packet.pinned_commit_probe.status = ProbeStatus::Failed;
    packet.pinned_commit_probe.resolved_commit = None;
    packet.pinned_commit_probe.resolved_tree = None;
    packet.pinned_commit_probe.entry_file_blobs.clear();
    packet.pinned_commit_probe.error = Some("fetch failed".to_string());
    let artifact = assert_fires_only(packet, DriftClass::SelectedRefChangedMissingOrUnreachable);
    // Old receipts may not silently remain current.
    assert!(
        artifact.impacted_evidence.iter().any(
            |impact| impact.action == "cannot_assume_current" && impact.item.contains("#10962")
        )
    );
}

#[test]
fn unreachable_pinned_packet_validates_and_classifies_without_instrument_error() {
    // A fetchable repo whose pinned commit will not fetch is a real drift
    // class, not an instrument failure: the packet must validate.
    let mut packet = unchanged_packet();
    packet.pinned_commit_probe.status = ProbeStatus::Failed;
    packet.pinned_commit_probe.resolved_commit = None;
    packet.pinned_commit_probe.resolved_tree = None;
    packet.pinned_commit_probe.entry_file_blobs.clear();
    packet.pinned_commit_probe.error = Some("fetch failed".to_string());
    crate::vim_lsp_subject_refresh::validate_packet(&packet, &pinned())
        .expect("unreachable-ref packet must validate");
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert_eq!(fired, vec![DriftClass::SelectedRefChangedMissingOrUnreachable], "got {fired:?}");
    assert!(!fired.contains(&DriftClass::InstrumentFailed));
}

#[test]
fn resolved_tree_mismatch_classifies_selected_tree_digest_changed() {
    let mut packet = unchanged_packet();
    packet.pinned_commit_probe.resolved_tree =
        Some("0000000000000000000000000000000000000001".to_string());
    let artifact = assert_fires_only(packet, DriftClass::SelectedTreeDigestChanged);
    assert!(artifact.drift_classes.iter().any(|entry| entry.detail.contains("tree")));
}

#[test]
fn entry_blob_mismatch_classifies_selected_tree_digest_changed() {
    let mut packet = unchanged_packet();
    packet.pinned_commit_probe.entry_file_blobs[0].git_blob_sha1 =
        Some("0000000000000000000000000000000000000002".to_string());
    assert_fires_only(packet, DriftClass::SelectedTreeDigestChanged);
}

#[test]
fn floor_change_classifies_vim_host_floor_changed_without_invalidating_tested_rows() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.floor = Some(FloorObservation {
        parsed: true,
        neovim_minimum: Some("0.3".to_string()),
        vim_minimum: Some("9.0.0001".to_string()),
    });
    let artifact = assert_fires_only(packet, DriftClass::VimHostFloorOrRequiredFeatureChanged);
    // Negative control: a theoretical floor change opens review only; it
    // never invalidates directly tested #10966 rows.
    for impact in &artifact.impacted_evidence {
        if impact.item.contains("#10966") {
            assert_eq!(
                impact.action, "review_disposition",
                "theoretical floor drift must stay a review disposition"
            );
        }
    }
    assert!(
        !artifact.impacted_evidence.iter().any(|impact| impact.action == "invalidate_and_rerun")
    );
}

#[test]
fn plugin_default_change_classifies_required_feature_changed() {
    let mut packet = unchanged_packet();
    packet
        .head_tree_probe
        .plugin_defaults
        .insert("g:lsp_use_lua".to_string(), "has('nvim-0.10.0')".to_string());
    assert_fires_only(packet, DriftClass::VimHostFloorOrRequiredFeatureChanged);
}

#[test]
fn missing_plugin_entry_classifies_plugin_load_shape_changed() {
    let mut packet = unchanged_packet();
    for file in &mut packet.head_tree_probe.files {
        if file.path == "plugin/lsp.vim" {
            file.present = false;
            file.git_blob_sha1 = None;
        }
    }
    packet.head_tree_probe.load_guard_present = None;
    assert_fires_only(packet, DriftClass::PluginLoadOrInstallShapeChanged);
}

#[test]
fn missing_load_guard_classifies_plugin_load_shape_changed() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.load_guard_present = Some(false);
    assert_fires_only(packet, DriftClass::PluginLoadOrInstallShapeChanged);
}

#[test]
fn missing_registration_api_classifies_registration_root_config_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "server registration and root callback",
        "function! lsp#register_server(",
    );
    let artifact = assert_fires_only(packet, DriftClass::RegistrationRootOrConfigApiChanged);
    assert!(classes(&artifact).contains(&DriftClass::SelectedPublicApiDeprecatedOrRemoved));
}

#[test]
fn missing_diagnostics_surface_classifies_readiness_diagnostics_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "client diagnostics state or event",
        "function! lsp#get_buffer_diagnostics_counts(",
    );
    let artifact =
        assert_fires_only(packet, DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged);
    assert!(classes(&artifact).contains(&DriftClass::SelectedPublicApiDeprecatedOrRemoved));
    assert!(artifact
        .impacted_evidence
        .iter()
        .any(|impact| impact.action == "invalidate_and_rerun" && impact.item.contains("#10962")));
}

#[test]
fn missing_completion_conversion_classifies_completion_model_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "completion conversion/application",
        "function! lsp#omni#get_vim_completion_items(",
    );
    let artifact =
        assert_fires_only(packet, DriftClass::CompletionOrSnippetApplicationModelChanged);
    assert!(classes(&artifact).contains(&DriftClass::SelectedPublicApiDeprecatedOrRemoved));
}

#[test]
fn missing_request_channel_classifies_navigation_action_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "generic request channel (hover/definition/references/rename/formatting/completion results)",
        "function! lsp#send_request(",
    );
    assert_fires_only(packet, DriftClass::NavigationOrWorkspaceEditActionChanged);
}

#[test]
fn missing_workspace_config_api_classifies_workspace_configuration_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "workspace configuration refresh",
        "function! lsp#update_workspace_config(",
    );
    assert_fires_only(packet, DriftClass::WorkspaceConfigurationBehaviorChanged);
}

#[test]
fn missing_did_change_handler_classifies_text_sync_surface_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "didChange observation/instrumentation seam",
        "on_text_document_did_change",
    );
    let artifact =
        assert_fires_only(packet, DriftClass::TextSyncOrDidChangeObservationSurfaceChanged);
    // Narrow impact: the text-sync class itself touches only wire receipts.
    for impact in &artifact.impacted_evidence {
        if impact.triggered_by == DriftClass::TextSyncOrDidChangeObservationSurfaceChanged {
            assert!(
                impact.item.contains("#11408"),
                "text-sync impact must stay on wire receipts: {}",
                impact.item
            );
        }
    }
}

#[test]
fn missing_stop_server_classifies_lifecycle_surface_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "server stop/restart and log/status inspection",
        "function! lsp#stop_server(",
    );
    let artifact =
        assert_fires_only(packet, DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged);
    // Narrow impact: recovery/lifecycle families only.
    assert!(artifact
        .impacted_evidence
        .iter()
        .any(|impact| impact.item.contains("#11386") && impact.action == "invalidate_and_rerun"));
}

#[test]
fn missing_workspace_folder_flag_classifies_workspace_folder_behavior_changed() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "experimental workspace folders",
        "let g:lsp_experimental_workspace_folders =",
    );
    let artifact =
        assert_fires_only(packet, DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged);
    // Narrow impact graph: #10960/#11405 only, not bounded core.
    let invalidated: Vec<&str> = artifact
        .impacted_evidence
        .iter()
        .filter(|impact| impact.action == "invalidate_and_rerun")
        .map(|impact| impact.item.as_str())
        .collect();
    assert_eq!(invalidated, vec!["#10960/#11405 workspace-folder cells"]);
}

#[test]
fn maintenance_marker_classifies_maintenance_state_changed() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.maintenance_markers = vec!["maintenance mode".to_string()];
    let artifact = assert_fires_only(packet, DriftClass::MaintenanceStateChanged);
    assert!(artifact.recommended_disposition.contains(&"refresh_upstream_packet_7712".to_string()));
}

// ---------------------------------------------------------------------------
// Fail-closed classes
// ---------------------------------------------------------------------------

#[test]
fn refs_probe_failure_classifies_instrument_failed_never_no_change() {
    let mut packet = unchanged_packet();
    packet.refs_probe.status = ProbeStatus::Failed;
    packet.refs_probe.head = None;
    packet.refs_probe.master = None;
    packet.refs_probe.tags.clear();
    packet.refs_probe.error = Some("network unreachable".to_string());
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert_eq!(fired, vec![DriftClass::InstrumentFailed], "got {fired:?}");
    for finding in &artifact.positive_findings {
        assert_ne!(
            finding.probe_id, "refs",
            "a failed probe must not report a checked_no_drift finding"
        );
    }
    assert!(artifact.recommended_disposition.contains(&"retain_pin".to_string()));
    assert!(artifact.recommended_disposition.contains(&"retry_observation".to_string()));
}

#[test]
fn head_probe_failure_classifies_instrument_failed_but_keeps_refs_derived_facts() {
    let mut packet = unchanged_packet();
    packet.refs_probe.master = Some(MOVED_MASTER.to_string());
    packet.head_tree_probe.status = ProbeStatus::Failed;
    packet.head_tree_probe.commit = None;
    packet.head_tree_probe.files.clear();
    packet.head_tree_probe.floor = None;
    packet.head_tree_probe.surface_findings.clear();
    packet.head_tree_probe.error = Some("rate limited".to_string());
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert!(fired.contains(&DriftClass::InstrumentFailed), "got {fired:?}");
    assert!(
        fired.contains(&DriftClass::NewUpstreamReleaseOrRefAvailable),
        "refs-derived facts stay claimable; got {fired:?}"
    );
    assert!(!fired.contains(&DriftClass::NoChange));
}

#[test]
fn unparseable_floor_classifies_unknown_authority() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.floor =
        Some(FloorObservation { parsed: false, neovim_minimum: None, vim_minimum: None });
    let artifact = assert_fires_only(packet, DriftClass::UnknownOrConflictingAuthority);
    assert!(
        artifact.impacted_evidence.iter().any(|impact| impact.action == "cannot_assume_current")
    );
}

#[test]
fn vanished_snippet_note_classifies_unknown_authority() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.snippet_note_present = Some(false);
    assert_fires_only(packet, DriftClass::UnknownOrConflictingAuthority);
}

#[test]
fn head_tree_read_from_another_commit_classifies_conflicting_authority() {
    let mut packet = unchanged_packet();
    // The refs probe says master is the pinned commit, but the head tree
    // claims to have been read from something else.
    packet.head_tree_probe.commit = Some(MOVED_MASTER.to_string());
    for file in &mut packet.head_tree_probe.files {
        file.commit = MOVED_MASTER.to_string();
    }
    let artifact = assert_fires_only(packet, DriftClass::UnknownOrConflictingAuthority);
    assert!(artifact.drift_classes.iter().any(|entry| entry.detail.contains("one ref's facts")));
}

// ---------------------------------------------------------------------------
// Impact-graph and boundedness totality
// ---------------------------------------------------------------------------

#[test]
fn failed_head_packet_validates_and_classifies_instrument_failed() {
    // A live head-fetch failure must reach the classifier (which reports
    // instrument_failed), not die in packet validation on empty findings.
    let mut packet = unchanged_packet();
    packet.refs_probe.master = Some(MOVED_MASTER.to_string());
    packet.refs_probe.head = Some(MOVED_MASTER.to_string());
    packet.head_tree_probe.status = ProbeStatus::Failed;
    packet.head_tree_probe.commit = None;
    packet.head_tree_probe.files.clear();
    packet.head_tree_probe.floor = None;
    packet.head_tree_probe.plugin_defaults.clear();
    packet.head_tree_probe.load_guard_present = None;
    packet.head_tree_probe.snippet_note_present = None;
    packet.head_tree_probe.surface_findings.clear();
    packet.head_tree_probe.error = Some("head fetch failed".to_string());
    crate::vim_lsp_subject_refresh::validate_packet(&packet, &pinned())
        .expect("a failed head probe must validate so it can classify instrument_failed");
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert!(fired.contains(&DriftClass::InstrumentFailed), "got {fired:?}");
    assert!(fired.contains(&DriftClass::NewUpstreamReleaseOrRefAvailable), "got {fired:?}");
    assert!(!fired.contains(&DriftClass::NoChange));
}

#[test]
fn moved_default_branch_with_pinned_master_classifies_new_upstream_ref() {
    // HEAD moved to a new default branch while refs/heads/master stays at
    // the pin: upstream drift must be visible, not reported as no_change.
    let mut packet = unchanged_packet();
    packet.refs_probe.head = Some(MOVED_MASTER.to_string());
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert!(
        fired.contains(&DriftClass::NewUpstreamReleaseOrRefAvailable),
        "a HEAD/master split must classify ref drift; got {fired:?}"
    );
    assert!(!fired.contains(&DriftClass::NoChange));
    assert!(
        artifact.drift_classes.iter().any(|entry| entry.detail.contains("default branch moved"))
    );
}

#[test]
fn masterless_refs_observation_is_valid_and_tracks_head() {
    // Default branch renamed away: refs/heads/master is gone, HEAD remains.
    let mut packet = unchanged_packet();
    packet.refs_probe.master = None;
    crate::vim_lsp_subject_refresh::validate_packet(&packet, &pinned())
        .expect("a masterless refs observation must validate");
    let artifact = run(&packet);
    assert_eq!(classes(&artifact), vec![DriftClass::NoChange]);
    assert_eq!(artifact.release_observation.master_matches_pin, Some(true));
}

#[test]
fn multiple_missing_needles_of_one_surface_coalesce_into_one_class_entry() {
    let mut packet = unchanged_packet();
    break_needle(
        &mut packet,
        "server registration and root callback",
        "function! lsp#register_server(",
    );
    break_needle(
        &mut packet,
        "server registration and root callback",
        "function! lsp#utils#path_to_uri(",
    );
    break_needle(&mut packet, "server registration and root callback", "'root_uri'");
    let artifact = run(&packet);
    let registration_entries: Vec<_> = artifact
        .drift_classes
        .iter()
        .filter(|entry| entry.class == DriftClass::RegistrationRootOrConfigApiChanged)
        .collect();
    assert_eq!(
        registration_entries.len(),
        1,
        "per-needle firings must coalesce into one class entry with joined evidence"
    );
    let entry = registration_entries[0];
    assert!(entry.detail.contains("lsp#register_server"), "joined detail: {}", entry.detail);
    assert!(entry.detail.contains("path_to_uri"), "joined detail: {}", entry.detail);
    // Many simultaneous missing needles still satisfy the class-count cap.
    for finding in &mut packet.head_tree_probe.surface_findings {
        finding.found = false;
    }
    let artifact = run(&packet);
    crate::vim_lsp_subject_refresh::validate_artifact_boundedness(&artifact)
        .expect("all-needles-missing artifact stays bounded and unique per class");
    assert!(artifact.drift_classes.len() <= DriftClass::ALL.len());
}

#[test]
fn unproven_byte_drift_on_a_moved_head_is_not_metadata_only() {
    // Upstream moved and one consumed entry file's bytes differ beyond the
    // probes: that is reviewable drift, never "metadata only".
    let mut packet = unchanged_packet();
    packet.refs_probe.master = Some(MOVED_MASTER.to_string());
    packet.refs_probe.head = Some(MOVED_MASTER.to_string());
    packet.head_tree_probe.commit = Some(MOVED_MASTER.to_string());
    for file in &mut packet.head_tree_probe.files {
        file.commit = MOVED_MASTER.to_string();
        if file.path == "plugin/lsp.vim" {
            file.git_blob_sha1 = Some("cccccccccccccccccccccccccccccccccccccccc".to_string());
        }
    }
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert!(fired.contains(&DriftClass::NewUpstreamReleaseOrRefAvailable), "got {fired:?}");
    assert!(
        !fired.contains(&DriftClass::MetadataOnlyNonSemantic),
        "byte drift beyond the probes must not ride along as metadata-only; got {fired:?}"
    );
    assert!(!fired.contains(&DriftClass::NoChange));
    assert!(artifact.drift_classes.iter().any(|entry| entry.detail.contains("unproven")));
    // Recommendation stays review-only: the pin still binds its own bytes.
    assert!(artifact.recommended_disposition.contains(&"open_reviewed_pin_update".to_string()));
}

#[test]
fn contradicting_head_bytes_under_the_pin_fail_closed() {
    // refs say the tracked ref is the pin, yet the head probe reports
    // different entry bytes: conflicting observations, never no_change.
    let mut packet = unchanged_packet();
    for file in &mut packet.head_tree_probe.files {
        if file.path == "autoload/lsp.vim" {
            file.git_blob_sha1 = Some("dddddddddddddddddddddddddddddddddddddddd".to_string());
        }
    }
    let artifact = run(&packet);
    let fired = classes(&artifact);
    assert!(fired.contains(&DriftClass::UnknownOrConflictingAuthority), "got {fired:?}");
    assert!(!fired.contains(&DriftClass::NoChange));
}

#[test]
fn proposal_writer_refuses_paths_resolving_outside_the_repository() {
    let artifact = run(&unchanged_packet());
    let root = tempfile::tempdir().expect("tempdir");
    let outside = std::env::temp_dir().join("vim-lsp-subject-refresh-escape.json");
    let error = crate::vim_lsp_subject_refresh::write_proposal(root.path(), &outside, &artifact)
        .expect_err("an absolute path outside the repository must be refused");
    assert!(
        error.to_string().contains("repository-local"),
        "the refusal must name the boundary; got {error:#}"
    );
    let as_directory = root.path().join("target");
    std::fs::create_dir_all(&as_directory).expect("mkdir target");
    let error =
        crate::vim_lsp_subject_refresh::write_proposal(root.path(), &as_directory, &artifact)
            .expect_err("a directory target must be refused");
    assert!(error.to_string().contains("regular file"), "got {error:#}");
}

#[test]
fn bounded_git_runner_enforces_its_output_ceiling() {
    // `git --version` emits more than 4 bytes: the ceiling must trip without
    // buffering unbounded output, entirely offline.
    let error = crate::vim_lsp_subject_refresh::observe::run_git_bounded(
        None,
        &["--version"],
        4,
        std::time::Duration::from_secs(60),
    )
    .expect_err("an oversized output must be rejected");
    assert!(
        error.to_string().contains("ceiling"),
        "the error must name the ceiling; got {error:#}"
    );
    // A generous ceiling on the same command succeeds.
    assert!(
        crate::vim_lsp_subject_refresh::observe::run_git_bounded(
            None,
            &["--version"],
            1024 * 1024,
            std::time::Duration::from_secs(60),
        )
        .is_ok()
    );
}

#[test]
fn oversized_blob_is_rejected_by_size_before_buffering() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let cwd = scratch.path();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("spawning git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    git(&["init", "--quiet"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "seed",
    ]);
    std::fs::write(cwd.join("small.txt"), b"small").expect("small file");
    let big = vec![0u8; 3 * 1024 * 1024];
    std::fs::write(cwd.join("big.bin"), &big).expect("big file");
    git(&["add", "."]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "-m", "files"]);
    let head = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(cwd)
            .output()
            .expect("rev-parse")
            .stdout,
    )
    .expect("utf8 head");
    let head = head.trim();
    assert_eq!(
        crate::vim_lsp_subject_refresh::observe::read_file_from_for_tests(cwd, head, "small.txt"),
        Some("small".to_string())
    );
    assert_eq!(
        crate::vim_lsp_subject_refresh::observe::read_file_from_for_tests(cwd, head, "big.bin"),
        None,
        "a 3 MiB blob must be rejected by the size pre-check"
    );
}

#[test]
fn every_drift_class_has_a_deterministic_impact_entry() {
    for class in DriftClass::ALL {
        let impacts = crate::vim_lsp_subject_refresh::classify::impact_entries(*class);
        assert!(!impacts.is_empty(), "class {} lacks a deterministic impact entry", class.token());
        for impact in &impacts {
            assert_eq!(impact.triggered_by, *class);
        }
    }
}

#[test]
fn classification_is_deterministic() {
    let first = run(&unchanged_packet());
    let second = run(&unchanged_packet());
    assert_eq!(first, second, "identical inputs must yield identical artifacts");
}

// ---------------------------------------------------------------------------
// Observer pure parsers (offline; transport itself is network-gated)
// ---------------------------------------------------------------------------

#[test]
fn ls_remote_parser_extracts_head_master_and_bare_tags() {
    let output = "e10d186452743beb7b43d2b3427020832f930c2b\tHEAD\n\
                  e10d186452743beb7b43d2b3427020832f930c2b\trefs/heads/master\n\
                  3bca7e8c8a794fde38075e7df9d14c286d055a84\trefs/tags/v0.1.4\n\
                  1111111111111111111111111111111111111111\trefs/tags/v0.1.4^{}\n\
                  2222222222222222222222222222222222222222\trefs/tags/v0.1.3\n";
    let (head, master, tags) = crate::vim_lsp_subject_refresh::observe::parse_ls_remote(output);
    assert_eq!(head.as_deref(), Some("e10d186452743beb7b43d2b3427020832f930c2b"));
    assert_eq!(master.as_deref(), Some("e10d186452743beb7b43d2b3427020832f930c2b"));
    assert_eq!(tags, vec!["v0.1.3".to_string(), "v0.1.4".to_string()]);
}

#[test]
fn floor_parser_reads_the_pinned_sentence_shape_including_line_wraps() {
    use crate::vim_lsp_subject_refresh::observe::extract_floor;
    let unwrapped = "Requires NeoVim with version 0.3 or Vim 8.1.1035 or newer.\n";
    let floor = extract_floor(unwrapped);
    assert!(floor.parsed);
    assert_eq!(floor.vim_minimum.as_deref(), Some("8.1.1035"));
    assert_eq!(floor.neovim_minimum.as_deref(), Some("0.3"));
    let wrapped = "    Requires NeoVim with version 0.3 or\n    Vim 8.1.1035 or newer.\n";
    let floor = extract_floor(wrapped);
    assert!(floor.parsed);
    assert_eq!(floor.vim_minimum.as_deref(), Some("8.1.1035"));
    assert!(!extract_floor("nothing here").parsed);
}

#[test]
fn plugin_default_extractor_handles_nested_parens() {
    use crate::vim_lsp_subject_refresh::observe::extract_global_default;
    let plugin = "let g:lsp_use_lua = get(g:, 'lsp_use_lua', has('nvim-0.4.0') || (has('lua') && has('patch-8.2.0775')))\n";
    assert_eq!(
        extract_global_default(plugin, "g:lsp_use_lua").as_deref(),
        Some("has('nvim-0.4.0') || (has('lua') && has('patch-8.2.0775'))")
    );
    let queue = "let g:lsp_use_event_queue = get(g:, 'lsp_use_event_queue', has('nvim') || has('patch-8.1.0889'))\n";
    assert_eq!(
        extract_global_default(queue, "g:lsp_use_event_queue").as_deref(),
        Some("has('nvim') || has('patch-8.1.0889')")
    );
    assert_eq!(extract_global_default("let g:lsp_other = 1", "g:lsp_use_lua"), None);
}

#[test]
fn maintenance_markers_use_the_closed_vocabulary() {
    use crate::vim_lsp_subject_refresh::observe::find_maintenance_markers;
    assert_eq!(
        find_maintenance_markers("# vim-lsp\n\nSome prose about snippets."),
        Vec::<String>::new()
    );
    assert_eq!(
        find_maintenance_markers("This project is now in Maintenance Mode."),
        vec!["maintenance mode".to_string()]
    );
}

// ---------------------------------------------------------------------------
// Packet validation negative controls
// ---------------------------------------------------------------------------

fn validated(packet: &ObservationPacket) -> anyhow::Result<()> {
    crate::vim_lsp_subject_refresh::validate_packet(packet, &pinned())
}

#[test]
fn packet_with_wrong_repository_is_rejected() {
    let mut packet = unchanged_packet();
    packet.upstream_repository = "https://github.com/example/other".to_string();
    assert!(validated(&packet).is_err());
}

#[test]
fn packet_attributing_files_to_another_commit_is_rejected() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.files[0].commit = MOVED_MASTER.to_string();
    let error = validated(&packet).unwrap_err();
    assert!(error.to_string().contains("another ref's facts"), "got: {error:#}");
}

#[test]
fn packet_missing_surface_findings_is_rejected() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.surface_findings.truncate(5);
    assert!(validated(&packet).is_err());
}

#[test]
fn packet_with_extra_surface_findings_is_rejected() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.surface_findings.push(SurfaceFinding {
        surface: "invented surface".to_string(),
        file: "README.md".to_string(),
        needle: "invented".to_string(),
        found: true,
    });
    assert!(validated(&packet).is_err());
}

#[test]
fn packet_with_oversized_error_is_rejected() {
    let mut packet = unchanged_packet();
    packet.refs_probe.status = ProbeStatus::Failed;
    packet.refs_probe.head = None;
    packet.refs_probe.master = None;
    packet.refs_probe.error = Some("x".repeat(301));
    assert!(validated(&packet).is_err());
}

#[test]
fn packet_with_marker_outside_closed_vocabulary_is_rejected() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.maintenance_markers = vec!["totally arbitrary prose".to_string()];
    assert!(validated(&packet).is_err());
}

#[test]
fn packet_with_non_version_floor_is_rejected() {
    let mut packet = unchanged_packet();
    packet.head_tree_probe.floor = Some(FloorObservation {
        parsed: true,
        neovim_minimum: Some("0.3".to_string()),
        vim_minimum: Some("8.1.1035 or newer; see the forum thread at <https://example.invalid/thread?id=1> for details".to_string()),
    });
    assert!(validated(&packet).is_err());
}

// ---------------------------------------------------------------------------
// Proposal writer guards
// ---------------------------------------------------------------------------

#[test]
fn proposal_writer_refuses_the_authority_tree_and_leaves_the_pin_untouched() {
    let artifact = run(&unchanged_packet());
    let root = tempfile::tempdir().expect("tempdir");
    let ci_dir = root.path().join(".ci");
    std::fs::create_dir_all(&ci_dir).expect("mkdir .ci");
    let pin_path = ci_dir.join("vim-vim-lsp-subject.v1.json");
    std::fs::write(&pin_path, "{\"pinned\":true}").expect("seed pin");
    let before = std::fs::read(&pin_path).expect("read pin");

    let into_authority = std::path::Path::new(".ci/editor-clients/vim-vim-lsp-subject.v1.json");
    assert!(
        crate::vim_lsp_subject_refresh::write_proposal(root.path(), into_authority, &artifact)
            .is_err()
    );
    let nested_authority = std::path::Path::new(".ci/other/proposal.json");
    assert!(
        crate::vim_lsp_subject_refresh::write_proposal(root.path(), nested_authority, &artifact)
            .is_err()
    );

    let allowed = std::path::Path::new("target/vim-lsp-subject-refresh.json");
    crate::vim_lsp_subject_refresh::write_proposal(root.path(), allowed, &artifact)
        .expect("writing outside .ci succeeds");
    let after = std::fs::read(&pin_path).expect("re-read pin");
    assert_eq!(before, after, "the landed pin must stay byte-identical");
    let written: ObservationPacketProbe = serde_json::from_str(
        &std::fs::read_to_string(root.path().join(allowed)).expect("read artifact"),
    )
    .expect("artifact parses as JSON");
    assert_eq!(written.schema_version, "vim_lsp_subject_refresh.v1");
}

/// Minimal probe type for round-trip assertions on written artifacts.
#[derive(serde::Deserialize)]
struct ObservationPacketProbe {
    schema_version: String,
}
