//! Atomic multi-range formatting plan builder (types + geometry).
//!
//! Live `textDocument/rangesFormatting` wiring and edit composition remain
//! successor slices; this module proves plan admission and refusal geometry.

use serde::Serialize;

use super::{Value, digest, json, parse_range};

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
        let start = *self.line_starts.get(line_index).ok_or_else(|| {
            PlanError::new(
                "invalid_position",
                format!("line {line} is outside the current document"),
            )
        })?;
        let end = self.line_content_end(source, line_index);

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
        let outside = build_plan(source, &[range(9, 0, 9, 1)])
            .err()
            .ok_or("out-of-document range was admitted")?;
        assert_eq!(outside.reason, "invalid_position");
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
}
