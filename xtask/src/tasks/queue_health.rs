//! Queue health classifier for orchestrator merge safety.
//!
//! Computes one of four queue health modes:
//! - GREEN: merge/cascade/promotion lanes are open.
//! - PENDING: read-only review/design lanes only while main checks settle.
//! - RED: merge drain frozen; only master-fix and read-only review lanes remain.
//! - NOT_PROVEN: main-CI evidence is missing, unknown, malformed, incomplete, or
//!   carries an unproven subject identity; only read-only investigation and exact
//!   evidence refresh are permitted. Unknown evidence is never equated with green,
//!   and never silently collapsed into pending or red.
//!
//! #15387: absence is evidence of nothing. A missing, unknown, or unsupported
//! `ci_state`, an unproved required-check denominator, a source/collector
//! failure, a fixture observation, or an unversioned producer object all fail
//! closed to [`QueueMode::NotProven`] instead of falling through to green.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::merge_ready::{CheckSubject, EvidenceClass, RequiredCheckEvidence};

const QUEUE_HEALTH_CHECK: &str = "queue-health";
const QUEUE_HEALTH_SCHEMA_VERSION: u32 = 2;
/// Input schema a live producer must declare before its object can support
/// GREEN. Absent or mismatched versions fail closed (#15387).
const INPUT_SCHEMA_VERSION: u32 = 1;
const DEFAULT_RECEIPT_PATH: &str = "target/receipts/queue-health.json";
const DEFAULT_INPUT_PATH: &str = "target/receipts/master-ci-state.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QueueMode {
    Green,
    Pending,
    Red,
    NotProven,
}

