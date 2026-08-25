//! Deterministic drift classification for the #11411 read-only vim-lsp
//! subject refresh.
//!
//! [`classify`] is pure: it consumes a validated observation packet plus the
//! landed #11369 pinned-subject manifest and emits the bounded review
//! artifact — every class with its evidence probe, every check with an
//! explicit positive finding, and a narrow deterministic impact graph.
//!
//! Fail-closed laws:
//!
//! - any failed probe yields `instrument_failed` and suppresses
//!   `no_change`/`metadata_only_non_semantic`;
//! - unparseable or contradictory authority yields
//!   `unknown_or_conflicting_authority`, never a convenient answer;
//! - upstream metadata (floors, release tags, capability prose) is compared
//!   as metadata: it can recommend, never invalidate directly-tested rows.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::vim_lsp_subject_refresh::model::{
    ClassifiedDrift, DriftClass, ImpactEntry, ObservationPacket, PositiveFinding, ProbeEvidence,
    ProbeStatus, ProposedSubjectFieldChange, REFRESH_SCHEMA_VERSION, RefreshArtifact,
    SelectedSubjectBefore, SurfaceObservationRow,
};
use crate::vim_lsp_subject_refresh::probe_table::{
    EXPECTED_PLUGIN_DEFAULTS, SURFACE_PROBES, SurfaceProbe,
};

/// Probe identifiers used across evidence rows.
pub const PROBE_REFS: &str = "refs";
pub const PROBE_PINNED: &str = "pinned_commit";
pub const PROBE_HEAD: &str = "head_tree";

/// Closed action vocabulary for impact rows.
pub const ACTION_NO_INVALIDATION: &str = "no_invalidation";
pub const ACTION_REVIEW: &str = "review_disposition";
pub const ACTION_REFRESH_AUTHORITY: &str = "refresh_authority";
pub const ACTION_CANNOT_ASSUME: &str = "cannot_assume_current";
pub const ACTION_INVALIDATE: &str = "invalidate_and_rerun";
pub const ACTION_RETRY: &str = "retry_observation";

/// Fixed advisory boundary lines serialized into every artifact.
pub const ADVISORY_BOUNDARY: &[&str] = &[
    "advisory evidence for maintainers only; never a CI gate",
    "never auto-updates the #11369 pin or any support state",
    "proposal output is bounded and review-only; nothing is applied",
    "instrument failure is reported as instrument_failed, never as no drift",
    "default path is offline; live observation requires an explicit --allow-network gate",
    "observed facts are upstream source metadata, never actual-host behavior",
];

/// The pinned-subject facts extracted from the landed #11369 manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedSubject {
    pub repository: String,
    pub selected_commit: String,
    pub tree_digest: String,
    pub latest_release_tag: String,
    pub vim_theoretical_minimum: String,
    pub neovim_theoretical_minimum: String,
    pub entry_files: Vec<(String, String)>,
}

impl PinnedSubject {
    /// Extract the pinned facts from the landed manifest JSON.
    pub fn from_manifest(manifest: &serde_json::Value) -> Result<Self> {
        let repository = manifest
            .pointer("/upstream/repository")
            .and_then(|value| value.as_str())
            .context("subject manifest is missing upstream.repository")?
            .to_string();
        let selected_commit = manifest
            .pointer("/upstream/selected_commit")
            .and_then(|value| value.as_str())
            .context("subject manifest is missing upstream.selected_commit")?
            .to_string();
        let tree_digest = manifest
            .pointer("/upstream/tree_digest/value")
            .and_then(|value| value.as_str())
            .context("subject manifest is missing upstream.tree_digest.value")?
            .to_string();
        let latest_release_tag = manifest
            .pointer("/upstream/latest_release_tag_observation/tag")
            .and_then(|value| value.as_str())
            .context("subject manifest is missing latest_release_tag_observation.tag")?
            .to_string();
        let mut vim_theoretical_minimum = String::new();
        let mut neovim_theoretical_minimum = String::new();
        let rows = manifest
            .pointer("/upstream_theoretical_prerequisites/rows")
            .and_then(|value| value.as_array())
            .context("subject manifest is missing upstream_theoretical_prerequisites.rows")?;
        for row in rows {
            let editor = row.get("editor").and_then(|value| value.as_str()).unwrap_or_default();
            let minimum =
                row.get("theoretical_minimum").and_then(|value| value.as_str()).unwrap_or_default();
            match editor {
                "vim" => vim_theoretical_minimum = minimum.to_string(),
                "neovim" => neovim_theoretical_minimum = minimum.to_string(),
                _ => {}
            }
        }
        ensure!(
            !vim_theoretical_minimum.is_empty() && !neovim_theoretical_minimum.is_empty(),
            "subject manifest prerequisite rows lack vim/neovim theoretical minimums"
        );
        let mut entry_files = Vec::new();
        let entries = manifest
            .pointer("/expected_content_identity/entry_files")
            .and_then(|value| value.as_array())
            .context("subject manifest is missing expected_content_identity.entry_files")?;
        for entry in entries {
            let path = entry.get("path").and_then(|value| value.as_str()).unwrap_or_default();
            let blob =
                entry.get("git_blob_sha1").and_then(|value| value.as_str()).unwrap_or_default();
            ensure!(
                !path.is_empty() && !blob.is_empty(),
                "subject manifest entry file lacks path or git_blob_sha1"
            );
            entry_files.push((path.to_string(), blob.to_string()));
        }
        ensure!(!entry_files.is_empty(), "subject manifest carries no entry files");
        Ok(PinnedSubject {
            repository,
            selected_commit,
            tree_digest,
            latest_release_tag,
            vim_theoretical_minimum,
            neovim_theoretical_minimum,
            entry_files,
        })
    }
}

