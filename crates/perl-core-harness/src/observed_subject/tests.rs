//! Fixtures and falsifiers for the observed runner subject fan-in (#12287).
//!
//! Positive fixtures prove the exact one-to-one denominator law, deterministic
//! bytes, and purity; each numbered falsifier is the discriminating test for
//! one law of the issue: an implementation missing that law fails the named
//! test (mutation control).

use crate::build::build_runner_plan;
use crate::invocation_trace::decode::{
    HeaderFrame, HeaderTag, RowFrame, RowTag, TerminalFrame, TerminalTag,
};
use crate::invocation_trace::model::{
    EffectiveInvocationField, EffectiveInvocationFields, TraceSubjectIdentity,
    UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use crate::invocation_trace::test_support::{all_observed_fields, find_entry, matrix, sha_hex};
use crate::invocation_trace::{
    build_invocation_trace_receipt, model::ObservedInvocationTraceInput,
};
use crate::model::{TargetMatrixEntry, UpstreamTargetMatrix};
use crate::observed_discovery::model::{
    DiscoverySubjectIdentity, ObservedDiscoveryInput, ProcessCompletion, RunnerArtifactIdentity,
    UpstreamDiscoveryReceiptV1,
};
use crate::observed_subject::model::{
    OrdinaryInstrumentedEquivalenceIdentity, ProducerSubjectIdentity, SubjectDiagnostic,
    SubjectJoinDisposition,
};
use crate::observed_subject::{
    ObservedRunnerSubjectInput, ObservedRunnerSubjectV1, ObservedSubjectState,
    build_observed_runner_subject, check_observed_runner_subject, observed_subject_freshness,
    observed_subject_payload_digest, validate_observed_runner_subject_shape,
};
use crate::runner_model::{DiscoveryFrame, RunnerKind, RunnerPlan, RunnerScheduling};
use color_eyre::eyre::{Result, eyre};

const TARGET: &str = "component_base";
const INSTRUMENT: &str = "trace-instrument-1";
const PREPARED_TREE: &str = "prepared-tree-generation-1";

fn ensure(outcome: Result<(), String>) -> Result<()> {
    outcome.map_err(|error| eyre!(error))
}

fn contract_digest(entry: &TargetMatrixEntry) -> Result<String> {
    let bytes = serde_json::to_vec(&entry.contract).map_err(|error| eyre!(error))?;
    Ok(sha_hex(&bytes))
}

/// Build a real parent discovery receipt with explicit capture controls so
/// falsifiers can vary truncation and capture identity independently of any
/// shared fixture constant.
fn parent_receipt(
    matrix: &UpstreamTargetMatrix,
    stdout: &str,
    stdout_truncated: bool,
    nonce: &str,
) -> Result<UpstreamDiscoveryReceiptV1> {
    let entry = find_entry(matrix, TARGET)?;
    crate::observed_discovery::build_observed_discovery_receipt(
        matrix,
        &ObservedDiscoveryInput {
            subject: DiscoverySubjectIdentity {
                repository_commit: "a".repeat(40),
                perl_ref: "perl-5.42.2".to_string(),
                prepared_tree_identity: PREPARED_TREE.to_string(),
                host_perl_identity: "host-perl-5.42.2".to_string(),
                matrix_fingerprint: matrix.fingerprint().map_err(|error| eyre!(error))?,
                target_id: TARGET.to_string(),
                target_contract_digest: contract_digest(entry)?,
                variant_target_id: None,
                instrumentation_id: Some(INSTRUMENT.to_string()),
            },
            runner: RunnerKind::Test,
            runner_artifact: RunnerArtifactIdentity {
                canonical_path: "t/TEST".to_string(),
                content_sha256: sha_hex(b"t/TEST"),
            },
            argv: vec!["./perl".to_string(), "../t/TEST".to_string(), "--dumptests".to_string()],
            working_directory: "t".to_string(),
            environment: std::collections::BTreeMap::from([(
                "LC_ALL".to_string(),
                "C".to_string(),
            )]),
            discovery_frame: DiscoveryFrame::CanonicalRepositoryPath,
            completion: ProcessCompletion::ExitStatus { code: 0 },
            process_nonce: nonce.to_string(),
            stdout_bytes: stdout.as_bytes().to_vec(),
            stdout_truncated,
            stderr_bytes: Vec::new(),
            stderr_truncated: false,
        },
    )
    .map_err(|error| eyre!(error))
}

/// One join scenario: a real parent receipt, its bound trace subject, and
/// exact wire emission through the decoder's own frame vocabulary.
struct Scenario {
    parent: UpstreamDiscoveryReceiptV1,
    subject: TraceSubjectIdentity,
    session: String,
    stdout: String,
}

impl Scenario {
    fn new(
        matrix: &UpstreamTargetMatrix,
        stdout: &str,
        truncated: bool,
        nonce: &str,
    ) -> Result<Self> {
        let parent = parent_receipt(matrix, stdout, truncated, nonce)?;
        let subject = TraceSubjectIdentity {
            repository_commit: parent.payload.subject.repository_commit.clone(),
            perl_ref: parent.payload.subject.perl_ref.clone(),
            prepared_tree_identity: parent.payload.subject.prepared_tree_identity.clone(),
            host_perl_identity: parent.payload.subject.host_perl_identity.clone(),
            matrix_fingerprint: parent.payload.subject.matrix_fingerprint.clone(),
            target_id: parent.payload.subject.target_id.clone(),
            target_contract_digest: parent.payload.subject.target_contract_digest.clone(),
            variant_target_id: parent.payload.subject.variant_target_id.clone(),
            instrumentation_id: parent.payload.subject.instrumentation_id.clone(),
            trace_session_id: "trace-session-0001".to_string(),
            parent_process_nonce: parent.payload.terminal.process_nonce.clone(),
            parent_receipt_digest: parent.payload_digest.clone(),
        };
        Ok(Self {
            parent,
            subject,
            session: "trace-session-0001".to_string(),
            stdout: stdout.to_string(),
        })
    }

    fn input(&self, trace_bytes: Vec<u8>) -> ObservedInvocationTraceInput {
        ObservedInvocationTraceInput {
            subject: self.subject.clone(),
            runner: RunnerKind::Test,
            runner_artifact: RunnerArtifactIdentity {
                canonical_path: "t/TEST".to_string(),
                content_sha256: sha_hex(b"t/TEST"),
            },
            parent_receipt: self.parent.clone(),
            trace_bytes,
            trace_truncated: false,
        }
    }

    /// Complete well-formed stream over complete rows for the given members.
    fn emit_complete(&self, members: &[&str]) -> Result<Vec<u8>> {
        self.emit_custom(
            members.iter().map(|member| (member.to_string(), all_observed_fields(member))),
        )
    }

    /// Stream over rows given as (member, fields), including partial fields.
    fn emit_custom(
        &self,
        specs: impl IntoIterator<Item = (String, EffectiveInvocationFields)>,
    ) -> Result<Vec<u8>> {
        let mut row_lines: Vec<String> = Vec::new();
        for (sequence, (member, fields)) in specs.into_iter().enumerate() {
            let frame = self.row_frame(&member, sequence as u32, fields);
            row_lines.push(serde_json::to_string(&frame).map_err(|error| eyre!(error))?);
        }
        let header = self.header_frame(row_lines.len() as u32);
        let terminal = self.terminal_frame(&row_lines, ProcessCompletion::ExitStatus { code: 0 });
        self.emit(&header, &row_lines, &terminal)
    }

    fn row_frame(
        &self,
        member: &str,
        sequence: u32,
        fields: EffectiveInvocationFields,
    ) -> RowFrame {
        RowFrame {
            frame: RowTag::Row,
            trace_session_id: self.session.clone(),
            sequence,
            row_id: format!("row-{sequence}-{member}"),
            member: member.to_string(),
            runner: RunnerKind::Test,
            target_id: self.subject.target_id.clone(),
            variant_target_id: self.subject.variant_target_id.clone(),
            instrumentation_id: self.subject.instrumentation_id.clone(),
            fields,
        }
    }

    fn header_frame(&self, expected_row_count: u32) -> HeaderFrame {
        HeaderFrame {
            frame: HeaderTag::Header,
            schema_version: UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION.to_string(),
            trace_session_id: self.session.clone(),
            parent_process_nonce: self.subject.parent_process_nonce.clone(),
            parent_receipt_digest: self.subject.parent_receipt_digest.clone(),
            expected_row_count,
            encoding: "utf-8".to_string(),
            newline: "lf".to_string(),
        }
    }

    fn terminal_frame(&self, row_lines: &[String], completion: ProcessCompletion) -> TerminalFrame {
        let mut integrity = Vec::new();
        for line in row_lines {
            integrity.extend_from_slice(line.as_bytes());
            integrity.push(b'\n');
        }
        TerminalFrame {
            frame: TerminalTag::Terminal,
            trace_session_id: self.session.clone(),
            row_count: row_lines.len() as u32,
            integrity_sha256: sha_hex(&integrity),
            completion,
        }
    }

    fn emit(
        &self,
        header: &HeaderFrame,
        row_lines: &[String],
        terminal: &TerminalFrame,
    ) -> Result<Vec<u8>> {
        let header_bytes = serde_json::to_vec(header).map_err(|error| eyre!("header: {error}"))?;
        let terminal_bytes =
            serde_json::to_vec(terminal).map_err(|error| eyre!("terminal: {error}"))?;
        let mut bytes = header_bytes;
        bytes.push(b'\n');
        for line in row_lines {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(&terminal_bytes);
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn plan_for(&self, raw_discovery: &str) -> Result<RunnerPlan> {
        build_runner_plan(
            &matrix()?,
            TARGET,
            RunnerKind::Test,
            raw_discovery.as_bytes(),
            RunnerScheduling::default(),
        )
        .map_err(|error| eyre!(error))
    }
}

/// The exact one-to-one fixture: discovery declares `declared`, the trace
/// observes `observed` completely, plan reconstructed from `declared`.
struct JoinFixture {
    scenario: Scenario,
    equivalence: Option<OrdinaryInstrumentedEquivalenceIdentity>,
}

impl JoinFixture {
    fn new(declared: &[&str]) -> Result<Self> {
        let matrix = matrix()?;
        let stdout: String = declared.join("\n") + "\n";
        let scenario = Scenario::new(&matrix, &stdout, false, "capture-0001")?;
        Ok(Self { scenario, equivalence: Some(default_equivalence()) })
    }

    fn set_equivalence(&mut self, equivalence: Option<OrdinaryInstrumentedEquivalenceIdentity>) {
        self.equivalence = equivalence;
    }

    fn join_with(&self, observed: &[&str]) -> Result<ObservedRunnerSubjectV1> {
        self.join_custom(self.scenario.emit_complete(observed)?)
    }

    fn join_custom(&self, trace_bytes: Vec<u8>) -> Result<ObservedRunnerSubjectV1> {
        let input = self.join_input(trace_bytes)?;
        build_observed_runner_subject(&matrix()?, &input).map_err(|error| eyre!(error))
    }

    /// Join whose observation stream is flagged truncated at capture time.
    fn join_with_truncated_trace(&self, trace_bytes: Vec<u8>) -> Result<ObservedRunnerSubjectV1> {
        let mut raw = self.scenario.input(trace_bytes);
        raw.trace_truncated = true;
        let trace = build_invocation_trace_receipt(&raw).map_err(|error| eyre!(error))?;
        let input = ObservedRunnerSubjectInput {
            producer: producer_from(&self.scenario.parent),
            plan: self.scenario.plan_for(&self.scenario.stdout)?,
            discovery: self.scenario.parent.clone(),
            trace,
            equivalence: self.equivalence.clone(),
        };
        build_observed_runner_subject(&matrix()?, &input).map_err(|error| eyre!(error))
    }

    fn join_input(&self, trace_bytes: Vec<u8>) -> Result<ObservedRunnerSubjectInput> {
        let trace = build_invocation_trace_receipt(&self.scenario.input(trace_bytes))
            .map_err(|error| eyre!(error))?;
        Ok(ObservedRunnerSubjectInput {
            producer: producer_from(&self.scenario.parent),
            plan: self.scenario.plan_for(&self.scenario.stdout)?,
            discovery: self.scenario.parent.clone(),
            trace,
            equivalence: self.equivalence.clone(),
        })
    }
}

fn default_equivalence() -> OrdinaryInstrumentedEquivalenceIdentity {
    OrdinaryInstrumentedEquivalenceIdentity {
        instrumentation_id: INSTRUMENT.to_string(),
        ordinary_runner_artifact_sha256: sha_hex(b"t/TEST"),
        instrumented_runner_artifact_sha256: sha_hex(b"t/TEST"),
        patch_subject_digest: sha_hex(b"patch-subject"),
    }
}

fn producer_from(parent: &UpstreamDiscoveryReceiptV1) -> ProducerSubjectIdentity {
    ProducerSubjectIdentity {
        repository_commit: parent.payload.subject.repository_commit.clone(),
        perl_ref: parent.payload.subject.perl_ref.clone(),
        prepared_tree_identity: parent.payload.subject.prepared_tree_identity.clone(),
        host_perl_identity: parent.payload.subject.host_perl_identity.clone(),
        matrix_fingerprint: parent.payload.subject.matrix_fingerprint.clone(),
        target_id: parent.payload.subject.target_id.clone(),
        target_contract_digest: parent.payload.subject.target_contract_digest.clone(),
        variant_target_id: parent.payload.subject.variant_target_id.clone(),
        runner: parent.payload.invocation.runner,
        runner_artifact: parent.payload.invocation.runner_artifact.clone(),
        working_directory: parent.payload.invocation.working_directory.clone(),
        environment_sha256: parent.payload.invocation.environment.sha256.clone(),
    }
}

fn diagnostics_for<'a>(
    payload: &'a crate::observed_subject::model::ObservedRunnerSubjectPayload,
    field_prefix: &str,
) -> Vec<&'a SubjectDiagnostic> {
    payload.diagnostics.iter().filter(|diag| diag.field.starts_with(field_prefix)).collect()
}

// ---------------------------------------------------------------------------
// Positive fixtures
// ---------------------------------------------------------------------------

#[test]
fn complete_join_proves_one_to_one_denominator_and_purity() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&members)?;
    assert_eq!(
        receipt.schema_version,
        crate::observed_subject::OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION
    );
    assert_eq!(receipt.payload.state, ObservedSubjectState::CompleteCurrent);
    // Denominator law: discovery membership == projected invocation set ==
    // plan membership.
    let joined_members: std::collections::BTreeSet<&str> = receipt
        .payload
        .rows
        .iter()
        .filter(|row| matches!(row.disposition, SubjectJoinDisposition::Joined))
        .map(|row| row.member_path.as_str())
        .collect();
    assert_eq!(joined_members, members.iter().copied().collect());
    for row in &receipt.payload.rows {
        assert!(matches!(row.disposition, SubjectJoinDisposition::Joined));
        assert!(row.projection_digest.is_some(), "joined row binds its projection digest");
        assert!(!row.row_fingerprint.is_empty());
        assert_eq!(row.field_counts.observed, 17);
    }
    let work = &receipt.payload.work;
    assert_eq!(work.joined_rows, 2);
    assert_eq!(work.missing_invocation_rows, 0);
    assert_eq!(work.extra_invocation_rows, 0);
    // Purity zeros are structural, recorded, and proven.
    assert_eq!(work.source_reads, 0);
    assert_eq!(work.filesystem_scans, 0);
    assert_eq!(work.runner_processes, 0);
    assert_eq!(work.direct_probe_inputs, 0);
    assert_eq!(work.reconstructed_fields, 0);
    let classes = &receipt.payload.evidence_classes;
    assert!(classes.contains(&crate::observed_discovery::model::EvidenceClass::ObservedUpstream));
    assert!(
        classes.contains(&crate::observed_discovery::model::EvidenceClass::InstrumentedUpstream)
    );
    ensure(validate_observed_runner_subject_shape(&receipt))?;
    ensure(check_observed_runner_subject(
        &matrix()?,
        &fixture.join_input(fixture.scenario.emit_complete(&members)?)?,
        &receipt,
    ))?;
    assert_eq!(
        observed_subject_freshness(&receipt, PREPARED_TREE),
        crate::observed_discovery::model::ReceiptFreshness::Current
    );
    Ok(())
}