/// Whether the classified observation was gathered live from the default
/// branch or replayed from an offline fixture.
///
/// Fixtures can teach and rehearse the classifier, but they are not an
/// observation of current main. An [`ObservationSource::OfflineFixture`]
/// observation can therefore never authorize mutating lanes (#15387).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationSource {
    Live,
    OfflineFixture,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueHealthReceipt {
    pub check: String,
    pub schema_version: u32,
    pub source: ObservationSource,
    /// Exact default-branch subject the observation binds to; empty only when
    /// no usable input was available at all.
    pub master_sha: String,
    pub mode: QueueMode,
    pub allowed_lanes: Vec<String>,
    pub blocked_lanes: Vec<String>,
    pub reasons: Vec<String>,
    pub verdict: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QueueHealthInput {
    pub master_sha: String,
    #[serde(default)]
    pub ci_state: Option<String>,
    #[serde(default)]
    pub pending_checks: Vec<String>,
    #[serde(default)]
    pub running_checks: Vec<String>,
    #[serde(default)]
    pub failed_checks: Vec<String>,
    /// Declared denominator: the exact required contexts a complete main-CI
    /// observation must account for. Absent and without `zero_applicable`,
    /// the denominator is unproved and GREEN is unreachable (#15387).
    #[serde(default)]
    pub expected_required_checks: Vec<String>,
    /// Subject-bound typed evidence rows (#15998 pattern). On the main-branch
    /// queue the satisfying subject is [`CheckSubject::CandidateHead`]: a row
    /// evaluated against the raw default-branch head itself.
    #[serde(default)]
    pub required_check_evidence: Vec<RequiredCheckEvidence>,
    /// The collector completed and legitimately found zero applicable
    /// required checks; distinct from a source failure (#15387).
    #[serde(default)]
    pub zero_applicable: bool,
    /// Whether the producer's observation denominator is complete. Absent
    /// means completeness unknown, which can never support GREEN.
    #[serde(default)]
    pub source_complete: Option<bool>,
    /// Collector/source failure, if any; a source error can never support GREEN.
    #[serde(default)]
    pub source_error: Option<String>,
    /// Input schema declared by the producer. Unversioned legacy objects and
    /// mismatched versions fail closed (#15387).
    #[serde(default)]
    pub input_schema_version: Option<u32>,
    #[serde(default)]
    pub failure_classifier: Option<FailureClassifier>,
    #[serde(default)]
    pub gate_policy: Option<GatePolicy>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FailureClassifier {
    #[serde(default)]
    pub shared_blocker: bool,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatePolicy {
    #[serde(default)]
    pub pending_allows_merge_ready_if_candidate_current: bool,
}

#[derive(Debug, Clone)]
pub struct QueueHealthArgs {
    pub receipt: Option<PathBuf>,
    pub fixture: Option<PathBuf>,
}

pub fn run(args: QueueHealthArgs) -> Result<()> {
    let source = if args.fixture.is_some() {
        ObservationSource::OfflineFixture
    } else {
        ObservationSource::Live
    };
    let (input, source) = match load_input(args.fixture.as_deref()) {
        Ok(loaded) => loaded,
        Err(err) if source == ObservationSource::Live => {
            // A missing or unreadable live input is itself evidence absence:
            // fail closed to a NOT_PROVEN receipt instead of aborting without
            // one (#15387).
            eprintln!("{err:#}");
            (
                QueueHealthInput {
                    master_sha: String::new(),
                    ci_state: None,
                    pending_checks: Vec::new(),
                    running_checks: Vec::new(),
                    failed_checks: Vec::new(),
                    expected_required_checks: Vec::new(),
                    required_check_evidence: Vec::new(),
                    zero_applicable: false,
                    source_complete: Some(false),
                    source_error: Some(format!("live queue-health input unavailable: {err:#}")),
                    input_schema_version: None,
                    failure_classifier: None,
                    gate_policy: None,
                },
                ObservationSource::Live,
            )
        }
        Err(err) => return Err(err),
    };
    let receipt = classify(&input, source);

    let out = serde_json::to_string_pretty(&receipt)?;
    let receipt_path = args.receipt.unwrap_or_else(|| PathBuf::from(DEFAULT_RECEIPT_PATH));

    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir: {}", parent.display()))?;
    }
    fs::write(&receipt_path, format!("{out}\n"))
        .with_context(|| format!("failed writing receipt to {}", receipt_path.display()))?;

    println!("{}", receipt.mode_as_str());
    println!("wrote {}", receipt_path.display());

    Ok(())
}

fn load_input(fixture: Option<&Path>) -> Result<(QueueHealthInput, ObservationSource)> {
    let input_path =
        fixture.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_INPUT_PATH));
    let source =
        if fixture.is_some() { ObservationSource::OfflineFixture } else { ObservationSource::Live };
    if fixture.is_none() && !input_path.exists() {
        bail!(
            "missing default input {} (pass --fixture <json> to classify from a fixture)",
            input_path.display()
        );
    }

    let raw = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read input {}", input_path.display()))?;
    let input: QueueHealthInput = serde_json::from_str(&raw)
        .with_context(|| format!("invalid queue-health input {}", input_path.display()))?;
    Ok((input, source))
}

