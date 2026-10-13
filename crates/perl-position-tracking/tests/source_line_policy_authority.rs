#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Binds ADR-0048 (the accepted LF-delimited source-line contract, #4973) to the
//! code that implements it.
//!
//! Two things are pinned here:
//!
//! 1. the **accepted contract**, as implemented by [`LineRecordTable`] — the
//!    ruling's decisive rows, plus the policy identity the ADR names;
//! 2. the **legacy divergence map** — the pre-ruling row model still exposed by
//!    `PositionMapper`, the one legacy surface this crate has not yet migrated.
//!
//! `LineStartsCache::new`, `LineStartsCache::new_rope`, and `LineIndex` were
//! migrated to the ruling by the #4973 constructor convergence, and
//! `offset_to_utf16_line_col` already matched it; their agreement is asserted
//! below. `PositionMapper` remains owned by #8687 (reconciliation) and
//! #8716/#8259 (classification and recurrence blocking); this file keeps that
//! divergence executable and visible instead of leaving it to be rediscovered.
//! Migrating it is expected to fail the corresponding test below, which is the
//! intended review checkpoint — update the map in the same change.
//!
//! Context: the committed property `prop_text_and_rope_offsets_agree` in
//! `line_starts_cache_fuzz.rs` asserts `new` and `new_rope` agree. Its
//! generator never emits VT, FF, NEL, LS, or PS, but after the constructor
//! convergence those characters no longer divide the two constructors, so
//! widening the corpus is no longer a red-`main` hazard.

use perl_position_tracking::{
    LineIndex, LineRecord, LineRecordTable, LineStartsCache, PositionMapper, SOURCE_LINE_POLICY_ID,
    SeparatorKind, offset_to_utf16_line_col,
};
use ropey::Rope;
use std::str::FromStr;

/// The five characters Ropey treats as line breaks and ADR-0048 rules to be
/// ordinary source content. Bare CR is tracked separately: it divides the legacy
/// surfaces differently from these five.
const ROPEY_ONLY_SEPARATORS: [(&str, &str); 5] =
    [("VT", "\u{0B}"), ("FF", "\u{0C}"), ("NEL", "\u{85}"), ("LS", "\u{2028}"), ("PS", "\u{2029}")];

/// Indexes `source` under the accepted contract, failing the test on any error.
fn table(source: &str) -> LineRecordTable {
    match LineRecordTable::from_str(source) {
        Ok(table) => table,
        Err(err) => unreachable!("fixture {source:?} must index: {err}"),
    }
}

/// Borrows one row, failing the test when the fixture does not have it.
fn row(table: &LineRecordTable, index: usize) -> LineRecord {
    match table.record(index) {
        Some(record) => *record,
        None => unreachable!("row {index} must exist in a {}-row table", table.line_count()),
    }
}

/// The three exact byte boundaries ADR-0048 requires each row to record.
fn bounds(record: LineRecord) -> (usize, usize, usize) {
    (record.start_byte(), record.content_end_byte(), record.separator_end_byte())
}

/// Row index reported for the final byte of `source` by each legacy surface.
///
/// `PositionMapper` is included because it is the most production-reachable of
/// these: LSP providers map through `byte_to_lsp_pos`, which resolves rows with
/// `Rope::byte_to_line` (`mapper.rs`). Classifying it in the ADR without gating
/// it here would leave a real provider-facing surface unpinned. It is also the
/// only surface still diverging from the ruling after the #4973 constructor
/// convergence.
fn legacy_last_rows(source: &str) -> LegacyRows {
    let rope = Rope::from_str(source);
    let end = source.len();
    LegacyRows {
        str_cache: LineStartsCache::new(source).offset_to_position(source, end).0,
        rope_cache: LineStartsCache::new_rope(&rope).offset_to_position_rope(&rope, end).0,
        line_index: LineIndex::new(source.to_string()).offset_to_position(end).0,
        position_mapper: PositionMapper::new(source).byte_to_lsp_pos(end).line,
        convert: offset_to_utf16_line_col(source, end).0,
    }
}