#[test]
fn missing_invocation_is_named_and_never_completes() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    // Complete discovery plus one missing invocation cannot become complete.
    let receipt = fixture.join_with(&["t/base/if.t"])?;
    assert_eq!(receipt.payload.state, ObservedSubjectState::PartialMissingInvocation);
    let missing_diag = diagnostics_for(&receipt.payload, "invocation_observations")
        .into_iter()
        .find(|diag| diag.member_path.as_deref() == Some("t/base/cond.t"))
        .ok_or_else(|| eyre!("missing member must be named"))?;
    assert!(missing_diag.detail.contains("no invocation observation"));
    assert_eq!(receipt.payload.work.missing_invocation_rows, 1);
    assert_eq!(receipt.payload.work.joined_rows, 1);
    let cond_row = receipt
        .payload
        .rows
        .iter()
        .find(|row| row.member_path == "t/base/cond.t")
        .ok_or_else(|| eyre!("admitted member keeps its row"))?;
    assert!(matches!(cond_row.disposition, SubjectJoinDisposition::MissingInvocation));
    Ok(())
}

#[test]
fn duplicate_member_claim_is_recorded_not_last_writer_wins() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    // Two well-formed complete frames claim the same member with identical
    // projections; the second must never overwrite or silently join.
    let fields_a = all_observed_fields("t/base/if.t");
    let fields_b = all_observed_fields("t/base/if.t");
    let bytes = fixture.scenario.emit_custom([
        ("t/base/if.t".to_string(), fields_a),
        ("t/base/if.t".to_string(), fields_b),
    ])?;
    let receipt = fixture.join_custom(bytes)?;
    assert_eq!(receipt.payload.state, ObservedSubjectState::PartialConflictingInvocation);
    assert_eq!(receipt.payload.work.duplicate_invocation_rows, 1);
    let dup_diag = diagnostics_for(&receipt.payload, "invocation_observations")
        .into_iter()
        .find(|diag| diag.detail.contains("identical projections"))
        .ok_or_else(|| eyre!("duplicate projection must be diagnosed"))?;
    assert_eq!(dup_diag.member_path.as_deref(), Some("t/base/if.t"));
    Ok(())
}

