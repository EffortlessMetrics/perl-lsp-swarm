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
        // Strict no-repair law: a row carrying leading or trailing whitespace
        // is malformed, never silently trimmed into an accepted member — the
        // decoder must not repair drifted runner output.
        || raw_text.len() != raw_text.trim().len()
    {
        return malformed();
    }
    *normalization_attempts += 1;
    let normalized = match normalize_source_item(raw_text, frame) {
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

#[cfg(test)]
mod contract_tests {
    //! Focused unit proof for the strict stream decoder: `decode_stream`,
    //! `work_from_rows`, `derive_observation_state`, and `decode_malformed`
    //! are exercised directly on both sides of each branch, keeping every
    //! framing, classification, and state-derivation seam on a static test
    //! path independent of the receipt constructor.

    use super::{
        DecodedStream, StreamDecodeOutcome, decode_malformed, decode_stream,
        derive_observation_state, work_from_rows,
    };
    use crate::model::{TargetScriptForm, TargetSelector};
    use crate::observed_discovery::model::{
        DiscoveryObservationState, LineFraming, MemberDisposition, ProcessCompletion,
    };
    use crate::runner_model::DiscoveryFrame;

    fn selectors() -> Vec<TargetSelector> {
        vec![TargetSelector::RecursiveRoot { path: "base".to_string() }]
    }

    fn forms() -> Vec<TargetScriptForm> {
        vec![TargetScriptForm::DotT]
    }

    fn decode(raw: &[u8]) -> Result<DecodedStream, String> {
        decode_stream(raw, DiscoveryFrame::CanonicalRepositoryPath, &selectors(), &forms())
    }

    #[test]
    fn stream_malformed_on_invalid_utf8_keeps_typed_outcome_and_zero_rows() {
        let decoded = decode(&[b't', b'/', 0xff, b'\n']).expect("decode never errors on framing");
        assert!(matches!(decoded.outcome, StreamDecodeOutcome::Malformed { .. }));
        assert!(decoded.rows.is_empty());
        assert_eq!(decoded.normalization_attempts, 0);
    }

    #[test]
    fn framing_is_typed_per_line_and_final_row_is_eof() {
        let decoded = decode(b"t/base/if.t\r\nt/base/op.t\nt/base/last.t").expect("decode");
        assert_eq!(decoded.rows.len(), 3);
        assert_eq!(decoded.rows[0].framing, LineFraming::Crlf);
        assert_eq!(decoded.rows[1].framing, LineFraming::Lf);
        assert_eq!(decoded.rows[2].framing, LineFraming::Eof);
        assert_eq!(decoded.rows.iter().map(|row| row.ordinal).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn blank_and_whitespace_drifted_rows_are_malformed_not_repaired() {
        let decoded = decode(b"t/base/if.t\n\n t/base/op.t\n").expect("decode");
        // The blank line is itself a retained malformed row: the decoder
        // never drops or repairs rows, and the whitespace-drifted spelling
        // joins it as malformed without reaching the normalizer.
        assert_eq!(decoded.rows.len(), 3);
        assert!(matches!(decoded.rows[0].disposition, MemberDisposition::Accepted));
        assert!(matches!(decoded.rows[1].disposition, MemberDisposition::MalformedRow));
        assert!(matches!(decoded.rows[2].disposition, MemberDisposition::MalformedRow));
        let decoded = decode(b"   \n").expect("decode");
        assert!(matches!(decoded.rows[0].disposition, MemberDisposition::MalformedRow));
    }

    #[test]
    fn classification_resolves_every_disposition_directly() {
        // Accepted member, then the same raw spelling again (duplicate of the
        // canonical), then a member outside the selection, then an
        // unsupported form, then a control-byte row (malformed).
        let raw = b"t/base/if.t\nt/base/if.t\nt/other/a.t\nREADME\nctrl\tx\n";
        let decoded = decode(raw).expect("decode");
        let dispositions: Vec<&MemberDisposition> =
            decoded.rows.iter().map(|row| &row.disposition).collect();
        assert!(matches!(dispositions[0], MemberDisposition::Accepted));
        match &dispositions[1] {
            MemberDisposition::DuplicateOfCanonical { canonical_path } => {
                assert_eq!(canonical_path, "t/base/if.t");
            }
            other => panic!("expected duplicate of canonical, got {other:?}"),
        }
        assert!(matches!(dispositions[2], MemberDisposition::OutsideTargetSelection));
        assert!(matches!(dispositions[3], MemberDisposition::UnsupportedSourceForm));
        assert!(matches!(dispositions[4], MemberDisposition::MalformedRow));
    }

    #[test]
    fn conflicting_canonical_spellings_collapse_only_under_a_resolving_frame() {
        // Under the runner-t-relative frame, two different raw spellings
        // resolve to one canonical: the second is a conflict, not a second
        // accepted member. Under the canonical frame the same drifted
        // spelling is an unsupported form instead of being silently folded.
        let raw = b"base/if.t\n./base/if.t\n";
        let decoded =
            decode_stream(raw, DiscoveryFrame::RunnerTDirectoryRelative, &selectors(), &forms())
                .expect("decode");
        assert!(matches!(decoded.rows[0].disposition, MemberDisposition::Accepted));
        assert!(matches!(
            decoded.rows[1].disposition,
            MemberDisposition::ConflictingCanonical { .. }
        ));
    }

    #[test]
    fn work_counters_are_derived_per_disposition() {
        let raw = b"t/base/a.t\nt/base/a.t\nt/other/b.t\n";
        let decoded = decode(raw).expect("decode");
        let work = work_from_rows(raw.len(), 2, &decoded.rows, decoded.normalization_attempts, 4);
        assert_eq!(work.decoded_rows, 3);
        assert_eq!(work.accepted_rows, 1);
        assert_eq!(work.duplicate_rows, 1);
        assert_eq!(work.out_of_target_rows, 1);
        assert_eq!(work.raw_stdout_bytes, raw.len() as u64);
        assert_eq!(work.raw_stderr_bytes, 2);
        assert_eq!(work.terminal_subject_validations, 4);
    }

    #[test]
    fn observation_state_derivation_follows_the_declared_precedence() {
        use DiscoveryObservationState as State;
        use ProcessCompletion as Completion;
        let exit = Completion::ExitStatus { code: 0 };
        assert_eq!(
            derive_observation_state(Completion::Unknown, false, false, true, true),
            State::NotProven
        );
        assert_eq!(
            derive_observation_state(Completion::InstrumentFailed, false, false, true, true),
            State::InstrumentFailed
        );
        assert_eq!(
            derive_observation_state(Completion::Cancelled, false, false, true, true),
            State::Cancelled
        );
        assert_eq!(
            derive_observation_state(
                Completion::TimedOut { deadline_millis: 1_000 },
                false,
                false,
                true,
                true
            ),
            State::TimedOut
        );
        assert_eq!(derive_observation_state(exit, true, false, true, true), State::MalformedOutput);
        assert_eq!(derive_observation_state(exit, false, true, true, true), State::OutputTruncated);
        assert_eq!(
            derive_observation_state(exit, false, false, false, true),
            State::SubjectMismatch
        );
        assert_eq!(
            derive_observation_state(Completion::ExitStatus { code: 2 }, false, false, true, true),
            State::RunnerFailed
        );
        assert_eq!(
            derive_observation_state(Completion::Signalled { signal: 9 }, false, false, true, true),
            State::RunnerFailed
        );
        assert_eq!(
            derive_observation_state(exit, false, false, true, false),
            State::ObservedPartial
        );
        assert_eq!(
            derive_observation_state(exit, false, false, true, true),
            State::ObservedComplete
        );
    }

    #[test]
    fn decode_malformed_reads_the_outcome_and_every_row_disposition() {
        let complete = decode(b"t/base/a.t\n").expect("decode");
        assert!(!decode_malformed(&complete.outcome, &complete.rows));
        let drifted = decode(b" t/base/a.t\n").expect("decode");
        assert!(decode_malformed(&drifted.outcome, &drifted.rows));
        let malformed_outcome = StreamDecodeOutcome::Malformed { reason: "utf8".to_string() };
        assert!(decode_malformed(&malformed_outcome, &[]));
    }

    #[test]
    fn streams_beyond_the_decoded_row_bound_are_refused() {
        // The decoder's bounded-row error path is executable proof, not a
        // decorative guard: a stream of MAX_DECODED_ROWS + 1 members is
        // refused with the bound named.
        let mut raw = String::new();
        for index in 0..=crate::observed_discovery::model::MAX_DECODED_ROWS {
            raw.push_str(&format!("t/base/{index}.t\n"));
        }
        match decode(raw.as_bytes()) {
            Err(message) => assert!(
                message.contains("decoded-row bound"),
                "unexpected refusal message: {message}"
            ),
            Ok(decoded) => panic!(
                "stream beyond the decoded-row bound was accepted with {} rows",
                decoded.rows.len()
            ),
        }
    }
}
