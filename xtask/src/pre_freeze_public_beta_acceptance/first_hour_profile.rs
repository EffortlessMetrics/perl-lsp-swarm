//! First-hour execution profile index over the canonical v2 acceptance bundle.
//!
//! This module is a bounded, non-authoritative index. It never authenticates
//! receipts, never qualifies installed execution, and never replaces
//! [`super::validate_v2`]. The only public entrypoint takes the typed
//! [`super::PacketV2`] plus [`super::TopologyRequirements`] and calls
//! [`super::validate_v2`] itself; a stale or hand-built report cannot be
//! substituted.
//!
//! Profile windows (`first_5_minutes`, `first_15_minutes`, `first_60_minutes`)
//! are observation windows, not indexing latency promises. They index existing
//! canonical [`super::CELLS`] per existing [`super::ROWS`]; every one of the 24
//! cells on each of the 3 host rows remains governed by `validate_v2`.
//!
//! Genuine new-human / fresh-agent first-hour observation currently has no
//! concrete producer. Missing adapters are explanatory checklist strings, never
//! fabricated receipt rows: no locator, digest, owner identity, or adapter
//! category is invented. Every window stays
//! [`super::InstalledQualification::NotProven`]. This index invents no parallel
//! cell IDs, no receipt schema, no status model, and no acceptance denominator.
//!
//! Platform basis: #13768 Gate 3 activates #4346's controller-expansion
//! exception. `windows_x64_current_stable` retains the full retained journey
//! at the recorded host; both Linux rows are retained. No new Windows
//! two-version matrix and no macOS semantic parity are introduced here. See
//! #6056 adjudication
//! <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056#issuecomment-5750256278>
//! and Friday platform reconciliation at
//! <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768#issuecomment-5925014719>.
//! Missing Windows producers/mappings stay `not_proven`.

use super::{
    Artifact, ArtifactSelection, CELLS, EvidenceRef, EvidenceRequirement, Fixture,
    InstalledQualification, PacketV2, ROWS, Recommendation, SCHEMA, Status, Subject,
    TopologyRequirements, ValidationReport,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Profile checkpoint labels. These index canonical cells; they are not
/// canonical cell IDs and must never be substituted for them.
pub const WINDOW_FIRST_5: &str = "first_5_minutes";
/// Profile checkpoint labels. These index canonical cells; they are not
/// canonical cell IDs and must never be substituted for them.
pub const WINDOW_FIRST_15: &str = "first_15_minutes";
/// Profile checkpoint labels. These index canonical cells; they are not
/// canonical cell IDs and must never be substituted for them.
pub const WINDOW_FIRST_60: &str = "first_60_minutes";
/// Fixed window order for deterministic output.
pub const PROFILE_WINDOWS: [&str; 3] = [WINDOW_FIRST_5, WINDOW_FIRST_15, WINDOW_FIRST_60];

const PROFILE_DOCUMENT: &str = include_str!("first_hour_profile.md");
const PROFILE_DOCUMENT_PATH: &str =
    "xtask/src/pre_freeze_public_beta_acceptance/first_hour_profile.md";

/// Canonical cells observed by minute 5: install, boot, first useful answer.
const FIRST_5_CELLS: [&str; 6] = [
    "install_upgrade_identity",
    "startup_readiness",
    "diagnostics",
    "completion",
    "definition",
    "result_state_distinctions",
];
/// Canonical cells observed by minute 15: ordinary change and correct recovery.
const FIRST_15_CELLS: [&str; 11] = [
    "hover",
    "references",
    "document_symbols",
    "workspace_symbols",
    "unicode_crlf_edit_save_requery",
    "file_create_rename_delete",
    "safe_rename_or_refusal",
    "whole_document_formatting",
    "native_only_critic",
    "doctor_optional_tools",
    "diagnosis_and_identity_repair",
];
/// Canonical cells observed by minute 60: sustained ordinary use.
/// Retains multi-root, DAP preview, and test-entry cells; nothing is erased.
const FIRST_60_CELLS: [&str; 7] = [
    "workspace_multiroot",
    "retained_code_action",
    "dap_preview",
    "retained_test_entry",
    "installed_contribution_reachability",
    "restart_recovery_rollback",
    "shutdown_cleanup",
];

/// Distinct first-hour observers. A guided expert run is neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileObserver {
    NewHuman,
    FreshAgent,
}