#[test]
fn conflicting_member_claims_fail_closed_with_both_sequences() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let mut divergent = all_observed_fields("t/base/if.t");
    let EffectiveInvocationField::Observed { value } = &mut divergent.include_roots else {
        return Err(eyre!("include_roots must start observed"));
    };
    value.reverse();
    let bytes = fixture.scenario.emit_custom([
        ("t/base/if.t".to_string(), all_observed_fields("t/base/if.t")),
        ("t/base/if.t".to_string(), divergent),
    ])?;
    let receipt = fixture.join_custom(bytes)?;
    assert_eq!(receipt.payload.state, ObservedSubjectState::PartialConflictingInvocation);
    assert_eq!(receipt.payload.work.conflicting_invocation_rows, 1);
    let conflict_diag = diagnostics_for(&receipt.payload, "invocation_observations")
        .into_iter()
        .find(|diag| diag.detail.contains("conflicting projections"))
        .ok_or_else(|| eyre!("conflicting projections must be diagnosed"))?;
    assert_eq!(conflict_diag.member_path.as_deref(), Some("t/base/if.t"));
    Ok(())
}

#[test]
fn cross_run_parent_receipt_is_refused_by_name() -> Result<()> {
    // A trace captured against one discovery process cannot be attached to a
    // different discovery receipt even when both spell similar memberships.
    let bound = JoinFixture::new(&["t/base/if.t", "t/base/cond.t"])?;
    let trace_bytes = bound.scenario.emit_complete(&["t/base/if.t", "t/base/cond.t"])?;
    let trace = build_invocation_trace_receipt(&bound.scenario.input(trace_bytes))
        .map_err(|error| eyre!(error))?;

    let other_matrix = matrix()?;
    let other_scenario = Scenario::new(
        &other_matrix,
        "t/base/if.t\nt/base/cond.t\nt/base/unless.t\n",
        false,
        "capture-0002",
    )?;
    let input = ObservedRunnerSubjectInput {
        producer: producer_from(&other_scenario.parent),
        plan: other_scenario.plan_for("t/base/if.t\nt/base/cond.t\nt/base/unless.t\n")?,
        discovery: other_scenario.parent,
        trace,
        equivalence: Some(default_equivalence()),
    };
    let outcome = build_observed_runner_subject(&other_matrix, &input);
    let error = outcome.err().ok_or_else(|| eyre!("foreign pairing must be refused"))?;
    assert!(error.contains("parent receipt"), "named binding failure: {error}");
    Ok(())
}

