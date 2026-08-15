//! Atomic multi-range formatting plan builder plus rangesFormatting wiring.
//!
//! Compose units cover empty-point bounds and same-line adjacent edit merges.

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

impl PositionRecord {
    fn at(line: u32, character: u32, byte: usize) -> Self {
        Self { line, character, byte }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NormalizedRange {
    start: PositionRecord,
    end: PositionRecord,
}

impl NormalizedRange {
    fn between(start: PositionRecord, end: PositionRecord) -> Self {
        Self { start, end }
    }

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

impl AdmittedRange {
    fn new(original_index: usize, normalized: NormalizedRange) -> Self {
        Self { original_index, normalized }
    }
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

    fn outside_document(line: u32) -> Self {
        Self::new("invalid_position", format!("line {line} is outside the current document"))
    }

    fn surrogate_split(line: u32, character: u32) -> Self {
        Self::new(
            "invalid_position",
            format!("UTF-16 character {character} on line {line} splits a surrogate pair"),
        )
    }

    fn outside_line(line: u32, character: u32, length: usize) -> Self {
        Self::new(
            "invalid_position",
            format!("UTF-16 character {character} is outside line {line} (length {length})"),
        )
    }

    fn reversed_range(original_index: usize) -> Self {
        Self::new("reversed_range", format!("ranges[{original_index}] ends before it starts"))
    }

    fn duplicate_range(right: usize, left: usize) -> Self {
        Self::new("duplicate_range", format!("ranges[{right}] duplicates ranges[{left}]"))
    }

    fn overlapping_ranges(right: usize, left: usize) -> Self {
        Self::new("overlapping_ranges", format!("ranges[{right}] overlaps ranges[{left}]"))
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
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                line_starts.push(i + 1);
            } else if bytes[i] == b'\r' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    line_starts.push(i + 2);
                    i += 1;
                } else {
                    line_starts.push(i + 1);
                }
            }
            i += 1;
        }
        Self { line_starts }
    }

    fn line_content_end(&self, source: &str, line_index: usize) -> usize {
        let Some(&next) = self.line_starts.get(line_index + 1) else {
            return source.len();
        };
        let bytes = source.as_bytes();
        if next >= 2 && bytes.get(next - 2) == Some(&b'\r') && bytes.get(next - 1) == Some(&b'\n') {
            next - 2
        } else if next >= 1 && matches!(bytes.get(next - 1), Some(&b'\n') | Some(&b'\r')) {
            next - 1
        } else {
            next
        }
    }

    fn byte_offset(&self, source: &str, line: u32, character: u32) -> Result<usize, PlanError> {
        let line_index = line as usize;
        let start = self
            .line_starts
            .get(line_index)
            .copied()
            .ok_or_else(|| PlanError::outside_document(line))?;
        let end = self.line_content_end(source, line_index);

        let target = character as usize;
        let mut units = 0usize;
        for (relative, ch) in source[start..end].char_indices() {
            if units == target {
                return Ok(start + relative);
            }
            let next = units.saturating_add(ch.len_utf16());
            if target < next {
                return Err(PlanError::surrogate_split(line, character));
            }
            units = next;
        }
        if units == target { Ok(end) } else { Err(PlanError::outside_line(line, character, units)) }
    }

    fn line_byte_span(&self, source: &str, line: u32) -> Result<(usize, usize), PlanError> {
        let line_index = line as usize;
        let start =
            *self.line_starts.get(line_index).ok_or_else(|| PlanError::outside_document(line))?;
        Ok((start, self.line_content_end(source, line_index)))
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
            return Err(PlanError::reversed_range(original_index));
        }
        let normalized = NormalizedRange::between(
            PositionRecord::at(wire.start.line, wire.start.character, start_byte),
            PositionRecord::at(wire.end.line, wire.end.character, end_byte),
        );
        normalized_ranges.push(AdmittedRange::new(original_index, normalized));
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
            return Err(PlanError::duplicate_range(right.original_index, left.original_index));
        }
        if right.normalized.start.byte < left.normalized.end.byte
            || right.normalized.start.byte == left.normalized.start.byte
        {
            return Err(PlanError::overlapping_ranges(right.original_index, left.original_index));
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
            // Formatter-emitted positions are instrument output, not client
            // request params. Remap byte_offset failures so the client sees
            // -32603 / formatting_outcome_contract instead of -32602.
            let reclassify = |error: PlanError| {
                PlanError::new(
                    "instrument_failure",
                    format!(
                        "formatter edit for normalized range {owner} has an unresolvable position: {error}"
                    ),
                )
            };
            let start_byte = geometry
                .byte_offset(source, edit.range.start.line, edit.range.start.character)
                .map_err(reclassify)?;
            let end_byte = geometry
                .byte_offset(source, edit.range.end.line, edit.range.end.character)
                .map_err(reclassify)?;
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
    plan_error_with_engine(server, snapshot, error, evidence, "not_started")
}

