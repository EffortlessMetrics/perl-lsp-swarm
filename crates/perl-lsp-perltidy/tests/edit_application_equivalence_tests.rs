//! Offline proof for the #8048 independent edit-application oracle.
//!
//! The oracle must reject reversed, unreachable, overlapping, duplicate
//! zero-width, and mid-code-point edits instead of clamping them, and must
//! apply valid whole-document edits byte-exactly through true EOF. It shares
//! no geometry code with any production range constructor.

use perl_lsp_perltidy::native::{
    EditApplicationError, EditSpec, FormatResult, PositionEncoding, TextEdit, TextPosition,
    TextRange, apply_edits_exact,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const UTF16: PositionEncoding = PositionEncoding::Utf16CodeUnits;
const BYTES: PositionEncoding = PositionEncoding::Utf8Bytes;

fn one_edit(
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
    new_text: &str,
) -> EditSpec {
    EditSpec::new(start_line, start_character, end_line, end_character, new_text)
}

#[test]
fn empty_and_identity_edits_reproduce_exact_source() -> TestResult {
    assert_eq!(apply_edits_exact("my $x = 1;\n", &[], UTF16)?, "my $x = 1;\n");
    assert_eq!(
        apply_edits_exact("my $x = 1;\n", &[one_edit(0, 0, 0, 0, "")], UTF16)?,
        "my $x = 1;\n"
    );
    Ok(())
}

#[test]
fn true_eof_whole_document_edits_apply_byte_exactly() -> TestResult {
    let rows: &[(&str, u32, u32, &str)] = &[
        // (source, end line, end character, replacement)
        ("my $x=1;", 0, 8, "my $x = 1;"),
        ("x;\n", 1, 0, "x;\n"),
        ("x;\r\n", 1, 0, "x;\r\n"),
        ("x;\r", 1, 0, "x;\r"),
        ("\n", 1, 0, "\n\n\n"),
        ("a\r\nb\r\n", 2, 0, "a\r\nb\r\nc\r\n"),
        ("ab😀", 0, 4, "ab😀"),
        ("x\n😀", 1, 2, "x\n😀!"),
        ("my $face=\"😀\";\n", 1, 0, "my $face = \"😀\";\n"),
        ("", 0, 0, "\n"),
    ];

    for (source, end_line, end_character, replacement) in rows {
        let applied = apply_edits_exact(
            source,
            &[one_edit(0, 0, *end_line, *end_character, replacement)],
            UTF16,
        )?;
        assert_eq!(
            applied, *replacement,
            "true-EOF edit over {source:?} must render the replacement exactly"
        );
    }
    Ok(())
}

#[test]
fn utf8_byte_positions_apply_byte_exactly() -> TestResult {
    // In byte encoding, columns count UTF-8 bytes since line start: the
    // four-byte emoji occupies columns 2..6, so this edit replaces it whole.
    let applied = apply_edits_exact("ab😀cd", &[one_edit(0, 2, 0, 6, "X")], BYTES)?;
    assert_eq!(applied, "abXcd");
    Ok(())
}

#[test]
fn multiple_valid_edits_apply_in_source_order_regardless_of_input_order() -> TestResult {
    let source = "alpha beta gamma\n";
    let edits =
        [one_edit(0, 11, 0, 16, "GAMMA"), one_edit(0, 5, 0, 6, "-"), one_edit(0, 0, 0, 5, "ALPHA")];
    let applied = apply_edits_exact(source, &edits, UTF16)?;
    assert_eq!(applied, "ALPHA-beta GAMMA\n");
    Ok(())
}

#[test]
fn adjacent_edits_are_allowed_but_overlap_is_rejected() -> TestResult {
    let source = "abcd";
    let adjacent = [one_edit(0, 0, 0, 2, "XY"), one_edit(0, 2, 0, 4, "Z")];
    assert_eq!(apply_edits_exact(source, &adjacent, UTF16)?, "XYZ");

    let overlapping = [one_edit(0, 0, 0, 3, "XY"), one_edit(0, 2, 0, 4, "Z")];
    assert_eq!(
        apply_edits_exact(source, &overlapping, UTF16),
        Err(EditApplicationError::OverlappingEdits { first_edit_index: 0, second_edit_index: 1 })
    );

    let nested = [one_edit(0, 0, 0, 4, "!"), one_edit(0, 1, 0, 3, "?")];
    assert_eq!(
        apply_edits_exact(source, &nested, UTF16),
        Err(EditApplicationError::OverlappingEdits { first_edit_index: 0, second_edit_index: 1 })
    );
    Ok(())
}

#[test]
fn duplicate_zero_width_insertion_points_are_rejected() -> TestResult {
    let edits = [one_edit(0, 2, 0, 2, "A"), one_edit(0, 2, 0, 2, "B")];
    assert_eq!(
        apply_edits_exact("abcd", &edits, UTF16),
        Err(EditApplicationError::DuplicateInsertionPoint {
            first_edit_index: 0,
            second_edit_index: 1
        })
    );
    Ok(())
}

#[test]
fn reversed_ranges_are_rejected_not_clamped() -> TestResult {
    assert_eq!(
        apply_edits_exact("hello world", &[one_edit(0, 5, 0, 1, "X")], UTF16),
        Err(EditApplicationError::ReversedRange { edit_index: 0 })
    );
    Ok(())
}

#[test]
fn unreachable_positions_are_rejected_not_clamped() -> TestResult {
    // Past line content.
    assert_eq!(
        apply_edits_exact("hi\n", &[one_edit(0, 3, 1, 0, "X")], UTF16),
        Err(EditApplicationError::UnreachablePosition { edit_index: 0, line: 0, character: 3 })
    );
    // Past last line (the terminal separator creates line 1; line 2 has no bytes).
    assert_eq!(
        apply_edits_exact("hi\n", &[one_edit(0, 0, 2, 0, "X")], UTF16),
        Err(EditApplicationError::UnreachablePosition { edit_index: 0, line: 2, character: 0 })
    );
    // Between CR and LF of a CRLF pair.
    assert_eq!(
        apply_edits_exact("a\r\n", &[one_edit(0, 0, 0, 2, "X")], UTF16),
        Err(EditApplicationError::UnreachablePosition { edit_index: 0, line: 0, character: 2 })
    );
    Ok(())
}

#[test]
fn mid_code_point_positions_are_rejected_in_both_encodings() -> TestResult {
    // UTF-16: 😀 occupies units 2..4 on its line; unit 3 is a surrogate half.
    assert_eq!(
        apply_edits_exact("ab😀\n", &[one_edit(0, 3, 1, 0, "X")], UTF16),
        Err(EditApplicationError::MidCodePoint { edit_index: 0, line: 0, character: 3 })
    );

    // UTF-8 bytes: 😀 spans byte columns 0..4; column 2 is inside the char.
    assert_eq!(
        apply_edits_exact("😀!", &[one_edit(0, 2, 0, 5, "X")], BYTES),
        Err(EditApplicationError::MidCodePoint { edit_index: 0, line: 0, character: 2 })
    );

    // A wide code point on an earlier line must not poison a later target.
    let applied = apply_edits_exact("😀\nabc", &[one_edit(1, 0, 1, 3, "xyz")], UTF16)?;
    assert_eq!(applied, "😀\nxyz");
    Ok(())
}

#[test]
fn production_replace_document_results_apply_equivalently_where_base_geometry_is_true() -> TestResult
{
    // On the current base, unterminated sources already reach true EOF in the
    // native constructor, so produced edits and independently applied edits
    // must agree byte-for-byte. Terminated-source parity lands with the
    // #11873/#10239 geometry cutover.
    for (source, formatted) in [("my $x=1;", "my $x = 1;"), ("$face=\"😀\"", "$face = \"😀\"")]
    {
        let result = FormatResult::replace_document(source, formatted);
        assert_eq!(result.edits.len(), 1);
        let specs: Vec<EditSpec> = result.edits.iter().map(EditSpec::from).collect();
        let applied = apply_edits_exact(source, &specs, UTF16)?;
        assert_eq!(applied, formatted, "produced edit for {source:?} must render exactly");
    }

    let unchanged = FormatResult::unchanged("keep me\n");
    assert!(unchanged.edits.is_empty());
    assert_eq!(apply_edits_exact("keep me\n", &[], UTF16)?, "keep me\n");
    Ok(())
}

#[test]
fn historical_last_content_line_geometry_is_detectably_wrong_through_the_oracle() -> TestResult {
    // The defect this issue exists for: a str::lines()-derived range ends at
    // the last content character, leaving the original terminal separator
    // outside the replacement. The oracle applies such an edit faithfully and
    // therefore exposes the doubled terminator as an applied/rendered
    // mismatch — without reusing any production constructor.
    let source = "y=1;\n";
    let formatted = "y = 1;\n";
    let defective_range = TextRange::new(TextPosition::new(0, 0), TextPosition::new(0, 4));
    let edit = TextEdit::new(defective_range, formatted.to_string());

    let applied = apply_edits_exact(source, &[EditSpec::from(&edit)], UTF16)?;
    assert_eq!(applied, "y = 1;\n\n", "defective geometry must reproduce the doubled terminator");
    assert_ne!(applied, formatted, "oracle must expose the mismatch from rendered bytes");

    let true_eof_range = TextRange::new(TextPosition::new(0, 0), TextPosition::new(1, 0));
    let correct = TextEdit::new(true_eof_range, formatted.to_string());
    let repaired = apply_edits_exact(source, &[EditSpec::from(&correct)], UTF16)?;
    assert_eq!(repaired, formatted, "true-EOF geometry renders the authoritative bytes");
    Ok(())
}