fn is_full_object_id(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn classify(input: &QueueHealthInput, source: ObservationSource) -> QueueHealthReceipt {
    let mut reasons = Vec::new();

    // Absence is evidence of nothing: a missing, empty, or unrecognized
    // `ci_state` must not inherit the settled branch below (#15387).
    let state = input
        .ci_state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());

    if !input.pending_checks.is_empty() {
        reasons.push(format!("{} pending check(s)", input.pending_checks.len()));
    }
    if !input.running_checks.is_empty() {
        reasons.push(format!("{} running check(s)", input.running_checks.len()));
    }
    if !input.failed_checks.is_empty() {
        reasons.push(format!("{} failed check(s)", input.failed_checks.len()));
    }

    if let Some(classifier) = &input.failure_classifier {
        if classifier.shared_blocker {
            reasons.push("failure classifier marked shared blocker".to_string());
        }
        if let Some(summary) = &classifier.summary {
            reasons.push(format!("failure summary: {summary}"));
        }
    }

    let has_pending = !input.pending_checks.is_empty() || !input.running_checks.is_empty();
    let has_failures = !input.failed_checks.is_empty();
    let shared_blocker = input.failure_classifier.as_ref().is_some_and(|c| c.shared_blocker);

    // Observed negative/pending evidence still dominates: a failing or
    // unsettled queue is honest even when the observation is otherwise weak,
    // and neither mode opens mutating lanes. Only the settled branch requires
    // complete, current, subject-bound evidence.
    let mode = if state.as_deref() == Some("red") || has_failures || shared_blocker {
        QueueMode::Red
    } else if state.as_deref() == Some("pending") || has_pending {
        QueueMode::Pending
    } else {
        let green_blockers = green_blockers(input, source);
        if green_blockers.is_empty() {
            QueueMode::Green
        } else {
            reasons.extend(green_blockers);
            QueueMode::NotProven
        }
    };

    if reasons.is_empty() {
        match mode {
            QueueMode::Green => reasons.push("main CI reported fully settled".to_string()),
            QueueMode::Pending => reasons.push("main CI is still settling".to_string()),
            QueueMode::Red => reasons.push("main CI reported failures".to_string()),
            QueueMode::NotProven => {
                reasons.push("main-CI evidence cannot be proven current".to_string())
            }
        }
    }

    let (allowed_lanes, blocked_lanes, verdict) = build_policy(mode, input.gate_policy.as_ref());

    QueueHealthReceipt {
        check: QUEUE_HEALTH_CHECK.to_string(),
        schema_version: QUEUE_HEALTH_SCHEMA_VERSION,
        source,
        master_sha: input.master_sha.clone(),
        mode,
        allowed_lanes,
        blocked_lanes,
        reasons,
        verdict,
    }
}

/// Every reason the settled (green) branch is unreachable, or empty when the
/// observation is a complete, current, subject-bound live observation (#15387).
fn green_blockers(input: &QueueHealthInput, source: ObservationSource) -> Vec<String> {
    let mut blockers = Vec::new();

    if source == ObservationSource::OfflineFixture {
        blockers.push(
            "offline fixture output cannot be consumed as live queue authorization".to_string(),
        );
    }
    if input.input_schema_version != Some(INPUT_SCHEMA_VERSION) {
        blockers.push(format!(
            "input schema version {:?} is not the supported version {INPUT_SCHEMA_VERSION}; \
             producer identity unproven",
            input.input_schema_version
        ));
    }
    if !is_full_object_id(&input.master_sha) {
        blockers.push(
            "master_sha is not a full 40-character object id; default-branch subject \
             identity unproven"
                .to_string(),
        );
    }
    if let Some(error) = &input.source_error {
        blockers.push(format!("observation source error: {error}"));
    }
    if input.source_complete != Some(true) {
        blockers.push(
            "observation completeness unproved; the collector may have seen only part of main CI"
                .to_string(),
        );
    }

    let state = input
        .ci_state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    match state.as_deref() {
        Some("green") => {}
        None => blockers.push("ci_state missing from input; main-CI state unknown".to_string()),
        Some(other) => {
            blockers.push(format!("unsupported ci_state value {other:?}; state unknown"));
        }
    }

    if input.expected_required_checks.is_empty() {
        if input.zero_applicable {
            // Legitimate zero-applicable population: the collector completed
            // and found no applicable required checks, so there are no rows
            // to account for.
        } else {
            blockers.push(
                "required-check denominator unproved; empty check arrays with no expected \
                 context list are not a settled observation"
                    .to_string(),
            );
        }
        return blockers;
    }

    let mut expected: BTreeSet<&str> = BTreeSet::new();
    for name in &input.expected_required_checks {
        if name.trim().is_empty() {
            blockers.push("expected required context list contains a blank name".to_string());
            continue;
        }
        if !expected.insert(name.as_str()) {
            blockers.push(format!("expected required context repeated: {name}"));
        }
    }

    let mut rows_by_name: BTreeMap<&str, Vec<&RequiredCheckEvidence>> = BTreeMap::new();
    for row in &input.required_check_evidence {
        rows_by_name.entry(row.name.as_str()).or_default().push(row);
    }

    for name in &expected {
        match rows_by_name.get(name).map(Vec::as_slice) {
            None | Some([]) => blockers
                .push(format!("required context {name} has no evidence row; queue state unproved")),
            Some([_, _, ..]) => {
                blockers.push(format!("required context {name} has ambiguous duplicate rows"))
            }
            Some([row]) if row.evaluated_sha != input.master_sha => blockers.push(format!(
                "required context {name} evaluated {} but the default-branch subject is {}",
                row.evaluated_sha, input.master_sha
            )),
            Some([row]) if row.subject != CheckSubject::CandidateHead => blockers.push(format!(
                "required context {name} was evaluated against a non-default-branch subject"
            )),
            Some([row]) if row.result != EvidenceClass::Success => {
                blockers.push(format!("required context {name} did not report terminal success"))
            }
            Some([_]) => {}
        }
    }

    blockers
}