/// Bump-style semantic ordering over tag names like `v0.1.4`: compare
/// numeric runs; a non-parseable side is ordered lower only when it is not
/// the pinned tag, and equality is exact string equality. Returns `None`
/// when either tag does not carry a parseable `v<major>.<minor>` prefix.
fn tag_key(tag: &str) -> Option<(u64, u64, u64)> {
    let body = tag.strip_prefix('v')?;
    let mut parts = body.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next().and_then(|part| part.parse::<u64>().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// The deterministic class -> impact graph. Every [`DriftClass`] variant has
/// an entry (even an explicit no-op), enforced by tests.
pub fn impact_entries(class: DriftClass) -> Vec<ImpactEntry> {
    let entry = |item: &str, action: &'static str, reason: &str| ImpactEntry {
        item: item.to_string(),
        action: action.to_string(),
        reason: reason.to_string(),
        triggered_by: class,
    };
    match class {
        DriftClass::NoChange => {
            vec![entry(
                "all dependent evidence",
                ACTION_NO_INVALIDATION,
                "every probe checked against the pinned subject; retained receipts remain current",
            )]
        }
        DriftClass::MetadataOnlyNonSemantic => {
            vec![entry(
                "all dependent evidence",
                ACTION_NO_INVALIDATION,
                "selected bytes and probed action/config semantics are identical; host receipts remain current",
            )]
        }
        DriftClass::NewUpstreamReleaseOrRefAvailable => {
            vec![entry(
                "#11369 pinned subject",
                ACTION_REVIEW,
                "newer upstream availability recommends a reviewed pin update; no automatic promotion or invalidation",
            )]
        }
        DriftClass::SelectedRefChangedMissingOrUnreachable => vec![
            entry(
                "#11369 subject manifest",
                ACTION_REFRESH_AUTHORITY,
                "the pinned commit could not be fetched while the repository stayed reachable; subject identity must be re-established",
            ),
            entry(
                "#11372 host toolchain rows",
                ACTION_CANNOT_ASSUME,
                "toolchain rows bind the pinned subject bytes and cannot be assumed current",
            ),
            entry(
                "#10944/#11380 driver bindings",
                ACTION_CANNOT_ASSUME,
                "driver bindings cite pinned bytes and cannot be assumed current",
            ),
            entry(
                "#10962 baseline cells",
                ACTION_CANNOT_ASSUME,
                "baseline cells bind the pinned subject and cannot be assumed current",
            ),
            entry(
                "#11408 specialized receipts",
                ACTION_CANNOT_ASSUME,
                "specialized receipts bind the pinned subject and cannot be assumed current",
            ),
        ],
        DriftClass::SelectedTreeDigestChanged => vec![
            entry(
                "#11369 subject manifest",
                ACTION_REFRESH_AUTHORITY,
                "recorded tree/entry digests no longer match the pinned commit; manifest integrity must be re-resolved",
            ),
            entry(
                "#11372 host toolchain rows",
                ACTION_CANNOT_ASSUME,
                "toolchain rows bind the pinned bytes and cannot be assumed current",
            ),
            entry(
                "#10962 baseline cells and #11408 specialized receipts",
                ACTION_CANNOT_ASSUME,
                "receipts binding the affected bytes cannot be assumed current",
            ),
        ],
        DriftClass::VimHostFloorOrRequiredFeatureChanged => vec![
            entry(
                "#10966 maintained/tested Vim rows",
                ACTION_REVIEW,
                "upstream theoretical minimum is metadata only; directly tested rows stay current until a reviewed row change",
            ),
            entry(
                "#11369 prerequisite rows",
                ACTION_REVIEW,
                "manifest prerequisite metadata would change on a reviewed pin update; nothing is auto-applied",
            ),
        ],
        DriftClass::PluginLoadOrInstallShapeChanged => vec![
            entry(
                "#11372 provisioning rows",
                ACTION_REVIEW,
                "load/install shape feeds the caller-pinned checkout provisioning; review before any rerun",
            ),
            entry(
                "activation family receipts (#11386/#11387/#11388)",
                ACTION_REVIEW,
                "activation/recovery/reopen families consume the load shape; review disposition only",
            ),
        ],
        DriftClass::RegistrationRootOrConfigApiChanged => vec![
            entry(
                "#11369 registration/root/config authority",
                ACTION_REFRESH_AUTHORITY,
                "registration/root/config API drifted; the subject authority must be refreshed",
            ),
            entry(
                "activation/root/config-dependent receipts",
                ACTION_INVALIDATE,
                "receipts binding registration/root/config behavior must be rerun against reviewed authority",
            ),
        ],
        DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged => vec![
            entry(
                "diagnostics/readiness baseline cells (#10962)",
                ACTION_INVALIDATE,
                "cells observing readiness/diagnostics surfaces must be rerun",
            ),
            entry(
                "#11408 specialized diagnostics receipts",
                ACTION_INVALIDATE,
                "specialized receipts binding the diagnostics/logging surface must be rerun",
            ),
        ],
        DriftClass::CompletionOrSnippetApplicationModelChanged => vec![
            entry(
                "#10944/#11380 completion bindings",
                ACTION_REFRESH_AUTHORITY,
                "completion conversion/application bindings must be refreshed",
            ),
            entry(
                "completion baseline cells and aggregate profiles (#10962)",
                ACTION_INVALIDATE,
                "completion cells and aggregates binding the changed model must be rerun",
            ),
        ],
        DriftClass::NavigationOrWorkspaceEditActionChanged => vec![
            entry(
                "#10944/#11380 navigation/rename/format bindings",
                ACTION_REFRESH_AUTHORITY,
                "hover/definition/references/rename/formatting bindings must be refreshed",
            ),
            entry(
                "navigation/edit baseline cells and aggregate profiles (#10962)",
                ACTION_INVALIDATE,
                "cells and aggregates binding the changed actions must be rerun",
            ),
        ],
        DriftClass::WorkspaceConfigurationBehaviorChanged => vec![
            entry(
                "#11369 configuration authority",
                ACTION_REFRESH_AUTHORITY,
                "workspace configuration behavior drifted; config authority must be refreshed",
            ),
            entry(
                "workspace-configuration cells",
                ACTION_INVALIDATE,
                "cells binding workspace configuration behavior must be rerun",
            ),
        ],
        DriftClass::TextSyncOrDidChangeObservationSurfaceChanged => vec![entry(
            "#11408 didChange wire receipts",
            ACTION_INVALIDATE,
            "wire-capture instrumentation receipts must be recaptured against the changed surface",
        )],
        DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged => vec![entry(
            "recovery/lifecycle families (#11386/#11387/#11388 receipts)",
            ACTION_INVALIDATE,
            "recovery/reopen/lifecycle receipts must be rerun; unaffected core semantics stay current",
        )],
        DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged => vec![entry(
            "#10960/#11405 workspace-folder cells",
            ACTION_INVALIDATE,
            "workspace-folder cells must be rerun; bounded core is unaffected unless the shared config/API also changed",
        )],
        DriftClass::SelectedPublicApiDeprecatedOrRemoved => vec![entry(
            "#10944/#11380 driver bindings",
            ACTION_REFRESH_AUTHORITY,
            "a consumed public API signature disappeared upstream; driver bindings must be refreshed",
        )],
        DriftClass::MaintenanceStateChanged => vec![
            entry(
                "#7712 upstream candidate packet",
                ACTION_REVIEW,
                "maintenance-state change opens a reviewed refresh of the upstream packet",
            ),
            entry(
                "#10974/#10978 support/docs",
                ACTION_REVIEW,
                "support/docs refresh only after matching evidence; no automatic promotion",
            ),
        ],
        DriftClass::UnknownOrConflictingAuthority => vec![entry(
            "named dependents (#11369/#11372/#10962/#11408)",
            ACTION_CANNOT_ASSUME,
            "unverifiable or conflicting authority fails closed; dependent evidence cannot be assumed current",
        )],
        DriftClass::InstrumentFailed => vec![entry(
            "observation instrument",
            ACTION_RETRY,
            "a probe failed; no drift claim is made and nothing may be assumed current or stale",
        )],
    }
}

/// Deterministic class -> disposition recommendations (advisory only).
fn dispositions(class: DriftClass) -> &'static [&'static str] {
    match class {
        DriftClass::NoChange | DriftClass::MetadataOnlyNonSemantic => &["retain_pin"],
        DriftClass::NewUpstreamReleaseOrRefAvailable => &["open_reviewed_pin_update"],
        DriftClass::SelectedRefChangedMissingOrUnreachable
        | DriftClass::SelectedTreeDigestChanged => &["refresh_subject_authority_11369"],
        DriftClass::VimHostFloorOrRequiredFeatureChanged => &[
            "review_theoretical_floor_against_10966_rows",
            "refresh_subject_authority_11369_on_pin_update",
        ],
        DriftClass::PluginLoadOrInstallShapeChanged => {
            &["open_reviewed_pin_update", "reprovision_toolchain_rows_11372"]
        }
        DriftClass::RegistrationRootOrConfigApiChanged
        | DriftClass::WorkspaceConfigurationBehaviorChanged => {
            &["refresh_subject_authority_11369", "refresh_driver_bindings_10944_11380"]
        }
        DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged
        | DriftClass::CompletionOrSnippetApplicationModelChanged
        | DriftClass::NavigationOrWorkspaceEditActionChanged
        | DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged => &[
            "refresh_driver_bindings_10944_11380",
            "invalidate_baseline_cells_10962",
            "invalidate_specialized_receipts_11408",
        ],
        DriftClass::TextSyncOrDidChangeObservationSurfaceChanged => {
            &["invalidate_specialized_receipts_11408"]
        }
        DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged => {
            &["invalidate_workspace_folder_cells_10960_11405"]
        }
        DriftClass::SelectedPublicApiDeprecatedOrRemoved => {
            &["refresh_driver_bindings_10944_11380"]
        }
        DriftClass::MaintenanceStateChanged => &[
            "refresh_upstream_packet_7712",
            "refresh_support_docs_10974_10978_after_matching_evidence",
        ],
        DriftClass::UnknownOrConflictingAuthority => {
            &["retain_pin", "manual_review_of_conflicting_authority"]
        }
        DriftClass::InstrumentFailed => &["retain_pin", "retry_observation"],
    }
}

