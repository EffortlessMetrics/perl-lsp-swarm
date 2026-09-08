//! Strict byte-level decoder for effective-invocation trace streams.
//!
//! The decoder consumes exact raw bytes of one independently framed JSONL
//! dialect (`upstream_effective_invocation_trace.v1`): one header frame, zero
//! or more ordered unique row frames, one terminal frame. It never sorts,
//! deduplicates, repairs, or replaces rows; framing violations are retained as
//! typed dispositions and stream-level defects as typed outcomes. It never
//! touches the filesystem or the runner.

use crate::invocation_trace::model::{
    EffectiveInvocationFields, EffectiveInvocationRow, InvocationObservationState, MAX_TRACE_ROWS,
    RowSubjectBinding, TraceHeader, TraceRowDisposition, TraceStreamOutcome, TraceTerminal,
    TraceWork, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use crate::observed_discovery::model::ProcessCompletion;
use crate::runner_model::RunnerKind;
use serde::Deserialize;
use std::collections::BTreeSet;

/// Wire shape of the header frame. Unknown keys are rejected by
/// `deny_unknown_fields`; the exact vocabulary is the contract.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(deny_unknown_fields)]
pub(crate) struct HeaderFrame {
    /// Always `header`; deserialization itself enforces the tag spelling.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Serde enforces the tag spelling; the dispatcher reads the raw tag before this typed field is consulted."
        )
    )]
    pub frame: HeaderTag,
    /// Stream schema identity.
    pub schema_version: String,
    /// Trace session identity.
    pub trace_session_id: String,
    /// Parent process capture identity.
    pub parent_process_nonce: String,
    /// Parent discovery receipt payload digest.
    pub parent_receipt_digest: String,
    /// Producer-declared expected row count.
    pub expected_row_count: u32,
    /// Declared encoding.
    pub encoding: String,
    /// Declared newline policy.
    pub newline: String,
}

/// Wire shape of the row frame.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(deny_unknown_fields)]
pub(crate) struct RowFrame {
    /// Always `row`; deserialization itself enforces the tag spelling.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Serde enforces the tag spelling; the dispatcher reads the raw tag before this typed field is consulted."
        )
    )]
    pub frame: RowTag,
    /// Trace session identity shared with the header.
    pub trace_session_id: String,
    /// Zero-based sequence position.
    pub sequence: u32,
    /// Producer-assigned stable row identity.
    pub row_id: String,
    /// Canonical member identity.
    pub member: String,
    /// Runner route the frame was captured under.
    pub runner: RunnerKind,
    /// Target identity.
    pub target_id: String,
    /// Environment-variant target when applicable.
    pub variant_target_id: Option<String>,
    /// Instrumentation subject when applicable.
    pub instrumentation_id: Option<String>,
    /// Typed per-field observation states.
    pub fields: EffectiveInvocationFields,
}

/// Wire shape of the terminal frame.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalFrame {
    /// Always `terminal`; deserialization itself enforces the tag spelling.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Serde enforces the tag spelling; the dispatcher reads the raw tag before this typed field is consulted."
        )
    )]
    pub frame: TerminalTag,
    /// Trace session identity shared with the header.
    pub trace_session_id: String,
    /// Producer-declared row count.
    pub row_count: u32,
    /// SHA-256 over the concatenated raw row lines including LF terminators.
    pub integrity_sha256: String,
    /// Typed terminal outcome of the traced runner process.
    pub completion: ProcessCompletion,
}

/// Frame tag vocabulary; foreign tags are rejected before field decoding.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(rename_all = "snake_case")]
pub(crate) enum HeaderTag {
    /// The `header` tag.
    Header,
}

/// Frame tag vocabulary for rows.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(rename_all = "snake_case")]
pub(crate) enum RowTag {
    /// The `row` tag.
    Row,
}

/// Frame tag vocabulary for the terminal frame.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(rename_all = "snake_case")]
pub(crate) enum TerminalTag {
    /// The `terminal` tag.
    Terminal,
}

