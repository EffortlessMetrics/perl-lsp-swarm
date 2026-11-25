//! Encoding translation only. Native syntax admission remains in `perl-tidy`.
use super::{
    FormatChangeSummary, FormatDiagnostic, FormatDiagnosticSeverity, FormatDisposition,
    FormatOutcome, FormatReasonCode, FormatRequestTarget, FormatResult, TextEdit, TextPosition,
    TextRange, TypedFormatResult,
};
use perl_tidy::{ExactSource, FormatRequest, SourceByteRange, TargetWidening};

use perl_position_tracking::{
    PositionEncoding, PositionMappingDisposition, WirePosition, WireRange,
    byte_range_to_wire_range, wire_position_to_byte, wire_range_to_bytes,
};

fn to_wire(position: TextPosition) -> WirePosition {
    WirePosition { line: position.line, character: position.character }
}

fn exact_byte_position(source: &str, position: WirePosition) -> Option<usize> {
    let mapping = wire_position_to_byte(source, position, PositionEncoding::Utf16);
    if !mapping.disposition().is_exact() {
        return None;
    }
    mapping.byte_offset()
}

/// Query the geometry owner for a line's end solely for the named widening
/// policy. Requested endpoints have already passed strict Exact admission.
/// The maximum-column probe is not a request and its normalized result never
/// replaces a requested coordinate; the returned line end is admitted again
/// as Exact before it may enlarge the policy interval.
fn legacy_line_body_end(source: &str, line: u32) -> Option<usize> {
    let probe = wire_position_to_byte(
        source,
        WirePosition { line, character: u32::MAX },
        PositionEncoding::Utf16,
    );
    if !matches!(
        probe.disposition(),
        PositionMappingDisposition::Exact | PositionMappingDisposition::LineEndNormalized
    ) {
        return None;
    }
    exact_byte_position(source, probe.normalized()?)
}

pub(super) fn admit_legacy_range(
    exact: &ExactSource<'_>,
    range: TextRange,
) -> Option<FormatRequest> {
    let source = exact.text();
    let mapping = wire_range_to_bytes(
        source,
        WireRange { start: to_wire(range.start), end: to_wire(range.end) },
        PositionEncoding::Utf16,
    );
    if !mapping.disposition().is_exact()
        || !mapping.start().disposition().is_exact()
        || !mapping.end().disposition().is_exact()
    {
        return None;
    }
    let bytes = mapping.byte_range()?;
    let requested = SourceByteRange::new(exact, bytes.start, bytes.end).ok()?;
    // This is explicit legacy policy, separate from coordinate admission:
    // a zero-character end excludes that line; a positive end includes its
    // complete body. A collapsed range at column zero remains empty.
    let (admitted_start, admitted_end) = if bytes.is_empty() && range.end.character == 0 {
        (bytes.start, bytes.end)
    } else {
        let start =
            exact_byte_position(source, WirePosition { line: range.start.line, character: 0 })?;
        let end = if range.end.character == 0 {
            bytes.end
        } else {
            legacy_line_body_end(source, range.end.line)?
        };
        (start, end)
    };
    let admitted = SourceByteRange::new(exact, admitted_start, admitted_end).ok()?;
    let widening = (admitted != requested).then(|| TargetWidening {
        owner: "perl-lsp-perltidy/legacy-utf16-lines-v1".to_string(),
        reason: "legacy ranges select complete line bodies".to_string(),
    });
    let request = FormatRequest {
        requested: perl_tidy::FormatRequestTarget::Range { range: requested },
        admitted,
        widening,
    };
    request.validate(exact).ok()?;
    Some(request)
}

fn project_range(source: &str, range: &SourceByteRange) -> Option<TextRange> {
    let mapping =
        byte_range_to_wire_range(source, range.start()..range.end(), PositionEncoding::Utf16);
    if !mapping.is_exact() || !mapping.start().is_exact() || !mapping.end().is_exact() {
        return None;
    }
    let range = mapping.wire_range()?;
    Some(TextRange::new(
        TextPosition::new(range.start.line, range.start.character),
        TextPosition::new(range.end.line, range.end.character),
    ))
}