/// In-memory, non-authoritative observer facts. This type has no serde
/// deserialization: it is never a persisted receipt format and never a
/// caller-verified authority. It exists only so tests and the CLI can state
/// explicitly what was (or was not) observed in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserverFact {
    pub observer: ProfileObserver,
    /// Actual wall-clock seconds observed for the 60-minute window.
    pub observed_60min_seconds: u64,
    /// True when this run was a guided expert transcript.
    pub guided_expert: bool,
    /// True when the observer inspected checkout sources.
    pub checkout_inspection: bool,
    /// True when a private repository rescue supplied an answer.
    pub private_rescue: bool,
    /// True for hidden flags, prior transcripts, or other assistance.
    pub hidden_assistance: bool,
    /// Interventions recorded with the unmet public instruction.
    pub recorded_interventions: u64,
    /// In-memory intervention reasons naming the assistance and the unmet
    /// public instruction. Required when any assistance flag is set; a bare
    /// count alone conceals the requested details.
    pub intervention_details: Vec<String>,
    /// True for accelerated validator self-tests; never genuine observation.
    pub synthetic_mechanism_only: bool,
}

impl ObserverFact {
    /// Explicitly unobserved in-memory facts for mechanism indexing.
    pub fn unobserved(observer: ProfileObserver) -> Self {
        Self {
            observer,
            observed_60min_seconds: 0,
            guided_expert: false,
            checkout_inspection: false,
            private_rescue: false,
            hidden_assistance: false,
            recorded_interventions: 0,
            intervention_details: Vec::new(),
            synthetic_mechanism_only: true,
        }
    }

    fn check(&self, expected: ProfileObserver) -> Result<()> {
        ensure!(self.observer == expected, "observer cross-fill: expected {expected:?}");
        ensure!(!self.guided_expert, "guided expert cannot fill {expected:?}");
        if self.checkout_inspection || self.private_rescue || self.hidden_assistance {
            ensure!(
                self.recorded_interventions > 0,
                "assistance without recorded intervention cannot fill {expected:?}"
            );
            ensure!(
                !self.intervention_details.is_empty(),
                "assistance without intervention details cannot fill {expected:?}"
            );
            for detail in &self.intervention_details {
                ensure!(
                    !detail.trim().is_empty(),
                    "empty intervention detail cannot fill {expected:?}"
                );
            }
        }
        Ok(())
    }
}

/// In-memory pair of distinct observer facts. Never persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstHourFacts {
    pub human: ObserverFact,
    pub agent: ObserverFact,
}

impl FirstHourFacts {
    /// Explicitly unobserved facts: the only honest default until concrete
    /// observation producers exist.
    pub fn unobserved() -> Self {
        Self {
            human: ObserverFact::unobserved(ProfileObserver::NewHuman),
            agent: ObserverFact::unobserved(ProfileObserver::FreshAgent),
        }
    }

    fn check(&self) -> Result<()> {
        self.human.check(ProfileObserver::NewHuman)?;
        self.agent.check(ProfileObserver::FreshAgent)?;
        Ok(())
    }

    fn for_observer(&self, observer: ProfileObserver) -> &ObserverFact {
        match observer {
            ProfileObserver::NewHuman => &self.human,
            ProfileObserver::FreshAgent => &self.agent,
        }
    }
}

/// Canonical cells indexed by one profile window.
pub fn window_cells(window: &str) -> Result<&'static [&'static str]> {
    match window {
        WINDOW_FIRST_5 => Ok(&FIRST_5_CELLS),
        WINDOW_FIRST_15 => Ok(&FIRST_15_CELLS),
        WINDOW_FIRST_60 => Ok(&FIRST_60_CELLS),
        _ => anyhow::bail!("unknown profile window {window}"),
    }
}