/// Result of strictly decoding one trace stream.
pub(crate) struct DecodedTrace {
    /// Stream-level decode outcome.
    pub outcome: TraceStreamOutcome,
    /// Decoded header facts; `None` only when the stream never decoded a
    /// valid header.
    pub header: Option<TraceHeader>,
    /// Decoded terminal facts; `None` when the terminal frame is absent,
    /// invalid, or unreachable.
    pub terminal: Option<TraceTerminal>,
    /// Rows in original observed order with dispositions assigned.
    pub rows: Vec<EffectiveInvocationRow>,
    /// Frames (header, rows, terminal) consumed.
    pub frames_consumed: u64,
}

/// Decode one raw trace stream strictly. Err is reserved for the hard
/// decoded-row bound; every framing defect is a typed outcome or disposition.
pub(crate) fn decode_trace_stream(raw: &[u8]) -> Result<DecodedTrace, String> {
    let mut state = DecodeState::default();
    if raw.is_empty() {
        state.fail("trace stream is empty".to_string());
        return Ok(state.finish());
    }
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(error) => {
            state.fail(format!("trace stream is not valid UTF-8: {error}"));
            return Ok(state.finish());
        }
    };

    // Newline policy: every frame line is LF-terminated. A stream ending
    // without LF has a partial final row; raw control bytes (including CR)
    // are framing drift outside the declared policy. Splitting always yields
    // one trailing segment after the final LF (or partial text): it is never
    // a frame and always comes off before decoding.
    let has_partial_tail = !text.ends_with('\n');
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();

    for line in &lines {
        state.decode_line(line)?;
        if state.has_failed() {
            return Ok(state.finish());
        }
    }
    if has_partial_tail {
        state.fail("partial final row: the last frame line is not LF-terminated".to_string());
        return Ok(state.finish());
    }
    state.finish_checks()?;
    Ok(state.finish())
}

/// Accumulating decoder state so every malformed path keeps the rows and
/// frames already consumed as typed evidence.
#[derive(Default)]
struct DecodeState {
    outcome: Option<TraceStreamOutcome>,
    header: Option<TraceHeader>,
    terminal: Option<TraceTerminal>,
    rows: Vec<EffectiveInvocationRow>,
    seen_row_ids: BTreeSet<String>,
    row_line_bytes: Vec<u8>,
    frames_consumed: u64,
}

impl DecodeState {
    fn fail(&mut self, reason: String) {
        if self.outcome.is_none() {
            self.outcome = Some(TraceStreamOutcome::Malformed { reason });
        }
    }

    fn has_failed(&self) -> bool {
        self.outcome.is_some()
    }

