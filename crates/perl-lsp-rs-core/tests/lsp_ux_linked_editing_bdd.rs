//! BDD-style UX coverage for `textDocument/linkedEditingRange`.
//!
//! This suite builds a tiny scenario harness around cursor-marked fixtures so
//! each test focuses on one user-facing behavior.

use std::error::Error;

use perl_lsp_rs_core::providers::lsp_compat::linked_editing::handle_linked_editing;
use perl_parser_core::position::offset_to_utf16_line_col;

type TestResult = Result<(), Box<dyn Error>>;

const CURSOR: &str = "<<CURSOR>>";

struct LinkedEditingScenario {
    text: String,
    line: u32,
    character: u32,
}

impl LinkedEditingScenario {
    fn from_fixture(fixture: &str) -> Result<Self, String> {
        let byte = fixture
            .find(CURSOR)
            .ok_or_else(|| "fixture must include <<CURSOR>> marker".to_string())?;
        let text = fixture.replacen(CURSOR, "", 1);
        let (line, character) = offset_to_utf16_line_col(&text, byte);

        Ok(Self { text, line, character })
    }

    fn linked_ranges(&self) -> Option<lsp_types::LinkedEditingRanges> {
        handle_linked_editing(&self.text, self.line, self.character)
    }

    fn assert_pair(self, expected_first: &str, expected_second: &str) -> TestResult {
        let ranges = self
            .linked_ranges()
            .ok_or_else(|| "expected linked range pair but got none".to_string())?;
        if ranges.ranges.len() != 2 {
            return Err(format!("expected exactly 2 ranges, got {}", ranges.ranges.len()).into());
        }

        let first = slice_for_range(&self.text, &ranges.ranges[0])?;
        let second = slice_for_range(&self.text, &ranges.ranges[1])?;

        if first != expected_first {
            return Err(format!(
                "first range mismatch: expected '{expected_first}', got '{first}'"
            )
            .into());
        }
        if second != expected_second {
            return Err(format!(
                "second range mismatch: expected '{expected_second}', got '{second}'"
            )
            .into());
        }

        Ok(())
    }
}

fn slice_for_range<'a>(text: &'a str, range: &lsp_types::Range) -> Result<&'a str, String> {
    let start = perl_parser_core::position::utf16_line_col_to_offset(
        text,
        range.start.line,
        range.start.character,
    );
    let end = perl_parser_core::position::utf16_line_col_to_offset(
        text,
        range.end.line,
        range.end.character,
    );
    text.get(start..end).ok_or_else(|| format!("invalid UTF-8 boundaries for range {start}..{end}"))
}

#[test]
fn given_cursor_on_opening_brace_when_linked_editing_requested_then_matching_braces_are_returned()
-> TestResult {
    LinkedEditingScenario::from_fixture("if (1) <<CURSOR>>{ say 'x'; }")?.assert_pair("{", "}")
}

#[test]
fn given_cursor_after_closing_brace_when_linked_editing_requested_then_matching_braces_are_still_returned()
-> TestResult {
    LinkedEditingScenario::from_fixture("if (1) { say 'x'; }<<CURSOR>>")?.assert_pair("{", "}")
}

#[test]
fn given_cursor_inside_quoted_heredoc_label_when_linked_editing_requested_then_label_and_terminator_are_returned()
-> TestResult {
    LinkedEditingScenario::from_fixture("my $doc = <<\"EO<<CURSOR>>D\";\ncontent\nEOD\n")?
        .assert_pair("EOD", "EOD")
}

#[test]
fn given_cursor_on_substitution_middle_delimiter_when_linked_editing_requested_then_replacement_pair_is_returned()
-> TestResult {
    LinkedEditingScenario::from_fixture("$x =~ s/foo<<CURSOR>>/bar/;")?.assert_pair("/", "/")
}

#[test]
fn given_cursor_on_regex_delimiter_when_linked_editing_requested_then_delimiter_pair_is_returned()
-> TestResult {
    LinkedEditingScenario::from_fixture("$x =~ m<<CURSOR>>#foo#;")?.assert_pair("#", "#")
}

