//! Shared fixture builders for the invocation-trace contract suites: exact
//! wire-frame emission through the decoder's own vocabulary, a real parent
//! discovery receipt from the pinned matrix, and complete-field fixtures for
//! the adapter seams. Test-only; never compiled into the production surface.

use crate::invocation_trace::decode::{
    HeaderFrame, HeaderTag, RowFrame, RowTag, TerminalFrame, TerminalTag,
};
use crate::invocation_trace::model::{
    EffectiveInvocationField, EffectiveInvocationFields, EffectiveInvocationRow, RowSubjectBinding,
    ScriptRole, TraceHeader, TraceSubjectIdentity, TraceTerminal,
};
use crate::io::read_matrix;
use crate::model::{TargetMatrixEntry, UpstreamTargetMatrix};
use crate::observed_discovery::model::{
    EnvironmentIdentity, ObservedDiscoveryInput, ProcessCompletion, RunnerArtifactIdentity,
    UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::{DiscoveryFrame, RunnerKind, RunnerScheduling, SourceForm};
use color_eyre::eyre::{Result, eyre};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

pub fn matrix() -> Result<UpstreamTargetMatrix> {
    read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))
}

pub fn find_entry<'m>(
    matrix: &'m UpstreamTargetMatrix,
    target_id: &str,
) -> Result<&'m TargetMatrixEntry> {
    matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == target_id)
        .ok_or_else(|| eyre!("matrix has no target {target_id}"))
}

pub fn sha_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Build a real parent discovery receipt over the given accepted members.
pub fn build_parent(
    matrix: &UpstreamTargetMatrix,
    target_id: &str,
    stdout: &str,
) -> Result<UpstreamDiscoveryReceiptV1> {
    let entry = find_entry(matrix, target_id)?;
    let contract_digest =
        sha_hex(&serde_json::to_vec(&entry.contract).map_err(|error| eyre!(error))?);
    let input = ObservedDiscoveryInput {
        subject: crate::observed_discovery::model::DiscoverySubjectIdentity {
            repository_commit: "a".repeat(40),
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree_identity: "prepared-tree-generation-1".to_string(),
            host_perl_identity: "host-perl-5.42.2".to_string(),
            matrix_fingerprint: matrix.fingerprint().map_err(|error| eyre!(error))?,
            target_id: target_id.to_string(),
            target_contract_digest: contract_digest,
            variant_target_id: None,
            instrumentation_id: Some("trace-instrument-1".to_string()),
        },
        runner: RunnerKind::Test,
        runner_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha_hex(b"t/TEST"),
        },
        argv: vec!["./perl".to_string(), "../t/TEST".to_string(), "--dumptests".to_string()],
        working_directory: "t".to_string(),
        environment: BTreeMap::from([("LC_ALL".to_string(), "C".to_string())]),
        discovery_frame: DiscoveryFrame::CanonicalRepositoryPath,
        completion: ProcessCompletion::ExitStatus { code: 0 },
        process_nonce: "capture-0001".to_string(),
        stdout_bytes: stdout.as_bytes().to_vec(),
        stdout_truncated: false,
        stderr_bytes: Vec::new(),
        stderr_truncated: false,
    };
    crate::observed_discovery::build::build_observed_discovery_receipt(matrix, &input)
        .map_err(|error| eyre!(error))
}

/// Complete observed fields for one member, with reviewable mutations.
pub fn all_observed_fields(member: &str) -> EffectiveInvocationFields {
    EffectiveInvocationFields {
        member_identity: EffectiveInvocationField::Observed { value: member.to_string() },
        source_form: EffectiveInvocationField::Observed { value: SourceForm::DotT },
        script_path: EffectiveInvocationField::Observed { value: member.to_string() },
        script_role: EffectiveInvocationField::Observed { value: ScriptRole::Base },
        run_cwd: EffectiveInvocationField::Observed { value: "t".to_string() },
        return_directory: EffectiveInvocationField::Observed { value: "t".to_string() },
        interpreter_switches: EffectiveInvocationField::Observed {
            value: vec!["-I../lib".to_string()],
        },
        include_roots: EffectiveInvocationField::Observed {
            value: vec!["../lib".to_string(), "../t/lib".to_string()],
        },
        test_init: EffectiveInvocationField::Observed {
            value: crate::invocation_trace::model::TestInitClass::Standard,
        },
        taint_mode: EffectiveInvocationField::Observed {
            value: crate::invocation_trace::model::TaintMode::None,
        },
        utf8_mode: EffectiveInvocationField::Observed {
            value: crate::invocation_trace::model::Utf8Switch::None,
        },
        wrapper_arguments: EffectiveInvocationField::Observed { value: Vec::new() },
        script_arguments: EffectiveInvocationField::Observed { value: Vec::new() },
        environment: EffectiveInvocationField::Observed {
            value: EnvironmentIdentity {
                variables: BTreeMap::from([("LC_ALL".to_string(), "C".to_string())]),
                sha256: sha_hex(b"LC_ALL=C\n"),
            },
        },
        scheduling: EffectiveInvocationField::Observed { value: RunnerScheduling::default() },
        capture_point: EffectiveInvocationField::Observed {
            value: crate::invocation_trace::model::CapturePoint::InvocationDecision,
        },
        upstream_operation: EffectiveInvocationField::Observed {
            value: "t/TEST runtests invocation decision".to_string(),
        },
    }
}