    fn decode_line(&mut self, line: &str) -> Result<(), String> {
        if line.bytes().any(|byte| byte.is_ascii_control()) {
            self.fail(
                "frame line carries framing outside the declared lf newline policy".to_string(),
            );
            return Ok(());
        }
        if line.trim().is_empty() {
            self.fail("blank frame line is not a trace frame".to_string());
            return Ok(());
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                self.fail(format!("frame line is not valid JSON: {error}"));
                return Ok(());
            }
        };
        let tag = value.get("frame").and_then(|tag| tag.as_str()).map(str::to_string);
        match tag.as_deref() {
            Some("header") => self.decode_header(value, line),
            Some("row") => self.decode_row(value, line),
            Some("terminal") => self.decode_terminal(value),
            other => {
                self.fail(format!(
                    "unknown frame tag {other:?}; the trace vocabulary admits header, row, and \
                     terminal only"
                ));
                Ok(())
            }
        }
    }

    fn decode_header(&mut self, value: serde_json::Value, _line: &str) -> Result<(), String> {
        if self.header.is_some() {
            self.fail("duplicate header frame".to_string());
            return Ok(());
        }
        match serde_json::from_value::<HeaderFrame>(value) {
            Ok(frame) => {
                self.header = Some(TraceHeader {
                    schema_version: frame.schema_version,
                    trace_session_id: frame.trace_session_id,
                    parent_process_nonce: frame.parent_process_nonce,
                    parent_receipt_digest: frame.parent_receipt_digest,
                    expected_row_count: frame.expected_row_count,
                    encoding: frame.encoding,
                    newline: frame.newline,
                });
                self.frames_consumed += 1;
                Ok(())
            }
            Err(error) => {
                self.fail(format!("header frame violates the exact vocabulary: {error}"));
                Ok(())
            }
        }
    }

    fn decode_row(&mut self, value: serde_json::Value, line: &str) -> Result<(), String> {
        if self.header.is_none() {
            self.fail("row frame appears before the header frame".to_string());
            return Ok(());
        }
        if self.terminal.is_some() {
            self.fail("row frame appears after the terminal frame".to_string());
            return Ok(());
        }
        if self.rows.len() >= MAX_TRACE_ROWS {
            return Err(format!("trace stream exceeds the {MAX_TRACE_ROWS} decoded-row bound"));
        }
        self.row_line_bytes.extend_from_slice(line.as_bytes());
        self.row_line_bytes.push(b'\n');
        self.frames_consumed += 1;
        match serde_json::from_value::<RowFrame>(value) {
            Ok(frame) => {
                let expected_sequence = self.rows.len() as u32;
                let session = self
                    .header
                    .as_ref()
                    .map(|header| header.trace_session_id.clone())
                    .unwrap_or_default();
                let parent_digest = self
                    .header
                    .as_ref()
                    .map(|header| header.parent_receipt_digest.clone())
                    .unwrap_or_default();
                let disposition = if frame.trace_session_id != session {
                    TraceRowDisposition::CrossRunInterleaved {
                        session_id: frame.trace_session_id.clone(),
                    }
                } else if !self.seen_row_ids.insert(frame.row_id.clone()) {
                    TraceRowDisposition::DuplicateRowId { row_id: frame.row_id.clone() }
                } else if frame.sequence != expected_sequence {
                    TraceRowDisposition::OutOfOrderSequence {
                        expected: expected_sequence,
                        actual: frame.sequence,
                    }
                } else {
                    TraceRowDisposition::Accepted
                };
                self.rows.push(EffectiveInvocationRow {
                    sequence: frame.sequence,
                    row_id: frame.row_id,
                    raw_line: line.to_string(),
                    subject: RowSubjectBinding {
                        trace_session_id: frame.trace_session_id,
                        parent_receipt_digest: parent_digest,
                        parent_member_path: frame.member,
                        runner: frame.runner,
                        target_id: frame.target_id,
                        variant_target_id: frame.variant_target_id,
                        instrumentation_id: frame.instrumentation_id,
                    },
                    fields: frame.fields,
                    disposition,
                    // Row state and projection are derived after the
                    // receipt-level subject binding is known; decoding leaves
                    // the neutral values.
                    state: InvocationObservationState::NotProven,
                    row_fingerprint: crate::build::sha256_bytes(line.as_bytes()),
                    projection: crate::invocation_trace::model::ProjectionRecord::Rejected {
                        reason: crate::invocation_trace::model::ProjectionRejectionKind::FrameNotAccepted,
                    },
                });
                Ok(())
            }
            Err(error) => {
                // The frame is JSON but violates the row vocabulary: retain it
                // as a typed malformed frame in original position.
                let expected_sequence = self.rows.len() as u32;
                self.rows.push(malformed_frame_row(line, expected_sequence, &error));
                Ok(())
            }
        }
    }

    fn decode_terminal(&mut self, value: serde_json::Value) -> Result<(), String> {
        if self.header.is_none() {
            self.fail("terminal frame appears before the header frame".to_string());
            return Ok(());
        }
        if self.terminal.is_some() {
            self.fail("duplicate terminal frame".to_string());
            return Ok(());
        }
        match serde_json::from_value::<TerminalFrame>(value) {
            Ok(frame) => {
                let session = self
                    .header
                    .as_ref()
                    .map(|header| header.trace_session_id.clone())
                    .unwrap_or_default();
                if frame.trace_session_id != session {
                    self.fail(
                        "terminal frame carries a foreign trace session identity".to_string(),
                    );
                    return Ok(());
                }
                self.terminal = Some(TraceTerminal {
                    row_count: frame.row_count,
                    integrity_sha256: frame.integrity_sha256,
                    completion: frame.completion,
                });
                self.frames_consumed += 1;
                Ok(())
            }
            Err(error) => {
                self.fail(format!("terminal frame violates the exact vocabulary: {error}"));
                Ok(())
            }
        }
    }

    fn finish_checks(&mut self) -> Result<(), String> {
        let Some(header) = self.header.clone() else {
            self.fail("trace stream carries no header frame".to_string());
            return Ok(());
        };
        if header.schema_version != UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION {
            self.fail(format!("unknown trace schema {}", header.schema_version));
            return Ok(());
        }
        if header.encoding != "utf-8" || header.newline != "lf" {
            self.fail(
                "declared encoding/newline policy is outside the admitted utf-8/lf contract"
                    .to_string(),
            );
            return Ok(());
        }
        let Some(terminal) = self.terminal.clone() else {
            self.fail("missing terminal frame".to_string());
            return Ok(());
        };
        if terminal.row_count != self.rows.len() as u32 {
            self.fail(format!(
                "terminal frame declares {} rows but the stream carries {}",
                terminal.row_count,
                self.rows.len()
            ));
            return Ok(());
        }
        if header.expected_row_count != self.rows.len() as u32 {
            self.fail(format!(
                "header frame declares {} rows but the stream carries {}",
                header.expected_row_count,
                self.rows.len()
            ));
            return Ok(());
        }
        let integrity = crate::build::sha256_bytes(&self.row_line_bytes);
        if integrity != terminal.integrity_sha256 {
            self.fail(
                "terminal frame integrity digest does not bind the retained row lines".to_string(),
            );
        }
        Ok(())
    }

    fn finish(mut self) -> DecodedTrace {
        if self.outcome.is_none() {
            // finish_checks already ran for the complete path; a missing
            // outcome here means the empty stream carried no frames at all.
            if self.header.is_none() {
                self.outcome = Some(TraceStreamOutcome::Malformed {
                    reason: "trace stream is empty".to_string(),
                });
            } else {
                self.outcome = Some(TraceStreamOutcome::Complete);
            }
        }
        DecodedTrace {
            outcome: self.outcome.unwrap_or(TraceStreamOutcome::Malformed {
                reason: "unreachable decode outcome".to_string(),
            }),
            header: self.header,
            terminal: self.terminal,
            rows: self.rows,
            frames_consumed: self.frames_consumed,
        }
    }
}

