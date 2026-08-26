//! Read-only upstream refresh and drift classification for the pinned
//! `prabirshrestha/vim-lsp` subject (#11411).
//!
//! Ownership split — consumed, never duplicated:
//!
//! - `.ci/editor-clients/vim-vim-lsp-subject.v1.json` (#11369) owns the
//!   pinned subject identity and stays the offline authority; this module
//!   never mutates it.
//! - `.ci/editor-clients/vim-vim-lsp-public-surface.v1.json` (#11369) owns
//!   the consumed public-surface inventory; [`probe_table`] binds every
//!   inventory surface to bounded textual needles and contract tests reject
//!   any drift between table and inventory.
//! - [`classify`] owns the deterministic drift classes, the narrow
//!   currentness impact graph, and the advisory disposition vocabulary.
//! - [`observe`] owns the network-assisted observation packet. Live
//!   observation is gated behind an explicit `--allow-network` flag; the
//!   default path (tests, CI, retained-packet classification) is offline.
//!
//! Fail-closed laws:
//!
//! - a failed probe classifies `instrument_failed`, never "no drift";
//! - unverifiable or contradictory authority classifies
//!   `unknown_or_conflicting_authority`, never a convenient answer;
//! - every classification carries its evidence probe and observation time;
//! - zero drift is reported as explicit positive findings, never as absent
//!   output;
//! - the artifact is bounded: digests, ref names, needle booleans, and
//!   closed vocabularies only — no raw upstream content;
//! - proposals are review-only: the writer refuses paths inside `.ci/`, and
//!   nothing in this module updates the pin, reruns hosts, touches support,
//!   or mutates GitHub/upstream.

pub mod classify;
pub mod model;
pub mod observe;
pub mod probe_table;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result, bail, ensure};
use chrono::DateTime;

use crate::vim_lsp_subject_refresh::classify::{PinnedSubject, classify};
use crate::vim_lsp_subject_refresh::model::{
    DriftClass, ObservationPacket, ProbeStatus, REFRESH_SCHEMA_VERSION, RefreshArtifact,
};
use crate::vim_lsp_subject_refresh::observe::observe;
use crate::vim_lsp_subject_refresh::probe_table::{
    MAINTENANCE_MARKERS, SURFACE_PROBES, validate_table_against_inventory,
};

/// Landed authority fixtures this module consumes read-only.
pub const SUBJECT_MANIFEST_PATH: &str = ".ci/editor-clients/vim-vim-lsp-subject.v1.json";
pub const PUBLIC_SURFACE_INVENTORY_PATH: &str =
    ".ci/editor-clients/vim-vim-lsp-public-surface.v1.json";

/// CLI surface for `cargo xtask vim-lsp-subject refresh` (#11411).
#[derive(Debug, Clone)]
pub struct RefreshOptions {
    /// Print the drift report without writing an artifact.
    pub check: bool,
    /// Write the bounded review artifact to this repository-local path.
    pub proposal: Option<std::path::PathBuf>,
    /// Classify a retained observation packet (offline) instead of probing
    /// the network.
    pub observation: Option<std::path::PathBuf>,
    /// Explicit gate for live network observation.
    pub allow_network: bool,
    /// Repository root used to resolve the landed authorities.
    pub repo_root: std::path::PathBuf,
}

/// Outcome of one refresh run: the artifact plus whether the instrument
/// failed (callers report instrument failure distinctly from drift).
#[derive(Debug, Clone)]
pub struct RefreshOutcome {
    pub artifact: RefreshArtifact,
    pub instrument_failed: bool,
}