/// Mutable accumulator used while classifying.
struct Classifier<'a> {
    packet: &'a ObservationPacket,
    pinned: &'a PinnedSubject,
    classes: Vec<ClassifiedDrift>,
    positives: Vec<PositiveFinding>,
    proposals: Vec<ProposedSubjectFieldChange>,
    refresh_set: Vec<String>,
}

impl<'a> Classifier<'a> {
    fn fire(&mut self, class: DriftClass, probes: &[&'static str], detail: String) {
        self.classes.push(ClassifiedDrift {
            class,
            evidence_probe_ids: probes.iter().map(|probe| (*probe).to_string()).collect(),
            detail: bounded(detail, 400),
        });
    }

    fn positive(&mut self, probe_id: &'static str, check: String) {
        self.positives.push(PositiveFinding {
            probe_id: probe_id.to_string(),
            check: bounded(check, 300),
            result: "checked_no_drift".to_string(),
        });
    }

    fn has_class(&self, class: DriftClass) -> bool {
        self.classes.iter().any(|entry| entry.class == class)
    }
}

/// Deterministic boundedness clamp: artifact strings may never exceed their
/// caps, regardless of how many joined evidence items a class carries.
fn bounded(value: String, cap: usize) -> String {
    if value.chars().count() <= cap {
        return value;
    }
    let mut truncated: String = value.chars().take(cap.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

/// Classify a validated packet against the pinned subject. Pure and
/// deterministic: identical inputs yield a byte-identical artifact.
pub fn classify(packet: &ObservationPacket, pinned: &PinnedSubject) -> Result<RefreshArtifact> {
    let mut classifier = Classifier {
        packet,
        pinned,
        classes: Vec::new(),
        positives: Vec::new(),
        proposals: Vec::new(),
        refresh_set: Vec::new(),
    };

    // --- instrument health ------------------------------------------------
    let refs_ok = packet.refs_probe.status == ProbeStatus::Ok;
    let pinned_ok = packet.pinned_commit_probe.status == ProbeStatus::Ok;
    let head_ok = packet.head_tree_probe.status == ProbeStatus::Ok;

    if !refs_ok {
        classifier.fire(
            DriftClass::InstrumentFailed,
            &[PROBE_REFS],
            format!(
                "refs probe failed: {}",
                packet.refs_probe.error.as_deref().unwrap_or("unavailable")
            ),
        );
    }
    if !pinned_ok {
        if refs_ok && head_ok {
            // The repository and its head are observable, but the pinned
            // commit is not fetchable: the selection itself is unreachable.
            classifier.fire(
                DriftClass::SelectedRefChangedMissingOrUnreachable,
                &[PROBE_PINNED],
                format!(
                    "depth-1 fetch of the pinned commit failed while refs and head probes succeeded: {}",
                    packet.pinned_commit_probe.error.as_deref().unwrap_or("unavailable")
                ),
            );
        } else {
            classifier.fire(
                DriftClass::InstrumentFailed,
                &[PROBE_PINNED],
                format!(
                    "pinned-commit probe failed: {}",
                    packet.pinned_commit_probe.error.as_deref().unwrap_or("unavailable")
                ),
            );
        }
    }
    if !head_ok {
        classifier.fire(
            DriftClass::InstrumentFailed,
            &[PROBE_HEAD],
            format!(
                "head-tree probe failed: {}",
                packet.head_tree_probe.error.as_deref().unwrap_or("unavailable")
            ),
        );
    }

    // --- ref/release observation -------------------------------------------
    let mut master_matches_pin: Option<bool> = None;
    let mut newer_release_available: Option<bool> = None;
    let mut newest_observed_tag: Option<String> = None;
    if refs_ok {
        let master = packet.refs_probe.master.as_deref();
        let head = packet.refs_probe.head.as_deref();
        // The tracked ref is master when it exists; an upstream default-
        // branch rename removes it, and HEAD then carries the maintained
        // tip. A HEAD/master split with master still pinned is itself
        // upstream drift the artifact must show.
        let tracked = master.or(head);
        master_matches_pin = tracked.map(|tracked| tracked == pinned.selected_commit);
        if let (Some(master), Some(head)) = (master, head)
            && master != head
        {
            classifier.fire(
                DriftClass::NewUpstreamReleaseOrRefAvailable,
                &[PROBE_REFS],
                format!(
                    "upstream HEAD {head} detached from refs/heads/master {master}: the default branch moved while the tracked ref stayed put, via {}",
                    packet.refs_probe.method
                ),
            );
        }
        if master_matches_pin == Some(true) {
            classifier.positive(
                PROBE_REFS,
                format!(
                    "tracked upstream ref == pinned selected_commit {} via {}",
                    pinned.selected_commit, packet.refs_probe.method
                ),
            );
        } else {
            // Ref moved: newer upstream bytes exist. This alone never
            // invalidates anything - it recommends a reviewed pin update.
            classifier.fire(
                DriftClass::NewUpstreamReleaseOrRefAvailable,
                &[PROBE_REFS],
                format!(
                    "tracked upstream ref observed {} != pinned selected_commit {} via {}",
                    tracked.unwrap_or("<missing>"),
                    pinned.selected_commit,
                    packet.refs_probe.method
                ),
            );
        }
        let pinned_key = tag_key(&pinned.latest_release_tag);
        let mut best: Option<(&str, (u64, u64, u64))> = None;
        for tag in &packet.refs_probe.tags {
            let Some(key) = tag_key(tag) else { continue };
            let improved = best.is_none_or(|(_, current)| key > current);
            if improved {
                best = Some((tag, key));
            }
        }
        if let (Some((tag, key)), Some(pinned_key)) = (best, pinned_key) {
            newest_observed_tag = Some(tag.to_string());
            if key > pinned_key {
                newer_release_available = Some(true);
                classifier.fire(
                    DriftClass::NewUpstreamReleaseOrRefAvailable,
                    &[PROBE_REFS],
                    format!(
                        "newer release tag {tag} observed vs pinned recorded {} via {}",
                        pinned.latest_release_tag, packet.refs_probe.method
                    ),
                );
                classifier.proposals.push(ProposedSubjectFieldChange {
                    field: "upstream.latest_release_tag_observation.tag".to_string(),
                    current: pinned.latest_release_tag.clone(),
                    observed: tag.to_string(),
                });
            } else {
                classifier.positive(
                    PROBE_REFS,
                    format!(
                        "newest observed release tag {tag} <= pinned recorded {} via {}",
                        pinned.latest_release_tag, packet.refs_probe.method
                    ),
                );
            }
        } else {
            // Tags exist but do not parse: the currentness observation is
            // unverifiable, never "no newer release".
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_REFS],
                format!(
                    "observed tags carry no parseable version beyond the pinned record: [{}]",
                    packet.refs_probe.tags.join(", ")
                ),
            );
        }
    }

    // --- pinned identity ----------------------------------------------------
    if pinned_ok {
        let probe = &packet.pinned_commit_probe;
        if probe.resolved_commit.as_deref() != Some(pinned.selected_commit.as_str())
            && probe.resolved_commit.is_some()
        {
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_PINNED],
                format!(
                    "fetched commit {} != requested {} — observation identity conflict",
                    probe.resolved_commit.as_deref().unwrap_or("<missing>"),
                    probe.requested_commit
                ),
            );
        } else if probe.resolved_commit.as_deref() == Some(pinned.selected_commit.as_str()) {
            classifier.positive(
                PROBE_PINNED,
                format!("pinned commit still resolves to itself via {}", probe.method),
            );
        }
        let mut digest_drift: Vec<String> = Vec::new();
        if probe.resolved_tree.as_deref() != Some(pinned.tree_digest.as_str()) {
            digest_drift.push(format!(
                "tree {} != recorded {}",
                probe.resolved_tree.as_deref().unwrap_or("<missing>"),
                pinned.tree_digest
            ));
        }
        for file in &probe.entry_file_blobs {
            let recorded = pinned
                .entry_files
                .iter()
                .find(|(path, _)| path == &file.path)
                .map(|(_, blob)| blob.as_str());
            match (recorded, &file.git_blob_sha1) {
                (Some(recorded), Some(observed)) if recorded == observed => {}
                (Some(recorded), observed) => digest_drift.push(format!(
                    "entry file {} blob {} != recorded {recorded}",
                    file.path,
                    observed.as_deref().unwrap_or("<missing>")
                )),
                (None, _) => digest_drift
                    .push(format!("entry file {} is not recorded in the manifest", file.path)),
            }
        }
        if digest_drift.is_empty() {
            classifier.positive(
                PROBE_PINNED,
                format!(
                    "pinned commit resolves recorded tree {} and all {} entry-file blobs via {}",
                    pinned.tree_digest,
                    pinned.entry_files.len(),
                    probe.method
                ),
            );
        } else {
            classifier.fire(
                DriftClass::SelectedTreeDigestChanged,
                &[PROBE_PINNED],
                digest_drift.join("; "),
            );
        }
    }

    // --- observed head tree -------------------------------------------------
    if head_ok {
        let head = &packet.head_tree_probe;
        let head_commit = head.commit.as_deref();
        if let (Some(head_commit), Some(master)) =
            (head_commit, packet.refs_probe.master.as_deref())
            && head_commit != master
        {
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD, PROBE_REFS],
                format!(
                    "head-tree probe read commit {head_commit} but refs observed master {master}; refusing to attribute one ref's facts to another"
                ),
            );
        }
        classify_head_tree(&mut classifier);
    }

    // --- terminal no-change / metadata-only combination ----------------------
    // Byte identity of the consumed entry files: `metadata_only_non_semantic`
    // is claimable only when the observed head carries the pinned bytes for
    // every manifest entry file. Differing bytes are drift the probes did
    // not classify, and must not ride along as "metadata".
    let mut unproven_entry_files: Vec<String> = Vec::new();
    if head_ok {
        for (path, pinned_blob) in &pinned.entry_files {
            let observed = packet.head_tree_probe.files.iter().find(|file| &file.path == path);
            let matches = observed.is_some_and(|file| {
                file.present && file.git_blob_sha1.as_deref() == Some(pinned_blob.as_str())
            });
            if !matches {
                unproven_entry_files.push(path.clone());
            }
        }
        if !unproven_entry_files.is_empty() && master_matches_pin == Some(false) {
            classifier.fire(
                DriftClass::NewUpstreamReleaseOrRefAvailable,
                &[PROBE_HEAD, PROBE_PINNED],
                format!(
                    "{} manifest entry file(s) differ from the pinned bytes on the observed head; probed surfaces are intact but semantics beyond the probes are unproven — review the diff before any pin update",
                    unproven_entry_files.len()
                ),
            );
        } else if !unproven_entry_files.is_empty() {
            // The refs claim the tracked ref is the pin, yet the head probe
            // reports different bytes: contradictory observations.
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD, PROBE_REFS],
                "head probe reports entry bytes differing from the pinned bytes while the tracked ref equals the pin".to_string(),
            );
        } else {
            classifier.positive(
                PROBE_HEAD,
                format!(
                    "observed head carries the pinned bytes for all {} manifest entry files via {}",
                    pinned.entry_files.len(),
                    packet.head_tree_probe.method
                ),
            );
        }
    }
    let instrument_failed = classifier.has_class(DriftClass::InstrumentFailed);
    let unknown = classifier.has_class(DriftClass::UnknownOrConflictingAuthority);
    let semantic_drift = classifier.classes.iter().any(|entry| {
        !matches!(
            entry.class,
            DriftClass::NewUpstreamReleaseOrRefAvailable
                | DriftClass::UnknownOrConflictingAuthority
                | DriftClass::InstrumentFailed
        )
    });
    let default_branch_moved = matches!(
        (packet.refs_probe.master.as_deref(), packet.refs_probe.head.as_deref()),
        (Some(master), Some(head)) if master != head
    );
    let upstream_moved = master_matches_pin == Some(false)
        || newer_release_available == Some(true)
        || default_branch_moved;
    if !instrument_failed && !unknown && !semantic_drift && unproven_entry_files.is_empty() {
        if upstream_moved {
            classifier.fire(
                DriftClass::MetadataOnlyNonSemantic,
                &[PROBE_REFS, PROBE_PINNED, PROBE_HEAD],
                "upstream refs/releases moved while the pinned bytes and every probed semantic surface are identical"
                    .to_string(),
            );
        } else {
            classifier.fire(
                DriftClass::NoChange,
                &[PROBE_REFS, PROBE_PINNED, PROBE_HEAD],
                "checked every probe against the pinned subject; no drift observed".to_string(),
            );
        }
    }

    // --- assemble artifact ----------------------------------------------------
    // Coalesce per-probe firings into one entry per class (merged evidence,
    // joined details) so a multi-needle regression cannot duplicate a class
    // or exceed the class count cap.
    let mut merged: BTreeMap<DriftClass, ClassifiedDrift> = BTreeMap::new();
    for entry in classifier.classes {
        match merged.entry(entry.class) {
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let existing = slot.get_mut();
                for probe in entry.evidence_probe_ids {
                    if !existing.evidence_probe_ids.contains(&probe) {
                        existing.evidence_probe_ids.push(probe);
                    }
                }
                if existing.detail != entry.detail {
                    existing.detail =
                        bounded(format!("{}; {}", existing.detail, entry.detail), 400);
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(entry);
            }
        }
    }
    let classes: Vec<ClassifiedDrift> = merged.into_values().collect();
    let impacted = collect_impacts(&classes);
    let mut disposition: Vec<&'static str> = Vec::new();
    for entry in &classes {
        for token in dispositions(entry.class) {
            if !disposition.contains(token) {
                disposition.push(token);
            }
        }
    }
    let mut refresh_set = classifier.refresh_set.clone();
    for impact in &impacted {
        let token = match impact.action.as_str() {
            ACTION_INVALIDATE => format!("rerun: {}", impact.item),
            ACTION_REFRESH_AUTHORITY => format!("refresh: {}", impact.item),
            ACTION_CANNOT_ASSUME => format!("re-verify: {}", impact.item),
            ACTION_REVIEW => format!("review: {}", impact.item),
            _ => continue,
        };
        if !refresh_set.contains(&token) {
            refresh_set.push(token);
        }
    }
    if refresh_set.is_empty() {
        refresh_set.push("none required".to_string());
    }

    Ok(RefreshArtifact {
        schema_version: REFRESH_SCHEMA_VERSION.to_string(),
        observed_at_utc: packet.observed_at_utc.clone(),
        advisory_boundary: ADVISORY_BOUNDARY.iter().map(|line| line.to_string()).collect(),
        selected_subject_before: SelectedSubjectBefore {
            manifest_path: ".ci/editor-clients/vim-vim-lsp-subject.v1.json".to_string(),
            repository: pinned.repository.clone(),
            selected_commit: pinned.selected_commit.clone(),
            tree_digest: pinned.tree_digest.clone(),
            latest_release_tag: pinned.latest_release_tag.clone(),
            vim_theoretical_minimum: pinned.vim_theoretical_minimum.clone(),
            neovim_theoretical_minimum: pinned.neovim_theoretical_minimum.clone(),
            entry_files: pinned
                .entry_files
                .iter()
                .map(|(path, blob)| crate::vim_lsp_subject_refresh::model::EntryFileBefore {
                    path: path.clone(),
                    git_blob_sha1: blob.clone(),
                })
                .collect(),
        },
        probes: vec![
            ProbeEvidence {
                id: PROBE_REFS.to_string(),
                method: packet.refs_probe.method.clone(),
                status: packet.refs_probe.status,
                error: packet.refs_probe.error.clone(),
            },
            ProbeEvidence {
                id: PROBE_PINNED.to_string(),
                method: packet.pinned_commit_probe.method.clone(),
                status: packet.pinned_commit_probe.status,
                error: packet.pinned_commit_probe.error.clone(),
            },
            ProbeEvidence {
                id: PROBE_HEAD.to_string(),
                method: packet.head_tree_probe.method.clone(),
                status: packet.head_tree_probe.status,
                error: packet.head_tree_probe.error.clone(),
            },
        ],
        release_observation: crate::vim_lsp_subject_refresh::model::ReleaseObservation {
            pinned_recorded_tag: pinned.latest_release_tag.clone(),
            newest_observed_tag,
            newer_release_available,
            master_matches_pin,
            probe_id: PROBE_REFS.to_string(),
        },
        prerequisite_observation: crate::vim_lsp_subject_refresh::model::PrerequisiteObservation {
            vim_before: pinned.vim_theoretical_minimum.clone(),
            neovim_before: pinned.neovim_theoretical_minimum.clone(),
            vim_observed: packet
                .head_tree_probe
                .floor
                .as_ref()
                .and_then(|floor| floor.vim_minimum.clone()),
            neovim_observed: packet
                .head_tree_probe
                .floor
                .as_ref()
                .and_then(|floor| floor.neovim_minimum.clone()),
            probe_id: PROBE_HEAD.to_string(),
        },
        public_surface_observation: surface_rows(packet),
        drift_classes: classes,
        positive_findings: classifier.positives,
        impacted_evidence: impacted,
        recommended_disposition: disposition.into_iter().map(String::from).collect(),
        proposed_subject_field_changes: classifier.proposals,
        required_evidence_refresh_set: refresh_set,
    })
}