fn malformed_frame_row(
    line: &str,
    expected_sequence: u32,
    error: &serde_json::Error,
) -> EffectiveInvocationRow {
    EffectiveInvocationRow {
        sequence: expected_sequence,
        row_id: format!("malformed-frame-{expected_sequence}"),
        raw_line: line.to_string(),
        subject: RowSubjectBinding {
            trace_session_id: String::new(),
            parent_receipt_digest: String::new(),
            parent_member_path: String::new(),
            runner: RunnerKind::Test,
            target_id: String::new(),
            variant_target_id: None,
            instrumentation_id: None,
        },
        // Nothing in a malformed frame was observed; every field stays
        // honestly not-observed instead of borrowing a sibling frame's value.
        fields: EffectiveInvocationFields::default(),
        disposition: TraceRowDisposition::MalformedFrame { reason: error.to_string() },
        state: InvocationObservationState::NotProven,
        row_fingerprint: crate::build::sha256_bytes(line.as_bytes()),
        projection: crate::invocation_trace::model::ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::FrameNotAccepted,
        },
    }
}

/// Derive per-row and field work counters from a decode result plus derived
/// row states. Row states must already be assigned.
pub(crate) fn work_from_rows(
    trace_bytes: usize,
    frames_consumed: u64,
    rows: &[EffectiveInvocationRow],
    projections_attempted: u64,
    projections_accepted: u64,
) -> TraceWork {
    let mut work = TraceWork {
        trace_bytes_consumed: trace_bytes as u64,
        trace_frames_consumed: frames_consumed,
        trace_rows_consumed: rows.len() as u64,
        canonical_plan_projections_attempted: projections_attempted,
        canonical_plan_projections_accepted: projections_accepted,
        canonical_plan_projections_rejected: projections_attempted
            .saturating_sub(projections_accepted),
        ..TraceWork::default()
    };
    for row in rows {
        match row.state {
            InvocationObservationState::ObservedComplete => work.complete_rows += 1,
            InvocationObservationState::ObservedPartial => work.partial_rows += 1,
            InvocationObservationState::RunnerFailed => work.runner_failed_rows += 1,
            InvocationObservationState::InstrumentFailed => work.instrument_failed_rows += 1,
            InvocationObservationState::SubjectMismatch => work.subject_mismatch_rows += 1,
            // `stale` is a consumer-side judgment; a retained row can never
            // carry it, so it counts as unproven work.
            InvocationObservationState::Stale | InvocationObservationState::NotProven => {
                work.not_proven_rows += 1
            }
        }
        if matches!(row.disposition, TraceRowDisposition::MalformedFrame { .. }) {
            work.malformed_rows += 1;
        }
        if row.disposition.is_conflicting() {
            work.conflicting_rows += 1;
        }
        let counts = row.fields.state_counts();
        work.fields_observed += counts.observed;
        work.fields_not_applicable += counts.not_applicable;
        work.fields_not_observed += counts.not_observed;
        work.fields_ambiguous += counts.ambiguous;
        work.fields_malformed += counts.malformed;
        work.fields_instrument_failed += counts.instrument_failure;
    }
    work
}