fn build_policy(
    mode: QueueMode,
    gate_policy: Option<&GatePolicy>,
) -> (Vec<String>, Vec<String>, String) {
    match mode {
        QueueMode::Green => (
            vec![
                "merge-drain".to_string(),
                "cascade-update".to_string(),
                "green-ci-promotion".to_string(),
            ],
            Vec::new(),
            "main healthy: merge drain/cascade/promotions allowed".to_string(),
        ),
        QueueMode::Pending => {
            let mut allowed = vec!["read-only-review".to_string(), "read-only-design".to_string()];
            if gate_policy
                .is_some_and(|policy| policy.pending_allows_merge_ready_if_candidate_current)
            {
                allowed.push("merge-ready-promotion-if-candidate-current".to_string());
            }

            (
                allowed,
                vec![
                    "merge-drain".to_string(),
                    "green-ci-promotion".to_string(),
                    "broad-cascade-final-labels".to_string(),
                ],
                "main unsettled: review/design only; defer broad promotion lanes".to_string(),
            )
        }
        QueueMode::Red => (
            vec!["master-fix".to_string(), "read-only-review".to_string()],
            vec![
                "merge-drain".to_string(),
                "cascade-update".to_string(),
                "green-ci-promotion".to_string(),
                "merge-ready-promotion".to_string(),
                "broad-cascade-final-labels".to_string(),
            ],
            "main failing: freeze merge drain and classify shared blocker before reopening lanes"
                .to_string(),
        ),
        QueueMode::NotProven => (
            vec!["read-only-investigation".to_string(), "evidence-refresh".to_string()],
            vec![
                "merge-drain".to_string(),
                "cascade-update".to_string(),
                "green-ci-promotion".to_string(),
                "merge-ready-promotion".to_string(),
                "broad-cascade-final-labels".to_string(),
                "master-fix".to_string(),
            ],
            "main-CI evidence unknown or incomplete: queue safety cannot be proven; investigate \
             or refresh exact evidence before opening any lanes"
                .to_string(),
        ),
    }
}