#[test]
fn instrumented_without_equivalence_stays_partial() -> Result<()> {
    let members = ["t/base/if.t"];
    let mut fixture = JoinFixture::new(&members)?;
    fixture.set_equivalence(None);
    let receipt = fixture.join_with(&members)?;
    // Every arithmetic law holds, yet the ordinary-runner proposition was not
    // transferred by an exact #12286 relation, so this is not complete.
    assert_eq!(receipt.payload.state, ObservedSubjectState::InstrumentedWithoutEquivalence);
    assert!(receipt.payload.equivalence.is_none());
    assert_eq!(
        diagnostics_for(&receipt.payload, "equivalence").len(),
        1,
        "the unbound relation itself is named"
    );
    Ok(())
}

#[test]
fn foreign_equivalence_relation_fails_closed_by_name() -> Result<()> {
    let members = ["t/base/if.t"];
    let mut fixture = JoinFixture::new(&members)?;
    let mut foreign = default_equivalence();
    foreign.instrumentation_id = "other-instrument".to_string();
    fixture.set_equivalence(Some(foreign));
    let receipt = fixture.join_with(&members)?;
    assert_eq!(receipt.payload.state, ObservedSubjectState::InstrumentedWithoutEquivalence);
    let diag = diagnostics_for(&receipt.payload, "equivalence.instrumentation_id")
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("mismatching transfer-relation field must be named"))?;
    assert!(diag.detail.contains("other instrument") || diag.detail.contains(INSTRUMENT));
    Ok(())
}