/// Row index each legacy surface reports for the same byte offset.
#[derive(Debug, PartialEq, Eq)]
struct LegacyRows {
    /// `LineStartsCache::new` — LF-delimited under the ruling.
    str_cache: u32,
    /// `LineStartsCache::new_rope` — LF-delimited under the ruling.
    rope_cache: u32,
    /// `LineIndex::new` — LF-delimited under the ruling.
    line_index: u32,
    /// `PositionMapper::byte_to_lsp_pos` — Ropey line model, provider-facing.
    position_mapper: u32,
    /// `offset_to_utf16_line_col` — LF-only.
    convert: u32,
}

impl LegacyRows {
    /// Every surface reporting the same row.
    const fn all(row: u32) -> Self {
        Self {
            str_cache: row,
            rope_cache: row,
            line_index: row,
            position_mapper: row,
            convert: row,
        }
    }
}

// ---------------------------------------------------------------------------
// ADR-0048 accepted contract
// ---------------------------------------------------------------------------

/// ADR-0048 names this identity. It travels with every table so a stored table
/// cannot be reinterpreted under a different ruling. Changing the ruling changes
/// this constant and forces the ADR to be revisited.
#[test]
fn accepted_policy_identity_matches_the_adr() {
    assert_eq!(SOURCE_LINE_POLICY_ID, "lf-source-lines/v1");
    assert_eq!(table("a\nb").policy_id(), SOURCE_LINE_POLICY_ID);
}

#[test]
fn lf_is_the_only_terminator() {
    let indexed = table("a\nb");
    assert_eq!(indexed.line_count(), 2);
    assert_eq!(row(&indexed, 0).separator_kind(), SeparatorKind::Lf);
    assert_eq!(row(&indexed, 1).separator_kind(), SeparatorKind::None);
}

/// `"abc\r\ndef"` is the ADR's worked example: CRLF is one two-byte separator,
/// excluded from content.
#[test]
fn crlf_is_one_separator_excluded_from_content() {
    let source = "abc\r\ndef";
    let indexed = table(source);
    assert_eq!(indexed.line_count(), 2);

    let first = row(&indexed, 0);
    assert_eq!(bounds(first), (0, 3, 5));
    assert_eq!(first.separator_kind(), SeparatorKind::CrLf);
    assert_eq!(first.content_str(source), Some("abc"));

    let second = row(&indexed, 1);
    assert_eq!(bounds(second), (5, 8, 8));
    assert_eq!(second.content_str(source), Some("def"));
}

/// The ADR's second worked example: one row, CR is addressable content.
#[test]
fn bare_cr_is_content_under_the_accepted_contract() {
    let source = "abc\rdef";
    let indexed = table(source);
    assert_eq!(indexed.line_count(), 1);
    assert_eq!(row(&indexed, 0).content_str(source), Some(source));
}

#[test]
fn ropey_only_separators_are_content_under_the_accepted_contract() {
    for (name, separator) in ROPEY_ONLY_SEPARATORS {
        let source = format!("a{separator}b");
        let indexed = table(&source);
        assert_eq!(indexed.line_count(), 1, "{name} must not terminate a source row");
        assert_eq!(row(&indexed, 0).content_str(&source), Some(source.as_str()));
    }
}

#[test]
fn empty_source_has_one_row_at_byte_zero() {
    let indexed = table("");
    assert_eq!(indexed.line_count(), 1);
    assert_eq!(bounds(row(&indexed, 0)), (0, 0, 0));
}

#[test]
fn final_lf_creates_a_terminal_empty_row() {
    let indexed = table("a\n");
    assert_eq!(indexed.line_count(), 2);
    let terminal = row(&indexed, 1);
    assert_eq!(terminal.start_byte(), terminal.content_end_byte());
    assert_eq!(terminal.separator_kind(), SeparatorKind::None);
}

