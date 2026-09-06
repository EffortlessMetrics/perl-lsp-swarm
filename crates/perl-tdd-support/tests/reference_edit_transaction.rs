//! Proof matrix for the independent old-generation edit transaction model
//! ([#7344]).
//!
//! Every expected final string, row table, and mapping in this file is authored
//! by hand from the predecessor bytes. Nothing here compares the model against
//! the production incremental edit applicator: that comparison is the thing the
//! model exists to make possible, so using it as the oracle would be circular.
//!
//! # Negative controls
//!
//! Each mutation #7344 names is killed by a specific test below. A change that
//! makes the model do the wrong thing must turn one of these red:
//!
//! | Deliberate mutation | Killed by |
//! |---|---|
//! | applies edits against already-mutated coordinates | `disjoint_edits_address_only_predecessor_coordinates` |
//! | accepts an overlap | `overlapping_edits_are_rejected` |
//! | accepts a same-start ambiguity | `duplicate_start_is_rejected`, `duplicate_pure_insertions_are_rejected` |
//! | splits a UTF-8 scalar | `edit_endpoint_inside_a_scalar_is_rejected`, `bom_interior_endpoint_is_rejected` |
//! | silently sorts an invalid transaction | `noncanonical_order_is_rejected_not_sorted` |
//! | computes final line starts from the predecessor | `inserted_newline_moves_row_count`, `deletion_moves_row_geometry` |
//! | partially applies before finding a later invalid edit | `rejection_leaves_predecessor_untouched` |
//! | calls the production applicator as its oracle | `reference_edit_independence.rs` |
//!
//! [#7344]: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7344

use perl_position_tracking::{LineRecordTable, SeparatorKind};
use perl_tdd_support::reference_edit::{
    REFERENCE_EDIT_COORDINATE_MODEL_ID, ReferenceByteMapSegment, ReferenceEdit, ReferenceEditError,
    ReferenceEditResult, ReferenceEditTransaction, ReferenceSourceState,
};
use perl_tdd_support::{must, must_err};

/// One row as `(start, content_end, separator_end, kind)`.
type Row = (usize, usize, usize, SeparatorKind);

fn rows(table: &LineRecordTable) -> Vec<Row> {
    table
        .records()
        .iter()
        .map(|record| {
            (
                record.start_byte(),
                record.content_end_byte(),
                record.separator_end_byte(),
                record.separator_kind(),
            )
        })
        .collect()
}

fn state(source: &str) -> ReferenceSourceState {
    must(ReferenceSourceState::new(source))
}

fn apply(source: &str, edits: Vec<ReferenceEdit>) -> ReferenceEditResult {
    must(state(source).apply(&ReferenceEditTransaction::new(edits)))
}

fn reject(source: &str, edits: Vec<ReferenceEdit>) -> ReferenceEditError {
    must_err(state(source).apply(&ReferenceEditTransaction::new(edits)))
}