pub fn row_subject_for(session: &str, member: &str) -> RowSubjectBinding {
    RowSubjectBinding {
        trace_session_id: session.to_string(),
        parent_receipt_digest: String::new(),
        parent_member_path: member.to_string(),
        runner: RunnerKind::Test,
        target_id: "component_base".to_string(),
        variant_target_id: None,
        instrumentation_id: Some("trace-instrument-1".to_string()),
    }
}

/// One trace fixture: a real parent receipt, the trace subject bound to it,
/// and exact wire emission through the decoder's own frame vocabulary.
pub struct TraceFixture {
    pub matrix: UpstreamTargetMatrix,
    pub parent: UpstreamDiscoveryReceiptV1,
    pub subject: TraceSubjectIdentity,
    pub session: String,
}

impl TraceFixture {
    pub fn new(target_id: &str, stdout: &str) -> Result<Self> {
        let matrix = matrix()?;
        let parent = build_parent(&matrix, target_id, stdout)?;
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
        Ok(Self { matrix, parent, subject, session: "trace-session-0001".to_string() })
    }

    /// The same fixture under a different trace session identity.
    pub fn clone_with_session(&self, session: &str) -> Self {
        let mut clone = Self {
            matrix: self.matrix.clone(),
            parent: self.parent.clone(),
            subject: self.subject.clone(),
            session: session.to_string(),
        };
        clone.subject.trace_session_id = session.to_string();
        clone
    }

    pub fn row(
        &self,
        member: &str,
        sequence: u32,
        fields: EffectiveInvocationFields,
    ) -> EffectiveInvocationRow {
        EffectiveInvocationRow {
            sequence,
            row_id: format!("row-{sequence}-{member}"),
            raw_line: String::new(),
            subject: RowSubjectBinding {
                trace_session_id: self.session.clone(),
                parent_receipt_digest: self.parent.payload_digest.clone(),
                parent_member_path: member.to_string(),
                runner: RunnerKind::Test,
                target_id: self.subject.target_id.clone(),
                variant_target_id: self.subject.variant_target_id.clone(),
                instrumentation_id: self.subject.instrumentation_id.clone(),
            },
            fields,
            disposition: crate::invocation_trace::model::TraceRowDisposition::Accepted,
            state: crate::invocation_trace::model::InvocationObservationState::NotProven,
            row_fingerprint: String::new(),
            projection: crate::invocation_trace::model::ProjectionRecord::Rejected {
                reason: crate::invocation_trace::model::ProjectionRejectionKind::FrameNotAccepted,
            },
        }
    }

    pub fn expected_binding(
        &self,
        row: &EffectiveInvocationRow,
    ) -> crate::invocation_trace::adapter::ExpectedInvocationBinding {
        crate::invocation_trace::adapter::ExpectedInvocationBinding::from_subject(
            &self.subject,
            &row.subject,
        )
    }

    /// Wire row frame for one member with the given fields.
    pub fn row_frame(
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

    /// Header frame bound to this fixture's subject.
    pub fn header_frame(&self, expected_row_count: u32) -> HeaderFrame {
        HeaderFrame {
            frame: HeaderTag::Header,
            schema_version:
                crate::invocation_trace::model::UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION.to_string(),
            trace_session_id: self.session.clone(),
            parent_process_nonce: self.subject.parent_process_nonce.clone(),
            parent_receipt_digest: self.parent.payload_digest.clone(),
            expected_row_count,
            encoding: "utf-8".to_string(),
            newline: "lf".to_string(),
        }
    }

    /// Terminal frame over the exact row lines already serialized.
    pub fn terminal_frame(
        &self,
        row_lines: &[String],
        completion: ProcessCompletion,
    ) -> TerminalFrame {
        let mut bytes = Vec::new();
        for line in row_lines {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
        }
        TerminalFrame {
            frame: TerminalTag::Terminal,
            trace_session_id: self.session.clone(),
            row_count: row_lines.len() as u32,
            integrity_sha256: sha_hex(&bytes),
            completion,
        }
    }

    /// Serialize frames into the exact JSONL stream bytes. Fixture emission
    /// fails loudly on serialization errors: a silently dropped frame would
    /// let a dependent test pass for the wrong reason.
    pub fn emit(
        &self,
        header: &HeaderFrame,
        row_lines: &[String],
        terminal: &TerminalFrame,
    ) -> Vec<u8> {
        let header_bytes = serde_json::to_vec(header)
            .unwrap_or_else(|error| panic!("fixture header must serialize: {error}"));
        let terminal_bytes = serde_json::to_vec(terminal)
            .unwrap_or_else(|error| panic!("fixture terminal must serialize: {error}"));
        let mut bytes = header_bytes;
        bytes.push(b'\n');
        for line in row_lines {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(&terminal_bytes);
        bytes.push(b'\n');
        bytes
    }

    /// Full well-formed stream over complete rows for the given members.
    pub fn emit_complete(&self, members: &[&str]) -> Result<Vec<u8>> {
        let row_lines: Vec<String> = members
            .iter()
            .enumerate()
            .map(|(sequence, member)| {
                let frame = self.row_frame(member, sequence as u32, all_observed_fields(member));
                serde_json::to_string(&frame).map_err(|error| eyre!(error))
            })
            .collect::<Result<Vec<_>>>()?;
        let header = self.header_frame(row_lines.len() as u32);
        let terminal = self.terminal_frame(&row_lines, ProcessCompletion::ExitStatus { code: 0 });
        Ok(self.emit(&header, &row_lines, &terminal))
    }

    /// Construction input for the given trace bytes.
    pub fn input(
        &self,
        trace_bytes: Vec<u8>,
    ) -> crate::invocation_trace::model::ObservedInvocationTraceInput {
        crate::invocation_trace::model::ObservedInvocationTraceInput {
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
}
