//! Accepted-edit receipts for deterministic inline completion.
//!
//! These tests apply returned ghost text through the same LSP UTF-16 range shape
//! an editor would use, then verify the accepted text does not make parser
//! diagnostics worse.

use std::error::Error;

use lsp_types::{Position, Range};
use perl_lsp_rs_core::providers::inline_completion::{
    InlineCompletionItem, InlineCompletionProvider,
};
use perl_parser::Parser;
use perl_parser_core::position::{offset_to_utf16_line_col, utf16_line_col_to_offset};

type TestResult = Result<(), Box<dyn Error>>;

const CURSOR: &str = "<<CURSOR>>";

#[derive(Debug)]
struct AcceptedEditScenario {
    name: &'static str,
    source: &'static str,
    expected_first: &'static str,
    expected_after: &'static str,
}

#[test]
fn accepted_inline_completion_edits_preserve_local_parse_state() -> TestResult {
    let scenarios = [
        AcceptedEditScenario {
            name: "partial_use_replacement",
            source: "use str<<CURSOR>>\n",
            expected_first: "strict;",
            expected_after: "use strict;\n",
        },
        AcceptedEditScenario {
            name: "visible_lexical_return",
            source: "sub compute {\n    my $result = build();\n    <<CURSOR>>\n}\n",
            expected_first: "return $result;",
            expected_after: "sub compute {\n    my $result = build();\n    return $result;\n}\n",
        },
        AcceptedEditScenario {
            name: "self_receiver_method",
            source: "package Demo;\nsub save {}\nsub caller {\n    my $self = shift;\n    $self-><<CURSOR>>\n}\n",
            expected_first: "save()",
            expected_after: "package Demo;\nsub save {}\nsub caller {\n    my $self = shift;\n    $self->save()\n}\n",
        },
        AcceptedEditScenario {
            name: "constructor_shift_style",
            source: "sub helper {\n    my $self = shift;\n}\n\nsub new<<CURSOR>>\n",
            expected_first: " {\n    my $class = shift;\n    my $self = bless {}, $class;\n    return $self;\n}",
            expected_after: "sub helper {\n    my $self = shift;\n}\n\nsub new {\n    my $class = shift;\n    my $self = bless {}, $class;\n    return $self;\n}\n",
        },
    ];

    for scenario in scenarios {
        assert_accepted_edit_preserves_parse_state(&scenario)?;
    }

    Ok(())
}

#[test]
fn accepted_inline_completion_edit_application_rejects_invalid_ranges() -> TestResult {
    let item = InlineCompletionItem {
        insert_text: "strict;".into(),
        filter_text: Some("strict".into()),
        range: Some(Range { start: Position::new(0, 4), end: Position::new(0, 1) }),
        command: None,
    };

    let err = match apply_inline_item("use str\n", 0, 7, &item) {
        Ok(edited) => {
            return Err(format!("invalid range should fail, edited text was {edited:?}").into());
        }
        Err(err) => err,
    };

    assert!(err.contains("range 4..1"), "unexpected invalid-range error: {err}");
    Ok(())
}

fn assert_accepted_edit_preserves_parse_state(scenario: &AcceptedEditScenario) -> TestResult {
    let cursor_offset = scenario
        .source
        .find(CURSOR)
        .ok_or_else(|| format!("{}: fixture must include cursor", scenario.name))?;
    let source = scenario.source.replacen(CURSOR, "", 1);
    let (line, character) = offset_to_utf16_line_col(source.as_str(), cursor_offset);
    let provider = InlineCompletionProvider::new();
    let completions = provider.get_inline_completions(source.as_str(), line, character);
    let first = completions
        .items
        .first()
        .ok_or_else(|| format!("{}: expected an inline completion", scenario.name))?;

    assert_eq!(
        first.insert_text, scenario.expected_first,
        "{}: unexpected first inline completion",
        scenario.name
    );

    let accepted = apply_inline_item(source.as_str(), line, character, first)
        .map_err(|err| format!("{}: {err}", scenario.name))?;
    assert_eq!(
        accepted, scenario.expected_after,
        "{}: accepted edit produced unexpected text",
        scenario.name
    );

    let before = parser_diagnostic_count(source.as_str());
    let after = parser_diagnostic_count(accepted.as_str());
    assert!(
        after <= before,
        "{}: accepted edit increased parser diagnostics from {before} to {after}",
        scenario.name
    );

    Ok(())
}

fn apply_inline_item(
    source: &str,
    line: u32,
    character: u32,
    item: &InlineCompletionItem,
) -> Result<String, String> {
    let cursor = utf16_line_col_to_offset(source, line, character);
    let (start, end) = item
        .range
        .as_ref()
        .map(|range| range_offsets(source, range))
        .transpose()?
        .unwrap_or((cursor, cursor));

    if start > end || end > source.len() {
        return Err(format!(
            "invalid edit offsets {start}..{end} for source length {}",
            source.len()
        ));
    }

    let mut edited = String::with_capacity(source.len() - (end - start) + item.insert_text.len());
    edited.push_str(source.get(..start).ok_or_else(|| format!("invalid start offset {start}"))?);
    edited.push_str(item.insert_text.as_str());
    edited.push_str(source.get(end..).ok_or_else(|| format!("invalid end offset {end}"))?);
    Ok(edited)
}

fn range_offsets(source: &str, range: &Range) -> Result<(usize, usize), String> {
    let start = utf16_line_col_to_offset(source, range.start.line, range.start.character);
    let end = utf16_line_col_to_offset(source, range.end.line, range.end.character);
    if source.get(start..end).is_none() {
        return Err(format!("range {start}..{end} does not align to UTF-8 boundaries"));
    }
    Ok((start, end))
}

fn parser_diagnostic_count(source: &str) -> usize {
    Parser::new(source).parse_with_recovery().diagnostics.len()
}
