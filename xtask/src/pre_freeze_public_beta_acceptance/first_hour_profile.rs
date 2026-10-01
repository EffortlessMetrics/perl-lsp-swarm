//! First-hour execution profile index over the canonical v2 acceptance bundle.
//!
//! This module is a bounded, non-authoritative index. It never authenticates
//! receipts, never qualifies installed execution, and never replaces
//! [`super::validate_v2`]. Callers must parse with [`super::parse_v2`] and
//! validate with [`super::validate_v2`] first, then pass the typed
//! [`super::PacketV2`] plus its [`super::ValidationReport`] here.
//!
//! Profile windows (`first_5_minutes`, `first_15_minutes`, `first_60_minutes`)
//! are observation windows, not indexing latency promises. They index existing
//! canonical [`super::CELLS`] per existing [`super::ROWS`]; every one of the 24
//! cells on each of the 3 host rows remains governed by `validate_v2`.
//!
//! Genuine new-human / fresh-agent first-hour observation currently has no
//! concrete producer. Where no adapter exists this index exposes a precise
//! [`super::EvidenceRequirement`] outstanding obligation and keeps
//! [`super::InstalledQualification::NotProven`]. It invents no parallel cell
//! IDs, no receipt schema, no status model, and no acceptance denominator.
//!
//! Platform basis: #13768 Gate 3 activates #4346's controller-expansion
//! exception. `windows_x64_current_stable` retains the full retained journey
//! at the recorded host; both Linux rows are retained. No new Windows
//! two-version matrix and no macOS semantic parity are introduced here. See
//! #6056 adjudication
//! <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056#issuecomment-5750256278>
//! and `inputs/friday-platform-reconciliation.json` at
//! <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768#issuecomment-5925014719>.
//! Missing Windows producers/mappings stay `not_proven`.

use super::{
    AdapterCategory, CELLS, EvidenceRequirement, InstalledQualification, PacketV2, ROWS,
    Recommendation, SCHEMA, Status, Subject, ValidationReport,
};
use anyhow::{Result, ensure};
use serde::Serialize;
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
    /// References into the canonical report's obligations for those cells.
    pub evidence: Vec<EvidenceRequirement>,
    /// Precise outstanding obligations with no concrete producer yet.
    pub outstanding: Vec<EvidenceRequirement>,
}

/// Deterministic index of canonical evidence plus outstanding first-hour
/// obligations. Serialize-only: this is never parsed as a new wire protocol.
#[derive(Debug, Clone, Serialize)]
pub struct FirstHourIndex {
    pub index_kind: String,
    pub canonical_schema: String,
    pub subject: Subject,
    /// Exactly [`super::ROWS`], in canonical order.
    pub canonical_rows: Vec<String>,
    /// Exactly [`super::CELLS`], in canonical order.
    pub canonical_cells: Vec<String>,
    /// Row ID to sorted fixture IDs, as declared by the canonical packet.
    pub row_fixtures: BTreeMap<String, Vec<String>>,
    pub observers: Vec<ObserverSummary>,
    pub windows: Vec<ProfileWindowIndex>,
    /// Copied from the canonical report; never recomputed here.
    pub bundle_recommendation: Recommendation,
    /// Always [`InstalledQualification::NotProven`].
    pub installed_qualification: InstalledQualification,
    pub claim_boundary: String,
}

fn obligation_sort_key(o: &EvidenceRequirement) -> String {
    format!(
        "{}:{}:{:?}:{}:{}:{:?}",
        o.owner_kind, o.owner_id, o.row_id, o.locator, o.sha256, o.category
    )
}