pub(super) fn project_typed(
    source: &str,
    target: FormatRequestTarget,
    typed: perl_tidy::TypedFormatResult,
) -> TypedFormatResult {
    let edits: Option<Vec<_>> = typed
        .result
        .edits
        .iter()
        .map(|edit| Some(TextEdit::new(project_range(source, &edit.range)?, edit.new_text.clone())))
        .collect();
    let mut outcome = FormatOutcome {
        disposition: typed.outcome.disposition,
        reason: typed.outcome.reason,
        identity: typed.outcome.identity,
        target,
        execution: typed.outcome.execution,
        change: typed.outcome.change,
        safety: typed.outcome.safety,
        next_action: typed.outcome.next_action,
    };
    let Some(edits) = edits else {
        outcome.disposition = FormatDisposition::FailedOrNotProven;
        outcome.reason = FormatReasonCode::InstrumentFailure;
        outcome.safety.line_endings = perl_tidy::FormatLineEndingDisposition::Preserved;
        outcome.change = FormatChangeSummary {
            edit_count: 0,
            source_bytes_changed: 0,
            rendered_bytes_changed: 0,
            changed_lines: 0,
        };
        outcome.next_action = Some(
            "use the canonical byte edit plan or report the compatibility projection".to_string(),
        );
        let mut result = FormatResult::unchanged(source);
        result.diagnostics.push(FormatDiagnostic::new(
            "native.format.compatibility_projection",
            FormatDiagnosticSeverity::Error,
            None,
            "a canonical byte edit cannot be represented by the legacy UTF-16 DTO",
        ));
        return TypedFormatResult { result, outcome };
    };
    let result = FormatResult {
        formatted: typed.result.formatted,
        edits,
        changed: typed.result.changed,
        diagnostics: typed
            .result
            .diagnostics
            .into_iter()
            .map(|diagnostic| FormatDiagnostic {
                code: diagnostic.code,
                severity: diagnostic.severity,
                range: diagnostic.range.as_ref().and_then(|range| project_range(source, range)),
                message: diagnostic.message,
            })
            .collect(),
    };
    TypedFormatResult { result, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn canonical_geometry_projects_non_bmp_crlf_and_bare_cr_exactly() -> TestResult {
        let source = "α😀\r\nz\rq\n";
        let exact = ExactSource::new(source);
        let bytes = SourceByteRange::new(&exact, 2, 9)?;
        assert_eq!(
            project_range(source, &bytes),
            Some(TextRange::new(TextPosition::new(0, 1), TextPosition::new(1, 1))),
        );
        let bare_cr_line = TextRange::new(TextPosition::new(2, 0), TextPosition::new(2, 1));
        let request =
            admit_legacy_range(&exact, bare_cr_line).ok_or("bare-CR geometry must map exactly")?;
        assert_eq!(request.admitted.start(), 10);
        assert_eq!(request.admitted.end(), 11);
        assert!(request.widening.is_none());
        assert_eq!(project_range(source, &request.admitted), Some(bare_cr_line));
        Ok(())
    }

    #[test]
    fn output_projection_rejects_the_interior_of_crlf() -> TestResult {
        let source = "a\r\nb";
        let exact = ExactSource::new(source);
        // Both offsets are UTF-8 boundaries; offset 2 is nevertheless inside
        // one CRLF separator and must not become an apparently valid edit.
        let bytes = SourceByteRange::new(&exact, 1, 2)?;
        assert!(project_range(source, &bytes).is_none());
        Ok(())
    }

    #[test]
    fn requested_endpoints_must_be_exact_before_named_line_widening() -> TestResult {
        let source = "a😀bc\r\nsecond\rthird";
        let exact = ExactSource::new(source);
        let requested = TextRange::new(TextPosition::new(0, 1), TextPosition::new(0, 3));
        let request = admit_legacy_range(&exact, requested).ok_or("valid range should admit")?;
        let perl_tidy::FormatRequestTarget::Range { range } = request.requested else {
            return Err("range identity must be retained".into());
        };
        assert_eq!((range.start(), range.end()), (1, 5));
        assert_eq!((request.admitted.start(), request.admitted.end()), (0, 7));
        let widening = request.widening.ok_or("whole-line expansion must be explicit")?;
        assert_eq!(widening.owner, "perl-lsp-perltidy/legacy-utf16-lines-v1");
        for invalid in [
            TextRange::new(TextPosition::new(0, 1), TextPosition::new(0, 99)),
            TextRange::new(TextPosition::new(0, 99), TextPosition::new(1, 0)),
            TextRange::new(TextPosition::new(0, 2), TextPosition::new(0, 3)),
            TextRange::new(TextPosition::new(3, 0), TextPosition::new(3, 0)),
        ] {
            assert!(admit_legacy_range(&exact, invalid).is_none(), "{invalid:?}");
        }
        Ok(())
    }
}
