#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Contract, chunk-partition stability, and negative-control proof for the
//! LF source-line scanner and immutable line-record table (#10574).
//!
//! Expected tables are stated as literal records assembled through
//! [`LineRecordTable::try_from_records`] — the production scanner is never the
//! source of its own expectations. Every fixture is scanned contiguously and
//! under every chunk partition the fixture size admits (exhaustive for small
//! fixtures, every-single-boundary plus combinatorial samples for larger ones),
//! so a CRLF split across chunks, a split inside a multi-byte scalar, or a
//! dropped terminal row cannot survive.

use perl_position_tracking::{
    LineRecord, LineRecordTable, SOURCE_LINE_POLICY_ID, SeparatorKind, SourceLineError,
};
use proptest::prelude::*;
use std::str::FromStr;

/// Builds a fixture through the scanner, failing the test on any error.
fn must_build(source: &str) -> LineRecordTable {
    match LineRecordTable::from_str(source) {
        Ok(table) => table,
        Err(err) => unreachable!("fixture {source:?} must build: {err}"),
    }
}

/// States the independent expectation as literal records and enforces the
/// coverage laws around them. This is the path #8172's fixture pack will take;
/// until that pack lands, the literals here are the independent facts.
fn expected_table(
    source_byte_length: usize,
    records: &[(usize, usize, usize, SeparatorKind)],
) -> LineRecordTable {
    let records = records
        .iter()
        .map(|&(start, content_end, separator_end, kind)| {
            LineRecord::new(start, content_end, separator_end, kind)
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|_| unreachable!("expected records satisfy the ordering laws"));
    match LineRecordTable::try_from_records(source_byte_length, records) {
        Ok(table) => table,
        Err(err) => unreachable!("expected records cover the source: {err}"),
    }
}

/// Enumerates every partition of `bytes` into contiguous borrowed chunks.
///
/// Bit `i` of the mask means "split between byte `i` and byte `i+1`".
fn all_partitions(bytes: &[u8]) -> Vec<Vec<&[u8]>> {
    let len = bytes.len();
    if len == 0 {
        return vec![Vec::new()];
    }
    let mut partitions = Vec::new();
    for mask in 0..(1u32 << (len - 1)) {
        let mut chunks = Vec::new();
        let mut start = 0usize;
        for split in 0..len.saturating_sub(1) {
            if mask & (1 << split) != 0 {
                chunks.push(&bytes[start..=split]);
                start = split + 1;
            }
        }
        chunks.push(&bytes[start..]);
        partitions.push(chunks);
    }
    partitions
}

/// Deterministic wide-coverage partitions for sources too long to enumerate.
///
/// Includes every single-boundary split (so the CR|LF boundary of any
/// separator is covered), every adjacent pair, and evenly spread multi-splits.
fn sampled_partitions(bytes: &[u8]) -> Vec<Vec<&[u8]>> {
    let len = bytes.len();
    let mut split_sets: Vec<Vec<usize>> = Vec::new();
    for split in 1..len {
        split_sets.push(vec![split]);
    }
    for split in 1..len.saturating_sub(1) {
        split_sets.push(vec![split, split + 1]);
    }
    let quarter = len / 4;
    if quarter > 0 && quarter != len / 2 {
        split_sets.push(vec![quarter, len / 2, quarter * 3]);
    }
    if quarter >= 2 {
        split_sets.push(vec![quarter - 1, quarter + 1]);
    }
    split_sets.push((1..len).collect());
    split_sets
        .into_iter()
        .map(|splits| {
            let mut chunks = Vec::new();
            let mut start = 0usize;
            for split in splits {
                chunks.push(&bytes[start..split]);
                start = split;
            }
            chunks.push(&bytes[start..]);
            chunks
        })
        .collect()
}

/// Asserts one fixture produces `expected` contiguously and under every
/// admitted partition, with empty chunks interleaved for determinism.
fn assert_partition_stable(source: &str, expected: &LineRecordTable) {
    let bytes = source.as_bytes();
    let contiguous = must_build(source);
    assert_eq!(contiguous, *expected, "contiguous scan of {source:?}");

    let partitions =
        if bytes.len() <= 11 { all_partitions(bytes) } else { sampled_partitions(bytes) };
    assert!(!partitions.is_empty(), "fixture {source:?} admits at least one partition");
    for partition in &partitions {
        let mut with_empties = Vec::with_capacity(partition.len() + 2);
        with_empties.push(&[][..]);
        with_empties.extend_from_slice(partition);
        with_empties.push(&[][..]);

        let scanned = LineRecordTable::from_chunks_utf8(with_empties.iter().copied());
        match scanned {
            Ok(table) => assert_eq!(table, *expected, "partition {partition:?} of {source:?}"),
            Err(err) => unreachable!("valid source {source:?} failed as {partition:?}: {err}"),
        }
    }
}

#[test]
fn empty_source_has_one_row_starting_at_zero() {
    assert_partition_stable("", &expected_table(0, &[(0, 0, 0, SeparatorKind::None)]));
}

#[test]
fn lone_lf_yields_empty_row_and_terminal_empty_row() {
    assert_partition_stable(
        "\n",
        &expected_table(1, &[(0, 0, 1, SeparatorKind::Lf), (1, 1, 1, SeparatorKind::None)]),
    );
}

#[test]
fn crlf_separator_excludes_both_bytes_from_content() {
    // Negative control 4: content_end 3 excludes the CR at byte 3.
    assert_partition_stable(
        "abc\r\ndef",
        &expected_table(8, &[(0, 3, 5, SeparatorKind::CrLf), (5, 8, 8, SeparatorKind::None)]),
    );
}

#[test]
fn bare_cr_is_content_not_a_boundary() {
    // Negative control 2: exactly one row; the CR stays addressable content.
    let table = assert_single_fixture("abc\rdef", &[(0, 7, 7, SeparatorKind::None)]);
    assert_eq!(table.line_count(), 1);
    let content = table.record(0).map(|r| r.content_str("abc\rdef"));
    assert_eq!(content, Some(Some("abc\rdef")));
}

#[test]
fn cr_at_eof_is_terminal_content() {
    assert_partition_stable("abc\r", &expected_table(4, &[(0, 4, 4, SeparatorKind::None)]));
}

#[test]
fn mixed_lf_and_crlf_rows_are_exact() {
    assert_partition_stable(
        "a\nb\r\nc",
        &expected_table(
            6,
            &[
                (0, 1, 2, SeparatorKind::Lf),
                (2, 3, 5, SeparatorKind::CrLf),
                (5, 6, 6, SeparatorKind::None),
            ],
        ),
    );
}

#[test]
fn consecutive_lf_rows_are_exact() {
    assert_partition_stable(
        "a\n\nb",
        &expected_table(
            4,
            &[
                (0, 1, 2, SeparatorKind::Lf),
                (2, 2, 3, SeparatorKind::Lf),
                (3, 4, 4, SeparatorKind::None),
            ],
        ),
    );
}

#[test]
fn lf_row_after_crlf_row_pins_exact_seam_geometry() {
    // Call-observation for the scanner's LF separator construction: the Lf
    // record is built from record_start 4, content_end 8, and separator_end
    // 9 — three distinct values reached only after a prior CRLF row and
    // multibyte content. Any drift in the LF branch's byte positions (swapped
    // or re-offset arguments) fails this literal expectation or one of the
    // record ordering laws exercised by the partition sweep.
    let source = "α\r\nββ\nγ";
    let table = assert_single_fixture(
        source,
        &[
            (0, 2, 4, SeparatorKind::CrLf),
            (4, 8, 9, SeparatorKind::Lf),
            (9, 11, 11, SeparatorKind::None),
        ],
    );
    // The LF row's content is exactly the two β scalars.
    assert_eq!(table.record(1).map(|r| r.content_str(source)), Some(Some("ββ")));
    // The byte after the LF separator already belongs to the terminal row.
    assert_eq!(table.line_index_at_byte(9), Some(2));
}

#[test]
fn final_lf_creates_terminal_empty_row() {
    // Negative control 5: the third row exists and is empty.
    let table = assert_single_fixture(
        "ab\n",
        &[(0, 2, 3, SeparatorKind::Lf), (3, 3, 3, SeparatorKind::None)],
    );
    assert_eq!(table.line_count(), 2);
    let last = table.record(1);
    assert_eq!(last.map(LineRecord::content_end_byte), Some(3));
    assert_eq!(last.map(|r| r.content_str("ab\n")), Some(Some("")));
}

#[test]
fn cr_before_crlf_makes_first_cr_content_and_second_the_separator() {
    // "a\r\r\nb": row 0 content is "a\r" (CR at byte 1 is content because it is
    // followed by another CR), the CRLF at bytes 2..4 is one separator.
    assert_partition_stable(
        "a\r\r\nb",
        &expected_table(5, &[(0, 2, 4, SeparatorKind::CrLf), (4, 5, 5, SeparatorKind::None)]),
    );
}

#[test]
fn lf_then_bare_cr_keeps_cr_in_terminal_row() {
    assert_partition_stable(
        "\n\r",
        &expected_table(2, &[(0, 0, 1, SeparatorKind::Lf), (1, 2, 2, SeparatorKind::None)]),
    );
}

#[test]
fn ropey_only_separators_are_ordinary_content() {
    // Negative controls 1 and 2: Ropey would break VT/FF/NEL/LS/PS into six
    // rows; the accepted contract keeps one row over all 16 bytes.
    let source = "a\u{0B}b\u{0C}c\u{85}d\u{2028}e\u{2029}f";
    let table = assert_single_fixture(source, &[(0, 16, 16, SeparatorKind::None)]);
    assert_eq!(table.line_count(), 1);
    assert_eq!(table.source_byte_length(), 16);
}

#[test]
fn leading_bom_is_row_content() {
    assert_partition_stable(
        "\u{FEFF}a\nb",
        &expected_table(6, &[(0, 4, 5, SeparatorKind::Lf), (5, 6, 6, SeparatorKind::None)]),
    );
}

#[test]
fn bom_only_source_is_one_content_row() {
    assert_partition_stable("\u{FEFF}", &expected_table(3, &[(0, 3, 3, SeparatorKind::None)]));
}

#[test]
fn non_leading_bom_is_row_content() {
    assert_partition_stable(
        "a\u{FEFF}\nb",
        &expected_table(6, &[(0, 4, 5, SeparatorKind::Lf), (5, 6, 6, SeparatorKind::None)]),
    );
}

#[test]
fn ascii_rows_are_exact() {
    assert_partition_stable(
        "hello\nworld",
        &expected_table(11, &[(0, 5, 6, SeparatorKind::Lf), (6, 11, 11, SeparatorKind::None)]),
    );
}

#[test]
fn bmp_multibyte_rows_split_on_lf_only() {
    // αβγ\nδ: 6-byte content, LF at 6, 2-byte content after.
    assert_partition_stable(
        "\u{3B1}\u{3B2}\u{3B3}\n\u{3B4}",
        &expected_table(9, &[(0, 6, 7, SeparatorKind::Lf), (7, 9, 9, SeparatorKind::None)]),
    );
}

#[test]
fn astral_scalar_rows_survive_mid_scalar_splits() {
    // 😀perl\n🦀: 13 bytes; partitions include splits inside both 4-byte scalars.
    assert_partition_stable(
        "\u{1F600}perl\n\u{1F980}",
        &expected_table(13, &[(0, 8, 9, SeparatorKind::Lf), (9, 13, 13, SeparatorKind::None)]),
    );
}

#[test]
fn combining_marks_stay_inside_their_row() {
    // e + U+0301 f \n c l
    assert_partition_stable(
        "e\u{301}f\ncl",
        &expected_table(7, &[(0, 4, 5, SeparatorKind::Lf), (5, 7, 7, SeparatorKind::None)]),
    );
}

#[test]
fn crlf_split_exactly_between_cr_and_lf_is_recognized() {
    // Negative control 3, stated explicitly even though the exhaustive
    // partition sweep above already contains this split.
    let parts: &[&[u8]] = &[b"abc\r", b"\ndef"];
    let scanned = LineRecordTable::from_chunks_utf8(parts.iter().copied());
    match scanned {
        Ok(table) => {
            assert_eq!(table.line_count(), 2);
            assert_eq!(table.record(0), Some(&record_of((0, 3, 5, SeparatorKind::CrLf))));
            assert_eq!(table.record(1), Some(&record_of((5, 8, 8, SeparatorKind::None))));
        }
        Err(err) => unreachable!("CR|LF chunk split must scan: {err}"),
    }
}

/// Negative control 8: assembly rejects non-covering or malformed records.
#[test]
fn record_assembly_rejects_non_covering_tables() {
    let ok = LineRecord::new(0, 3, 5, SeparatorKind::CrLf);
    assert!(ok.is_ok());

    // Ordering violation: content_end before start.
    assert!(matches!(
        LineRecord::new(3, 2, 5, SeparatorKind::Lf),
        Err(SourceLineError::InvalidRecord { .. })
    ));
    // Kind mismatch: declared Lf but a two-byte gap.
    assert!(matches!(
        LineRecord::new(0, 3, 5, SeparatorKind::Lf),
        Err(SourceLineError::SeparatorKindMismatch { .. })
    ));
    // Gap: second record does not start at the previous separator end.
    let records =
        vec![record_of((0, 1, 2, SeparatorKind::Lf)), record_of((4, 5, 5, SeparatorKind::None))];
    assert!(matches!(
        LineRecordTable::try_from_records(5, records),
        Err(SourceLineError::NonCoveringRecords { index: 1, expected_start: 2, found_start: 4 })
    ));
    // Short cover: records end before the declared source length.
    let records = vec![record_of((0, 1, 2, SeparatorKind::Lf))];
    assert!(matches!(
        LineRecordTable::try_from_records(9, records),
        Err(SourceLineError::NonCoveringRecords { index: 1, expected_start: 2, found_start: 9 })
    ));
    // Empty records cannot claim a nonzero source.
    assert!(matches!(
        LineRecordTable::try_from_records(1, Vec::new()),
        Err(SourceLineError::NonCoveringRecords { .. })
    ));
    // The empty source carries exactly one (0, 0, 0, None) terminal record.
    assert!(
        LineRecordTable::try_from_records(0, vec![record_of((0, 0, 0, SeparatorKind::None))])
            .is_ok()
    );
    // No records at all means no terminal row, even for an empty source.
    assert!(matches!(
        LineRecordTable::try_from_records(0, Vec::new()),
        Err(SourceLineError::MissingTerminalRow)
    ));
}

/// Negative control for the terminal-row law: assembly rejects tables whose
/// final record still carries a separator, and mid-table separator-free rows.
#[test]
fn record_assembly_rejects_missing_or_misplaced_terminal_rows() {
    // A table ending on the Lf itself omits the required terminal empty row.
    let records = vec![record_of((0, 2, 3, SeparatorKind::Lf))];
    assert!(matches!(
        LineRecordTable::try_from_records(3, records),
        Err(SourceLineError::MissingTerminalRow)
    ));
    // Same shape with a CRLF terminator.
    let records = vec![record_of((0, 1, 3, SeparatorKind::CrLf))];
    assert!(matches!(
        LineRecordTable::try_from_records(3, records),
        Err(SourceLineError::MissingTerminalRow)
    ));
    // Only the last row may lack a separator.
    let records = vec![
        record_of((0, 0, 0, SeparatorKind::None)),
        record_of((0, 1, 2, SeparatorKind::Lf)),
        record_of((2, 3, 3, SeparatorKind::None)),
    ];
    assert!(matches!(
        LineRecordTable::try_from_records(3, records),
        Err(SourceLineError::NonTerminalRowWithNoSeparator { index: 0 })
    ));
}

/// Negative control 8, lookup half: out-of-range requests fail honestly.
#[test]
fn lookups_fail_out_of_range_instead_of_clamping() {
    let table = must_build("ab\ncd");
    assert_eq!(table.line_count(), 2);
    assert!(table.record(2).is_none());
    assert!(table.record(usize::MAX).is_none());
    assert!(table.line_index_at_byte(6).is_none());
    assert!(table.line_index_at_byte(usize::MAX).is_none());
    assert!(table.line_record_at_byte(6).is_none());
    assert!(table.content_span(2).is_none());

    // In-range boundaries stay exact: EOF addresses the final row.
    assert_eq!(table.line_index_at_byte(5), Some(1));
    assert_eq!(table.line_index_at_byte(0), Some(0));
    assert_eq!(table.content_span(0), Some(perl_position_tracking::ByteSpan::new(0, 2)));
}

#[test]
fn invalid_utf8_chunks_fail_typed_without_scanning_text() {
    fn err_of(parts: &[&[u8]]) -> Result<LineRecordTable, SourceLineError> {
        LineRecordTable::from_chunks_utf8(parts.iter().copied())
    }
    // Stray continuation byte.
    assert!(matches!(err_of(&[b"ab", &[0xFF]]), Err(SourceLineError::InvalidUtf8)));
    // Overlong encoding of U+0020.
    assert!(matches!(err_of(&[b"a", &[0xC0, 0xA0]]), Err(SourceLineError::InvalidUtf8)));
    // UTF-16 surrogate half.
    assert!(matches!(err_of(&[b"a\n", &[0xED, 0xA0, 0x80]]), Err(SourceLineError::InvalidUtf8)));
    // Above U+10FFFF.
    assert!(matches!(err_of(&[&[0xF5, 0x80, 0x80, 0x80]]), Err(SourceLineError::InvalidUtf8)));
    // Truncated lead at end of stream.
    assert!(matches!(err_of(&[b"x", &[0xE2, 0x82]]), Err(SourceLineError::InvalidUtf8)));
    // Lone LF is fine; validity failures carry no partial table.
    assert!(err_of(&[b"\n"]).is_ok());
}

#[test]
fn scalars_split_across_many_chunks_still_validate() {
    // U+20AC across three chunks, then a four-byte scalar split in half.
    let scanned = LineRecordTable::from_chunks_utf8([
        &[0xE2][..],
        &[0x82][..],
        &[0xAC][..],
        b"\n",
        &[0xF0, 0x9F][..],
        &[0xA6, 0x80][..],
    ]);
    match scanned {
        Ok(table) => {
            assert_eq!(table.line_count(), 2);
            assert_eq!(table.source_byte_length(), 3 + 1 + 4);
            let first = table.record(0).map(|r| r.content_str("\u{20AC}\n\u{1F986}"));
            let second = table.record(1).map(|r| r.content_str("\u{20AC}\n\u{1F986}"));
            assert_eq!(first, Some(Some("\u{20AC}")));
            assert_eq!(second, Some(Some("\u{1F986}")));
        }
        Err(err) => unreachable!("mid-scalar chunk splits are valid UTF-8: {err}"),
    }
}

#[test]
fn policy_identity_pins_the_accepted_contract() {
    let table = must_build("x\n");
    assert_eq!(table.policy_id(), SOURCE_LINE_POLICY_ID);
    assert_eq!(SOURCE_LINE_POLICY_ID, "lf-source-lines/v1");
}

#[test]
fn separator_kind_widths_match_their_separators() {
    assert_eq!(SeparatorKind::None.byte_len(), 0);
    assert_eq!(SeparatorKind::Lf.byte_len(), 1);
    assert_eq!(SeparatorKind::CrLf.byte_len(), 2);
}

#[test]
fn record_accessors_expose_geometry_only() {
    let record = record_of((0, 3, 5, SeparatorKind::CrLf));
    assert_eq!(record.start_byte(), 0);
    assert_eq!(record.content_end_byte(), 3);
    assert_eq!(record.separator_end_byte(), 5);
    assert_eq!(record.separator_kind(), SeparatorKind::CrLf);

    let source = "abc\r\nabcdef\nrest";
    let record_ref = &record;
    assert_eq!(record.content_str(source), Some("abc"));
    assert_eq!(record.content(source.as_bytes()), Some(b"abc".as_slice()));
    // A range outside the given bytes fails instead of slicing unrelated text.
    assert_eq!(record_ref.content_str(""), None);
    assert_eq!(record_ref.content(b"ab"), None);
}

/// Scans one fixture contiguously, asserts it equals the literal expectation,
/// runs the partition sweep, and returns the contiguous table.
fn assert_single_fixture(
    source: &str,
    expected: &[(usize, usize, usize, SeparatorKind)],
) -> LineRecordTable {
    let expected = expected_table(source.len(), expected);
    assert_partition_stable(source, &expected);
    must_build(source)
}

fn record_of(spec: (usize, usize, usize, SeparatorKind)) -> LineRecord {
    match LineRecord::new(spec.0, spec.1, spec.2, spec.3) {
        Ok(record) => record,
        Err(err) => unreachable!("test record {spec:?} must be lawful: {err}"),
    }
}

proptest! {
    /// Coverage laws and partition invariance hold for arbitrary sources,
    /// newline-heavy and Unicode-rich alike.
    #[test]
    fn laws_and_chunk_invariance_hold_for_arbitrary_sources(
        source in arbitrary_source_strategy(),
    ) {
        let built = LineRecordTable::from_str(&source);
        prop_assert!(
            built.is_ok(),
            "source {source:?} failed to scan: {:?}",
            built.as_ref().err()
        );
        let table = match built {
            Ok(table) => table,
            Err(_) => unreachable!("scan success asserted above"),
        };

        let records = table.records();
        prop_assert_eq!(table.source_byte_length(), source.len());
        prop_assert_eq!(records.len(), table.line_count());

        let mut expected_start = 0usize;
        for (index, record) in records.iter().enumerate() {
            prop_assert_eq!(record.start_byte(), expected_start);
            prop_assert!(record.content_end_byte() >= record.start_byte());
            prop_assert!(record.separator_end_byte() >= record.content_end_byte());
            let is_last = index + 1 == records.len();
            if is_last {
                prop_assert_eq!(record.separator_kind(), SeparatorKind::None);
                prop_assert_eq!(record.separator_end_byte(), source.len());
            } else {
                // Only an LF can end a nonterminal record, and the separator
                // bytes really sit where the record claims they do.
                let bytes = source.as_bytes();
                let sep_end = record.separator_end_byte();
                match record.separator_kind() {
                    SeparatorKind::Lf => {
                        prop_assert_eq!(bytes[sep_end - 1], b'\n');
                    }
                    SeparatorKind::CrLf => {
                        prop_assert_eq!(bytes[sep_end - 2], b'\r');
                        prop_assert_eq!(bytes[sep_end - 1], b'\n');
                    }
                    SeparatorKind::None => {
                        prop_assert!(false, "only the terminal row may lack a separator");
                    }
                }
            }
            expected_start = record.separator_end_byte();
        }

        // Partition invariance at every single boundary, including CR|LF and
        // mid-scalar positions.
        let bytes = source.as_bytes();
        for split in 0..=bytes.len() {
            let chunks: Vec<&[u8]> = if split == bytes.len() {
                vec![bytes]
            } else {
                vec![&bytes[..split], &bytes[split..]]
            };
            let rebuilt = LineRecordTable::from_chunks_utf8(chunks);
            prop_assert_eq!(Ok(&table), rebuilt.as_ref(), "single split at {}", split);
        }

        // Binary-search lookups agree with a linear walk.
        for probe in 0..=bytes.len() {
            let linear = records
                .iter()
                .rposition(|r| r.start_byte() <= probe)
                .unwrap_or(0);
            prop_assert_eq!(Some(linear), table.line_index_at_byte(probe));
        }
    }
}

/// Newline-heavy mixed-separator sources with Unicode content mixed in.
fn arbitrary_source_strategy() -> impl Strategy<Value = String> {
    let newline = prop_oneof![
        4 => Just('\n'),
        2 => Just('\r'),
        1 => Just('\u{0B}'),
        1 => Just('\u{85}'),
        1 => Just('\u{2028}'),
    ];
    let filler = prop_oneof![
        6 => prop::char::range('a', 'z'),
        1 => Just('\u{1F600}'),
        1 => Just('\u{3B1}'),
        1 => Just('\u{301}'),
        1 => Just('\u{FEFF}'),
    ];
    prop::collection::vec(prop_oneof![3 => newline, 5 => filler], 0..96)
        .prop_map(|chars| chars.into_iter().collect())
}