/// Independent UTF-16 columns for `say "<prefix>"; $x =~ m/foo/;`
///
/// `"say \""` is 5 units, `"\"; $x =~ m"` is 10 units, so the first `/` sits at
/// `15 + prefix_utf16`. `é` and `€` are one UTF-16 unit (column 16); `😀` is a
/// supplementary-plane surrogate pair (column 17). The second `/` is four units
/// later (`foo` is three ASCII characters).
fn assert_m_slash_foo_pair(scenario: &LinkedEditingScenario, first_column: u32) -> TestResult {
    let ranges = scenario.linked_ranges().ok_or("expected regex delimiter ranges")?;
    if ranges.ranges.len() != 2 {
        return Err(format!("expected exactly 2 ranges, got {}", ranges.ranges.len()).into());
    }

    let first = &ranges.ranges[0];
    let second = &ranges.ranges[1];
    let expected_first = lsp_types::Position::new(0, first_column);
    let expected_second = lsp_types::Position::new(0, first_column + 4);
    if first.start != expected_first {
        return Err(format!(
            "first delimiter start mismatch: expected {expected_first:?}, got {:?}",
            first.start
        )
        .into());
    }
    if second.start != expected_second {
        return Err(format!(
            "second delimiter start mismatch: expected {expected_second:?}, got {:?}",
            second.start
        )
        .into());
    }
    if slice_for_range(&scenario.text, first)? != "/" {
        return Err("first range is not the ASCII '/' delimiter".into());
    }
    if slice_for_range(&scenario.text, second)? != "/" {
        return Err("second range is not the ASCII '/' delimiter".into());
    }
    Ok(())
}

#[test]
fn regex_delimiters_after_2_3_4_byte_prefixes_keep_literal_utf16_ranges() -> TestResult {
    // 2-byte UTF-8 BMP, 3-byte UTF-8 BMP, 4-byte UTF-8 supplementary.
    for (prefix, first_column) in [("é", 16_u32), ("€", 16), ("😀", 17)] {
        let fixture = format!("say \"{prefix}\"; $x =~ m<<CURSOR>>/foo/;");
        let scenario = LinkedEditingScenario::from_fixture(&fixture)?;
        assert_m_slash_foo_pair(&scenario, first_column)?;
    }
    Ok(())
}

#[test]
fn regex_delimiters_after_multibyte_prefix_on_crlf_line_keep_literal_utf16_ranges() -> TestResult {
    let scenario =
        LinkedEditingScenario::from_fixture("say \"é\"; $x =~ m<<CURSOR>>/foo/;\r\nnext line\r\n")?;
    assert_m_slash_foo_pair(&scenario, 16)
}

#[test]
fn regex_delimiters_survive_sequential_full_document_replacements() -> TestResult {
    // Each fixture is a complete replacement of the previous document text.
    let sequence: &[(&str, Option<u32>)] = &[
        ("say \"é\"; $x =~ m<<CURSOR>>/foo/;", Some(16)),
        ("say \"€\"; $x =~ m<<CURSOR>>/foo/;", Some(16)),
        ("my <<CURSOR>>$name = 'x';", None),
        ("say \"😀\"; $x =~ m<<CURSOR>>/foo/;", Some(17)),
        ("say \"é\"; $x =~ m<<CURSOR>>/foo/;\r\n", Some(16)),
    ];

    for (fixture, expected_first_column) in sequence {
        let scenario = LinkedEditingScenario::from_fixture(fixture)?;
        match expected_first_column {
            Some(column) => assert_m_slash_foo_pair(&scenario, *column)?,
            None => {
                if scenario.linked_ranges().is_some() {
                    return Err(
                        "expected no linked ranges after replacing with a plain identifier".into(),
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn given_cursor_on_plain_identifier_when_linked_editing_requested_then_no_ranges_are_returned()
-> TestResult {
    let scenario = LinkedEditingScenario::from_fixture("my <<CURSOR>>$name = 'x';")?;
    if scenario.linked_ranges().is_some() {
        return Err("expected no linked ranges for plain identifier cursor".into());
    }
    Ok(())
}