#[test]
fn mixed_lf_and_crlf_are_supported_without_normalization() {
    let source = "a\nb\r\nc";
    let indexed = table(source);
    assert_eq!(indexed.line_count(), 3);
    assert_eq!(row(&indexed, 0).separator_kind(), SeparatorKind::Lf);
    assert_eq!(row(&indexed, 1).separator_kind(), SeparatorKind::CrLf);
    assert_eq!(row(&indexed, 2).separator_kind(), SeparatorKind::None);
    // Exact bytes are preserved: the rows cover the source with no normalization.
    assert_eq!(indexed.source_byte_length(), source.len());
    assert_eq!(row(&indexed, 2).separator_end_byte(), source.len());
}

/// To the line table a BOM is content, leading or not. Stripping is an ingress
/// decision owned by #8707, never a line-boundary case.
#[test]
fn bom_is_row_content_not_a_boundary() {
    let source = "\u{FEFF}a";
    let indexed = table(source);
    assert_eq!(indexed.line_count(), 1);
    assert_eq!(row(&indexed, 0).content_str(source), Some(source));
}

// ---------------------------------------------------------------------------
// Legacy divergence map — behavior ADR-0048 rules against, pinned for #8687
// ---------------------------------------------------------------------------

/// All five legacy surfaces already agree with the ruling on LF and CRLF. Only
/// bare CR and the Ropey-only set are contested.
#[test]
fn legacy_surfaces_agree_with_the_ruling_on_lf_and_crlf() {
    assert_eq!(legacy_last_rows("a\nb"), LegacyRows::all(1));
    assert_eq!(legacy_last_rows("a\r\nb"), LegacyRows::all(1));
}

/// PINNED DIVERGENCE (#8687): the ruling makes bare CR content. The three
/// constructor surfaces migrated with #4973 and now agree; only
/// `PositionMapper` still breaks on bare CR.
#[test]
fn legacy_bare_cr_divergence_is_pinned() {
    let rows = legacy_last_rows("a\rb");

    assert_eq!(rows.str_cache, 0, "LineStartsCache::new matches the ruling");
    assert_eq!(rows.rope_cache, 0, "LineStartsCache::new_rope matches the ruling");
    assert_eq!(rows.line_index, 0, "LineIndex matches the ruling");
    assert_eq!(rows.position_mapper, 1, "PositionMapper still breaks on bare CR");
    assert_eq!(rows.convert, 0, "offset_to_utf16_line_col already matches the ruling");

    // The accepted contract agrees with the migrated surfaces.
    assert_eq!(table("a\rb").line_count(), 1);
}

/// PINNED DIVERGENCE (#8687): after the #4973 constructor convergence, only
/// `PositionMapper` still inherits Ropey's Unicode line model. This is the seam
/// that can shift a row for a `U+2028` inside an ordinary Perl string, because
/// LSP providers map through it.
#[test]
fn legacy_ropey_only_separator_divergence_is_pinned() {
    for (name, separator) in ROPEY_ONLY_SEPARATORS {
        let source = format!("a{separator}b");
        let rows = legacy_last_rows(&source);

        assert_eq!(rows.rope_cache, 0, "{name}: new_rope matches the ruling");
        assert_eq!(rows.position_mapper, 1, "{name}: PositionMapper still inherits it");
        assert_eq!(rows.str_cache, 0, "{name}: LineStartsCache::new matches the ruling");
        assert_eq!(rows.line_index, 0, "{name}: LineIndex matches the ruling");
        assert_eq!(rows.convert, 0, "{name}: convert.rs already matches the ruling");

        assert_eq!(table(&source).line_count(), 1, "{name}: accepted contract keeps one row");
    }
}

/// The exact input that used to falsify `prop_text_and_rope_offsets_agree` once
/// VT entered its corpus. The #4973 constructor convergence closed that seam:
/// both constructors now treat VT as content, so they agree.
#[test]
fn vt_no_longer_divides_the_two_constructors() {
    let source = "\u{0B}";
    let rope = Rope::from_str(source);

    let from_str = LineStartsCache::new(source).offset_to_position(source, 1);
    let from_rope = LineStartsCache::new_rope(&rope).offset_to_position_rope(&rope, 1);

    assert_eq!(from_str, (0, 1), "VT is content for the string constructor");
    assert_eq!(from_rope, (0, 1), "VT is content for the Rope constructor");
    assert_eq!(from_str, from_rope, "the constructors now agree under the ruling");
}