/// Completeness law for one row: derive the single row state.
///
/// Precedence (first match wins):
/// 1. malformed or conflicting frame, stream malformation, or missing
///    terminal evidence (unknown, cancelled, or timed-out completions carry
///    no finished-run evidence) → `not_proven`
/// 2. an instrument-failed completion or any field `instrument_failure` →
///    `instrument_failed`
/// 3. subject binding mismatch → `subject_mismatch`
/// 4. nonzero exit or signal → `runner_failed`
/// 5. any field not `observed` → `observed_partial`
/// 6. otherwise → `observed_complete`
///
/// A matching expected plan never repairs a partial or failed row.
pub fn derive_row_state(
    frame_accepted: bool,
    stream_complete: bool,
    completion: ProcessCompletion,
    subject_consistent: bool,
    fields: &EffectiveInvocationFields,
) -> InvocationObservationState {
    if !frame_accepted || !stream_complete {
        return InvocationObservationState::NotProven;
    }
    match completion {
        ProcessCompletion::Unknown
        | ProcessCompletion::Cancelled
        | ProcessCompletion::TimedOut { .. } => return InvocationObservationState::NotProven,
        _ => {}
    }
    if completion == ProcessCompletion::InstrumentFailed || fields.any_instrument_failure() {
        return InvocationObservationState::InstrumentFailed;
    }
    if !subject_consistent {
        return InvocationObservationState::SubjectMismatch;
    }
    match completion {
        ProcessCompletion::ExitStatus { code } if code != 0 => {
            InvocationObservationState::RunnerFailed
        }
        ProcessCompletion::Signalled { .. } => InvocationObservationState::RunnerFailed,
        _ if !fields.all_observed() => InvocationObservationState::ObservedPartial,
        _ => InvocationObservationState::ObservedComplete,
    }
}