/// `(old_start, old_end, new_start, new_end, replaced)` for each segment.
fn mapping(result: &ReferenceEditResult) -> Vec<(usize, usize, usize, usize, bool)> {
    result
        .mapping()
        .iter()
        .map(|segment| {
            let old = segment.old();
            let new = segment.new_span();
            (old.start, old.end, new.start, new.end, segment.is_replaced())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Accepted transactions: exact bytes
// ---------------------------------------------------------------------------

#[test]
fn insertion_produces_exact_bytes_and_mapping() {
    // "my $x = 1;\n" is 11 bytes; byte 9 is the ';'.
    let result = apply("my $x = 1;\n", vec![ReferenceEdit::insert(9, "2")]);

    assert_eq!(result.source(), "my $x = 12;\n");
    assert_eq!(result.generation(), 1);
    assert_eq!(result.predecessor_generation(), 0);
    assert_eq!(
        mapping(&result),
        vec![(0, 9, 0, 9, false), (9, 9, 9, 10, true), (9, 11, 10, 12, false),],
    );
    assert_eq!(result.changed_old().len(), 1);
    assert_eq!(result.changed_old()[0].to_range(), 9..9);
    assert_eq!(result.changed_new()[0].to_range(), 9..10);
}

#[test]
fn deletion_produces_exact_bytes() {
    // "abc\ndef\n" is 8 bytes; [4, 7) is "def".
    let result = apply("abc\ndef\n", vec![ReferenceEdit::delete(4, 7)]);

    assert_eq!(result.source(), "abc\n\n");
    assert_eq!(
        mapping(&result),
        vec![(0, 4, 0, 4, false), (4, 7, 4, 4, true), (7, 8, 4, 5, false)],
    );
}

#[test]
fn equal_width_replacement_preserves_length() {
    let result = apply("abc", vec![ReferenceEdit::replace(1, 2, "B")]);

    assert_eq!(result.source(), "aBc");
    assert_eq!(result.source().len(), 3);
}

#[test]
fn repeated_identical_text_is_selected_by_exact_range() {
    // "x x x" contains three identical "x" runs, so a uniqueness-based
    // substring helper cannot address the middle one at all. Exact ranges can.
    let result = apply("x x x", vec![ReferenceEdit::replace(2, 3, "y")]);

    assert_eq!(result.source(), "x y x");
}

#[test]
fn disjoint_edits_address_only_predecessor_coordinates() {
    // "aaa bbb ccc": [0, 3) is "aaa" and [8, 11) is "ccc". Both ranges are
    // predecessor coordinates. An implementation that applied them in ascending
    // order against the buffer it is mutating would widen the first range by one
    // byte and produce "AAAA bbbC c" instead.
    let result = apply(
        "aaa bbb ccc",
        vec![ReferenceEdit::replace(0, 3, "AAAA"), ReferenceEdit::replace(8, 11, "C")],
    );

    assert_eq!(result.source(), "AAAA bbb C");
    assert_ne!(result.source(), "AAAA bbbC c");
    assert_eq!(
        mapping(&result),
        vec![(0, 3, 0, 4, true), (3, 8, 4, 9, false), (8, 11, 9, 10, true),],
    );
}

#[test]
fn edits_at_file_start_and_eof_are_addressable() {
    assert_eq!(apply("bc", vec![ReferenceEdit::insert(0, "a")]).source(), "abc");
    assert_eq!(apply("ab", vec![ReferenceEdit::insert(2, "c")]).source(), "abc");
}

#[test]
fn empty_source_accepts_an_insertion() {
    let predecessor = state("");
    assert_eq!(rows(predecessor.lines()), vec![(0, 0, 0, SeparatorKind::None)]);

    let result = apply("", vec![ReferenceEdit::insert(0, "a")]);
    assert_eq!(result.source(), "a");
    assert_eq!(rows(result.lines()), vec![(0, 1, 1, SeparatorKind::None)]);
}

// ---------------------------------------------------------------------------
// Line geometry is derived from the successor, under `lf-source-lines/v1`
// ---------------------------------------------------------------------------

#[test]
fn inserted_newline_moves_row_count() {
    let predecessor = state("ab");
    assert_eq!(rows(predecessor.lines()).len(), 1);

    // A terminal LF creates a terminal empty row, so a predecessor-derived
    // table would report one row where the successor has two.
    let result = apply("ab", vec![ReferenceEdit::insert(2, "\n")]);
    assert_eq!(result.source(), "ab\n");
    assert_eq!(
        rows(result.lines()),
        vec![(0, 2, 3, SeparatorKind::Lf), (3, 3, 3, SeparatorKind::None)],
    );
}

#[test]
fn deletion_moves_row_geometry() {
    let predecessor = state("abc\ndef\n");
    assert_eq!(
        rows(predecessor.lines()),
        vec![
            (0, 3, 4, SeparatorKind::Lf),
            (4, 7, 8, SeparatorKind::Lf),
            (8, 8, 8, SeparatorKind::None),
        ],
    );

    let result = apply("abc\ndef\n", vec![ReferenceEdit::delete(4, 7)]);
    assert_eq!(
        rows(result.lines()),
        vec![
            (0, 3, 4, SeparatorKind::Lf),
            (4, 4, 5, SeparatorKind::Lf),
            (5, 5, 5, SeparatorKind::None),
        ],
    );
}

#[test]
fn crlf_is_one_separator_whose_lf_terminates_the_row() {
    // "a\r\nb" is 4 bytes: 'a', CR, LF, 'b'.
    let result = apply("a\r\nb", vec![ReferenceEdit::insert(4, "X")]);

    assert_eq!(result.source(), "a\r\nbX");
    assert_eq!(
        rows(result.lines()),
        vec![(0, 1, 3, SeparatorKind::CrLf), (3, 5, 5, SeparatorKind::None)],
    );
}

#[test]
fn bare_cr_stays_addressable_content() {
    // ADR-0048: a bare CR does not terminate a row.
    let result = apply("a\rb", vec![ReferenceEdit::insert(3, "Z")]);

    assert_eq!(result.source(), "a\rbZ");
    assert_eq!(rows(result.lines()), vec![(0, 4, 4, SeparatorKind::None)]);
}

#[test]
fn non_leading_bom_is_ordinary_content() {
    // "a\u{feff}b": 'a' at 0, the BOM occupies bytes 1..4, 'b' at 4.
    let result = apply("a\u{feff}b", vec![ReferenceEdit::insert(5, "Z")]);

    assert_eq!(result.source(), "a\u{feff}bZ");
    assert_eq!(rows(result.lines()), vec![(0, 6, 6, SeparatorKind::None)]);
}

#[test]
fn mixed_line_endings_survive_without_normalization() {
    let result = apply("a\r\nb\nc", vec![ReferenceEdit::insert(6, "!")]);

    assert_eq!(result.source(), "a\r\nb\nc!");
    assert_eq!(
        rows(result.lines()),
        vec![
            (0, 1, 3, SeparatorKind::CrLf),
            (3, 4, 5, SeparatorKind::Lf),
            (5, 7, 7, SeparatorKind::None),
        ],
    );
}

// ---------------------------------------------------------------------------
// Identity and generations
// ---------------------------------------------------------------------------

#[test]
fn edit_and_undo_are_distinct_generations_with_equal_digests() {
    let original = state("ab");
    let edited =
        must(original.apply(&ReferenceEditTransaction::new(vec![ReferenceEdit::insert(1, "X")])));
    assert_eq!(edited.source(), "aXb");
    assert_eq!(edited.generation(), 1);

    let undone = must(
        edited.state().apply(&ReferenceEditTransaction::new(vec![ReferenceEdit::delete(1, 2)])),
    );

    assert_eq!(undone.source(), "ab");
    assert_eq!(undone.generation(), 2);
    // Same bytes, same digest, different generation: identity does not collapse
    // the two subjects into one.
    assert_eq!(undone.digest(), original.digest());
    assert_ne!(undone.generation(), original.generation());
}

#[test]
fn result_records_the_predecessor_it_was_addressed_against() {
    let predecessor = state("abc");
    let result = must(
        predecessor.apply(&ReferenceEditTransaction::new(vec![ReferenceEdit::insert(3, "d")])),
    );

    assert_eq!(result.predecessor_digest(), predecessor.digest());
    assert_ne!(result.digest(), predecessor.digest());
}

#[test]
fn mapping_tiles_both_subjects_completely() {
    let result = apply(
        "aaa bbb ccc",
        vec![ReferenceEdit::replace(0, 3, "AAAA"), ReferenceEdit::replace(8, 11, "C")],
    );

    let mut old_cursor = 0;
    let mut new_cursor = 0;
    for segment in result.mapping() {
        assert_eq!(segment.old().start, old_cursor);
        assert_eq!(segment.new_span().start, new_cursor);
        old_cursor = segment.old().end;
        new_cursor = segment.new_span().end;
        if let ReferenceByteMapSegment::Unchanged { old, new } = *segment {
            assert_eq!(old.len(), new.len());
        }
    }
    assert_eq!(old_cursor, "aaa bbb ccc".len());
    assert_eq!(new_cursor, result.source().len());
}

#[test]
fn unchanged_offsets_translate_and_replaced_offsets_do_not() {
    let result = apply(
        "aaa bbb ccc",
        vec![ReferenceEdit::replace(0, 3, "AAAA"), ReferenceEdit::replace(8, 11, "C")],
    );

    // " bbb " shifts by one byte.
    assert_eq!(result.map_old_to_new(3), Some(4));
    assert_eq!(result.map_old_to_new(7), Some(8));
    // Offsets inside a replaced range have no offset-preserving image.
    assert_eq!(result.map_old_to_new(1), None);
    assert_eq!(result.map_old_to_new(9), None);
}

// ---------------------------------------------------------------------------
// Multibyte boundaries
// ---------------------------------------------------------------------------

#[test]
fn multibyte_scalars_are_editable_on_their_boundaries() {
    // "héllo": 'h' at 0, 'é' at 1..3, then 'l', 'l', 'o' at 3, 4, 5.
    let result = apply("héllo", vec![ReferenceEdit::replace(3, 4, "L")]);

    assert_eq!(result.source(), "héLlo");
}

#[test]
fn edit_endpoint_inside_a_scalar_is_rejected() {
    let error = reject("héllo", vec![ReferenceEdit::replace(1, 2, "x")]);
    assert_eq!(error.reason(), "split_utf8_scalar");

    let error = reject("héllo", vec![ReferenceEdit::replace(2, 3, "x")]);
    assert_eq!(error.reason(), "split_utf8_scalar");
}

#[test]
fn bom_interior_endpoint_is_rejected() {
    let error = reject("a\u{feff}b", vec![ReferenceEdit::replace(2, 3, "x")]);
    assert_eq!(error.reason(), "split_utf8_scalar");
}

// ---------------------------------------------------------------------------
// Typed rejections
// ---------------------------------------------------------------------------

#[test]
fn out_of_bounds_is_rejected() {
    let error = reject("abc", vec![ReferenceEdit::delete(0, 4)]);
    assert_eq!(error.reason(), "out_of_bounds");
    assert_eq!(
        error,
        ReferenceEditError::OutOfBounds { index: 0, start_byte: 0, old_end_byte: 4, source_len: 3 },
    );
}

#[test]
fn reversed_range_is_rejected_before_bounds() {
    // Reversed *and* out of bounds: the shape violation is reported, so the
    // caller is told what is actually wrong with the range it wrote.
    let error = reject("abc", vec![ReferenceEdit::delete(9, 4)]);
    assert_eq!(error.reason(), "reversed_range");
}

#[test]
fn overlapping_edits_are_rejected() {
    let error = reject("abcdef", vec![ReferenceEdit::delete(0, 4), ReferenceEdit::delete(2, 6)]);
    assert_eq!(error.reason(), "overlap");
}

#[test]
fn duplicate_start_is_rejected() {
    let error = reject("abcdef", vec![ReferenceEdit::delete(2, 4), ReferenceEdit::delete(2, 5)]);
    assert_eq!(error.reason(), "duplicate_start");
}

#[test]
fn duplicate_pure_insertions_are_rejected() {
    // Two empty ranges at the same point do not overlap, so only the same-start
    // rule can catch the ambiguity of which replacement lands first.
    let error =
        reject("abcdef", vec![ReferenceEdit::insert(2, "X"), ReferenceEdit::insert(2, "Y")]);
    assert_eq!(error.reason(), "duplicate_start");
}

#[test]
fn noncanonical_order_is_rejected_not_sorted() {
    let predecessor = state("abcdef");
    let transaction = ReferenceEditTransaction::new(vec![
        ReferenceEdit::replace(4, 5, "E"),
        ReferenceEdit::replace(0, 1, "A"),
    ]);

    let error = must_err(predecessor.apply(&transaction));
    assert_eq!(error.reason(), "noncanonical_order");

    // The same edits in canonical order are accepted, which proves the
    // rejection above is about the supplied order and not about the edits.
    let accepted = must(predecessor.apply(&ReferenceEditTransaction::new(vec![
        ReferenceEdit::replace(0, 1, "A"),
        ReferenceEdit::replace(4, 5, "E"),
    ])));
    assert_eq!(accepted.source(), "AbcdEf");
}

#[test]
fn recorded_new_end_is_validated() {
    // "abc" with [0, 1) -> "XY" ends at successor byte 2, not 1.
    let error = reject("abc", vec![ReferenceEdit::replace(0, 1, "XY").with_expected_new_end(1)]);
    assert_eq!(error.reason(), "new_end_mismatch");
    assert_eq!(
        error,
        ReferenceEditError::NewEndMismatch {
            index: 0,
            expected_new_end_byte: 1,
            actual_new_end_byte: 2,
        },
    );

    let accepted = apply("abc", vec![ReferenceEdit::replace(0, 1, "XY").with_expected_new_end(2)]);
    assert_eq!(accepted.source(), "XYbc");
}

#[test]
fn recorded_new_end_accounts_for_earlier_edits() {
    // The second replacement starts at predecessor byte 8, which the first
    // edit's one-byte growth shifts to successor byte 9.
    let accepted = apply(
        "aaa bbb ccc",
        vec![
            ReferenceEdit::replace(0, 3, "AAAA").with_expected_new_end(4),
            ReferenceEdit::replace(8, 11, "C").with_expected_new_end(10),
        ],
    );
    assert_eq!(accepted.source(), "AAAA bbb C");

    let error = must_err(state("aaa bbb ccc").apply(&ReferenceEditTransaction::new(vec![
        ReferenceEdit::replace(0, 3, "AAAA").with_expected_new_end(4),
        // 9 would be correct only if the earlier edit had not grown the source.
        ReferenceEdit::replace(8, 11, "C").with_expected_new_end(9),
    ])));
    assert_eq!(error.reason(), "new_end_mismatch");
}

#[test]
fn other_coordinate_models_are_rejected() {
    let transaction = ReferenceEditTransaction::new(vec![ReferenceEdit::insert(0, "x")])
        .with_coordinate_model("successor-utf16-units/v1");

    let error = must_err(state("abc").apply(&transaction));
    assert_eq!(error.reason(), "unsupported_coordinate_model");
}

#[test]
fn the_supported_coordinate_model_is_the_default() {
    let transaction = ReferenceEditTransaction::new(Vec::new());
    assert_eq!(transaction.coordinate_model(), REFERENCE_EDIT_COORDINATE_MODEL_ID,);
}

// ---------------------------------------------------------------------------
// Atomicity
// ---------------------------------------------------------------------------

#[test]
fn rejection_leaves_predecessor_untouched() {
    let predecessor = state("abc\ndef\n");
    let before = predecessor.clone();

    // The first edit is valid; the second is out of bounds. Nothing may be
    // applied before the later edit is found invalid.
    let error = must_err(predecessor.apply(&ReferenceEditTransaction::new(vec![
        ReferenceEdit::replace(0, 3, "XYZ"),
        ReferenceEdit::delete(4, 99),
    ])));
    assert_eq!(error.reason(), "out_of_bounds");

    assert_eq!(predecessor.source(), before.source());
    assert_eq!(predecessor.digest(), before.digest());
    assert_eq!(predecessor.generation(), before.generation());
    assert_eq!(rows(predecessor.lines()), rows(before.lines()));
    assert_eq!(predecessor, before);
}

#[test]
fn an_empty_transaction_advances_only_the_generation() {
    let result = apply("abc\n", Vec::new());

    assert_eq!(result.source(), "abc\n");
    assert_eq!(result.digest(), state("abc\n").digest());
    assert_eq!(result.generation(), 1);
    assert!(result.changed_old().is_empty());
    assert!(result.changed_new().is_empty());
    assert_eq!(mapping(&result), vec![(0, 4, 0, 4, false)]);
}

// ---------------------------------------------------------------------------
// The substring-helper gap this model closes
// ---------------------------------------------------------------------------
//
// `crates/perl-parser/tests/incremental_parser_accuracy.rs` derives one edit by
// locating a uniquely occurring `old_text` and diffing a common prefix/suffix.
// That helper refuses any `old_text`/`new_text` containing a newline, requires
// `source.matches(old_text).count() == 1`, and yields exactly one edit. The
// tests below express the same edit families, and the ones the helper cannot
// reach, through exact predecessor ranges instead. This model is additive:
// nothing here changes that file, which #7344 leaves to a later slice.

/// The fixture `incremental_parser_accuracy.rs` shares across its edit families.
const THREE_DECLARATIONS: &str = "my $before = 1;\nmy $value = 20;\nmy $after = 3;\n";

#[test]
fn the_6801_insertion_and_deletion_families_are_expressible() {
    // The literal "20" occupies bytes 28..30 on the second line.
    let inserted = apply(THREE_DECLARATIONS, vec![ReferenceEdit::insert(30, "0")]);
    assert_eq!(inserted.source(), "my $before = 1;\nmy $value = 200;\nmy $after = 3;\n",);

    let deleted = apply(THREE_DECLARATIONS, vec![ReferenceEdit::delete(29, 30)]);
    assert_eq!(deleted.source(), "my $before = 1;\nmy $value = 2;\nmy $after = 3;\n",);
}

#[test]
fn a_slash_reclassification_edit_is_an_ordinary_range_replacement() {
    // #6801's third family inserts `=~` so a division slash reparses as a regex
    // delimiter. To this model that is bytes, not a structural special case.
    let result = apply("my $m = $s / 2;\n", vec![ReferenceEdit::replace(10, 14, " =~ /2/")]);

    assert_eq!(result.source(), "my $m = $s =~ /2/;\n");
}

#[test]
fn edits_the_substring_helper_cannot_express_are_expressible_here() {
    // 1. "my" occurs three times, so no unique-substring search can select the
    //    second declaration. An exact range can: bytes 16..18.
    let ambiguous = apply(THREE_DECLARATIONS, vec![ReferenceEdit::replace(16, 18, "our")]);
    assert_eq!(ambiguous.source(), "my $before = 1;\nour $value = 20;\nmy $after = 3;\n",);

    // 2. The helper rejects any edit touching a newline. Byte 15 is the first LF.
    let joined = apply(THREE_DECLARATIONS, vec![ReferenceEdit::delete(15, 16)]);
    assert_eq!(joined.source(), "my $before = 1;my $value = 20;\nmy $after = 3;\n",);
    assert_eq!(rows(joined.lines()).len(), 3);

    // 3. The helper derives exactly one edit. Two disjoint edits, on the first
    //    and third declarations, are one old-generation transaction here.
    let multi = apply(
        THREE_DECLARATIONS,
        vec![ReferenceEdit::replace(13, 14, "11"), ReferenceEdit::replace(44, 45, "33")],
    );
    assert_eq!(multi.source(), "my $before = 11;\nmy $value = 20;\nmy $after = 33;\n",);
    assert_eq!(multi.changed_old().len(), 2);
}
