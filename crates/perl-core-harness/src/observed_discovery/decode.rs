//! Strict byte-level decoder for observed upstream discovery streams.
//!
//! The decoder consumes exact raw bytes (never a pre-split row list), retains
//! original order and raw spelling, rejects or type-limits malformed encoding,
//! framing, and control bytes, and resolves every well-formed row through the
//! one current runner-plan normalizer under an explicit discovery frame. It
//! never sorts, deduplicates, repairs, or touches the filesystem.

use crate::model::{TargetScriptForm, TargetSelector};
use crate::normalize::{matches_any_selector, normalize_source_item, source_form_allowed};
use crate::observed_discovery::model::{
    DecoderWork, DiscoveryObservationState, LineFraming, MAX_DECODED_ROWS, MemberDisposition,
    ObservedDiscoveryRow, ProcessCompletion, StreamDecodeOutcome,
};
use crate::runner_model::{DiscoveryFrame, RunnerSourceItem};
use std::collections::BTreeMap;

/// Result of strictly decoding one raw stdout stream.
pub(crate) struct DecodedStream {
    /// Stream-level decode outcome.
    pub outcome: StreamDecodeOutcome,
    /// Rows in original observed order with dispositions assigned.
    pub rows: Vec<ObservedDiscoveryRow>,
    /// Number of rows that reached the current runner-plan normalizer.
    pub normalization_attempts: u64,
}

impl StreamDecodeOutcome {
    /// True when the stream decoded strictly without malformation.
    pub fn is_complete(&self) -> bool {
        matches!(self, StreamDecodeOutcome::Complete)
    }
}

/// Decode one raw stdout stream under `frame`, classifying rows against
/// `selectors`/`script_forms` without dropping or repairing anything.
pub(crate) fn decode_stream(
    raw: &[u8],
    frame: DiscoveryFrame,
    selectors: &[TargetSelector],
    script_forms: &[TargetScriptForm],
) -> Result<DecodedStream, String> {
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(error) => {
            return Ok(DecodedStream {
                outcome: StreamDecodeOutcome::Malformed {
                    reason: format!("stream is not valid UTF-8: {error}"),
                },
                rows: Vec::new(),
                normalization_attempts: 0,
            });
        }
    };

    let mut rows = Vec::new();
    // Canonical identity -> first contributing raw spelling.
    let mut canonical_first: BTreeMap<String, String> = BTreeMap::new();
    let mut normalization_attempts = 0u64;
    let mut segments = text.split('\n').peekable();
    while let Some(segment) = segments.next() {
        let is_last = segments.peek().is_none();
        if is_last && segment.is_empty() {
            break;
        }
        if rows.len() >= MAX_DECODED_ROWS {
            return Err(format!(
                "observed stream exceeds the {MAX_DECODED_ROWS} decoded-row bound"
            ));
        }
        let ordinal = rows.len() as u32;
        let (raw_text, framing) = if is_last {
            (segment, LineFraming::Eof)
        } else if let Some(stripped) = segment.strip_suffix('\r') {
            (stripped, LineFraming::Crlf)
        } else {
            (segment, LineFraming::Lf)
        };
        let mut classification =
            classify_row(raw_text, frame, selectors, script_forms, &mut normalization_attempts);
        if matches!(classification.disposition, MemberDisposition::Accepted) {
            let canonical = classification
                .normalized
                .as_ref()
                .map(|item| item.canonical_path.clone())
                .ok_or_else(|| "accepted row is missing its normalized identity".to_string())?;
            classification.disposition = match canonical_first.get(&canonical) {
                Some(first_raw) if first_raw == raw_text => {
                    MemberDisposition::DuplicateOfCanonical { canonical_path: canonical }
                }
                Some(_) => MemberDisposition::ConflictingCanonical { canonical_path: canonical },
                None => {
                    canonical_first.insert(canonical, raw_text.to_string());
                    MemberDisposition::Accepted
                }
            };
        }
        rows.push(ObservedDiscoveryRow {
            ordinal,
            raw_text: raw_text.to_string(),
            framing,
            discovery_frame: frame,
            disposition: classification.disposition,
            normalized: classification.normalized,
        });
    }

    Ok(DecodedStream { outcome: StreamDecodeOutcome::Complete, rows, normalization_attempts })
}

struct RowClassification {
    disposition: MemberDisposition,
    normalized: Option<RunnerSourceItem>,
}

