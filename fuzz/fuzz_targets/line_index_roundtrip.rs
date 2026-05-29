#![no_main]

use libfuzzer_sys::fuzz_target;
use perl_line_index::LineIndex as ByteLineIndex;
use perl_position_tracking::{
    apply_edit_utf8, last_line_column_utf8, newline_count, offset_to_utf16_line_col,
    utf16_line_col_to_offset, LineStartsCache, PositionMapper, WirePosition,
};

const MAX_INPUT_BYTES: usize = 2048;
const MAX_EDIT_CHARS: usize = 128;

fn bounded_utf8_lossy(data: &[u8]) -> std::borrow::Cow<'_, str> {
    if data.is_empty() {
        return std::borrow::Cow::Borrowed("");
    }

    let capped = if data.len() <= MAX_INPUT_BYTES { data } else { &data[..MAX_INPUT_BYTES] };
    String::from_utf8_lossy(capped)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn char_boundaries(text: &str) -> Vec<usize> {
    let mut offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    offsets.push(text.len());
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

fn exercise_byte_line_index(text: &str) {
    let index = ByteLineIndex::new(text);
    for byte in 0..=text.len() {
        let (line, column) = index.byte_to_position(byte);
        if let Some(roundtrip) = index.position_to_byte(line, column) {
            let _ = index.byte_to_position(roundtrip.min(text.len()));
        }
        let _ = index.position_to_byte_checked(line, column);
    }

    for line in 0..=16usize {
        let _ = index.position_to_byte(line, usize::MAX / 4);
        let _ = index.position_to_byte_checked(line, usize::MAX / 4);
    }
}

fn exercise_position_tracking(text: &str) {
    let cache = LineStartsCache::new(text);
    for byte in 0..=text.len() {
        let (line, column) = cache.offset_to_position(text, byte);
        let _ = cache.position_to_offset(text, line, column);

        let (utf16_line, utf16_col) = offset_to_utf16_line_col(text, byte);
        let utf16_roundtrip = utf16_line_col_to_offset(text, utf16_line, utf16_col);
        let _ = offset_to_utf16_line_col(text, utf16_roundtrip.min(text.len()));
    }

    let mapper = PositionMapper::new(text);
    for byte in char_boundaries(text) {
        let pos = mapper.byte_to_lsp_pos(byte);
        let _ = mapper.lsp_pos_to_byte(pos);
        let _ = mapper.lsp_pos_to_byte(WirePosition {
            line: pos.line,
            character: pos.character.saturating_add(1),
        });
    }

    let _ = newline_count(text);
    let _ = last_line_column_utf8(text);
}

fn exercise_edits(text: &str, replacement: &str) {
    let boundaries = char_boundaries(text);
    if boundaries.is_empty() {
        return;
    }

    let start = boundaries[boundaries.len() / 3];
    let end = boundaries[(boundaries.len() * 2) / 3].max(start);

    let mut mapper = PositionMapper::new(text);
    mapper.apply_edit(start, end, replacement);
    let edited = mapper.text();
    exercise_position_tracking(&edited);

    let mut edited_utf8 = text.to_string();
    apply_edit_utf8(&mut edited_utf8, start, end, replacement);
    exercise_position_tracking(&edited_utf8);
}

fuzz_target!(|data: &[u8]| {
    let input = bounded_utf8_lossy(data);
    let replacement = truncate_chars(&input, MAX_EDIT_CHARS);
    let variants = [
        input.to_string(),
        format!("{input}\n{input}"),
        format!("{input}\r\n{input}"),
        format!("αβ😀\n{input}\n末尾"),
        input.chars().rev().collect::<String>(),
    ];

    for variant in &variants {
        exercise_byte_line_index(variant);
        exercise_position_tracking(variant);
        exercise_edits(variant, &replacement);
    }
});
