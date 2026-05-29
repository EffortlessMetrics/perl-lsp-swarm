#![no_main]

use libfuzzer_sys::fuzz_target;
use perl_line_index::LineIndex;

const MAX_INPUT_BYTES: usize = 4096;
const MAX_PROBE_LINES: usize = 128;
const MAX_EXTRA_COLUMNS: usize = 8;

fn bounded_utf8_lossy(data: &[u8]) -> std::borrow::Cow<'_, str> {
    let capped = if data.len() <= MAX_INPUT_BYTES { data } else { &data[..MAX_INPUT_BYTES] };
    String::from_utf8_lossy(capped)
}

fn line_count(text: &str) -> usize {
    text.as_bytes().iter().filter(|byte| **byte == b'\n').count() + 1
}

fn exercise_line_index(text: &str) {
    let index = LineIndex::new(text);

    for byte in 0..=text.len() {
        let (line, column) = index.byte_to_position(byte);
        let roundtrip = index.position_to_byte(line, column);
        assert_eq!(roundtrip, Some(byte), "byte offset failed to round-trip");

        let checked = index.position_to_byte_checked(line, column);
        assert_eq!(checked, Some(byte), "checked byte offset failed to round-trip");

        if byte > 0 {
            let previous = index.byte_to_position(byte - 1);
            assert!(previous <= (line, column), "positions must be monotonic");
        }
    }

    let lines = line_count(text);
    assert_eq!(index.position_to_byte(lines, 0), None, "line count must be one past EOF");
    assert_eq!(
        index.position_to_byte_checked(lines, 0),
        None,
        "checked line count must be one past EOF"
    );

    for line in 0..lines.min(MAX_PROBE_LINES) {
        let start = index.position_to_byte(line, 0);
        assert!(start.is_some(), "valid lines must have a start offset");

        for column in 0..=text.len().saturating_add(MAX_EXTRA_COLUMNS) {
            let unchecked = index.position_to_byte(line, column);
            let checked = index.position_to_byte_checked(line, column);
            assert_eq!(unchecked, checked, "checked and unchecked APIs diverged");

            if let Some(byte) = unchecked {
                assert!(byte <= text.len(), "position_to_byte returned an out-of-range byte");
                assert_eq!(index.byte_to_position(byte), (line, column));
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let input = bounded_utf8_lossy(data);

    exercise_line_index(&input);

    let perl_wrapped = format!("use strict;\nmy $value = q{{{input}}};\n# {input}\n");
    exercise_line_index(&perl_wrapped);

    let crlf_wrapped = input.replace('\n', "\r\n");
    exercise_line_index(&crlf_wrapped);
});