fn plan_error_with_engine(
    server: &LspServer,
    snapshot: &Snapshot,
    error: PlanError,
    evidence: Option<Value>,
    actual_engine: &str,
) -> JsonRpcError {
    let code = error.json_rpc_code();
    let error_kind = error.error_kind();
    let receipt = server.record_formatting_receipt(
        snapshot,
        "blocked",
        json!(error.reason),
        actual_engine,
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
            plan_error_with_engine(
                server,
                &snapshot,
                error,
                Some(plan_evidence(&plan, outcomes.clone(), None)),
                actual_engine_for_mode(snapshot.config.mode),
            )
        })?;
    let (edits, final_edit_digest) = compose_edits(
        &snapshot.text,
        &plan,
        per_range_edits,
        snapshot.generation,
        &snapshot.config.fingerprint,
    )
    .map_err(|error| {
        plan_error_with_engine(
            server,
            &snapshot,
            error,
            Some(plan_evidence(&plan, outcomes.clone(), None)),
            actual_engine_for_mode(snapshot.config.mode),
        )
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
    use super::*;

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
        assert!(
            overlap.message.contains("ranges[1]") && overlap.message.contains("ranges[0]"),
            "overlap message must name original indices: {}",
            overlap.message
        );
        let duplicate = build_plan(source, &[range(0, 0, 0, 5), range(0, 0, 0, 5)])
            .err()
            .ok_or("duplicate ranges were admitted")?;
        assert_eq!(duplicate.reason, "duplicate_range");
        assert!(
            duplicate.message.contains("ranges[1]") && duplicate.message.contains("ranges[0]"),
            "duplicate message must name original indices: {}",
            duplicate.message
        );
        let reversal =
            build_plan(source, &[range(0, 5, 0, 2)]).err().ok_or("reversed range was admitted")?;
        assert_eq!(reversal.reason, "reversed_range");
        assert!(
            reversal.message.contains("ranges[0]") && reversal.message.contains("ends before"),
            "reversal message must name the range: {}",
            reversal.message
        );
        Ok(())
    }

    #[test]
    fn astral_and_crlf_positions_are_strict() -> Result<(), Box<dyn std::error::Error>> {
        let source = "a🦀b\r\nnext\r\n";
        let plan = build_plan(source, &[range(0, 1, 0, 3), range(1, 0, 1, 4)])?;
        assert_eq!(plan.normalized_ranges[0].start.byte, 1);
        assert_eq!(plan.normalized_ranges[0].end.byte, 5);
        assert_eq!(plan.range_provenance[0].original_index, 0);
        assert_eq!(plan.range_provenance[1].original_index, 1);
        let surrogate_split = build_plan(source, &[range(0, 2, 0, 3)])
            .err()
            .ok_or("surrogate-splitting position was admitted")?;
        assert_eq!(surrogate_split.reason, "invalid_position");
        assert!(
            surrogate_split.message.contains("splits a surrogate pair"),
            "surrogate message must discriminate: {}",
            surrogate_split.message
        );
        let past_line_end = build_plan(source, &[range(0, 0, 0, 99)])
            .err()
            .ok_or("past-end character was admitted")?;
        assert_eq!(past_line_end.reason, "invalid_position");
        assert!(
            past_line_end.message.contains("outside line 0"),
            "past-end message must discriminate: {}",
            past_line_end.message
        );
        Ok(())
    }

    #[test]
    fn lone_cr_line_endings_are_recognized() -> Result<(), Box<dyn std::error::Error>> {
        let source = "a\rb";
        let plan = build_plan(source, &[range(1, 0, 1, 1)])?;
        assert_eq!(plan.normalized_ranges[0].start.byte, 2);
        assert_eq!(plan.normalized_ranges[0].end.byte, 3);
        let across_terminator = build_plan(source, &[range(0, 0, 0, 1)])?;
        assert_eq!(across_terminator.normalized_ranges[0].end.byte, 1);
        Ok(())
    }

    #[test]
    fn malformed_and_out_of_document_ranges_are_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "abc\n";
        let malformed = build_plan(source, &[json!({"start": {"line": 0}})])
            .err()
            .ok_or("malformed range was admitted")?;
        assert_eq!(malformed.reason, "invalid_range");
        assert!(!malformed.message.is_empty(), "invalid_range must carry the parse_range message");
        let outside = build_plan(source, &[range(9, 0, 9, 1)])
            .err()
            .ok_or("out-of-document range was admitted")?;
        assert_eq!(outside.reason, "invalid_position");
        assert!(
            outside.message.contains("line 9 is outside the current document"),
            "out-of-document message must discriminate: {}",
            outside.message
        );
        Ok(())
    }

    #[test]
    fn adjacent_and_single_empty_ranges_are_defined() -> Result<(), Box<dyn std::error::Error>> {
        let source = "abcdef\n";
        let adjacent = build_plan(source, &[range(0, 0, 0, 3), range(0, 3, 0, 6)])?;
        assert_eq!(adjacent.normalized_ranges.len(), 2);
        assert_eq!(adjacent.normalized_ranges[0].start.byte, 0);
        assert_eq!(adjacent.normalized_ranges[0].end.byte, 3);
        assert_eq!(adjacent.normalized_ranges[1].start.byte, 3);
        assert_eq!(adjacent.normalized_ranges[1].end.byte, 6);
        assert_eq!(adjacent.range_provenance[0].original_index, 0);
        assert_eq!(adjacent.range_provenance[1].original_index, 1);
        let empty = build_plan(source, &[range(0, 3, 0, 3)])?;
        assert_eq!(empty.normalized_ranges[0].start.byte, empty.normalized_ranges[0].end.byte);
        assert_eq!(empty.normalized_ranges[0].start.byte, 3);
        Ok(())
    }

    #[test]
    fn position_record_at_preserves_wire_and_byte_fields() {
        let record = PositionRecord::at(2, 5, 17);
        assert_eq!(record.line, 2, "PositionRecord::at must keep line");
        assert_eq!(record.character, 5, "PositionRecord::at must keep character");
        assert_eq!(record.byte, 17, "PositionRecord::at must keep byte");
        let normalized = NormalizedRange::between(record, PositionRecord::at(2, 8, 20));
        assert_eq!(normalized.start.byte, 17, "NormalizedRange::between must keep start");
        assert_eq!(normalized.end.byte, 20, "NormalizedRange::between must keep end");
        let admitted = AdmittedRange::new(3, normalized);
        assert_eq!(admitted.original_index, 3, "AdmittedRange::new must keep original_index");
        assert_eq!(admitted.normalized.end.byte, 20, "AdmittedRange::new must keep normalized end");
    }

    #[test]
    fn plan_error_constructors_name_geometry_failures() {
        assert_eq!(
            PlanError::outside_document(9).message,
            "line 9 is outside the current document"
        );
        assert!(
            PlanError::surrogate_split(0, 2).message.contains("splits a surrogate pair"),
            "surrogate constructor must keep discriminant text"
        );
        assert!(
            PlanError::outside_line(0, 99, 4).message.contains("outside line 0"),
            "outside-line constructor must keep discriminant text"
        );
        assert_eq!(PlanError::reversed_range(0).message, "ranges[0] ends before it starts");
        assert_eq!(PlanError::duplicate_range(1, 0).message, "ranges[1] duplicates ranges[0]");
        assert_eq!(PlanError::overlapping_ranges(1, 0).message, "ranges[1] overlaps ranges[0]");
    }

    #[test]
    fn byte_offset_observes_outside_surrogate_and_past_end()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "a🦀b\n";
        let geometry = SourceGeometry::new(source);
        let outside = geometry
            .byte_offset(source, 9, 0)
            .err()
            .ok_or("outside-document byte_offset succeeded")?;
        assert_eq!(outside.reason, "invalid_position");
        assert_eq!(outside.message, "line 9 is outside the current document");
        let surrogate = geometry
            .byte_offset(source, 0, 2)
            .err()
            .ok_or("surrogate-splitting byte_offset succeeded")?;
        assert_eq!(surrogate.reason, "invalid_position");
        assert!(
            surrogate.message.contains("splits a surrogate pair"),
            "surrogate byte offset must reject a split pair: {}",
            surrogate.message
        );
        let past =
            geometry.byte_offset(source, 0, 99).err().ok_or("past-end byte_offset succeeded")?;
        assert_eq!(past.reason, "invalid_position");
        assert!(
            past.message.contains("outside line 0"),
            "byte offset must reject a character past line end: {}",
            past.message
        );
        assert_eq!(geometry.byte_offset(source, 0, 0)?, 0);
        assert_eq!(geometry.byte_offset(source, 0, 1)?, 1);
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

    fn text_edit(sl: u32, sc: u32, el: u32, ec: u32, new_text: &str) -> FormatTextEdit {
        use crate::features::formatting::{FormatPosition, FormatRange};
        FormatTextEdit {
            range: FormatRange::new(FormatPosition::new(sl, sc), FormatPosition::new(el, ec)),
            new_text: new_text.to_string(),
        }
    }

    #[test]
    fn empty_point_compose_keeps_zero_width_bound() -> Result<(), Box<dyn std::error::Error>> {
        let source = "abcdef\n";
        let plan = build_plan(source, &[range(0, 3, 0, 3)])?;
        let ok = compose_edits(source, &plan, vec![vec![text_edit(0, 3, 0, 3, "X")]], 1, "cfg")?;
        assert_eq!(ok.0.len(), 1);
        assert_eq!(ok.0[0].new_text, "X");

        // Bound each side independently so a one-sided gate regression cannot hide.
        let left_escape =
            compose_edits(source, &plan, vec![vec![text_edit(0, 2, 0, 3, "X")]], 1, "cfg")
                .err()
                .ok_or("empty-point span must reject lower-bound escape")?;
        assert_eq!(left_escape.reason, "edit_outside_range");

        let right_escape =
            compose_edits(source, &plan, vec![vec![text_edit(0, 3, 0, 4, "X")]], 1, "cfg")
                .err()
                .ok_or("empty-point span must reject upper-bound escape")?;
        assert_eq!(right_escape.reason, "edit_outside_range");
        Ok(())
    }

    #[test]
    fn same_line_adjacent_edits_compose_in_order() -> Result<(), Box<dyn std::error::Error>> {
        let source = "abcdefgh\n";
        // One admitted span with right-to-left formatter edits proves canonical sort.
        let plan = build_plan(source, &[range(0, 0, 0, 8)])?;
        let (edits, digest) = compose_edits(
            source,
            &plan,
            vec![vec![text_edit(0, 4, 0, 8, "BBBB"), text_edit(0, 0, 0, 4, "AAAA")]],
            7,
            "cfg-fingerprint",
        )?;
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "AAAA");
        assert_eq!(edits[1].new_text, "BBBB");
        assert!(!digest.is_empty(), "composed edit digest must be non-empty");

        let conflict = compose_edits(
            source,
            &plan,
            vec![vec![text_edit(0, 0, 0, 5, "AAAAA"), text_edit(0, 4, 0, 8, "BBBB")]],
            7,
            "cfg-fingerprint",
        )
        .err()
        .ok_or("overlapping same-line edits must refuse")?;
        assert_eq!(conflict.reason, "edit_conflict");
        Ok(())
    }
}