#[test]
fn unobserved_field_keeps_row_partial_and_never_projects() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let mut partial = all_observed_fields("t/base/if.t");
    partial.include_roots =
        EffectiveInvocationField::NotObserved { reason: "instrument dropped frame".to_string() };
    let bytes = fixture.scenario.emit_custom([("t/base/if.t".to_string(), partial)])?;
    let receipt = fixture.join_custom(bytes)?;
    assert_eq!(receipt.payload.state, ObservedSubjectState::PartialUnobservedFields);
    let row = &receipt.payload.rows[0];
    assert!(matches!(row.disposition, SubjectJoinDisposition::PartialFields { .. }));
    assert!(row.projection_digest.is_none(), "a reconstructed plan may not fill the gap");
    assert_eq!(receipt.payload.work.partial_invocation_rows, 1);
    let field_diag = diagnostics_for(&receipt.payload, "fields.")
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("the unobserved field must be named"))?;
    assert_eq!(field_diag.field, "fields.include_roots");
    Ok(())
}

#[test]
fn renamed_suite_plan_refused_on_raw_discovery_identity() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    // The plan spells plausible members but was reconstructed from different
    // declared bytes; spelling agreement never replaces byte identity.
    let renamed_plan = fixture.scenario.plan_for("t/base/if.t\nt/base/renamed.t\n")?;
    let trace_bytes = fixture.scenario.emit_complete(&members)?;
    let trace = build_invocation_trace_receipt(&fixture.scenario.input(trace_bytes))
        .map_err(|error| eyre!(error))?;
    let input = ObservedRunnerSubjectInput {
        producer: producer_from(&fixture.scenario.parent),
        plan: renamed_plan,
        discovery: fixture.scenario.parent.clone(),
        trace,
        equivalence: Some(default_equivalence()),
    };
    let error = build_observed_runner_subject(&matrix()?, &input)
        .err()
        .ok_or_else(|| eyre!("renamed suite plan must be refused"))?;
    assert!(error.contains("raw_discovery_digest"), "byte identity must be named: {error}");
    Ok(())
}