fn surface_rows(packet: &ObservationPacket) -> Vec<SurfaceObservationRow> {
    let findings: BTreeMap<(&str, &str, &str), bool> = packet
        .head_tree_probe
        .surface_findings
        .iter()
        .map(|finding| {
            (
                (finding.surface.as_str(), finding.file.as_str(), finding.needle.as_str()),
                finding.found,
            )
        })
        .collect();
    SURFACE_PROBES
        .iter()
        .map(|probe| SurfaceObservationRow {
            surface: probe.surface.to_string(),
            file: probe.file.to_string(),
            needle: probe.needle.to_string(),
            found_observed: findings
                .get(&(probe.surface, probe.file, probe.needle))
                .copied()
                .unwrap_or(false),
            drift_class_if_absent: probe.class_if_absent,
        })
        .collect()
}

/// Classify the observed head tree: load shape, prerequisites, defaults,
/// maintenance markers, capability note, and surface needles.
fn classify_head_tree(classifier: &mut Classifier<'_>) {
    let head = &classifier.packet.head_tree_probe;
    let file_present = |path: &str| head.files.iter().any(|file| file.path == path && file.present);

    // Load shape: plugin entry file, once-guard, and manifest entry set.
    let plugin_present = file_present(crate::vim_lsp_subject_refresh::probe_table::FILE_PLUGIN);
    let missing_entries: Vec<&str> = classifier
        .pinned
        .entry_files
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| !file_present(path))
        .collect();
    if !plugin_present {
        classifier.fire(
            DriftClass::PluginLoadOrInstallShapeChanged,
            &[PROBE_HEAD],
            format!(
                "plugin entry file {} absent from the observed head tree via {}",
                crate::vim_lsp_subject_refresh::probe_table::FILE_PLUGIN,
                head.method
            ),
        );
    } else {
        match head.load_guard_present {
            Some(true) => classifier.positive(
                PROBE_HEAD,
                format!(
                    "plugin entry file present and once-guard g:lsp_loaded intact via {}",
                    head.method
                ),
            ),
            Some(false) => classifier.fire(
                DriftClass::PluginLoadOrInstallShapeChanged,
                &[PROBE_HEAD],
                format!(
                    "plugin once-guard g:lsp_loaded absent from {} via {}",
                    crate::vim_lsp_subject_refresh::probe_table::FILE_PLUGIN,
                    head.method
                ),
            ),
            None => classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD],
                "plugin entry file present but the load-guard observation is missing".to_string(),
            ),
        }
    }
    if missing_entries.is_empty() {
        classifier.positive(
            PROBE_HEAD,
            format!(
                "all {} manifest entry files still exist in the observed head tree via {}",
                classifier.pinned.entry_files.len(),
                head.method
            ),
        );
    } else {
        classifier.fire(
            DriftClass::PluginLoadOrInstallShapeChanged,
            &[PROBE_HEAD],
            format!(
                "manifest entry files absent from the observed head tree: {}",
                missing_entries.join(", ")
            ),
        );
    }

    // Prerequisites: the theoretical floor sentence is metadata only.
    let doc_present = file_present(crate::vim_lsp_subject_refresh::probe_table::FILE_DOC);
    match (&head.floor, doc_present) {
        (Some(floor), true) if floor.parsed => {
            if floor.vim_minimum.as_deref()
                == Some(classifier.pinned.vim_theoretical_minimum.as_str())
                && floor.neovim_minimum.as_deref()
                    == Some(classifier.pinned.neovim_theoretical_minimum.as_str())
            {
                classifier.positive(
                    PROBE_HEAD,
                    format!(
                        "theoretical floor vim {} / neovim {} unchanged in {} via {}",
                        classifier.pinned.vim_theoretical_minimum,
                        classifier.pinned.neovim_theoretical_minimum,
                        crate::vim_lsp_subject_refresh::probe_table::FILE_DOC,
                        head.method
                    ),
                );
            } else {
                classifier.fire(
                    DriftClass::VimHostFloorOrRequiredFeatureChanged,
                    &[PROBE_HEAD],
                    format!(
                        "theoretical floor observed vim {} / neovim {} != recorded vim {} / neovim {}",
                        floor.vim_minimum.as_deref().unwrap_or("<missing>"),
                        floor.neovim_minimum.as_deref().unwrap_or("<missing>"),
                        classifier.pinned.vim_theoretical_minimum,
                        classifier.pinned.neovim_theoretical_minimum
                    ),
                );
                classifier.proposals.push(ProposedSubjectFieldChange {
                    field: "upstream_theoretical_prerequisites.rows.theoretical_minimum"
                        .to_string(),
                    current: format!(
                        "vim {} / neovim {}",
                        classifier.pinned.vim_theoretical_minimum,
                        classifier.pinned.neovim_theoretical_minimum
                    ),
                    observed: format!(
                        "vim {} / neovim {}",
                        floor.vim_minimum.as_deref().unwrap_or("<missing>"),
                        floor.neovim_minimum.as_deref().unwrap_or("<missing>")
                    ),
                });
            }
        }
        (Some(_), true) => {
            // The doc exists but the floor sentence no longer parses: the
            // recorded prerequisite authority is unverifiable.
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD],
                format!(
                    "floor sentence could not be re-parsed from {} via {}",
                    crate::vim_lsp_subject_refresh::probe_table::FILE_DOC,
                    head.method
                ),
            );
        }
        (_, false) => {
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD],
                format!(
                    "prerequisite authority {} absent from the observed head tree",
                    crate::vim_lsp_subject_refresh::probe_table::FILE_DOC
                ),
            );
        }
        (None, true) => {
            // The doc exists but the floor observation is absent: the
            // validator rejects this shape, and classification still fails
            // closed rather than assuming an unchanged floor.
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD],
                "floor observation missing while the prerequisite doc exists".to_string(),
            );
        }
    }

    // Plugin feature-gate defaults: required-feature metadata.
    for (name, expected) in EXPECTED_PLUGIN_DEFAULTS {
        match head.plugin_defaults.get(*name) {
            Some(observed) if observed == expected => {}
            Some(observed) => {
                classifier.fire(
                    DriftClass::VimHostFloorOrRequiredFeatureChanged,
                    &[PROBE_HEAD],
                    format!("plugin default {name} changed: {observed} != {expected}"),
                );
            }
            None if plugin_present => {
                classifier.fire(
                    DriftClass::VimHostFloorOrRequiredFeatureChanged,
                    &[PROBE_HEAD],
                    format!("plugin feature gate {name} absent from the observed plugin entry"),
                );
            }
            None => {}
        }
    }
    if plugin_present
        && EXPECTED_PLUGIN_DEFAULTS
            .iter()
            .all(|(name, expected)| head.plugin_defaults.get(*name) == Some(&expected.to_string()))
    {
        classifier.positive(
            PROBE_HEAD,
            format!(
                "all {} recorded plugin feature-gate defaults unchanged via {}",
                EXPECTED_PLUGIN_DEFAULTS.len(),
                head.method
            ),
        );
    }

    // Maintenance markers (closed vocabulary).
    if head.maintenance_markers.is_empty() {
        let readme_present = file_present(crate::vim_lsp_subject_refresh::probe_table::FILE_README);
        if readme_present {
            classifier.positive(
                PROBE_HEAD,
                format!(
                    "no maintenance-state marker from the closed vocabulary found in {} via {}",
                    crate::vim_lsp_subject_refresh::probe_table::FILE_README,
                    head.method
                ),
            );
        } else {
            classifier.fire(
                DriftClass::UnknownOrConflictingAuthority,
                &[PROBE_HEAD],
                format!(
                    "capability/maintenance authority {} absent from the observed head tree",
                    crate::vim_lsp_subject_refresh::probe_table::FILE_README
                ),
            );
        }
    } else {
        classifier.fire(
            DriftClass::MaintenanceStateChanged,
            &[PROBE_HEAD],
            format!(
                "maintenance markers observed in {}: {}",
                crate::vim_lsp_subject_refresh::probe_table::FILE_README,
                head.maintenance_markers.join(", ")
            ),
        );
    }

    // Recorded capability note (snippets not supported by default).
    match head.snippet_note_present {
        Some(true) => classifier.positive(
            PROBE_HEAD,
            format!(
                "recorded capability note still present in {} via {}",
                crate::vim_lsp_subject_refresh::probe_table::FILE_README,
                head.method
            ),
        ),
        Some(false) => classifier.fire(
            DriftClass::UnknownOrConflictingAuthority,
            &[PROBE_HEAD],
            "recorded snippet capability note is no longer verifiable upstream".to_string(),
        ),
        None => {
            // README absent is already classified above; nothing to add.
        }
    }

    // Public-surface needles.
    let mut missing: Vec<&SurfaceProbe> = Vec::new();
    for probe in SURFACE_PROBES {
        let found = packet_finding(classifier.packet, probe);
        if found {
            continue;
        }
        missing.push(probe);
    }
    if missing.is_empty() {
        classifier.positive(
            PROBE_HEAD,
            format!(
                "all {} public-surface needles still present in the observed head tree via {}",
                SURFACE_PROBES.len(),
                head.method
            ),
        );
    } else {
        let mut api_removed = false;
        for probe in &missing {
            classifier.fire(
                probe.class_if_absent,
                &[PROBE_HEAD],
                format!("needle {:?} absent from {} upstream", probe.needle, probe.file),
            );
            api_removed = true;
        }
        if api_removed {
            classifier.fire(
                DriftClass::SelectedPublicApiDeprecatedOrRemoved,
                &[PROBE_HEAD],
                format!(
                    "{} consumed public API signature(s) absent from the observed upstream tree",
                    missing.len()
                ),
            );
        }
    }
}

fn packet_finding(packet: &ObservationPacket, probe: &SurfaceProbe) -> bool {
    packet.head_tree_probe.surface_findings.iter().any(|finding| {
        finding.surface == probe.surface
            && finding.file == probe.file
            && finding.needle == probe.needle
            && finding.found
    })
}

fn collect_impacts(classes: &[ClassifiedDrift]) -> Vec<ImpactEntry> {
    let mut impacted: Vec<ImpactEntry> = Vec::new();
    let mut seen: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    for entry in classes {
        for impact in impact_entries(entry.class) {
            if seen.insert((impact.item.clone(), impact.action.clone())) {
                impacted.push(impact);
            }
        }
    }
    impacted
}
