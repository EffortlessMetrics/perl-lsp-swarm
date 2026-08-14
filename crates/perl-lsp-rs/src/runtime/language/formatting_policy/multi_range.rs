use serde::Serialize;

use super::super::{
    CodeFormatter, FormatContext, FormatDisposition, FormatTextEdit, FormattingDecision,
    JsonRpcError, JsonRpcId, LspServer, RequestCleanupGuard, Snapshot, Surface, Value,
    WirePosition, WireRange, actual_engine_for_mode, cancellation_token, digest, invalid_params,
    json, parse_range, sanitized_outcome,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct PositionRecord {
    line: u32,
    character: u32,
    byte: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NormalizedRange {
    start: PositionRecord,
    end: PositionRecord,
}

impl NormalizedRange {
    fn wire(&self) -> WireRange {
        WireRange::new(
            WirePosition::new(self.start.line, self.start.character),
            WirePosition::new(self.end.line, self.end.character),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdmittedRange {
    original_index: usize,
    normalized: NormalizedRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RangeProvenance {
    normalized_index: usize,
    original_index: usize,
}

#[derive(Debug, Clone, Serialize)]
struct RangePlan {
    requested_ranges: Vec<Value>,
    normalized_ranges: Vec<NormalizedRange>,
    range_provenance: Vec<RangeProvenance>,
    plan_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanError {
    reason: &'static str,
    message: String,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.reason, self.message)
    }
}

impl std::error::Error for PlanError {}

impl PlanError {
    fn new(reason: &'static str, message: impl Into<String>) -> Self {
        Self { reason, message: message.into() }
    }

    fn json_rpc_code(&self) -> i32 {
        match self.reason {
            "invalid_range" | "invalid_position" | "reversed_range" | "duplicate_range"
            | "overlapping_ranges" => -32602,
            _ => -32603,
        }
    }

    fn error_kind(&self) -> &'static str {
        if self.json_rpc_code() == -32602 {
            "invalid_multi_range_plan"
        } else {
            "formatting_outcome_contract"
        }
    }
}

struct SourceGeometry {
    line_starts: Vec<usize>,
}

impl SourceGeometry {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        Self { line_starts }
    }

    fn byte_offset(&self, source: &str, line: u32, character: u32) -> Result<usize, PlanError> {
        let line_index = line as usize;
        let start = *self.line_starts.get(line_index).ok_or_else(|| {
            PlanError::new(
                "invalid_position",
                format!("line {line} is outside the current document"),
            )
        })?;
        let physical_end = self
            .line_starts
            .get(line_index + 1)
            .map_or(source.len(), |next| next.saturating_sub(1));
        let end = if physical_end > start
            && source.as_bytes().get(physical_end.saturating_sub(1)) == Some(&b'\r')
        {
            physical_end - 1
        } else {
            physical_end
        };

        let target = character as usize;
        let mut units = 0usize;
        for (relative, ch) in source[start..end].char_indices() {
            if units == target {
                return Ok(start + relative);
            }
            let next = units.saturating_add(ch.len_utf16());
            if target < next {
                return Err(PlanError::new(
                    "invalid_position",
                    format!("UTF-16 character {character} on line {line} splits a surrogate pair"),
                ));
            }
            units = next;
        }
        if units == target {
            Ok(end)
        } else {
            Err(PlanError::new(
                "invalid_position",
                format!("UTF-16 character {character} is outside line {line} (length {units})"),
            ))
        }
    }

    fn line_byte_span(&self, source: &str, line: u32) -> Result<(usize, usize), PlanError> {
        let line_index = line as usize;
        let start = *self.line_starts.get(line_index).ok_or_else(|| {
            PlanError::new(
                "invalid_position",
                format!("line {line} is outside the current document"),
            )
        })?;
        let mut end = self
            .line_starts
            .get(line_index + 1)
            .map_or(source.len(), |next| next.saturating_sub(1));
        if end > start && source.as_bytes().get(end.saturating_sub(1)) == Some(&b'\r') {
            end -= 1;
        }
        Ok((start, end))
    }
}

fn build_plan(source: &str, ranges: &[Value]) -> Result<RangePlan, PlanError> {
    let geometry = SourceGeometry::new(source);
    let requested_ranges = ranges.to_vec();
    let mut normalized_ranges = Vec::with_capacity(ranges.len());

    for (original_index, value) in ranges.iter().enumerate() {
        let wire = parse_range(value, &format!("ranges[{original_index}]"))
            .map_err(|error| PlanError::new("invalid_range", error.message))?;
        let start_byte = geometry.byte_offset(source, wire.start.line, wire.start.character)?;
        let end_byte = geometry.byte_offset(source, wire.end.line, wire.end.character)?;
        if end_byte < start_byte {
            return Err(PlanError::new(
                "reversed_range",
                format!("ranges[{original_index}] ends before it starts"),
            ));
        }
        normalized_ranges.push(AdmittedRange {
            original_index,
            normalized: NormalizedRange {
                start: PositionRecord {
                    line: wire.start.line,
                    character: wire.start.character,
                    byte: start_byte,
                },
                end: PositionRecord {
                    line: wire.end.line,
                    character: wire.end.character,
                    byte: end_byte,
                },
            },
        });
    }

    normalized_ranges.sort_by_key(|range| {
        (
            range.normalized.start.byte,
            range.normalized.end.byte,
            range.normalized.start.line,
            range.normalized.start.character,
        )
    });
    for pair in normalized_ranges.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if left.normalized.start.byte == right.normalized.start.byte
            && left.normalized.end.byte == right.normalized.end.byte
        {
            return Err(PlanError::new(
                "duplicate_range",
                format!(
                    "ranges[{}] duplicates ranges[{}]",
                    right.original_index, left.original_index
                ),
            ));
        }
        if right.normalized.start.byte < left.normalized.end.byte
            || right.normalized.start.byte == left.normalized.start.byte
        {
            return Err(PlanError::new(
                "overlapping_ranges",
                format!(
                    "ranges[{}] overlaps ranges[{}]",
                    right.original_index, left.original_index
                ),
            ));
        }
    }

    let range_provenance = normalized_ranges
        .iter()
        .enumerate()
        .map(|(normalized_index, range)| RangeProvenance {
            normalized_index,
            original_index: range.original_index,
        })
        .collect();
    let normalized_ranges: Vec<NormalizedRange> =
        normalized_ranges.into_iter().map(|range| range.normalized).collect();
    let canonical = normalized_ranges
        .iter()
        .map(|range| format!("{}:{}", range.start.byte, range.end.byte))
        .collect::<Vec<_>>()
        .join("|");
    Ok(RangePlan {
        requested_ranges,
        normalized_ranges,
        range_provenance,
        plan_digest: digest(&format!("multi-range-plan-v1|{canonical}")),
    })
}

#[derive(Debug)]
struct PlannedEdit {
    edit: FormatTextEdit,
    start_byte: usize,
    end_byte: usize,
    owner: usize,
}

fn compose_edits(
    source: &str,
    plan: &RangePlan,
    per_range_edits: Vec<Vec<FormatTextEdit>>,
    generation: u64,
    config_fingerprint: &str,
) -> Result<(Vec<FormatTextEdit>, String), PlanError> {
    if per_range_edits.len() != plan.normalized_ranges.len() {
        return Err(PlanError::new(
            "instrument_failure",
            "per-range edit count does not match the admitted plan",
        ));
    }

    let geometry = SourceGeometry::new(source);
    let mut edits = Vec::new();
    for (owner, (admitted, range_edits)) in
        plan.normalized_ranges.iter().zip(per_range_edits).enumerate()
    {
        for edit in range_edits {
            let start_byte =
                geometry.byte_offset(source, edit.range.start.line, edit.range.start.character)?;
            let end_byte =
                geometry.byte_offset(source, edit.range.end.line, edit.range.end.character)?;
            if end_byte < start_byte {
                return Err(PlanError::new(
                    "edit_conflict",
                    format!("formatter edit for normalized range {owner} is reversed"),
                ));
            }
            // Native range formatting is line-oriented: a partial-line request
            // may legitimately produce an edit covering the touched lines. Keep
            // the safety boundary at those lines while still rejecting edits
            // that escape the admitted line set.
            let (allowed_start, allowed_end) = if admitted.start.line == admitted.end.line
                && admitted.end.character == admitted.start.character
            {
                (admitted.start.byte, admitted.end.byte)
            } else if admitted.end.character == 0 {
                let (start, _) = geometry.line_byte_span(source, admitted.start.line)?;
                (start, admitted.end.byte)
            } else {
                let (start, _) = geometry.line_byte_span(source, admitted.start.line)?;
                let (_, end) = geometry.line_byte_span(source, admitted.end.line)?;
                (start, end)
            };
            if start_byte < allowed_start || end_byte > allowed_end {
                return Err(PlanError::new(
                    "edit_outside_range",
                    format!(
                        "formatter edit {start_byte}..{end_byte} escapes admitted line span {allowed_start}..{allowed_end}"
                    ),
                ));
            }
            edits.push(PlannedEdit { edit, start_byte, end_byte, owner });
        }
    }

    edits.sort_by(|left, right| {
        (left.start_byte, left.end_byte, left.owner, left.edit.new_text.as_str()).cmp(&(
            right.start_byte,
            right.end_byte,
            right.owner,
            right.edit.new_text.as_str(),
        ))
    });
    for pair in edits.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if left.start_byte == right.start_byte && left.end_byte == right.end_byte {
            return Err(PlanError::new(
                "duplicate_edit",
                format!(
                    "normalized ranges {} and {} produced duplicate edits",
                    left.owner, right.owner
                ),
            ));
        }
        if right.start_byte < left.end_byte || right.start_byte == left.start_byte {
            return Err(PlanError::new(
                "edit_conflict",
                format!(
                    "normalized ranges {} and {} produced overlapping edits",
                    left.owner, right.owner
                ),
            ));
        }
    }

    let canonical = edits
        .iter()
        .map(|edit| {
            format!("{}:{}:{}", edit.start_byte, edit.end_byte, digest(&edit.edit.new_text))
        })
        .collect::<Vec<_>>()
        .join("|");
    let final_digest = digest(&format!(
        "multi-range-edits-v1|generation={generation}|config={config_fingerprint}|plan={}|{canonical}",
        plan.plan_digest
    ));
    Ok((edits.into_iter().map(|edit| edit.edit).collect(), final_digest))
}

fn plan_evidence(
    plan: &RangePlan,
    outcomes: Vec<Value>,
    final_edit_digest: Option<String>,
) -> Value {
    json!({
        "schema_version": "formatting_multi_range.v1",
        "requested_ranges": plan.requested_ranges,
        "normalized_ranges": plan.normalized_ranges,
        "range_provenance": plan.range_provenance,
        "plan_digest": plan.plan_digest,
        "per_range_outcomes": outcomes,
        "final_edit_digest": final_edit_digest,
        "empty_range_policy": "one empty point range is admitted; duplicates or points inside another range are rejected",
        "adjacent_range_policy": "half-open ranges sharing one boundary are admitted",
    })
}

fn plan_outcomes(plan: &RangePlan, decisions: &[FormattingDecision]) -> Vec<Value> {
    decisions
        .iter()
        .zip(plan.normalized_ranges.iter().zip(&plan.range_provenance))
        .map(|(decision, (admitted, provenance))| {
            json!({
                "original_index": provenance.original_index,
                "normalized_range": admitted,
                "outcome": sanitized_outcome(decision),
            })
        })
        .collect()
}

fn blocked_decision(decisions: &[FormattingDecision]) -> Option<&FormattingDecision> {
    decisions.iter().find(|decision| {
        matches!(
            decision.outcome.disposition,
            FormatDisposition::Refused | FormatDisposition::FailedOrNotProven
        )
    })
}

fn typed_outcome_error(
    server: &LspServer,
    snapshot: &Snapshot,
    plan: &RangePlan,
    outcomes: Vec<Value>,
    decision: &FormattingDecision,
    message: &str,
) -> JsonRpcError {
    let outcome = sanitized_outcome(decision);
    let reason = outcome.get("reason").cloned().unwrap_or_else(|| json!("unknown"));
    let engine =
        outcome.pointer("/identity/actual_engine").and_then(Value::as_str).unwrap_or("unknown");
    let evidence = plan_evidence(plan, outcomes, None);
    let receipt = server.record_formatting_receipt(
        snapshot,
        "blocked",
        reason.clone(),
        engine,
        "no_edit",
        0,
        Some(evidence),
    );
    JsonRpcError {
        code: -32603,
        message: message.to_string(),
        data: Some(json!({
            "error_kind": "formatting_outcome_contract",
            "reason": reason,
            "identity": outcome.get("identity").cloned().unwrap_or(Value::Null),
            "formatting_outcome": outcome,
            "formatting_receipt": receipt,
        })),
    }
}

fn plan_error(
    server: &LspServer,
    snapshot: &Snapshot,
    error: PlanError,
    evidence: Option<Value>,
) -> JsonRpcError {
    let code = error.json_rpc_code();
    let error_kind = error.error_kind();
    let receipt = server.record_formatting_receipt(
        snapshot,
        "blocked",
        json!(error.reason),
        "not_started",
        "no_edit",
        0,
        evidence,
    );
    JsonRpcError {
        code,
        message: error.message,
        data: Some(json!({
            "error_kind": error_kind,
            "reason": error.reason,
            "formatting_receipt": receipt,
        })),
    }
}

pub(super) fn handle(
    server: &LspServer,
    params: Option<Value>,
    request_id: Option<&Value>,
) -> Result<Option<Value>, JsonRpcError> {
    let typed_id = request_id.and_then(JsonRpcId::try_from_value);
    let _cleanup = RequestCleanupGuard::from_ref(typed_id.as_ref());
    let token = cancellation_token(typed_id.as_ref(), Surface::Ranges);
    server.ensure_not_cancelled(Surface::Ranges, token.as_ref(), None, None)?;
    let params =
        params.ok_or_else(|| invalid_params("Missing multi-range formatting parameters"))?;
    let snapshot = server.admit(Surface::Ranges, &params)?;
    let ranges = params
        .get("ranges")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params("Missing required parameter: ranges"))?;
    let plan = build_plan(&snapshot.text, ranges)
        .map_err(|error| plan_error(server, &snapshot, error, None))?;

    server.ensure_not_cancelled(Surface::Ranges, token.as_ref(), Some(&snapshot), None)?;
    if plan.normalized_ranges.is_empty() {
        server.ensure_current(&snapshot)?;
        server.record_formatting_receipt(
            &snapshot,
            "acted",
            json!("already_formatted"),
            "not_started",
            "none",
            0,
            Some(plan_evidence(&plan, Vec::new(), Some(digest("empty-edit-set")))),
        );
        return Ok(Some(json!([])));
    }

    let formatter =
        CodeFormatter::with_config_and_mode(snapshot.config.perltidy.clone(), snapshot.config.mode);
    let context = FormatContext::new(Some(snapshot.uri.clone()), Some(snapshot.generation));
    let mut decisions = Vec::with_capacity(plan.normalized_ranges.len());
    for (normalized_index, admitted) in plan.normalized_ranges.iter().enumerate() {
        server.ensure_not_cancelled(
            Surface::Ranges,
            token.as_ref(),
            Some(&snapshot),
            Some(actual_engine_for_mode(snapshot.config.mode)),
        )?;
        let decision = match formatter.format_range_decision(
            &snapshot.text,
            &admitted.wire(),
            &snapshot.options,
            &context,
        ) {
            Ok(decision) => decision,
            Err(error) => {
                server.ensure_not_cancelled(
                    Surface::Ranges,
                    token.as_ref(),
                    Some(&snapshot),
                    Some(actual_engine_for_mode(snapshot.config.mode)),
                )?;
                let original_index = plan
                    .range_provenance
                    .get(normalized_index)
                    .map_or(normalized_index, |provenance| provenance.original_index);
                let outcomes = plan_outcomes(&plan, &decisions);
                return Err(server.formatting_failure_with_evidence(
                    &snapshot,
                    &format!("Range formatting failed for ranges[{original_index}]"),
                    error,
                    Some(plan_evidence(&plan, outcomes, None)),
                ));
            }
        };
        decisions.push(decision);
    }

    server.ensure_not_cancelled(
        Surface::Ranges,
        token.as_ref(),
        Some(&snapshot),
        Some(actual_engine_for_mode(snapshot.config.mode)),
    )?;
    server.ensure_current(&snapshot)?;
    let outcomes = plan_outcomes(&plan, &decisions);

    if let Some(blocked) = blocked_decision(&decisions) {
        if decisions.iter().any(|decision| !decision.document.edits.is_empty()) {
            return Err(typed_outcome_error(
                server,
                &snapshot,
                &plan,
                outcomes,
                blocked,
                "one range was blocked after another produced edits; no edits were returned",
            ));
        }
        if blocked.outcome.disposition == FormatDisposition::FailedOrNotProven {
            return Err(typed_outcome_error(
                server,
                &snapshot,
                &plan,
                outcomes,
                blocked,
                "formatting returned an unproven successful value; no edits were returned",
            ));
        }
        let outcome = sanitized_outcome(blocked);
        let reason = outcome.get("reason").cloned().unwrap_or_else(|| json!("unknown"));
        let engine = outcome
            .pointer("/identity/actual_engine")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        server.record_formatting_receipt(
            &snapshot,
            "blocked",
            reason,
            &engine,
            "no_edit",
            0,
            Some(plan_evidence(&plan, outcomes, None)),
        );
        return Ok(Some(json!([])));
    }

    let per_range_edits = decisions
        .iter()
        .map(|decision| match decision.outcome.disposition {
            FormatDisposition::Applied if !decision.document.edits.is_empty() => {
                Ok(decision.document.edits.clone())
            }
            FormatDisposition::NoChange if decision.document.edits.is_empty() => Ok(Vec::new()),
            _ => Err(PlanError::new(
                "instrument_failure",
                "per-range outcome and edit shape disagree",
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            plan_error(server, &snapshot, error, Some(plan_evidence(&plan, outcomes.clone(), None)))
        })?;
    let (edits, final_edit_digest) = compose_edits(
        &snapshot.text,
        &plan,
        per_range_edits,
        snapshot.generation,
        &snapshot.config.fingerprint,
    )
    .map_err(|error| {
        plan_error(server, &snapshot, error, Some(plan_evidence(&plan, outcomes.clone(), None)))
    })?;

    server.ensure_not_cancelled(
        Surface::Ranges,
        token.as_ref(),
        Some(&snapshot),
        Some(actual_engine_for_mode(snapshot.config.mode)),
    )?;
    server.ensure_current(&snapshot)?;
    server.record_formatting_receipt(
        &snapshot,
        "acted",
        if edits.is_empty() { json!("already_formatted") } else { json!("applied") },
        actual_engine_for_mode(snapshot.config.mode),
        "none",
        edits.len(),
        Some(plan_evidence(&plan, outcomes, Some(final_edit_digest))),
    );
    Ok(Some(json!(edits)))
}

#[cfg(test)]
mod tests {
    use super::super::default_options;
    use super::*;
    use perl_lsp_rs_core::tooling::perltidy::native::FormatReasonCode;

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
        json!({
            "start": { "line": sl, "character": sc },
            "end": { "line": el, "character": ec }
        })
    }

    #[test]
    fn equivalent_input_order_has_same_plan_digest() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my$x=1;\nmy$y=2;\n";
        let first = build_plan(source, &[range(1, 0, 1, 7), range(0, 0, 0, 7)])?;
        let second = build_plan(source, &[range(0, 0, 0, 7), range(1, 0, 1, 7)])?;
        assert_eq!(first.plan_digest, second.plan_digest);
        assert_eq!(first.normalized_ranges, second.normalized_ranges);
        assert_ne!(first.range_provenance, second.range_provenance);
        let first_evidence = plan_evidence(&first, Vec::new(), None);
        let second_evidence = plan_evidence(&second, Vec::new(), None);
        assert_eq!(first_evidence["normalized_ranges"], second_evidence["normalized_ranges"]);
        assert_ne!(first_evidence["range_provenance"], second_evidence["range_provenance"]);
        Ok(())
    }

    #[test]
    fn overlap_duplicate_and_reversal_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let source = "abcdefgh\n";
        let overlap = build_plan(source, &[range(0, 0, 0, 5), range(0, 4, 0, 7)])
            .err()
            .ok_or("overlapping ranges were admitted")?;
        assert_eq!(overlap.reason, "overlapping_ranges");
        let duplicate = build_plan(source, &[range(0, 0, 0, 5), range(0, 0, 0, 5)])
            .err()
            .ok_or("duplicate ranges were admitted")?;
        assert_eq!(duplicate.reason, "duplicate_range");
        let reversal =
            build_plan(source, &[range(0, 5, 0, 2)]).err().ok_or("reversed range was admitted")?;
        assert_eq!(reversal.reason, "reversed_range");
        Ok(())
    }

    #[test]
    fn astral_and_crlf_positions_are_strict() -> Result<(), Box<dyn std::error::Error>> {
        let source = "a🦀b\r\nnext\r\n";
        let plan = build_plan(source, &[range(0, 1, 0, 3), range(1, 0, 1, 4)])?;
        assert_eq!(plan.normalized_ranges[0].start.byte, 1);
        assert_eq!(plan.normalized_ranges[0].end.byte, 5);
        let surrogate_split = build_plan(source, &[range(0, 2, 0, 3)])
            .err()
            .ok_or("surrogate-splitting position was admitted")?;
        assert_eq!(surrogate_split.reason, "invalid_position");
        Ok(())
    }

    #[test]
    fn adjacent_and_single_empty_ranges_are_defined() -> Result<(), Box<dyn std::error::Error>> {
        let source = "abcdef\n";
        let adjacent = build_plan(source, &[range(0, 0, 0, 3), range(0, 3, 0, 6)])?;
        assert_eq!(adjacent.normalized_ranges.len(), 2);
        let empty = build_plan(source, &[range(0, 3, 0, 3)])?;
        assert_eq!(empty.normalized_ranges[0].start.byte, empty.normalized_ranges[0].end.byte);
        Ok(())
    }

    #[test]
    fn plan_errors_distinguish_invalid_input_from_contract_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let invalid = PlanError::new("invalid_position", "outside document");
        assert_eq!(invalid.json_rpc_code(), -32602);
        assert_eq!(invalid.error_kind(), "invalid_multi_range_plan");

        let conflict = PlanError::new("edit_conflict", "formatter edits overlap");
        assert_eq!(conflict.json_rpc_code(), -32603);
        assert_eq!(conflict.error_kind(), "formatting_outcome_contract");
        Ok(())
    }

    #[test]
    fn conflict_detector_rejects_adjacent_boundary_edits() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "abcdef\n";
        let plan = build_plan(source, &[range(0, 0, 0, 3), range(0, 3, 0, 6)])?;
        let edits = vec![
            vec![FormatTextEdit {
                range: crate::features::formatting::FormatRange::new(
                    crate::features::formatting::FormatPosition::new(0, 3),
                    crate::features::formatting::FormatPosition::new(0, 3),
                ),
                new_text: "left".to_string(),
            }],
            vec![FormatTextEdit {
                range: crate::features::formatting::FormatRange::new(
                    crate::features::formatting::FormatPosition::new(0, 3),
                    crate::features::formatting::FormatPosition::new(0, 4),
                ),
                new_text: "right".to_string(),
            }],
        ];
        let conflict = compose_edits(source, &plan, edits, 1, "cfg")
            .err()
            .ok_or("adjacent edits were not rejected")?;
        assert_eq!(conflict.reason, "edit_conflict");
        Ok(())
    }

    #[test]
    fn failed_range_after_edit_is_atomic_with_typed_plan_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "my$x=1;\nmy $y = 2;\n";
        let uri = "file:///multi-range-failed-outcome.pl";
        let plan = build_plan(source, &[range(0, 0, 0, 7), range(1, 0, 1, 10)])?;
        let formatter = CodeFormatter::new();
        let context = FormatContext::new(Some(uri.to_string()), Some(1));
        let first = formatter.format_range_decision(
            source,
            &plan.normalized_ranges[0].wire(),
            &default_options(),
            &context,
        )?;
        assert_eq!(first.outcome.disposition, FormatDisposition::Applied);
        assert!(!first.document.edits.is_empty(), "first range must produce edits");

        let mut second = formatter.format_range_decision(
            source,
            &plan.normalized_ranges[1].wire(),
            &default_options(),
            &context,
        )?;
        second.outcome.disposition = FormatDisposition::FailedOrNotProven;
        second.outcome.reason = FormatReasonCode::InstrumentFailure;
        assert!(second.document.edits.is_empty(), "failed range must carry no edits");

        let decisions = vec![first, second];
        let blocked = blocked_decision(&decisions).ok_or("failed outcome was not blocked")?;
        assert_eq!(blocked.outcome.disposition, FormatDisposition::FailedOrNotProven);
        assert_eq!(blocked.outcome.reason, FormatReasonCode::InstrumentFailure);
        assert!(
            decisions.iter().any(|decision| !decision.document.edits.is_empty()),
            "atomic refusal requires at least one range with edits"
        );

        let server = LspServer::new();
        server.advertised_feature_ids.lock().push(Surface::Ranges.feature_id());
        server.test_apply_did_open(uri, source, 1)?;
        let snapshot = server.admit(
            Surface::Ranges,
            &json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true },
            }),
        )?;
        let error = typed_outcome_error(
            &server,
            &snapshot,
            &plan,
            plan_outcomes(&plan, &decisions),
            blocked,
            "one range was blocked after another produced edits; no edits were returned",
        );
        let data = error.data.ok_or("missing failed-outcome data")?;
        assert_eq!(error.code, -32603);
        assert_eq!(data["reason"], "instrument_failure");
        assert_eq!(data["identity"]["source_id_hash"], digest(uri));
        assert_eq!(data["formatting_outcome"]["reason"], "instrument_failure");

        let receipt = data["formatting_receipt"].clone();
        assert_eq!(receipt["decision"], "blocked");
        assert_eq!(receipt["result_count"], 0);
        let evidence = &receipt["format_outcome"];
        assert_eq!(evidence["plan_digest"], plan.plan_digest);
        assert_eq!(evidence["requested_ranges"].as_array().map(Vec::len), Some(2));
        assert_eq!(evidence["normalized_ranges"].as_array().map(Vec::len), Some(2));
        assert_eq!(evidence["range_provenance"].as_array().map(Vec::len), Some(2));
        assert_eq!(evidence["per_range_outcomes"].as_array().map(Vec::len), Some(2));
        assert_eq!(evidence["per_range_outcomes"][0]["original_index"], 0);
        assert_eq!(evidence["per_range_outcomes"][1]["original_index"], 1);
        assert_eq!(evidence["per_range_outcomes"][1]["outcome"]["reason"], "instrument_failure");
        assert_eq!(
            evidence["per_range_outcomes"][1]["outcome"]["identity"]["source_id_hash"],
            digest(uri)
        );
        Ok(())
    }

    #[test]
    fn line_expanded_edits_stay_within_touched_lines() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my$x=1;\n";
        let plan = build_plan(source, &[range(0, 2, 0, 5)])?;
        let edits = vec![vec![FormatTextEdit {
            range: crate::features::formatting::FormatRange::new(
                crate::features::formatting::FormatPosition::new(0, 0),
                crate::features::formatting::FormatPosition::new(0, 7),
            ),
            new_text: "my $x = 1;".to_string(),
        }]];
        assert!(
            compose_edits(source, &plan, edits, 1, "cfg").is_ok(),
            "line-expanded edit inside the touched lines must compose"
        );
        Ok(())
    }

    #[test]
    fn end_at_line_start_excludes_that_line_from_line_expansion()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "first\nsecond\n";
        let plan = build_plan(source, &[range(0, 2, 1, 0)])?;
        let edits = vec![vec![FormatTextEdit {
            range: crate::features::formatting::FormatRange::new(
                crate::features::formatting::FormatPosition::new(0, 0),
                crate::features::formatting::FormatPosition::new(1, 0),
            ),
            new_text: "first\n".to_string(),
        }]];
        assert!(
            compose_edits(source, &plan, edits, 1, "cfg").is_ok(),
            "edit ending at the next line start must compose"
        );

        let edits = vec![vec![FormatTextEdit {
            range: crate::features::formatting::FormatRange::new(
                crate::features::formatting::FormatPosition::new(0, 0),
                crate::features::formatting::FormatPosition::new(1, 1),
            ),
            new_text: "first\nsecond".to_string(),
        }]];
        let outside_range = compose_edits(source, &plan, edits, 1, "cfg")
            .err()
            .ok_or("edit extending into the excluded end line was admitted")?;
        assert_eq!(outside_range.reason, "edit_outside_range");
        Ok(())
    }

    #[test]
    fn adjacent_edits_sharing_half_open_boundary_are_composed()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "abcdef\n";
        let plan = build_plan(source, &[range(0, 0, 0, 3), range(0, 3, 0, 6)])?;
        let edits = vec![
            vec![FormatTextEdit {
                range: crate::features::formatting::FormatRange::new(
                    crate::features::formatting::FormatPosition::new(0, 0),
                    crate::features::formatting::FormatPosition::new(0, 3),
                ),
                new_text: "ABC".to_string(),
            }],
            vec![FormatTextEdit {
                range: crate::features::formatting::FormatRange::new(
                    crate::features::formatting::FormatPosition::new(0, 3),
                    crate::features::formatting::FormatPosition::new(0, 6),
                ),
                new_text: "DEF".to_string(),
            }],
        ];
        assert_eq!(compose_edits(source, &plan, edits, 1, "cfg")?.0.len(), 2);
        Ok(())
    }
}