#[test]
fn producer_identity_mismatch_refused_by_name() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let mut producer = producer_from(&fixture.scenario.parent);
    producer.host_perl_identity = "host-perl-other".to_string();
    let trace_bytes = fixture.scenario.emit_complete(&members)?;
    let trace = build_invocation_trace_receipt(&fixture.scenario.input(trace_bytes))
        .map_err(|error| eyre!(error))?;
    let input = ObservedRunnerSubjectInput {
        producer,
        plan: fixture.scenario.plan_for(&fixture.scenario.stdout)?,
        discovery: fixture.scenario.parent.clone(),
        trace,
        equivalence: Some(default_equivalence()),
    };
    let error = build_observed_runner_subject(&matrix()?, &input)
        .err()
        .ok_or_else(|| eyre!("another producer subject must be refused"))?;
    assert!(error.contains("host_perl_identity"), "field must be named: {error}");
    Ok(())
}

#[test]
fn truncated_discovery_cannot_become_complete() -> Result<()> {
    let matrix = matrix()?;
    let stdout = "t/base/if.t\nt/base/cond.t\n";
    let scenario = Scenario::new(&matrix, stdout, true, "capture-truncated")?;
    let trace_bytes = scenario.emit_complete(&["t/base/if.t", "t/base/cond.t"])?;
    let trace = build_invocation_trace_receipt(&scenario.input(trace_bytes))
        .map_err(|error| eyre!(error))?;
    let input = ObservedRunnerSubjectInput {
        producer: producer_from(&scenario.parent),
        plan: scenario.plan_for(stdout)?,
        discovery: scenario.parent.clone(),
        trace,
        equivalence: Some(default_equivalence()),
    };
    let receipt = build_observed_runner_subject(&matrix, &input).map_err(|error| eyre!(error))?;
    assert_ne!(receipt.payload.state, ObservedSubjectState::CompleteCurrent);
    assert_eq!(receipt.payload.state, ObservedSubjectState::NotProven);
    let state_diag = diagnostics_for(&receipt.payload, "discovery.state")
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("the incomplete discovery state must be named"))?;
    assert!(state_diag.detail.contains("cannot be complete"));
    // Per-member evidence remains observable but incomplete overall.
    assert_eq!(receipt.payload.work.complete_invocation_rows, 2);
    Ok(())
}

#[test]
fn truncated_observation_stream_blocks_completion_explicitly() -> Result<()> {
    // A capture cut at the retention bound leaves the invocation denominator
    // unknowable even though every retained row looks individually complete;
    // the aggregate count may never hide this.
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    let bytes = fixture.scenario.emit_complete(&members)?;
    let receipt = fixture.join_with_truncated_trace(bytes)?;
    assert_eq!(receipt.payload.state, ObservedSubjectState::NotProven);
    let decode_diag = diagnostics_for(&receipt.payload, "trace.decode")
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("the unprovable observation stream must be named"))?;
    assert!(decode_diag.detail.contains("incomplete"));
    // Retained evidence remains counted, never silently completed.
    assert_eq!(receipt.payload.work.invocation_rows_considered, 2);
    Ok(())
}

#[test]
fn determinism_holds_and_behavior_bearing_order_survives() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let first = JoinFixture::new(&members)?.join_with(&members)?;
    let second = JoinFixture::new(&members)?.join_with(&members)?;
    assert_eq!(first, second, "independent reconstructions are byte-identical");

    // Reordering behavior-bearing membership changes the subject identity
    // while remaining a valid one-to-one join under the new order.
    let flipped = ["t/base/cond.t", "t/base/if.t"];
    let reordered_fixture = JoinFixture::new(&flipped)?;
    let reordered = reordered_fixture.join_with(&flipped)?;
    assert_eq!(reordered.payload.state, ObservedSubjectState::CompleteCurrent);
    assert_ne!(first.payload_digest, reordered.payload_digest);
    assert_eq!(reordered.payload.rows[0].member_path, "t/base/cond.t");
    assert_eq!(reordered.payload.rows[0].discovery_ordinal, Some(0));
    assert_eq!(first.payload.rows[0].member_path, "t/base/if.t");
    Ok(())
}