impl QueueHealthReceipt {
    fn mode_as_str(&self) -> &'static str {
        match self.mode {
            QueueMode::Green => "GREEN",
            QueueMode::Pending => "PENDING",
            QueueMode::Red => "RED",
            QueueMode::NotProven => "NOT_PROVEN",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must;

    const MASTER_SHA: &str = "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// Legacy unversioned producer object: aggregate strings only, no typed
    /// evidence, no denominator, no completeness declaration.
    fn legacy_input(state: Option<&str>) -> QueueHealthInput {
        QueueHealthInput {
            master_sha: MASTER_SHA.to_string(),
            ci_state: state.map(str::to_string),
            pending_checks: Vec::new(),
            running_checks: Vec::new(),
            failed_checks: Vec::new(),
            expected_required_checks: Vec::new(),
            required_check_evidence: Vec::new(),
            zero_applicable: false,
            source_complete: None,
            source_error: None,
            input_schema_version: None,
            failure_classifier: None,
            gate_policy: None,
        }
    }

    fn success_row(name: &str) -> RequiredCheckEvidence {
        RequiredCheckEvidence {
            name: name.to_string(),
            evaluated_sha: MASTER_SHA.to_string(),
            subject: CheckSubject::CandidateHead,
            result: EvidenceClass::Success,
        }
    }

    /// Complete typed live producer object that earns GREEN.
    fn complete_live_input() -> QueueHealthInput {
        QueueHealthInput {
            master_sha: MASTER_SHA.to_string(),
            ci_state: Some("green".to_string()),
            pending_checks: Vec::new(),
            running_checks: Vec::new(),
            failed_checks: Vec::new(),
            expected_required_checks: vec!["merge-gate".to_string(), "clippy".to_string()],
            required_check_evidence: vec![success_row("merge-gate"), success_row("clippy")],
            zero_applicable: false,
            source_complete: Some(true),
            source_error: None,
            input_schema_version: Some(INPUT_SCHEMA_VERSION),
            failure_classifier: None,
            gate_policy: None,
        }
    }

    #[test]
    fn missing_ci_state_is_never_green() {
        let receipt = classify(&legacy_input(None), ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert!(!receipt.allowed_lanes.iter().any(|lane| lane == "merge-drain"));
        assert!(receipt.reasons.iter().any(|reason| reason.contains("ci_state missing")));
    }

    #[test]
    fn unknown_or_malformed_ci_state_is_never_green() {
        for state in ["unknown", "bananas", ""] {
            let receipt = classify(&legacy_input(Some(state)), ObservationSource::Live);
            assert_eq!(receipt.mode, QueueMode::NotProven, "state {state:?} must not be green");
        }
    }

    #[test]
    fn empty_arrays_with_unproved_denominator_are_not_fully_settled() {
        let input = legacy_input(Some("green"));
        let receipt = classify(&input, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert!(receipt.reasons.iter().any(|reason| reason.contains("denominator unproved")));
        assert!(receipt.reasons.iter().all(|reason| !reason.contains("fully settled")));
    }

    #[test]
    fn advisory_success_cannot_compensate_for_absent_required_checks() {
        // Aggregate green state plus one advisory success row, but no declared
        // denominator and no zero-applicable declaration: still unproved.
        let mut input = complete_live_input();
        input.expected_required_checks = Vec::new();
        input.required_check_evidence = vec![success_row("advisory-lint")];
        let receipt = classify(&input, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert!(receipt.reasons.iter().any(|reason| reason.contains("denominator unproved")));
    }

    #[test]
    fn complete_live_typed_evidence_is_green() {
        let receipt = classify(&complete_live_input(), ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::Green);
        assert!(receipt.allowed_lanes.contains(&"merge-drain".to_string()));
        assert_eq!(receipt.schema_version, QUEUE_HEALTH_SCHEMA_VERSION);
        assert_eq!(receipt.source, ObservationSource::Live);
    }

    #[test]
    fn missing_one_required_context_is_not_green() {
        let mut input = complete_live_input();
        input.required_check_evidence = vec![success_row("merge-gate")];
        let receipt = classify(&input, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert!(receipt.reasons.iter().any(|reason| reason.contains("clippy has no evidence row")));
    }

    #[test]
    fn stale_or_wrong_subject_evidence_is_not_green() {
        let mut stale = complete_live_input();
        stale.required_check_evidence[1].evaluated_sha =
            "2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        assert_eq!(classify(&stale, ObservationSource::Live).mode, QueueMode::NotProven);

        let mut wrong_subject = complete_live_input();
        wrong_subject.required_check_evidence[1].subject =
            CheckSubject::MergeGroup { merge_group_sha: MASTER_SHA.to_string() };
        assert_eq!(classify(&wrong_subject, ObservationSource::Live).mode, QueueMode::NotProven);
    }

    #[test]
    fn short_master_sha_cannot_earn_green() {
        let mut input = complete_live_input();
        input.master_sha = "1111111".to_string();
        input
            .required_check_evidence
            .iter_mut()
            .for_each(|row| row.evaluated_sha = "1111111".to_string());
        let receipt = classify(&input, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert!(
            receipt.reasons.iter().any(|reason| reason.contains("full 40-character object id"))
        );
    }

    #[test]
    fn unversioned_or_mismatched_input_fails_closed() {
        let mut input = complete_live_input();
        input.input_schema_version = None;
        assert_eq!(classify(&input, ObservationSource::Live).mode, QueueMode::NotProven);

        let mut input = complete_live_input();
        input.input_schema_version = Some(INPUT_SCHEMA_VERSION + 1);
        assert_eq!(classify(&input, ObservationSource::Live).mode, QueueMode::NotProven);
    }

    #[test]
    fn source_failure_and_zero_applicable_are_distinct() {
        // A source/API failure is not a settled observation.
        let mut failed = complete_live_input();
        failed.source_error = Some("github check-run collector unavailable".to_string());
        let receipt = classify(&failed, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert!(receipt.reasons.iter().any(|reason| reason.contains("observation source error")));

        // A completed collector with a legitimate zero-applicable population
        // is settled without any evidence rows.
        let mut zero = complete_live_input();
        zero.expected_required_checks = Vec::new();
        zero.zero_applicable = true;
        let receipt = classify(&zero, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::Green);
    }

    #[test]
    fn incomplete_source_is_not_green() {
        let mut input = complete_live_input();
        input.source_complete = None;
        assert_eq!(classify(&input, ObservationSource::Live).mode, QueueMode::NotProven);

        let mut input = complete_live_input();
        input.source_complete = Some(false);
        assert_eq!(classify(&input, ObservationSource::Live).mode, QueueMode::NotProven);
    }

    #[test]
    fn fixture_output_cannot_authorize_live_lanes() {
        let receipt = classify(&complete_live_input(), ObservationSource::OfflineFixture);
        assert_eq!(receipt.mode, QueueMode::NotProven);
        assert_eq!(receipt.source, ObservationSource::OfflineFixture);
        assert!(receipt.reasons.iter().any(|reason| reason.contains("offline fixture")));
    }

    #[test]
    fn pending_stays_pending() {
        let mut input = legacy_input(Some("green"));
        input.pending_checks = vec!["merge-gate".to_string()];
        input.gate_policy =
            Some(GatePolicy { pending_allows_merge_ready_if_candidate_current: true });
        let receipt = classify(&input, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::Pending);
        assert!(
            receipt
                .allowed_lanes
                .contains(&"merge-ready-promotion-if-candidate-current".to_string())
        );
    }

    #[test]
    fn red_stays_red() {
        let mut input = legacy_input(Some("green"));
        input.failed_checks = vec!["clippy".to_string()];
        input.failure_classifier = Some(FailureClassifier {
            shared_blocker: true,
            summary: Some("clippy broken".to_string()),
        });
        let receipt = classify(&input, ObservationSource::Live);
        assert_eq!(receipt.mode, QueueMode::Red);
        assert!(receipt.allowed_lanes.contains(&"master-fix".to_string()));
    }

    #[test]
    fn not_proven_lanes_and_serialization() {
        let receipt = classify(&legacy_input(None), ObservationSource::Live);
        assert_eq!(
            receipt.allowed_lanes,
            vec!["read-only-investigation".to_string(), "evidence-refresh".to_string()]
        );
        assert!(receipt.blocked_lanes.contains(&"merge-drain".to_string()));
        assert!(receipt.blocked_lanes.contains(&"master-fix".to_string()));

        let json = must(serde_json::to_string(&receipt));
        assert!(json.contains("\"NOT_PROVEN\""));
        assert!(json.contains("\"LIVE\""));

        let parsed: QueueHealthReceipt = must(serde_json::from_str(&json));
        assert_eq!(parsed.mode, QueueMode::NotProven);
        assert_eq!(parsed.schema_version, QUEUE_HEALTH_SCHEMA_VERSION);
    }
}
