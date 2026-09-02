//! Text processing utilities for the Perl LSP runtime.
//!
//! Most established helpers remain in the legacy module file while source-byte
//! decoding is routed through one typed authority. The public compatibility
//! decoder keeps its historical `String` result until the source-ingress
//! consumers migrate, but it no longer owns an independent decoding algorithm.

#[path = "util/mod.rs"]
mod legacy;

pub use legacy::{
    anchor_arg_start, arg_starts_in_call_body, arg_starts_top_level, byte_offset_utf16,
    byte_to_line_col, byte_to_utf16_col, code_slice, command_timeout, escape_markdown_text,
    extract_module_reference, extract_module_reference_extended, find_data_marker_byte_lexed,
    find_matching_paren, first_char, first_char_is, first_char_string, get_text_around_offset,
    get_text_window_around_offset, is_modchar, is_word_boundary, line_window_around_offset,
    nth_char, nth_char_is, offset_to_position, pos_to_offset_bytes, position_to_offset,
    run_command_with_timeout, slice_in_range, slice_until_stmt_end, smart_arg_anchor,
    token_under_cursor, uri,
};

#[path = "util/source_decoding.rs"]
mod source_decoding;

pub(crate) use source_decoding::{
    DecodedText, decode_source_bytes, read_source_file_with_encoding,
};

use std::io;
use std::path::Path;

/// Decode source text bytes under the current compatibility policy.
///
/// This compatibility surface returns only decoded text. New source-ingress
/// code should consume [`decode_source_bytes`] so encoding selection, BOM
/// handling, fidelity, and original-byte mapping limits remain available.
#[must_use]
pub fn decode_text_bytes(bytes: &[u8]) -> String {
    decode_source_bytes(bytes).into_text()
}

/// Read and decode a source file under the current compatibility policy.
///
/// This compatibility surface returns only decoded text. New source-ingress
/// code should consume [`read_source_file_with_encoding`] and retain its typed
/// result through source construction.
pub fn read_text_file_with_encoding(path: &Path) -> io::Result<String> {
    read_source_file_with_encoding(path).map(DecodedText::into_text)
}

#[cfg(test)]
mod tests {
    use super::{decode_source_bytes, decode_text_bytes};

    #[test]
    fn compatibility_decoder_delegates_to_typed_authority() {
        for bytes in [
            b"Perl".as_slice(),
            &[0xEF, 0xBB, 0xBF, b'P', b'e', b'r', b'l'],
            &[0xFF, 0xFE, b'P', 0x00, b'e', 0x00, b'r', 0x00, b'l', 0x00],
            &[0x63, 0x61, 0x66, 0xE9],
            &[0xFF, 0xFE, 0x6D, 0x00, 0x79],
        ] {
            assert_eq!(decode_text_bytes(bytes), decode_source_bytes(bytes).text);
        }
    }
}