/// One observer's summary of in-memory observation facts. Informational only.
#[derive(Debug, Clone, Serialize)]
pub struct ObserverSummary {
    pub observer: ProfileObserver,
    pub observed_60min_seconds: u64,
    pub synthetic_mechanism_only: bool,
    pub recorded_interventions: u64,
    pub checkout_inspection: bool,
    pub private_rescue: bool,
    pub hidden_assistance: bool,
    pub intervention_details: Vec<String>,
}

/// One window/observer slice of the canonical evidence index.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileWindowIndex {
    pub window: String,
    pub observer: ProfileObserver,
    /// Reuses the canonical status vocabulary. Always `not_proven` until a
    /// concrete first-hour observation producer exists.
    pub status: Status,
    pub reason: Option<String>,
    /// Canonical cell IDs indexed in this window, in canonical order.
    pub canonical_cells: Vec<String>,
    /// Selector view into the retained canonical report's cell obligations
    /// for those cells. All other canonical obligations remain in
    /// [`FirstHourIndex::canonical_report`].
    pub evidence: Vec<EvidenceRequirement>,
    /// Explanatory missing-producer checklist. Plain strings only; never
    /// receipt rows, locators, digests, or adapter mappings.
    pub missing: Vec<String>,
}

/// Declared canonical row context. These values identify the supplied bundle;
/// they do not authenticate a host, fixture, generation, or installed run.
#[derive(Debug, Clone, Serialize)]
pub struct DeclaredRowContext {
    pub row_id: String,
    pub platform: String,
    pub architecture: String,
    pub host_role: String,
    pub vscode_version: String,
    pub host_selection: EvidenceRef,
    pub clean_profile_id: String,
    pub configuration_identity: String,
    pub fixtures: Vec<Fixture>,
    pub artifacts: ArtifactSelection,
    pub subject: Subject,
}

/// Deterministic index beside the retained canonical report. Serialize-only:
/// this is never parsed as a new wire protocol.
#[derive(Debug, Serialize)]
pub struct FirstHourIndex {
    pub index_kind: String,
    pub canonical_schema: String,
    /// Content identity of the LF-normalized profile document and ordered
    /// observer/window/cell partition, independent of the product subject.
    pub profile_digest: String,
    pub profile_document: String,
    pub subject: Subject,
    pub phase: String,
    pub source_version: String,
    pub target_release: String,
    /// Exactly [`super::ROWS`], in canonical order.
    pub canonical_rows: Vec<String>,
    /// Exactly [`super::CELLS`], in canonical order.
    pub canonical_cells: Vec<String>,
    /// Row ID to sorted fixture IDs, as declared by the canonical packet.
    /// Declared IDs do not prove conventional/dynamic execution.
    pub row_fixtures: BTreeMap<String, Vec<String>>,
    pub declared_rows: Vec<DeclaredRowContext>,
    pub declared_artifacts: Vec<Artifact>,
    pub observers: Vec<ObserverSummary>,
    pub windows: Vec<ProfileWindowIndex>,
    /// The complete actual canonical report, retained without replacement.
    pub canonical_report: ValidationReport,
    /// Exactly `canonical_report.bundle_recommendation`; never recomputed here.
    pub bundle_recommendation: Recommendation,
    /// Exactly `canonical_report.installed_qualification`; always `not_proven`.
    pub installed_qualification: InstalledQualification,
    pub claim_boundary: String,
}

fn content_digest(document: &str, partition: &[(&str, &[&str])]) -> Result<String> {
    let definition = serde_json::to_vec(&(
        "first_hour_profile_index",
        document.replace("\r\n", "\n"),
        [ProfileObserver::NewHuman, ProfileObserver::FreshAgent],
        partition,
    ))?;
    Ok(format!("sha256:{:x}", Sha256::digest(definition)))
}

fn profile_digest() -> Result<String> {
    let partition = PROFILE_WINDOWS
        .into_iter()
        .map(|window| Ok((window, window_cells(window)?)))
        .collect::<Result<Vec<_>>>()?;
    content_digest(PROFILE_DOCUMENT, &partition)
}