/// Run the #11411 refresh: load the landed authorities, obtain an
/// observation packet (retained, or live behind the network gate), classify
/// drift, and optionally write the bounded proposal artifact.
pub fn run(options: RefreshOptions) -> Result<RefreshOutcome> {
    if !options.check && options.proposal.is_none() {
        bail!(
            "pass --check to print the drift report, or --proposal <path> to also write the artifact"
        );
    }
    let manifest = read_json(&options.repo_root.join(SUBJECT_MANIFEST_PATH))?;
    let inventory = read_json(&options.repo_root.join(PUBLIC_SURFACE_INVENTORY_PATH))?;
    validate_table_against_inventory(&inventory)
        .context("probe table drifted from the landed inventory")?;
    let pinned = PinnedSubject::from_manifest(&manifest)?;

    let packet = match &options.observation {
        Some(path) => {
            let packet: ObservationPacket = serde_json::from_slice(
                &std::fs::read(path)
                    .with_context(|| format!("reading observation packet {}", path.display()))?,
            )
            .with_context(|| format!("parsing observation packet {}", path.display()))?;
            packet
        }
        None => observe(&pinned.repository, &pinned, options.allow_network)?,
    };
    validate_packet(&packet, &pinned)
        .with_context(|| "observation packet failed boundedness/consistency validation")?;
    let artifact = classify(&packet, &pinned)?;
    let instrument_failed =
        artifact.drift_classes.iter().any(|entry| entry.class == DriftClass::InstrumentFailed);

    if let Some(path) = &options.proposal {
        write_proposal(&options.repo_root, path, &artifact)?;
    }

    print_summary(&artifact, options.proposal.as_ref());
    Ok(RefreshOutcome { artifact, instrument_failed })
}

fn read_json(path: &std::path::Path) -> Result<serde_json::Value> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

/// Write the bounded proposal artifact. The writer refuses any path inside
/// `.ci/` so a proposal can never silently become pin authority.
pub fn write_proposal(
    repo_root: &std::path::Path,
    path: &std::path::Path,
    artifact: &RefreshArtifact,
) -> Result<()> {
    for component in path.components() {
        if let std::path::Component::Normal(value) = component {
            ensure!(
                value != ".ci",
                "proposal path {} would write into the retained authority tree; proposals are review-only",
                path.display()
            );
        }
    }
    let absolute = if path.is_absolute() { path.to_path_buf() } else { repo_root.join(path) };
    let subject = repo_root.join(SUBJECT_MANIFEST_PATH);
    let surface = repo_root.join(PUBLIC_SURFACE_INVENTORY_PATH);
    ensure!(
        absolute != subject && absolute != surface,
        "proposal path {} would overwrite a landed authority fixture",
        path.display()
    );
    // Repository-local boundary, resolved: the canonicalized parent must sit
    // inside the canonical repository root and outside the authority tree,
    // so neither an absolute path elsewhere on the host nor a symlinked
    // parent can redirect the write.
    if let Some(parent) = absolute.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating proposal parent {}", parent.display()))?;
        let canonical_root = std::fs::canonicalize(repo_root)
            .with_context(|| format!("canonicalizing repository root {}", repo_root.display()))?;
        let canonical_parent = std::fs::canonicalize(parent)
            .with_context(|| format!("canonicalizing proposal parent {}", parent.display()))?;
        ensure!(
            canonical_parent.starts_with(&canonical_root),
            "proposal path {} resolves outside the repository; proposals are repository-local only",
            path.display()
        );
        for component in canonical_parent.components() {
            if let std::path::Component::Normal(value) = component {
                ensure!(
                    value != ".ci",
                    "proposal path {} resolves into the retained authority tree; proposals are review-only",
                    path.display()
                );
            }
        }
    }
    // Refuse to follow an existing symlink at the write target.
    if let Ok(metadata) = std::fs::symlink_metadata(&absolute) {
        ensure!(
            metadata.is_file(),
            "proposal target {} exists and is not a regular file",
            path.display()
        );
    }
    let rendered =
        serde_json::to_string_pretty(artifact).context("serializing the refresh artifact")?;
    std::fs::write(&absolute, format!("{rendered}\n"))
        .with_context(|| format!("writing proposal artifact {}", absolute.display()))?;
    Ok(())
}

