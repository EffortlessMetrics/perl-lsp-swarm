//! #9153: unterminated heredoc diagnostics must pin to the opener/body, not EOF.
//!
//! Discriminates the `drain_pending_heredocs` location bug that parked
//! `malformed_heredoc_recovery` in `RECOVERY_GEOMETRY_FOLLOWUPS`.

use perl_parser_core::Parser;

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

#[test]
fn unterminated_heredoc_diagnostic_pins_to_opener_not_eof() -> Result<(), String> {
    let source =
        include_str!("../../perl-corpus/fixtures/parser_accuracy/malformed_heredoc_recovery.pl");
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let diagnostics = &output.diagnostics;
    assert!(
        !diagnostics.is_empty(),
        "unterminated heredoc must emit at least one diagnostic"
    );

    let opener = source
        .find("<<'BROKEN'")
        .ok_or_else(|| "fixture must contain the heredoc opener".to_string())?;
    let body = source
        .find("unterminated body")
        .ok_or_else(|| "fixture must contain the heredoc body".to_string())?;
    let eof = source.len();

    let locations: Vec<usize> = diagnostics.iter().filter_map(|d| d.location()).collect();
    assert!(
        !locations.is_empty(),
        "unterminated heredoc diagnostic must carry a byte location"
    );

    assert!(
        locations.iter().any(|&loc| loc == opener),
        "expected diagnostic at opener offset {opener}, got {locations:?}"
    );
    assert!(
        locations.iter().any(|&loc| loc == body),
        "expected diagnostic at body offset {body}, got {locations:?}"
    );
    assert!(
        locations.iter().all(|&loc| loc != eof),
        "unterminated heredoc must not report at EOF ({eof}); got {locations:?}"
    );

    let diagnostic_lines: Vec<usize> =
        locations.iter().copied().map(|loc| line_of(source, loc)).collect();
    let first_line = diagnostic_lines.iter().copied().min();
    assert_eq!(
        first_line,
        Some(3),
        "first diagnostic must land on opener line 3, got {first_line:?} from {locations:?}"
    );
    assert!(
        diagnostic_lines.contains(&4),
        "body-line diagnostic evidence required for region 3..=4; lines={diagnostic_lines:?}"
    );

    let recovery_line = 6usize;
    assert!(
        diagnostic_lines.iter().all(|&line| line != recovery_line),
        "diagnostic must not spill onto post-error recovery line {recovery_line}; locations={locations:?}"
    );
    Ok(())
}

#[test]
fn later_same_line_heredoc_body_diagnostic_uses_collected_span() -> Result<(), String> {
    // Two openers on one declaration line: B's collected body starts at `b`, not
    // at the shared queue-time `decl.body_start` (which points at `a`).
    let source = "f(<<'A', <<'B');\na\nA\nb\n";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let locations: Vec<usize> = output.diagnostics.iter().filter_map(|d| d.location()).collect();

    let opener_b = source
        .find("<<'B'")
        .ok_or_else(|| "fixture must contain second heredoc opener".to_string())?;
    let body_b = source
        .rfind('b')
        .ok_or_else(|| "fixture must contain unterminated second body".to_string())?;
    let body_a = source
        .find('\n')
        .and_then(|idx| source.get(idx + 1..).map(|_| idx + 1))
        .ok_or_else(|| "fixture must contain first body line".to_string())?;

    assert!(
        locations.iter().any(|&loc| loc == opener_b),
        "expected unterminated diagnostic at B opener {opener_b}, got {locations:?}"
    );
    assert!(
        locations.iter().any(|&loc| loc == body_b),
        "expected body diagnostic at collected B body {body_b}, got {locations:?}"
    );
    assert!(
        !locations.iter().any(|&loc| loc == body_a),
        "B must not report its body diagnostic on A's body offset {body_a}; locations={locations:?}"
    );
    Ok(())
}
