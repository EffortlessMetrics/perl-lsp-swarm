//! Semantic consumer proof runs only after fixture integrity validation.

use perl_position_fixtures::{load, raw_bytes};
use perl_position_tracking::LineRecordTable;
use ropey::Rope;
use std::{path::Path, str::FromStr};

#[test]
fn literal_lf_rows_survive_all_single_byte_chunk_cuts() -> Result<(), String> {
    let manifest = load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").as_path())?;
    for case in manifest.cases() {
        if case.decode != "valid_utf8" {
            continue;
        }
        let raw = raw_bytes(case)?;
        let source = std::str::from_utf8(&raw).map_err(|error| error.to_string())?;
        let actual = LineRecordTable::from_str(source).map_err(|error| error.to_string())?;
        compare(case, &actual)?;
        let rope = Rope::from_str(source);
        let rope_backed = LineRecordTable::from_chunks_utf8(rope.chunks().map(str::as_bytes))
            .map_err(|error| format!("{} Rope chunks: {error}", case.id))?;
        compare(case, &rope_backed)?;
        if matches!(case.id.as_str(), "ff" | "ls" | "nel" | "ps" | "vt")
            && rope.len_lines() <= actual.line_count()
        {
            return Err(format!("{} lost the Ropey negative control", case.id));
        }
        for cut in 0..=raw.len() {
            let chunks: [&[u8]; 4] = [&[], &raw[..cut], &raw[cut..], &[]];
            let partitioned = LineRecordTable::from_chunks_utf8(chunks)
                .map_err(|error| format!("{} cut {cut}: {error}", case.id))?;
            compare(case, &partitioned)?;
        }
    }
    Ok(())
}

fn compare(case: &perl_position_fixtures::Case, table: &LineRecordTable) -> Result<(), String> {
    if table.line_count() != case.lines.len() {
        return Err(format!("{} line count", case.id));
    }
    for (row, expected) in case.lines.iter().enumerate() {
        let actual = table.record(row).ok_or_else(|| format!("{} missing row {row}", case.id))?;
        let kind = format!("{:?}", actual.separator_kind()).to_ascii_lowercase();
        if (
            actual.start_byte(),
            actual.content_end_byte(),
            actual.separator_end_byte(),
            kind.as_str(),
        ) != (
            expected.start,
            expected.content_end,
            expected.separator_end,
            expected.separator.as_str(),
        ) {
            return Err(format!("{} row {row}: actual {actual:?}, expected {expected:?}", case.id));
        }
    }
    Ok(())
}