fn print_summary(artifact: &RefreshArtifact, proposal: Option<&std::path::PathBuf>) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let report = |handle: &mut std::io::StdoutLock| {
        let mut render = |line: String| writeln!(handle, "{line}");
        render(format!(
            "vim-lsp subject refresh ({REFRESH_SCHEMA_VERSION}) observed {}",
            artifact.observed_at_utc
        ))?;
        render(format!(
            "pinned subject: {} @ {}",
            artifact.selected_subject_before.repository,
            artifact.selected_subject_before.selected_commit
        ))?;
        for entry in &artifact.drift_classes {
            render(format!(
                "drift: {} — {} (evidence: {})",
                entry.class.token(),
                entry.detail,
                entry.evidence_probe_ids.join(", ")
            ))?;
        }
        render(format!("positive findings: {}", artifact.positive_findings.len()))?;
        for finding in &artifact.positive_findings {
            render(format!(
                "checked: [{}] {} — {}",
                finding.probe_id, finding.check, finding.result
            ))?;
        }
        render(format!("disposition (advisory): {}", artifact.recommended_disposition.join(", ")))?;
        if let Some(path) = proposal {
            render(format!("proposal artifact written: {}", path.display()))?;
        }
        render(
            "boundary: advisory only; no pin, support, or evidence state was changed".to_string(),
        )?;
        Ok::<(), std::io::Error>(())
    };
    if let Err(error) = report(&mut handle) {
        // A summary print failure must never turn a classified observation
        // into a false failure; the artifact itself remains authoritative.
        let _ = writeln!(handle, "summary print failed: {error}");
    }
}

// ---------------------------------------------------------------------------
// Packet validation: boundedness + internal consistency, offline and pure
// ---------------------------------------------------------------------------

fn is_sha(value: &str) -> bool {
    (40..=64).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_bounded(value: &str, cap: usize) -> bool {
    value.chars().count() <= cap
}

fn is_relative_path(value: &str) -> bool {
    !value.is_empty()
        && is_bounded(value, 200)
        && !value.contains('\\')
        && !value.contains("..")
        && std::path::Path::new(value).is_relative()
}

fn is_rfc3339(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value).is_ok()
}

fn is_version(value: &str) -> bool {
    !value.is_empty()
        && is_bounded(value, 32)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
}