#[test]
fn aggregation_precedence_is_total_and_typed() {
    use crate::observed_subject::build::{JoinOutcomes, aggregate_state};
    let base = || JoinOutcomes {
        upstream: None,
        subject_mismatch_rows: 0,
        missing: 0,
        extra: 0,
        conflicting_members: 0,
        duplicate_only_members: 0,
        partial_fields: 0,
        instrumented_without_equivalence: false,
    };
    // Nothing beats an inherited upstream shortfall.
    for mutation in [
        |o: &mut JoinOutcomes| o.missing = 1,
        |o: &mut JoinOutcomes| o.extra = 1,
        |o: &mut JoinOutcomes| o.conflicting_members = 1,
        |o: &mut JoinOutcomes| o.partial_fields = 1,
        |o: &mut JoinOutcomes| o.instrumented_without_equivalence = true,
    ] {
        let mut outcomes = base();
        outcomes.upstream = Some(ObservedSubjectState::Cancelled);
        mutation(&mut outcomes);
        assert_eq!(aggregate_state(&outcomes), ObservedSubjectState::Cancelled);
    }
    type Mutation = Box<dyn Fn(&mut JoinOutcomes)>;
    let ranked: Vec<(Mutation, ObservedSubjectState)> = vec![
        (
            Box::new(|o: &mut JoinOutcomes| {
                o.missing = 1;
                o.extra = 1;
                o.partial_fields = 1;
                o.instrumented_without_equivalence = true;
            }),
            ObservedSubjectState::PartialMissingInvocation,
        ),
        (
            Box::new(|o: &mut JoinOutcomes| {
                o.extra = 1;
                o.partial_fields = 1;
                o.instrumented_without_equivalence = true;
            }),
            ObservedSubjectState::PartialExtraInvocation,
        ),
        (
            Box::new(|o: &mut JoinOutcomes| {
                o.conflicting_members = 1;
                o.partial_fields = 1;
            }),
            ObservedSubjectState::PartialConflictingInvocation,
        ),
        (
            Box::new(|o: &mut JoinOutcomes| {
                o.partial_fields = 1;
                o.instrumented_without_equivalence = true;
            }),
            ObservedSubjectState::PartialUnobservedFields,
        ),
        (
            Box::new(|o: &mut JoinOutcomes| o.instrumented_without_equivalence = true),
            ObservedSubjectState::InstrumentedWithoutEquivalence,
        ),
    ];
    for (mutation, expected) in ranked {
        let mut outcomes = base();
        mutation(&mut outcomes);
        assert_eq!(aggregate_state(&outcomes), expected);
    }
    assert_eq!(aggregate_state(&base()), ObservedSubjectState::CompleteCurrent);
}

#[test]
fn counterfeit_structural_counters_are_rejected_by_shape_validation() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&members)?;

    // A forged receipt re-binds its own digest, so only the structural-zero
    // invariant can catch the counterfeit pure-work claim.
    let mut forged = receipt.clone();
    forged.payload.work.source_reads = 3;
    forged.payload_digest =
        observed_subject_payload_digest(&forged.payload).map_err(|error| eyre!(error))?;
    let error = validate_observed_runner_subject_shape(&forged)
        .err()
        .ok_or_else(|| eyre!("counterfeit purity claims must fail"))?;
    assert!(error.contains("source_reads"), "named zero invariant: {error}");

    let mut unpinned = receipt.clone();
    unpinned.payload.work.filesystem_scans = 7;
    assert!(validate_observed_runner_subject_shape(&unpinned).is_err());
    Ok(())
}

#[test]
fn receipt_roundtrips_and_denies_unknown_fields() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&members)?;
    let text = serde_json::to_string(&receipt).map_err(|error| eyre!(error))?;
    let reparsed: ObservedRunnerSubjectV1 =
        serde_json::from_str(&text).map_err(|error| eyre!(error))?;
    assert_eq!(reparsed, receipt);

    let mut value = serde_json::to_value(&receipt).map_err(|error| eyre!(error))?;
    value["surprise_field"] = serde_json::Value::Bool(true);
    assert!(
        serde_json::from_value::<ObservedRunnerSubjectV1>(value).is_err(),
        "unknown fields must be denied"
    );
    Ok(())
}

#[test]
fn freshness_helper_reports_current_and_stale() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&members)?;
    assert_eq!(
        observed_subject_freshness(&receipt, PREPARED_TREE),
        crate::observed_discovery::model::ReceiptFreshness::Current
    );
    assert_eq!(
        observed_subject_freshness(&receipt, "prepared-tree-generation-2"),
        crate::observed_discovery::model::ReceiptFreshness::Stale
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Review-driven falsifiers: plan forgery, redundant partials, receipt relabels
// ---------------------------------------------------------------------------

fn rebind_digest(mut receipt: ObservedRunnerSubjectV1) -> Result<ObservedRunnerSubjectV1> {
    receipt.payload_digest =
        observed_subject_payload_digest(&receipt.payload).map_err(|error| eyre!(error))?;
    Ok(receipt)
}

#[test]
fn forged_plan_membership_refused_by_matrix_rebuild() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    // Internally coherent foreign membership wearing this stream's raw digest:
    // structure checks and field agreement pass, so only the full matrix +
    // bytes rebuild can refuse it.
    let forged = build_runner_plan(
        &matrix()?,
        TARGET,
        RunnerKind::Test,
        b"t/base/if.t\nt/base/renamed.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| eyre!(error))?;
    let mut forged = forged;
    forged.raw_discovery_digest = sha_hex(fixture.scenario.stdout.as_bytes());
    let trace_bytes = fixture.scenario.emit_complete(&members)?;
    let trace = build_invocation_trace_receipt(&fixture.scenario.input(trace_bytes))
        .map_err(|error| eyre!(error))?;
    let input = ObservedRunnerSubjectInput {
        producer: producer_from(&fixture.scenario.parent),
        plan: forged,
        discovery: fixture.scenario.parent.clone(),
        trace,
        equivalence: Some(default_equivalence()),
    };
    let error = build_observed_runner_subject(&matrix()?, &input)
        .err()
        .ok_or_else(|| eyre!("forged plan membership must be refused"))?;
    assert!(error.contains("does not match"), "rebuild refusal: {error}");
    Ok(())
}

#[test]
fn partial_observation_beside_complete_projection_blocks_completion() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let mut partial = all_observed_fields("t/base/if.t");
    partial.include_roots =
        EffectiveInvocationField::NotObserved { reason: "instrument dropped frame".to_string() };
    let bytes = fixture.scenario.emit_custom([
        ("t/base/if.t".to_string(), all_observed_fields("t/base/if.t")),
        ("t/base/if.t".to_string(), partial),
    ])?;
    let receipt = fixture.join_custom(bytes)?;
    // The member keeps its complete projection, but the accepted second
    // observation destroys exact one-to-one; completion is refused.
    assert_eq!(receipt.payload.state, ObservedSubjectState::PartialUnobservedFields);
    assert_eq!(receipt.payload.work.joined_rows, 1);
    assert_eq!(receipt.payload.work.partial_invocation_rows, 1);
    let redundant = diagnostics_for(&receipt.payload, "fields.redundant_partial")
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("the redundant partial must be named"))?;
    assert_eq!(redundant.member_path.as_deref(), Some("t/base/if.t"));
    Ok(())
}