/// Build the deterministic profile index over an already-validated bundle.
///
/// `report` must be the [`super::validate_v2`] result for `packet`; this
/// function checks subject binding and the exact [`super::ROWS`] by
/// [`super::CELLS`] denominator, then indexes. It performs no canonical
/// packet, observation, counter, or refusal semantics of its own.
pub fn index_first_hour(
    packet: &PacketV2,
    report: &ValidationReport,
    facts: &FirstHourFacts,
) -> Result<FirstHourIndex> {
    facts.check()?;
    ensure!(
        !report.evidence_requirements.is_empty(),
        "profile requires a validated canonical report with obligations"
    );
    for obligation in &report.evidence_requirements {
        ensure!(
            obligation.subject == packet.subject,
            "profile report subject mismatch on {}",
            obligation.owner_id
        );
    }
    let packet_rows: BTreeSet<&str> = packet.rows.iter().map(|row| row.id.as_str()).collect();
    let required_rows: BTreeSet<&str> = ROWS.into_iter().collect();
    ensure!(
        packet.rows.len() == ROWS.len() && packet_rows == required_rows,
        "profile cannot reduce canonical rows"
    );
    for row in &packet.rows {
        let cells: BTreeSet<&str> = row.cells.iter().map(|cell| cell.id.as_str()).collect();
        let required_cells: BTreeSet<&str> = CELLS.into_iter().collect();
        ensure!(
            row.cells.len() == CELLS.len() && cells == required_cells,
            "profile cannot reduce canonical cells on {}",
            row.id
        );
    }

    let mut all_artifact_ids: Vec<String> =
        packet.artifacts.iter().map(|artifact| artifact.id.clone()).collect();
    all_artifact_ids.sort();
    all_artifact_ids.dedup();

    let mut row_fixtures = BTreeMap::new();
    for row in &packet.rows {
        let mut ids: Vec<String> = row.fixtures.iter().map(|fixture| fixture.id.clone()).collect();
        ids.sort();
        row_fixtures.insert(row.id.clone(), ids);
    }

    let observers = [ProfileObserver::NewHuman, ProfileObserver::FreshAgent]
        .into_iter()
        .map(|observer| {
            let fact = facts.for_observer(observer);
            ObserverSummary {
                observer,
                observed_60min_seconds: fact.observed_60min_seconds,
                synthetic_mechanism_only: fact.synthetic_mechanism_only,
                recorded_interventions: fact.recorded_interventions,
            }
        })
        .collect();

    let mut windows = Vec::with_capacity(6);
    for window in PROFILE_WINDOWS {
        let cells = window_cells(window)?;
        let wanted: BTreeSet<&str> = cells.iter().copied().collect();
        let mut evidence: Vec<EvidenceRequirement> = report
            .evidence_requirements
            .iter()
            .filter(|o| o.owner_kind == "cell" && wanted.contains(o.owner_id.as_str()))
            .cloned()
            .collect();
        evidence.sort_by_cached_key(obligation_sort_key);
        for observer in [ProfileObserver::NewHuman, ProfileObserver::FreshAgent] {
            let fact = facts.for_observer(observer);
            let observer_name = match observer {
                ProfileObserver::NewHuman => "new_human",
                ProfileObserver::FreshAgent => "fresh_agent",
            };
            let outstanding = vec![EvidenceRequirement {
                owner_kind: "first_hour_observation".into(),
                owner_id: format!("{observer_name}_{window}"),
                subject: packet.subject.clone(),
                row_id: None,
                artifact_ids: all_artifact_ids.clone(),
                locator: format!("missing:genuine-first-hour-observation:{observer_name}:{window}"),
                sha256: "0".repeat(64),
                category: AdapterCategory::FirstTenMinutes,
            }];
            let reason = if fact.synthetic_mechanism_only {
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
            windows.push(ProfileWindowIndex {
                window: window.to_owned(),
                observer,
                status: Status::NotProven,
                reason: Some(reason),
                canonical_cells: cells.iter().map(ToString::to_string).collect(),
                evidence: evidence.clone(),
                outstanding,
            });
        }
    }

    Ok(FirstHourIndex {
        index_kind: "first_hour_profile_index".into(),
        canonical_schema: SCHEMA.to_owned(),
        subject: packet.subject.clone(),
        canonical_rows: ROWS.into_iter().map(str::to_owned).collect(),
        canonical_cells: CELLS.into_iter().map(str::to_owned).collect(),
        row_fixtures,
        observers,
        windows,
        bundle_recommendation: report.bundle_recommendation,
        installed_qualification: InstalledQualification::NotProven,
        claim_boundary: "profile index only; genuine new-human/fresh-agent 5/15/60 \
            observation not established; no concrete observation producer; \
            conventional plus dynamic fixture coverage remains owned by #5902/#6056; \
            byte leaves cannot fill DAP identity or authenticated provenance; \
            installed qualification not_proven"
            .into(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{
        Artifact, ArtifactSelection, Cell, EvidenceRef, Fixture, JourneyRow, Mechanism,
        Observation, PreparationRow, Proposition, Provenance, Role, ZeroBudgetCounts,
    };
    use super::*;
    use anyhow::Context;

    fn test_subject() -> Subject {
        Subject {
            candidate_id: "synthetic-first-hour-index".into(),
            repository_sha: "a".repeat(40),
            topology_digest: format!("sha256:{}", "b".repeat(64)),
            artifact_set_id: "synthetic-first-hour-artifacts".into(),
        }
    }

    fn evidence(id: &str, row: Option<&str>, ids: Vec<String>, subject: &Subject) -> EvidenceRef {
        EvidenceRef {
            id: id.into(),
            locator: format!("synthetic-first-hour/{id}"),
            sha256: "e".repeat(64),
            subject: subject.clone(),
            row_id: row.map(str::to_owned),
            artifact_ids: ids,
        }
    }

    /// Index-mechanism packet fixture. Canonical packet/observation/counter
    /// semantics remain covered by `super::super::tests`; this builds only the
    /// shape the index needs (subject, rows, cells, fixtures, artifacts).
    fn test_packet() -> PacketV2 {
        let subject = test_subject();
        let mut artifacts = Vec::new();
        for (platform, digit) in [("linux", 'c'), ("windows", 'd')] {
            for (role, name) in
                [(Role::Perllsp, "server"), (Role::PerlDap, "dap"), (Role::Vsix, "vsix")]
            {
                artifacts.push(Artifact {
                    id: format!("{platform}-{name}"),
                    role,
                    target: format!("{platform}-x64"),
                    path: format!("synthetic-first-hour/{platform}/{name}"),
                    sha256: digit.to_string().repeat(64),
                    subject: subject.clone(),
                    provenance: Provenance::ReleaseShaped,
                });
            }
        }
        let rows: Vec<JourneyRow> = ROWS
            .into_iter()
            .map(|id| {
                let platform = if id.starts_with("linux") { "linux" } else { "windows" };
                let selection = ArtifactSelection {
                    perllsp: format!("{platform}-server"),
                    perl_dap: format!("{platform}-dap"),
                    vsix: format!("{platform}-vsix"),
                };
                let ids = vec![
                    selection.perllsp.clone(),
                    selection.perl_dap.clone(),
                    selection.vsix.clone(),
                ];
                JourneyRow {
                    id: id.into(),
                    platform: platform.into(),
                    architecture: "x64".into(),
                    host_role: if id.ends_with("minimum_supported") {
                        "minimum_supported"
                    } else {
                        "current_stable"
                    }
                    .into(),
                    vscode_version: "1.100.0".into(),
                    host_selection: evidence("host", Some(id), ids.clone(), &subject),
                    clean_profile_id: format!("synthetic-first-hour-profile-{id}"),
                    configuration_identity: "synthetic-first-hour-config".into(),
                    fixtures: vec![Fixture {
                        id: "synthetic-first-hour-fixture".into(),
                        content_sha256: "f".repeat(64),
                    }],
                    subject: subject.clone(),
                    cells: CELLS
                        .into_iter()
                        .map(|cell| Cell {
                            id: cell.into(),
                            status: Status::Pass,
                            proposition: Proposition::Executed,
                            evidence: vec![evidence(cell, Some(id), ids.clone(), &subject)],
                            reason: None,
                        })
                        .collect(),
                    artifacts: selection,
                }
            })
            .collect();
        let all_ids: Vec<String> = artifacts.iter().map(|a| a.id.clone()).collect();
        PacketV2 {
            check: "pre-freeze-public-beta-acceptance".into(),
            schema_version: SCHEMA.into(),
            phase: "pre_freeze_product".into(),
            subject: subject.clone(),
            source_version: "0.17.0".into(),
            target_release: "0.18.0".into(),
            artifacts,
            rows,
            first_ten_minutes: Observation {
                status: Status::Pass,
                observed_rows: vec!["linux_x64_current_stable".into()],
                evidence: vec![evidence(
                    "observation",
                    Some("linux_x64_current_stable"),
                    vec!["linux-server".into(), "linux-dap".into(), "linux-vsix".into()],
                    &subject,
                )],
                reason: None,
            },
            preparation: ["linux", "windows"]
                .into_iter()
                .map(|p| {
                    let ids = vec![format!("{p}-server"), format!("{p}-dap"), format!("{p}-vsix")];
                    PreparationRow {
                        target: format!("{p}-x64"),
                        status: Status::Pass,
                        evidence: vec![evidence("prep", None, ids.clone(), &subject)],
                        artifact_ids: ids,
                        reason: None,
                    }
                })
                .collect(),
            mechanisms: ["#5900", "#5901", "#5902", "#5903"]
                .into_iter()
                .map(|issue| Mechanism {
                    issue: issue.into(),
                    status: Status::Pass,
                    evidence: vec![evidence(issue, None, all_ids.clone(), &subject)],
                    reason: None,
                })
                .collect(),
            zero_budget_counts: ZeroBudgetCounts::default(),
            product_blockers: vec![],
            expected_beta_limitations: vec!["synthetic index mechanism only".into()],
            friction_findings: vec![],
            freeze_recommendation: Recommendation::Ready,
            claim_boundary: "synthetic index mechanism; no installed execution".into(),
        }
    }

    fn cell_obligation(
        packet: &PacketV2,
        row_id: &str,
        cell_id: &str,
        category: AdapterCategory,
    ) -> EvidenceRequirement {
        let row = packet.rows.iter().find(|r| r.id == row_id).expect("row");
        let ids = vec![
            row.artifacts.perllsp.clone(),
            row.artifacts.perl_dap.clone(),
            row.artifacts.vsix.clone(),
        ];
        EvidenceRequirement {
            owner_kind: "cell".into(),
            owner_id: cell_id.into(),
            subject: packet.subject.clone(),
            row_id: Some(row_id.into()),
            artifact_ids: ids,
            locator: format!("synthetic-first-hour/{row_id}/{cell_id}"),
            sha256: "e".repeat(64),
            category,
        }
    }

    /// Minimal index-mechanism report. Canonical `validate_v2` obligations are
    /// covered by existing tests; this carries only what the index propagates.
    fn test_report(packet: &PacketV2, recommendation: Recommendation) -> ValidationReport {
        let all_ids: Vec<String> = packet.artifacts.iter().map(|a| a.id.clone()).collect();
        // Deliberately unsorted input: the index must sort deterministically.
        let mut obligations = vec![
            cell_obligation(
                packet,
                "windows_x64_current_stable",
                "dap_preview",
                AdapterCategory::InstalledJourney,
            ),
            cell_obligation(
                packet,
                "linux_x64_current_stable",
                "unicode_crlf_edit_save_requery",
                AdapterCategory::InstalledJourney,
            ),
            cell_obligation(
                packet,
                "linux_x64_minimum_supported",
                "diagnostics",
                AdapterCategory::InstalledJourney,
            ),
            EvidenceRequirement {
                owner_kind: "artifact".into(),
                owner_id: "linux-dap".into(),
                subject: packet.subject.clone(),
                row_id: None,
                artifact_ids: vec!["linux-dap".into()],
                locator: "synthetic-first-hour/linux/dap".into(),
                sha256: "c".repeat(64),
                category: AdapterCategory::ArtifactProvenance,
            },
            EvidenceRequirement {
                owner_kind: "topology".into(),
                owner_id: packet.subject.topology_digest.clone(),
                subject: packet.subject.clone(),
                row_id: None,
                artifact_ids: all_ids,
                locator: packet.subject.topology_digest.clone(),
                sha256: "b".repeat(64),
                category: AdapterCategory::TopologyBinding,
            },
        ];
        obligations.reverse();
        ValidationReport {
            bundle_recommendation: recommendation,
            installed_qualification: InstalledQualification::NotProven,
            evidence_requirements: obligations,
        }
    }

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
    fn positive_index_is_deterministic_candidate_bound_and_permanently_not_proven() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        let facts = FirstHourFacts::unobserved();
        let index = index_first_hour(&packet, &report, &facts)?;
        ensure!(index.subject == packet.subject, "index lost candidate binding");
        ensure!(index.canonical_schema == SCHEMA, "index lost canonical schema");
        ensure!(
            index.canonical_rows == ROWS.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            "index lost canonical rows"
        );
        ensure!(
            index.canonical_cells == CELLS.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            "index lost canonical cells"
        );
        ensure!(
            index.bundle_recommendation == Recommendation::Ready,
            "index must propagate canonical recommendation"
        );
        ensure!(
            index.installed_qualification == InstalledQualification::NotProven,
            "index must never qualify installed execution"
        );
        ensure!(index.windows.len() == 6, "two observers times three windows");
        for window in &index.windows {
            ensure!(window.status == Status::NotProven, "unobserved window must stay not_proven");
            ensure!(
                window.reason.as_ref().context("window reason")?.contains("not_proven"),
                "window must name not_proven boundary"
            );
            ensure!(window.outstanding.len() == 1, "one outstanding obligation per window");
        }
        let first = serde_json::to_vec(&index)?;
        let second = serde_json::to_vec(&index_first_hour(&packet, &report, &facts)?)?;
        ensure!(first == second, "index must be deterministic");
        // Input obligation order must not leak into output order.
        let mut reordered = report.clone();
        reordered.evidence_requirements.reverse();
        let third = serde_json::to_vec(&index_first_hour(&packet, &reordered, &facts)?)?;
        ensure!(first == third, "index must sort canonical evidence");
        Ok(())
    }

    #[test]
    fn report_subject_mismatch_rejects_wrong_artifact_source() -> Result<()> {
        let packet = test_packet();
        let mut report = test_report(&packet, Recommendation::Ready);
        report.evidence_requirements.first_mut().context("obligation")?.subject.candidate_id =
            "other-candidate".into();
        ensure!(index_first_hour(&packet, &report, &FirstHourFacts::unobserved()).is_err());
        let empty = ValidationReport {
            bundle_recommendation: Recommendation::Ready,
            installed_qualification: InstalledQualification::NotProven,
            evidence_requirements: vec![],
        };
        ensure!(index_first_hour(&packet, &empty, &FirstHourFacts::unobserved()).is_err());
        Ok(())
    }

    #[test]
    fn denominator_reduction_rejected() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        let facts = FirstHourFacts::unobserved();
        let mut missing_row = packet.clone();
        missing_row.rows.pop();
        ensure!(index_first_hour(&missing_row, &report, &facts).is_err());
        let mut duplicate_row = packet.clone();
        let row = duplicate_row.rows.first().context("row")?.clone();
        duplicate_row.rows.push(row);
        ensure!(index_first_hour(&duplicate_row, &report, &facts).is_err());
        let mut missing_cell = packet.clone();
        missing_cell.rows.first_mut().context("row")?.cells.pop();
        ensure!(index_first_hour(&missing_cell, &report, &facts).is_err());
        let mut duplicate_cell = packet.clone();
        let cell =
            duplicate_cell.rows.first().context("row")?.cells.first().context("cell")?.clone();
        duplicate_cell.rows.first_mut().context("row")?.cells.push(cell);
        ensure!(index_first_hour(&duplicate_cell, &report, &facts).is_err());
        Ok(())
    }

    #[test]
    fn guided_expert_cannot_fill_either_row() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        for observer in [ProfileObserver::NewHuman, ProfileObserver::FreshAgent] {
            let mut facts = FirstHourFacts::unobserved();
            match observer {
                ProfileObserver::NewHuman => facts.human.guided_expert = true,
                ProfileObserver::FreshAgent => facts.agent.guided_expert = true,
            }
            ensure!(
                index_first_hour(&packet, &report, &facts).is_err(),
                "guided expert filled {observer:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn observer_cross_fill_rejected() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        let mut swapped = FirstHourFacts::unobserved();
        swapped.human.observer = ProfileObserver::FreshAgent;
        swapped.agent.observer = ProfileObserver::NewHuman;
        ensure!(index_first_hour(&packet, &report, &swapped).is_err());
        let mut same = FirstHourFacts::unobserved();
        same.agent.observer = ProfileObserver::NewHuman;
        ensure!(index_first_hour(&packet, &report, &same).is_err());
        // Outstanding obligations must stay distinct per observer.
        let index = index_first_hour(&packet, &report, &FirstHourFacts::unobserved())?;
        let owners: BTreeSet<String> = index
            .windows
            .iter()
            .flat_map(|w| w.outstanding.iter().map(|o| o.owner_id.clone()))
            .collect();
        ensure!(owners.len() == 6, "observer windows must not share obligations");
        Ok(())
    }

    #[test]
    fn omitted_intervention_rejected() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        for field in ["checkout", "rescue", "hidden"] {
            let mut facts = FirstHourFacts::unobserved();
            match field {
                "checkout" => facts.agent.checkout_inspection = true,
                "rescue" => facts.agent.private_rescue = true,
                _ => facts.agent.hidden_assistance = true,
            }
            ensure!(
                index_first_hour(&packet, &report, &facts).is_err(),
                "{field} assistance without intervention accepted"
            );
            facts.agent.recorded_interventions = 1;
            index_first_hour(&packet, &report, &facts)?;
        }
        Ok(())
    }

    #[test]
    fn short_or_synthetic_run_never_marks_60min_pass() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        for (seconds, synthetic) in [(30, true), (0, true), (3599, false), (3600, false)] {
            let mut facts = FirstHourFacts::unobserved();
            facts.human.observed_60min_seconds = seconds;
            facts.human.synthetic_mechanism_only = synthetic;
            facts.agent.observed_60min_seconds = seconds;
            facts.agent.synthetic_mechanism_only = synthetic;
            let index = index_first_hour(&packet, &report, &facts)?;
            for window in index.windows.iter().filter(|w| w.window == WINDOW_FIRST_60) {
                ensure!(
                    window.status == Status::NotProven && !window.outstanding.is_empty(),
                    "{seconds}s synthetic={synthetic} filled 60-minute observation"
                );
            }
            ensure!(
                index.installed_qualification == InstalledQualification::NotProven,
                "mechanism must never claim installed acceptance"
            );
        }
        Ok(())
    }

    #[test]
    fn byte_leaf_cannot_substitute_dap_or_provenance() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        let index = index_first_hour(&packet, &report, &FirstHourFacts::unobserved())?;
        let dap_windows: Vec<_> =
            index.windows.iter().filter(|w| w.window == WINDOW_FIRST_60).collect();
        ensure!(!dap_windows.is_empty(), "60-minute window missing");
        for window in dap_windows {
            ensure!(
                window.evidence.iter().any(|o| o.owner_id == "dap_preview"),
                "DAP preview obligation lost from 60-minute index"
            );
            ensure!(
                window.outstanding.iter().all(|o| !o.locator.contains("byte")),
                "byte leaf must not satisfy genuine observation"
            );
            ensure!(
                window
                    .outstanding
                    .iter()
                    .all(|o| o.locator.starts_with("missing:genuine-first-hour-observation:")),
                "outstanding DAP/observation boundary must stay explicit"
            );
        }
        ensure!(
            index.claim_boundary.contains("byte leaves cannot fill DAP identity"),
            "claim boundary must name byte-leaf limit"
        );
        Ok(())
    }

    #[test]
    fn safe_refusal_preserved_not_mislabeled_failure() -> Result<()> {
        let packet = test_packet();
        let mut report = test_report(&packet, Recommendation::Ready);
        report.evidence_requirements.push(cell_obligation(
            &packet,
            "linux_x64_current_stable",
            "safe_rename_or_refusal",
            AdapterCategory::AcceptedClaim,
        ));
        let index = index_first_hour(&packet, &report, &FirstHourFacts::unobserved())?;
        ensure!(
            index.bundle_recommendation == Recommendation::Ready,
            "safe refusal must not become failure"
        );
        let refusal_windows: Vec<_> =
            index.windows.iter().filter(|w| w.window == WINDOW_FIRST_15).collect();
        ensure!(!refusal_windows.is_empty(), "15-minute window missing");
        for window in refusal_windows {
            ensure!(
                window.evidence.iter().any(|o| o.owner_id == "safe_rename_or_refusal"
                    && o.category == AdapterCategory::AcceptedClaim),
                "accepted-claim refusal obligation lost"
            );
        }
        Ok(())
    }

    #[test]
    fn zero_budget_and_blocked_propagate_without_averaging() -> Result<()> {
        let packet = test_packet();
        for recommendation in [Recommendation::Blocked, Recommendation::NotProven] {
            let report = test_report(&packet, recommendation);
            let index = index_first_hour(&packet, &report, &FirstHourFacts::unobserved())?;
            ensure!(
                index.bundle_recommendation == recommendation,
                "canonical recommendation must propagate exactly"
            );
            ensure!(
                index.installed_qualification == InstalledQualification::NotProven,
                "blocked bundle must never qualify installed execution"
            );
        }
        Ok(())
    }

    #[test]
    fn linux_floor_and_full_denominator_required() -> Result<()> {
        let packet = test_packet();
        let report = test_report(&packet, Recommendation::Ready);
        let index = index_first_hour(&packet, &report, &FirstHourFacts::unobserved())?;
        for row in [
            "linux_x64_minimum_supported",
            "linux_x64_current_stable",
            "windows_x64_current_stable",
        ] {
            ensure!(index.canonical_rows.contains(&row.to_owned()), "lost retained row {row}");
        }
        ensure!(!index.canonical_rows.iter().any(|r| r.contains("macos")), "no macOS parity row");
        ensure!(
            index.row_fixtures.len() == 3 && index.row_fixtures.values().all(|v| !v.is_empty()),
            "row fixtures must remain indexed"
        );
        ensure!(
            index.claim_boundary.contains("conventional plus dynamic"),
            "fixture boundary must name conventional plus dynamic"
        );
        Ok(())
    }
}