fn is_tag(value: &str) -> bool {
    let Some(body) = value.strip_prefix('v') else { return false };
    !body.is_empty()
        && is_bounded(value, 64)
        && body.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Validate the observation packet: hard caps, closed vocabularies, sha
/// shapes, and internal consistency (probe-commit attribution). Offline,
/// pure, fail-closed.
pub fn validate_packet(packet: &ObservationPacket, pinned: &PinnedSubject) -> Result<()> {
    ensure!(
        packet.schema_version == model::OBSERVATION_SCHEMA_VERSION,
        "unexpected observation schema {} (expected {})",
        packet.schema_version,
        model::OBSERVATION_SCHEMA_VERSION
    );
    ensure!(is_rfc3339(&packet.observed_at_utc), "observed_at_utc is not RFC3339");
    ensure!(
        packet.upstream_repository == pinned.repository,
        "packet repository {} != pinned repository {}",
        packet.upstream_repository,
        pinned.repository
    );

    // Refs probe.
    ensure!(is_bounded(&packet.refs_probe.method, 300), "refs method exceeds cap");
    match packet.refs_probe.status {
        ProbeStatus::Ok => {
            // HEAD is required; refs/heads/master is best-effort because an
            // upstream default-branch rename removes it, and that removal is
            // itself drift the classifier must see, not an invalid packet.
            ensure!(
                packet.refs_probe.head.as_deref().is_some_and(is_sha),
                "refs head missing or malformed"
            );
            if let Some(master) = packet.refs_probe.master.as_deref() {
                ensure!(is_sha(master), "refs master is malformed");
            }
        }
        ProbeStatus::Failed => {
            ensure!(
                packet.refs_probe.error.as_deref().is_some_and(|error| is_bounded(error, 300)),
                "failed refs probe lacks a bounded error"
            );
        }
    }
    ensure!(packet.refs_probe.tags.len() <= 200, "tag list exceeds cap");
    ensure!(packet.refs_probe.tags.iter().all(|tag| is_tag(tag)), "malformed tag name");

    // Pinned-commit probe.
    let pinned_probe = &packet.pinned_commit_probe;
    ensure!(is_bounded(&pinned_probe.method, 300), "pinned method exceeds cap");
    ensure!(is_sha(&pinned_probe.requested_commit), "requested commit is not a sha");
    ensure!(
        pinned_probe.requested_commit == pinned.selected_commit,
        "packet requested {} but the pin selects {}",
        pinned_probe.requested_commit,
        pinned.selected_commit
    );
    match pinned_probe.status {
        ProbeStatus::Ok => {
            ensure!(
                pinned_probe.resolved_commit.as_deref().is_some_and(is_sha),
                "resolved commit missing or malformed"
            );
            ensure!(
                pinned_probe.resolved_tree.as_deref().is_some_and(is_sha),
                "resolved tree missing or malformed"
            );
            if let Some(subject) = &pinned_probe.commit_subject {
                ensure!(is_bounded(subject, 200), "commit subject exceeds cap");
            }
            if let Some(date) = &pinned_probe.commit_author_date {
                ensure!(is_rfc3339(date), "commit author date is not RFC3339");
            }
        }
        ProbeStatus::Failed => {
            ensure!(
                pinned_probe.error.as_deref().is_some_and(|error| is_bounded(error, 300)),
                "failed pinned probe lacks a bounded error"
            );
        }
    }
    // Entry-set equality is enforceable only for a successful fetch; a
    // failed pinned probe must carry no partial blobs so the classifier can
    // honestly classify the unreachable-ref class.
    let expected_entries: std::collections::BTreeSet<&str> =
        pinned.entry_files.iter().map(|(path, _)| path.as_str()).collect();
    let observed_entries: std::collections::BTreeSet<&str> =
        pinned_probe.entry_file_blobs.iter().map(|file| file.path.as_str()).collect();
    match pinned_probe.status {
        ProbeStatus::Ok => ensure!(
            expected_entries == observed_entries,
            "pinned probe entry files do not match the manifest entry set"
        ),
        ProbeStatus::Failed => ensure!(
            observed_entries.is_empty(),
            "a failed pinned probe must not carry partial entry-file observations"
        ),
    }
    for file in &pinned_probe.entry_file_blobs {
        ensure!(is_relative_path(&file.path), "entry path {} is not bounded-relative", file.path);
        let commit =
            pinned_probe.resolved_commit.as_deref().unwrap_or(&pinned_probe.requested_commit);
        ensure!(file.commit == commit, "entry file {} attributed to the wrong commit", file.path);
        if let Some(blob) = &file.git_blob_sha1 {
            ensure!(is_sha(blob), "entry blob for {} is not a sha", file.path);
        }
    }

    // Head-tree probe.
    let head = &packet.head_tree_probe;
    ensure!(is_bounded(&head.method, 300), "head method exceeds cap");
    if head.status == ProbeStatus::Failed {
        ensure!(
            head.error.as_deref().is_some_and(|error| is_bounded(error, 300)),
            "failed head probe lacks a bounded error"
        );
    }
    if let Some(commit) = &head.commit {
        ensure!(is_sha(commit), "head commit is not a sha");
    }
    match head.status {
        ProbeStatus::Ok => {
            ensure!(head.commit.is_some(), "ok head probe lacks a commit");
        }
        ProbeStatus::Failed => {}
    }
    ensure!(head.files.len() <= 128, "head file list exceeds cap");
    for file in &head.files {
        ensure!(is_relative_path(&file.path), "head path {} is not bounded-relative", file.path);
        if let Some(blob) = &file.git_blob_sha1 {
            ensure!(is_sha(blob), "head blob for {} is not a sha", file.path);
        }
        if let (Some(file_commit), Some(probe_commit)) = (Some(&file.commit), head.commit.as_ref())
        {
            ensure!(
                file_commit == probe_commit,
                "head file {} attributed to {} but the probe read {}; another ref's facts cannot be applied to the observed subject",
                file.path,
                file_commit,
                probe_commit
            );
        }
    }
    if head.status == ProbeStatus::Ok {
        let required: std::collections::BTreeSet<String> = {
            let mut paths: Vec<String> =
                SURFACE_PROBES.iter().map(|probe| probe.file.to_string()).collect();
            paths.extend(pinned.entry_files.iter().map(|(path, _)| path.clone()));
            paths.push(probe_table::FILE_DOC.to_string());
            paths.push(probe_table::FILE_README.to_string());
            paths.sort();
            paths.dedup();
            paths.into_iter().collect()
        };
        let observed: std::collections::BTreeSet<&str> =
            head.files.iter().map(|file| file.path.as_str()).collect();
        for path in &required {
            ensure!(
                observed.contains(path.as_str()),
                "ok head probe did not observe required path {path}"
            );
        }
    }
    ensure!(
        head.plugin_defaults.len() <= EXPECTED_DEFAULT_COUNT,
        "plugin defaults exceed the expected gate set"
    );
    let allowed_defaults: std::collections::BTreeSet<&str> =
        probe_table::EXPECTED_PLUGIN_DEFAULTS.iter().map(|(name, _)| *name).collect();
    for (name, value) in &head.plugin_defaults {
        ensure!(allowed_defaults.contains(name.as_str()), "unexpected plugin default {name}");
        ensure!(is_bounded(value, 200), "plugin default {name} exceeds cap");
    }
    ensure!(
        head.maintenance_markers
            .iter()
            .all(|marker| MAINTENANCE_MARKERS.contains(&marker.as_str())),
        "maintenance marker outside the closed vocabulary"
    );
    if let Some(floor) = &head.floor {
        if let Some(value) = &floor.vim_minimum {
            ensure!(is_version(value), "vim floor {value} is not a bounded version");
        }
        if let Some(value) = &floor.neovim_minimum {
            ensure!(is_version(value), "neovim floor {value} is not a bounded version");
        }
    }
    // Surface findings must cover the probe table exactly, but only a
    // successful head probe can carry them: a failed probe carries none so
    // the classifier can emit instrument_failed instead of a rejected packet.
    let expected_findings: std::collections::BTreeSet<(String, String, String)> = SURFACE_PROBES
        .iter()
        .map(|probe| (probe.surface.to_string(), probe.file.to_string(), probe.needle.to_string()))
        .collect();
    let observed_findings: std::collections::BTreeSet<(String, String, String)> = head
        .surface_findings
        .iter()
        .map(|finding| (finding.surface.clone(), finding.file.clone(), finding.needle.clone()))
        .collect();
    match head.status {
        ProbeStatus::Ok => ensure!(
            expected_findings == observed_findings,
            "surface findings do not match the probe table exactly ({} expected, {} observed)",
            expected_findings.len(),
            observed_findings.len()
        ),
        ProbeStatus::Failed => ensure!(
            observed_findings.is_empty(),
            "a failed head probe must not carry partial surface findings"
        ),
    }
    Ok(())
}

const EXPECTED_DEFAULT_COUNT: usize = 3;

/// Re-parse and re-validate an artifact (used by tests to prove the writer
/// round-trips a bounded artifact).
pub fn validate_artifact_boundedness(artifact: &RefreshArtifact) -> Result<()> {
    ensure!(artifact.schema_version == REFRESH_SCHEMA_VERSION, "artifact schema mismatch");
    ensure!(is_rfc3339(&artifact.observed_at_utc), "artifact observed_at_utc is not RFC3339");
    ensure!(artifact.drift_classes.len() <= DriftClass::ALL.len(), "impossible drift class count");
    for entry in &artifact.drift_classes {
        ensure!(is_bounded(&entry.detail, 400), "drift detail exceeds cap");
        ensure!(DriftClass::ALL.contains(&entry.class), "unknown drift class");
    }
    for finding in &artifact.positive_findings {
        ensure!(is_bounded(&finding.check, 300), "positive finding exceeds cap");
    }
    for impact in &artifact.impacted_evidence {
        ensure!(is_bounded(&impact.item, 200), "impact item exceeds cap");
        ensure!(is_bounded(&impact.reason, 300), "impact reason exceeds cap");
    }
    for proposal in &artifact.proposed_subject_field_changes {
        ensure!(is_bounded(&proposal.field, 200), "proposal field exceeds cap");
        ensure!(is_bounded(&proposal.current, 300), "proposal current value exceeds cap");
        ensure!(is_bounded(&proposal.observed, 300), "proposal observed value exceeds cap");
    }
    Ok(())
}
