//! Standalone Kani 0.68.0 harnesses for #19300; no copied production algorithm.
//! This file is not a Cargo integration-test target. See ../../README.md.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("The initial position proof is scoped to 64-bit usize.");

#[path = "../../src/lib.rs"]
mod production;
use production::LineIndex;

mod contract;

fn symbolic_text() -> contract::BoundedText {
    let case: u16 = kani::any();
    kani::assume(case < contract::CASE_COUNT);
    contract::case_text(case)
}

#[kani::proof]
#[kani::unwind(32)]
fn byte_geometry_and_reverse_acceptance() {
    let storage = symbolic_text();
    let decoded = storage.as_str();
    assert!(decoded.is_ok(), "FV_POSITION_GENERATOR_UTF8");
    if let Ok(text) = decoded {
        let index = LineIndex::new(text);
        let offset: usize = kani::any();
        let line: usize = kani::any();
        let column: usize = kani::any();
        contract::check_bytes(text, &index, offset, line, column);
        kani::cover!(text.is_empty(), "FV_POSITION_COVER_EMPTY");
        kani::cover!(text == "😀😀😀", "FV_POSITION_COVER_MAX_BYTES");
        kani::cover!(text == "\n\n\n", "FV_POSITION_COVER_MAX_LINES");
        kani::cover!(text == "\r\n", "FV_POSITION_COVER_CRLF");
        kani::cover!(offset == usize::MAX, "FV_POSITION_COVER_CLAMP_MAX");
        kani::cover!(line == usize::MAX, "FV_POSITION_COVER_INVALID_LINE_MAX");
        kani::cover!(column == usize::MAX, "FV_POSITION_COVER_INVALID_COLUMN_MAX");
    }
}

#[kani::proof]
#[kani::unwind(32)]
fn strict_utf16_soundness() {
    let storage = symbolic_text();
    let decoded = storage.as_str();
    assert!(decoded.is_ok(), "FV_POSITION_GENERATOR_UTF8");
    if let Ok(text) = decoded {
        let index = LineIndex::new(text);
        let line: usize = kani::any();
        let column: usize = kani::any();
        contract::check_utf16_sound(text, &index, line, column);
        let actual = index.position_to_byte_utf16(text, line, column);
        kani::cover!(actual.is_some(), "FV_POSITION_COVER_UTF16_ACCEPTANCE");
        kani::cover!(
            text == "😀" && line == 0 && column == 1 && actual.is_none(),
            "FV_POSITION_COVER_SURROGATE_REJECTION"
        );
        kani::cover!(
            text == "\r\n" && line == 0 && column == 1 && actual.is_none(),
            "FV_POSITION_COVER_CRLF_REJECTION"
        );
        kani::cover!(
            column == usize::MAX && actual.is_none(),
            "FV_POSITION_COVER_UTF16_COLUMN_MAX"
        );
    }
}

#[kani::proof]
#[kani::unwind(32)]
fn strict_utf16_completeness() {
    let storage = symbolic_text();
    let decoded = storage.as_str();
    assert!(decoded.is_ok(), "FV_POSITION_GENERATOR_UTF8");
    if let Ok(text) = decoded {
        let index = LineIndex::new(text);
        let byte: usize = kani::any();
        contract::check_utf16_complete(text, &index, byte);
        kani::cover!(text.is_empty() && byte == 0, "FV_POSITION_COVER_EMPTY_EOF");
        kani::cover!(text == "😀" && byte == 4, "FV_POSITION_COVER_NON_BMP_EOF");
        kani::cover!(text == "a\u{0301}" && byte == 3, "FV_POSITION_COVER_COMBINING_EOF");
        kani::cover!(text == "中" && byte == 3, "FV_POSITION_COVER_BMP_EOF");
        kani::cover!(text == "é" && byte == 2, "FV_POSITION_COVER_TWO_BYTE_EOF");
        kani::cover!(text == "\t" && byte == 1, "FV_POSITION_COVER_TAB_EOF");
        kani::cover!(text == "\r\n" && byte == 2, "FV_POSITION_COVER_TRAILING_LINE");
    }
}