fn classify_row(
    raw_text: &str,
    frame: DiscoveryFrame,
    selectors: &[TargetSelector],
    script_forms: &[TargetScriptForm],
    normalization_attempts: &mut u64,
) -> RowClassification {
    let malformed =
        || RowClassification { disposition: MemberDisposition::MalformedRow, normalized: None };
    if raw_text.contains('\0')
        || raw_text.chars().any(is_forbidden_control)
        || raw_text.trim().is_empty()
    {
        return malformed();
    }
    *normalization_attempts += 1;
    let normalized = match normalize_source_item(raw_text.trim(), frame) {
        Ok(item) => item,
        Err(_) => {
            return RowClassification {
                disposition: MemberDisposition::UnsupportedSourceForm,
                normalized: None,
            };
        }
    };
    if !source_form_allowed(normalized.source_form, script_forms) {
        return RowClassification {
            disposition: MemberDisposition::UnsupportedSourceForm,
            normalized: Some(normalized),
        };
    }
    if !matches_any_selector(&normalized.canonical_path, selectors) {
        return RowClassification {
            disposition: MemberDisposition::OutsideTargetSelection,
            normalized: Some(normalized),
        };
    }
    RowClassification { disposition: MemberDisposition::Accepted, normalized: Some(normalized) }
}

fn is_forbidden_control(character: char) -> bool {
    character.is_control()
}

/// Derive per-row work counters from a decode result.
pub(crate) fn work_from_rows(
    stdout_bytes: usize,
    stderr_bytes: usize,
    rows: &[ObservedDiscoveryRow],
    normalization_operations: u64,
    terminal_subject_validations: u64,
) -> DecoderWork {
    let mut work = DecoderWork {
        raw_stdout_bytes: stdout_bytes as u64,
        raw_stderr_bytes: stderr_bytes as u64,
        decoded_rows: rows.len() as u64,
        normalization_operations,
        terminal_subject_validations,
        ..DecoderWork::default()
    };
    for row in rows {
        match row.disposition {
            MemberDisposition::Accepted => work.accepted_rows += 1,
            MemberDisposition::DuplicateOfCanonical { .. } => work.duplicate_rows += 1,
            MemberDisposition::ConflictingCanonical { .. } => work.conflicting_rows += 1,
            MemberDisposition::OutsideTargetSelection => work.out_of_target_rows += 1,
            MemberDisposition::UnsupportedSourceForm => work.unsupported_source_form_rows += 1,
            MemberDisposition::MalformedRow => work.malformed_rows += 1,
        }
    }
    work
}

/// Completeness law: derive the single observation state from envelope facts.
///
/// Precedence (first match wins):
/// 1. missing terminal evidence → `not_proven`
/// 2. instrumentation failure → `instrument_failed`
/// 3. cancellation / timeout → `cancelled` / `timed_out`
/// 4. malformed stream decode or malformed rows → `malformed_output`
/// 5. truncated capture → `output_truncated`
/// 6. subject-relation mismatch → `subject_mismatch`
/// 7. nonzero exit or signal → `runner_failed`
/// 8. any remaining non-accepted row → `observed_partial`
/// 9. otherwise → `observed_complete`
///
/// Matching a declared plan never repairs a partial or failed observation.
pub fn derive_observation_state(
    completion: ProcessCompletion,
    decode_malformed: bool,
    truncated: bool,
    subject_consistent: bool,
    all_rows_accepted: bool,
) -> DiscoveryObservationState {
    match completion {
        ProcessCompletion::Unknown => DiscoveryObservationState::NotProven,
        ProcessCompletion::InstrumentFailed => DiscoveryObservationState::InstrumentFailed,
        ProcessCompletion::Cancelled => DiscoveryObservationState::Cancelled,
        ProcessCompletion::TimedOut { .. } => DiscoveryObservationState::TimedOut,
        _ if decode_malformed => DiscoveryObservationState::MalformedOutput,
        _ if truncated => DiscoveryObservationState::OutputTruncated,
        _ if !subject_consistent => DiscoveryObservationState::SubjectMismatch,
        ProcessCompletion::ExitStatus { code } if code != 0 => {
            DiscoveryObservationState::RunnerFailed
        }
        ProcessCompletion::Signalled { .. } => DiscoveryObservationState::RunnerFailed,
        _ if !all_rows_accepted => DiscoveryObservationState::ObservedPartial,
        _ => DiscoveryObservationState::ObservedComplete,
    }
}

/// True when the decode outcome or any retained row records malformation.
pub(crate) fn decode_malformed(
    outcome: &StreamDecodeOutcome,
    rows: &[ObservedDiscoveryRow],
) -> bool {
    !outcome.is_complete()
        || rows.iter().any(|row| matches!(row.disposition, MemberDisposition::MalformedRow))
}