#[test]
fn relabeled_complete_receipt_rejected_by_shape_validation() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    let missing = fixture.join_with(&["t/base/if.t"])?;
    let mut relabeled = missing;
    relabeled.payload.state = ObservedSubjectState::CompleteCurrent;
    let relabeled = rebind_digest(relabeled)?;
    let error = validate_observed_runner_subject_shape(&relabeled)
        .err()
        .ok_or_else(|| eyre!("a relabeled complete state must fail"))?;
    assert!(error.contains("fully joined"), "coherence refusal: {error}");
    Ok(())
}

#[test]
fn emptied_complete_receipt_rejected_by_shape_validation() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&members)?;
    let mut emptied = receipt.clone();
    emptied.payload.rows.clear();
    emptied.payload.work.joined_rows = 0;
    for field in [
        "missing_invocation_rows",
        "extra_invocation_rows",
        "duplicate_invocation_rows",
        "conflicting_invocation_rows",
        "partial_invocation_rows",
    ] {
        set_work_counter(&mut emptied.payload.work, field, 0)?;
    }
    let emptied = rebind_digest(emptied)?;
    let error = validate_observed_runner_subject_shape(&emptied)
        .err()
        .ok_or_else(|| eyre!("an emptied complete receipt must fail"))?;
    assert!(error.contains("fully joined"), "emptiness refusal: {error}");
    Ok(())
}

#[test]
fn counterfeit_row_counter_rejected_even_when_state_partial() -> Result<()> {
    let members = ["t/base/if.t", "t/base/cond.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&["t/base/if.t"])?;
    let mut tampered = receipt;
    set_work_counter(&mut tampered.payload.work, "missing_invocation_rows", 7)?;
    let tampered = rebind_digest(tampered)?;
    let error = validate_observed_runner_subject_shape(&tampered)
        .err()
        .ok_or_else(|| eyre!("counterfeit row counters must fail"))?;
    assert!(
        error.contains("records 7 but its retained rows derive 1"),
        "named counter disagreement: {error}"
    );
    Ok(())
}

fn set_work_counter(
    work: &mut crate::observed_subject::model::JoinWork,
    field: &str,
    value: u64,
) -> Result<()> {
    match field {
        "joined_rows" => work.joined_rows = value,
        "missing_invocation_rows" => work.missing_invocation_rows = value,
        "extra_invocation_rows" => work.extra_invocation_rows = value,
        "duplicate_invocation_rows" => work.duplicate_invocation_rows = value,
        "conflicting_invocation_rows" => work.conflicting_invocation_rows = value,
        "partial_invocation_rows" => work.partial_invocation_rows = value,
        other => return Err(eyre!("fixture helper does not touch counter {other}")),
    }
    Ok(())
}

#[test]
fn normalized_identity_tamper_breaks_row_fingerprint() -> Result<()> {
    let members = ["t/base/if.t"];
    let fixture = JoinFixture::new(&members)?;
    let receipt = fixture.join_with(&members)?;
    let mut tampered = receipt;
    let item = tampered.payload.rows[0]
        .normalized
        .as_mut()
        .ok_or_else(|| eyre!("joined rows retain their normalized source identity"))?;
    item.canonical_path.push('x');
    // The outer digest alone cannot hide the tamper: shape validation
    // recomputes each row fingerprint, which covers every field including the
    // normalized source identity.
    let recomputed = crate::observed_subject::build::row_fingerprint(&tampered.payload.rows[0])
        .map_err(|error| eyre!(error))?;
    assert_ne!(
        recomputed, tampered.payload.rows[0].row_fingerprint,
        "the normalized identity participates in the row fingerprint"
    );
    tampered.payload_digest =
        observed_subject_payload_digest(&tampered.payload).map_err(|error| eyre!(error))?;
    let error = validate_observed_runner_subject_shape(&tampered)
        .err()
        .ok_or_else(|| eyre!("tampered normalized identity must fail"))?;
    assert!(error.contains("fingerprint"), "fingerprint refusal: {error}");
    Ok(())
}