fn missing_checklist() -> Vec<String> {
    [
        "public instruction entrypoint/version/content digest not bound to an observed run",
        "actual new-human/fresh-agent identity and allowed environment not recorded by a producer",
        "installed server/DAP/VSIX archive bytes and release-shaped provenance not authenticated",
        "real host identity and clean-profile/config generation not bound to this observation",
        "fixture source/config/trust/root/document/session generations not observed (canonical IDs, content digests and row context are declarations)",
        "verified 5/15/60 start/end/elapsed windows and first-useful/first-correct timings not recorded",
        "conventional AND dynamic fixture execution not proven (declared IDs do not prove execution)",
        "safe edit/recovery/restart/update/cleanup outcomes not observed through a genuine producer",
        "sustained request/edit/cancel/indexing overlap and clean shutdown vs forced kill not observed",
        "assistance/intervention details and unmet public instruction not satisfied by a count alone",
        "no concrete first-hour observation producer; window stays NOT_PROVEN",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn window_reason(observer_name: &str, window: &str, fact: &ObserverFact) -> String {
    let mut reason = if fact.synthetic_mechanism_only {
        format!(
            "genuine {observer_name} {window} observation not established \
             (observed {}s; synthetic mechanism self-test only); \
             no concrete observation producer; installed qualification not_proven",
            fact.observed_60min_seconds
        )
    } else {
        format!(
            "genuine {observer_name} {window} observation not established \
             (in-memory {}s; no concrete observation producer); \
             installed qualification not_proven",
            fact.observed_60min_seconds
        )
    };
    if fact.checkout_inspection || fact.private_rescue || fact.hidden_assistance {
        reason.push_str("; assistance recorded: ");
        reason.push_str(&fact.intervention_details.join("; "));
    }
    reason
}

/// Private projection over an already-validated canonical report. The public
/// wrapper obtains `report` from [`super::validate_v2`]; this helper performs
/// no canonical packet, observation, counter, or refusal semantics of its own.
fn project_first_hour(
    packet: &PacketV2,
    report: ValidationReport,
    facts: &FirstHourFacts,
) -> Result<FirstHourIndex> {
    facts.check()?;

    let mut row_fixtures = BTreeMap::new();
    for row in &packet.rows {
        let mut ids: Vec<String> = row.fixtures.iter().map(|fixture| fixture.id.clone()).collect();
        ids.sort();
        row_fixtures.insert(row.id.clone(), ids);
    }
    let mut declared_rows = Vec::with_capacity(ROWS.len());
    for id in ROWS {
        let row = packet.rows.iter().find(|row| row.id == id).context("validated canonical row")?;
        let mut fixtures = row.fixtures.clone();
        fixtures.sort_by(|a, b| a.id.cmp(&b.id));
        let mut host_selection = row.host_selection.clone();
        host_selection.artifact_ids.sort();
        declared_rows.push(DeclaredRowContext {
            row_id: row.id.clone(),
            platform: row.platform.clone(),
            architecture: row.architecture.clone(),
            host_role: row.host_role.clone(),
            vscode_version: row.vscode_version.clone(),
            host_selection,
            clean_profile_id: row.clean_profile_id.clone(),
            configuration_identity: row.configuration_identity.clone(),
            fixtures,
            artifacts: row.artifacts.clone(),
            subject: row.subject.clone(),
        });
    }
    let mut declared_artifacts = packet.artifacts.clone();
    declared_artifacts.sort_by(|a, b| a.id.cmp(&b.id));

    let observers = [ProfileObserver::NewHuman, ProfileObserver::FreshAgent]
        .into_iter()
        .map(|observer| {
            let fact = facts.for_observer(observer);
            ObserverSummary {
                observer,
                observed_60min_seconds: fact.observed_60min_seconds,
                synthetic_mechanism_only: fact.synthetic_mechanism_only,
                recorded_interventions: fact.recorded_interventions,
                checkout_inspection: fact.checkout_inspection,
                private_rescue: fact.private_rescue,
                hidden_assistance: fact.hidden_assistance,
                intervention_details: fact.intervention_details.clone(),
            }
        })
        .collect();

    let mut windows = Vec::with_capacity(6);
    for window in PROFILE_WINDOWS {
        let cells = window_cells(window)?;
        let wanted: BTreeSet<&str> = cells.iter().copied().collect();
        // Selector view only. The canonical report is already deterministically
        // ordered by validate_v2; filtering preserves that order.
        let evidence: Vec<EvidenceRequirement> = report
            .evidence_requirements
            .iter()
            .filter(|o| o.owner_kind == "cell" && wanted.contains(o.owner_id.as_str()))
            .cloned()
            .collect();
        for observer in [ProfileObserver::NewHuman, ProfileObserver::FreshAgent] {
            let fact = facts.for_observer(observer);
            let observer_name = match observer {
                ProfileObserver::NewHuman => "new_human",
                ProfileObserver::FreshAgent => "fresh_agent",
            };
            windows.push(ProfileWindowIndex {
                window: window.to_owned(),
                observer,
                status: Status::NotProven,
                reason: Some(window_reason(observer_name, window, fact)),
                canonical_cells: cells.iter().map(ToString::to_string).collect(),
                evidence: evidence.clone(),
                missing: missing_checklist(),
            });
        }
    }

    let bundle_recommendation = report.bundle_recommendation;
    let installed_qualification = report.installed_qualification;
    Ok(FirstHourIndex {
        index_kind: "first_hour_profile_index".into(),
        canonical_schema: SCHEMA.to_owned(),
        profile_digest: profile_digest()?,
        profile_document: PROFILE_DOCUMENT_PATH.to_owned(),
        subject: packet.subject.clone(),
        phase: packet.phase.clone(),
        source_version: packet.source_version.clone(),
        target_release: packet.target_release.clone(),
        canonical_rows: ROWS.into_iter().map(str::to_owned).collect(),
        canonical_cells: CELLS.into_iter().map(str::to_owned).collect(),
        row_fixtures,
        declared_rows,
        declared_artifacts,
        observers,
        windows,
        canonical_report: report,
        bundle_recommendation,
        installed_qualification,
        claim_boundary: "profile index only; genuine new-human/fresh-agent 5/15/60 \
            observation not established; no concrete observation producer; \
            conventional plus dynamic fixture coverage remains owned by #5902/#6056; \
            byte leaves cannot fill DAP identity or authenticated provenance; \
            installed qualification not_proven"
            .into(),
    })
}

/// Build the deterministic profile index over the canonical bundle.
///
/// Calls [`super::validate_v2`] on `packet` plus `requirements`, then projects
/// the actual report. Ordinary v2 output stays unchanged; historical v1 input
/// is rejected by the canonical parser before this wrapper runs.
pub fn index_first_hour(
    packet: &PacketV2,
    requirements: &TopologyRequirements,
    facts: &FirstHourFacts,
) -> Result<FirstHourIndex> {
    let report = super::validate_v2(packet, requirements)?;
    project_first_hour(packet, report, facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, ensure};
    use std::collections::BTreeSet;

    #[test]
    fn window_cells_partition_canonical_cells_without_new_ids() -> Result<()> {
        let mut seen = BTreeSet::new();
        for window in PROFILE_WINDOWS {
            for cell in window_cells(window)? {
                ensure!(CELLS.contains(cell), "parallel cell id {cell}");
                ensure!(seen.insert(*cell), "duplicate indexed cell {cell}");
            }
        }
        let canonical: BTreeSet<&str> = CELLS.into_iter().collect();
        ensure!(seen == canonical, "profile must index every canonical cell once");
        ensure!(window_cells("unknown_window").is_err(), "unknown window accepted");
        Ok(())
    }

    #[test]
    fn profile_digest_binds_document_and_partition_without_checkout_line_endings() -> Result<()> {
        let partition = PROFILE_WINDOWS
            .into_iter()
            .map(|window| Ok((window, window_cells(window)?)))
            .collect::<Result<Vec<_>>>()?;
        let digest = content_digest("public instructions\n", &partition)?;
        ensure!(digest == content_digest("public instructions\r\n", &partition)?);
        ensure!(digest != content_digest("changed public instructions\n", &partition)?);
        let mut changed = partition.clone();
        changed.swap(0, 1);
        ensure!(digest != content_digest("public instructions\n", &changed)?);
        changed[0] = (WINDOW_FIRST_15, &FIRST_5_CELLS);
        ensure!(digest != content_digest("public instructions\n", &changed)?);
        Ok(())
    }
}
