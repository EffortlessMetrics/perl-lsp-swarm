//! Independent reference relations shared by ordinary tests and Kani (#19300).
//!
//! The reference does not use cached line starts, binary search, or production
//! conversion methods. Byte positions enumerate raw bytes; UTF-16 positions use
//! independently enumerated scalar boundaries and `encode_utf16`.

use super::LineIndex;

pub const CASE_COUNT: u16 = 585;
pub const MAX_SCALARS: usize = 3;
pub const MAX_BYTES: usize = 12;
pub const ALPHABET: [&str; 8] = ["a", "\t", "\r", "\n", "é", "中", "😀", "\u{0301}"];

pub struct BoundedText {
    bytes: [u8; MAX_BYTES],
    len: usize,
}

impl BoundedText {
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.bytes[..self.len])
    }
}

/// A bijection from 0..585 to the empty string and all one-to-three-symbol texts.
/// Fixed storage avoids adding a second symbolic heap to the production index.
pub fn case_text(case: u16) -> BoundedText {
    assert!(case < CASE_COUNT, "FV_POSITION_DOMAIN_CASE");
    let (length, mut digits) = match case {
        0 => (0, 0),
        1..=8 => (1, case - 1),
        9..=72 => (2, case - 9),
        _ => (MAX_SCALARS, case - 73),
    };
    let mut text = BoundedText { bytes: [0; MAX_BYTES], len: 0 };
    for _ in 0..length {
        let symbol = ALPHABET[usize::from(digits % 8)];
        digits /= 8;
        for byte in symbol.bytes() {
            text.bytes[text.len] = byte;
            text.len += 1;
        }
    }
    text
}

/// Count LF bytes strictly before the clamped offset, not Unicode scalars.
pub fn byte_position(text: &str, offset: usize) -> (usize, usize) {
    let mut line = 0;
    let mut column = 0;
    for byte in text.bytes().take(offset) {
        if byte == b'\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Enumerate all addressable bytes, including EOF and UTF-8 continuation bytes.
pub fn byte_at(text: &str, line: usize, column: usize) -> Option<usize> {
    (0..=text.len()).find(|&byte| byte_position(text, byte) == (line, column))
}

/// The independent strict UTF-16 relation. The LF *inside* CRLF is excluded;
/// the CR starting that terminator and the next line's start remain addressable.
pub fn utf16_position(text: &str, byte: usize) -> Option<(usize, usize)> {
    if byte > text.len() || !text.is_char_boundary(byte) {
        return None;
    }
    if byte > 0
        && text.as_bytes().get(byte) == Some(&b'\n')
        && text.as_bytes().get(byte - 1) == Some(&b'\r')
    {
        return None;
    }
    let (line, byte_column) = byte_position(text, byte);
    let prefix = text.get(byte - byte_column..byte)?;
    Some((line, prefix.encode_utf16().count()))
}

pub fn utf16_at(text: &str, line: usize, column: usize) -> Option<usize> {
    (0..=text.len()).find(|&byte| utf16_position(text, byte) == Some((line, column)))
}

/// Check forward geometry, both round trips, and independent reverse rejection.
pub fn check_bytes(text: &str, index: &LineIndex, offset: usize, line: usize, column: usize) {
    let actual = index.byte_to_position(offset);
    assert!(actual == byte_position(text, offset), "FV_POSITION_BYTE_GEOMETRY");
    assert!(
        index.position_to_byte(actual.0, actual.1) == Some(offset.min(text.len())),
        "FV_POSITION_BYTE_ROUNDTRIP"
    );
    assert!(
        index.position_to_byte_checked(actual.0, actual.1) == Some(offset.min(text.len())),
        "FV_POSITION_BYTE_CHECKED_ROUNDTRIP"
    );
    let expected = byte_at(text, line, column);
    assert!(index.position_to_byte(line, column) == expected, "FV_POSITION_BYTE_ACCEPTANCE");
    assert!(
        index.position_to_byte_checked(line, column) == expected,
        "FV_POSITION_BYTE_CHECKED_ACCEPTANCE"
    );
}

/// Soundness alone permits always-None; `check_utf16_complete` closes that hole.
pub fn check_utf16_sound(text: &str, index: &LineIndex, line: usize, column: usize) {
    if let Some(byte) = index.position_to_byte_utf16(text, line, column) {
        assert!(byte <= text.len(), "FV_POSITION_UTF16_BOUNDS");
        assert!(text.is_char_boundary(byte), "FV_POSITION_UTF16_SCALAR_BOUNDARY");
        assert!(
            utf16_position(text, byte) == Some((line, column)),
            "FV_POSITION_UTF16_SOUNDNESS"
        );
    }
}

/// Require every independently admitted scalar boundary, including empty EOF.
pub fn check_utf16_complete(text: &str, index: &LineIndex, byte: usize) {
    if let Some((line, column)) = utf16_position(text, byte) {
        assert!(
            index.position_to_byte_utf16(text, line, column) == Some(byte),
            "FV_POSITION_UTF16_COMPLETENESS"
        );
    }
}
