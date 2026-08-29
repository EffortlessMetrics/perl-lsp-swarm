//! Typed source-byte decoding for Perl source ingress.
//!
//! This module makes the current decoder decision observable without changing
//! the text produced by the historical compatibility path. Original file bytes
//! and decoded UTF-8 text remain different coordinate subjects unless the
//! result explicitly reports identity mapping.

use std::io;
use std::path::Path;

/// Versioned identity of the current source-decoding policy.
pub(crate) const SOURCE_DECODE_POLICY_VERSION: &str = "perl-lsp-source-decode/v1";

/// Encoding profile selected for one source-byte payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceEncoding {
    /// Exact UTF-8 bytes without a byte-order mark.
    Utf8,
    /// UTF-8 payload after consuming one leading UTF-8 BOM.
    Utf8Bom,
    /// UTF-16 little-endian payload selected by its BOM.
    Utf16LeBom,
    /// UTF-16 big-endian payload selected by its BOM.
    Utf16BeBom,
    /// Historical byte-preserving Latin-1 fallback for non-UTF-8 input.
    Latin1Fallback,
}

/// Why the decoder selected one encoding profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecodeSelectionReason {
    /// The complete input was valid UTF-8.
    ValidUtf8,
    /// A UTF-8 BOM was present and the remaining payload was valid UTF-8.
    Utf8Bom,
    /// A UTF-16LE BOM was present and its payload had complete code units.
    Utf16LeBom,
    /// A UTF-16BE BOM was present and its payload had complete code units.
    Utf16BeBom,
    /// The input was not valid UTF-8 and used the historical Latin-1 fallback.
    InvalidUtf8Latin1Fallback,
    /// A UTF-8 BOM was present but its payload was invalid UTF-8, so the full
    /// original byte sequence used the historical Latin-1 fallback.
    InvalidUtf8BomPayloadLatin1Fallback,
    /// A UTF-16LE BOM was followed by an odd payload length, so the full
    /// original byte sequence used the historical Latin-1 fallback.
    OddLengthUtf16LeLatin1Fallback,
    /// A UTF-16BE BOM was followed by an odd payload length, so the full
    /// original byte sequence used the historical Latin-1 fallback.
    OddLengthUtf16BeLatin1Fallback,
}

/// How a leading byte-order mark participated in decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceBomDisposition {
    /// No recognized leading BOM was present.
    Absent,
    /// One UTF-8 BOM was consumed before decoding the payload.
    Utf8Consumed,
    /// One UTF-16LE BOM was consumed before decoding the payload.
    Utf16LeConsumed,
    /// One UTF-16BE BOM was consumed before decoding the payload.
    Utf16BeConsumed,
    /// A UTF-8 BOM-shaped prefix remained in the full-byte Latin-1 fallback.
    Utf8PreservedByFallback,
    /// A UTF-16LE BOM-shaped prefix remained in the full-byte Latin-1 fallback.
    Utf16LePreservedByFallback,
    /// A UTF-16BE BOM-shaped prefix remained in the full-byte Latin-1 fallback.
    Utf16BePreservedByFallback,
}

/// Fidelity of decoded text relative to the selected encoding profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecodeFidelity {
    /// Original bytes were already the exact decoded UTF-8 bytes.
    Exact,
    /// Decoding was lossless, although decoded UTF-8 bytes differ from input.
    LosslessDecode,
    /// Decoding inserted one or more Unicode replacement characters.
    LossyDecode,
    /// A compatibility fallback, rather than the primary encoding, was used.
    FallbackDecode,
}

/// What exact original-byte to decoded-UTF-8 mapping is currently available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OriginalByteMapping {
    /// Original bytes and decoded UTF-8 bytes are identical.
    IdentityBytes,
    /// A fixed leading byte prefix was consumed before otherwise exact UTF-8.
    LeadingBytesConsumed {
        /// Number of original bytes consumed from the leading prefix.
        byte_count: usize,
    },
    /// Decoding changed byte representation and no reverse byte map is claimed.
    ReencodedUnavailable,
}

/// Typed result of decoding one exact source-byte payload.
#[allow(dead_code, reason = "metadata consumers land under #10581, #10077, and #8612 after #13530")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct DecodedText {
    /// Exact valid UTF-8 text produced by the decoder.
    pub(crate) text: String,
    /// Number of original input bytes.
    pub(crate) original_byte_len: usize,
    /// Number of bytes in the decoded UTF-8 representation.
    pub(crate) decoded_utf8_byte_len: usize,
    /// Selected source encoding profile.
    pub(crate) encoding: SourceEncoding,
    /// Typed reason the profile was selected.
    pub(crate) selection_reason: DecodeSelectionReason,
    /// Leading-BOM disposition.
    pub(crate) bom: SourceBomDisposition,
    /// Fidelity of the decoded result.
    pub(crate) fidelity: DecodeFidelity,
    /// Number of replacement characters inserted by UTF-16 decoding.
    pub(crate) replacement_count: usize,
    /// Available relationship between original and decoded byte offsets.
    pub(crate) original_mapping: OriginalByteMapping,
    /// Stable decoder policy identity.
    pub(crate) policy_version: &'static str,
}

impl DecodedText {
    fn new(
        bytes: &[u8],
        text: String,
        encoding: SourceEncoding,
        selection_reason: DecodeSelectionReason,
        bom: SourceBomDisposition,
        fidelity: DecodeFidelity,
        replacement_count: usize,
        original_mapping: OriginalByteMapping,
    ) -> Self {
        Self {
            original_byte_len: bytes.len(),
            decoded_utf8_byte_len: text.len(),
            text,
            encoding,
            selection_reason,
            bom,
            fidelity,
            replacement_count,
            original_mapping,
            policy_version: SOURCE_DECODE_POLICY_VERSION,
        }
    }

    /// Consume the typed result and return only its compatibility text.
    pub(crate) fn into_text(self) -> String {
        self.text
    }
}

struct Utf16Decode {
    text: String,
    replacement_count: usize,
}

/// Decode source bytes under the currently shipping compatibility policy.
///
/// This preserves the exact text output of the historical decoder while
/// retaining which profile was selected and whether the result was lossy.
#[must_use]
pub(crate) fn decode_source_bytes(bytes: &[u8]) -> DecodedText {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(utf8) = std::str::from_utf8(&bytes[3..]) {
            return DecodedText::new(
                bytes,
                utf8.to_string(),
                SourceEncoding::Utf8Bom,
                DecodeSelectionReason::Utf8Bom,
                SourceBomDisposition::Utf8Consumed,
                DecodeFidelity::LosslessDecode,
                0,
                OriginalByteMapping::LeadingBytesConsumed { byte_count: 3 },
            );
        }

        return decode_latin1_fallback(
            bytes,
            DecodeSelectionReason::InvalidUtf8BomPayloadLatin1Fallback,
            SourceBomDisposition::Utf8PreservedByFallback,
        );
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        if let Some(decoded) = decode_utf16(&bytes[2..], true) {
            let fidelity = if decoded.replacement_count == 0 {
                DecodeFidelity::LosslessDecode
            } else {
                DecodeFidelity::LossyDecode
            };
            return DecodedText::new(
                bytes,
                decoded.text,
                SourceEncoding::Utf16LeBom,
                DecodeSelectionReason::Utf16LeBom,
                SourceBomDisposition::Utf16LeConsumed,
                fidelity,
                decoded.replacement_count,
                OriginalByteMapping::ReencodedUnavailable,
            );
        }

        return decode_latin1_fallback(
            bytes,
            DecodeSelectionReason::OddLengthUtf16LeLatin1Fallback,
            SourceBomDisposition::Utf16LePreservedByFallback,
        );
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        if let Some(decoded) = decode_utf16(&bytes[2..], false) {
            let fidelity = if decoded.replacement_count == 0 {
                DecodeFidelity::LosslessDecode
            } else {
                DecodeFidelity::LossyDecode
            };
            return DecodedText::new(
                bytes,
                decoded.text,
                SourceEncoding::Utf16BeBom,
                DecodeSelectionReason::Utf16BeBom,
                SourceBomDisposition::Utf16BeConsumed,
                fidelity,
                decoded.replacement_count,
                OriginalByteMapping::ReencodedUnavailable,
            );
        }

        return decode_latin1_fallback(
            bytes,
            DecodeSelectionReason::OddLengthUtf16BeLatin1Fallback,
            SourceBomDisposition::Utf16BePreservedByFallback,
        );
    }

    match std::str::from_utf8(bytes) {
        Ok(utf8) => DecodedText::new(
            bytes,
            utf8.to_string(),
            SourceEncoding::Utf8,
            DecodeSelectionReason::ValidUtf8,
            SourceBomDisposition::Absent,
            DecodeFidelity::Exact,
            0,
            OriginalByteMapping::IdentityBytes,
        ),
        Err(_) => decode_latin1_fallback(
            bytes,
            DecodeSelectionReason::InvalidUtf8Latin1Fallback,
            SourceBomDisposition::Absent,
        ),
    }
}

/// Read and decode a source file while retaining the typed decode outcome.
pub(crate) fn read_source_file_with_encoding(path: &Path) -> io::Result<DecodedText> {
    std::fs::read(path).map(|bytes| decode_source_bytes(&bytes))
}

fn decode_latin1_fallback(
    bytes: &[u8],
    selection_reason: DecodeSelectionReason,
    bom: SourceBomDisposition,
) -> DecodedText {
    let text = bytes.iter().copied().map(char::from).collect();
    DecodedText::new(
        bytes,
        text,
        SourceEncoding::Latin1Fallback,
        selection_reason,
        bom,
        DecodeFidelity::FallbackDecode,
        0,
        OriginalByteMapping::ReencodedUnavailable,
    )
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Option<Utf16Decode> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }

    let words = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });

    let mut text = String::with_capacity(bytes.len());
    let mut replacement_count = 0;
    for decoded in char::decode_utf16(words) {
        match decoded {
            Ok(ch) => text.push(ch),
            Err(_) => {
                replacement_count += 1;
                text.push(char::REPLACEMENT_CHARACTER);
            }
        }
    }

    Some(Utf16Decode { text, replacement_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_utf8_retains_identity_mapping() {
        let bytes = "café\r\n".as_bytes();
        let decoded = decode_source_bytes(bytes);

        assert_eq!(decoded.text, "café\r\n");
        assert_eq!(decoded.original_byte_len, bytes.len());
        assert_eq!(decoded.decoded_utf8_byte_len, bytes.len());
        assert_eq!(decoded.encoding, SourceEncoding::Utf8);
        assert_eq!(decoded.selection_reason, DecodeSelectionReason::ValidUtf8);
        assert_eq!(decoded.bom, SourceBomDisposition::Absent);
        assert_eq!(decoded.fidelity, DecodeFidelity::Exact);
        assert_eq!(decoded.replacement_count, 0);
        assert_eq!(decoded.original_mapping, OriginalByteMapping::IdentityBytes);
        assert_eq!(decoded.policy_version, SOURCE_DECODE_POLICY_VERSION);
    }

    #[test]
    fn utf8_bom_records_consumed_prefix() {
        let bytes = [0xEF, 0xBB, 0xBF, b'P', b'e', b'r', b'l'];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "Perl");
        assert_eq!(decoded.encoding, SourceEncoding::Utf8Bom);
        assert_eq!(decoded.bom, SourceBomDisposition::Utf8Consumed);
        assert_eq!(decoded.fidelity, DecodeFidelity::LosslessDecode);
        assert_eq!(
            decoded.original_mapping,
            OriginalByteMapping::LeadingBytesConsumed { byte_count: 3 }
        );
    }

    #[test]
    fn double_utf8_bom_consumes_exactly_one_prefix() {
        let bytes = [0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "\u{FEFF}");
        assert_eq!(decoded.encoding, SourceEncoding::Utf8Bom);
        assert_eq!(decoded.bom, SourceBomDisposition::Utf8Consumed);
    }

    #[test]
    fn utf16le_decodes_astral_scalar_losslessly() {
        let bytes = [0xFF, 0xFE, 0x41, 0x00, 0x3D, 0xD8, 0x00, 0xDE];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "A😀");
        assert_eq!(decoded.encoding, SourceEncoding::Utf16LeBom);
        assert_eq!(decoded.selection_reason, DecodeSelectionReason::Utf16LeBom);
        assert_eq!(decoded.bom, SourceBomDisposition::Utf16LeConsumed);
        assert_eq!(decoded.fidelity, DecodeFidelity::LosslessDecode);
        assert_eq!(decoded.replacement_count, 0);
        assert_eq!(decoded.original_mapping, OriginalByteMapping::ReencodedUnavailable);
    }

    #[test]
    fn utf16be_decodes_astral_scalar_losslessly() {
        let bytes = [0xFE, 0xFF, 0x00, 0x41, 0xD8, 0x3D, 0xDE, 0x00];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "A😀");
        assert_eq!(decoded.encoding, SourceEncoding::Utf16BeBom);
        assert_eq!(decoded.bom, SourceBomDisposition::Utf16BeConsumed);
        assert_eq!(decoded.fidelity, DecodeFidelity::LosslessDecode);
        assert_eq!(decoded.replacement_count, 0);
    }

    #[test]
    fn unpaired_utf16_surrogate_records_lossy_replacement() {
        let bytes = [0xFF, 0xFE, 0x00, 0xD8];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "\u{FFFD}");
        assert_eq!(decoded.encoding, SourceEncoding::Utf16LeBom);
        assert_eq!(decoded.fidelity, DecodeFidelity::LossyDecode);
        assert_eq!(decoded.replacement_count, 1);
    }

    #[test]
    fn literal_replacement_character_is_not_decoder_loss() {
        let exact = decode_source_bytes("\u{FFFD}".as_bytes());
        let lossy = decode_source_bytes(&[0xFF, 0xFE, 0x00, 0xD8]);

        assert_eq!(exact.text, lossy.text);
        assert_eq!(exact.fidelity, DecodeFidelity::Exact);
        assert_eq!(exact.replacement_count, 0);
        assert_eq!(lossy.fidelity, DecodeFidelity::LossyDecode);
        assert_eq!(lossy.replacement_count, 1);
    }

    #[test]
    fn odd_utf16le_payload_preserves_current_latin1_fallback() {
        let bytes = [0xFF, 0xFE, 0x6D, 0x00, 0x79];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "\u{00FF}\u{00FE}m\0y");
        assert_eq!(decoded.encoding, SourceEncoding::Latin1Fallback);
        assert_eq!(decoded.selection_reason, DecodeSelectionReason::OddLengthUtf16LeLatin1Fallback);
        assert_eq!(decoded.bom, SourceBomDisposition::Utf16LePreservedByFallback);
        assert_eq!(decoded.fidelity, DecodeFidelity::FallbackDecode);
    }

    #[test]
    fn invalid_utf8_bom_payload_preserves_full_bytes_in_fallback() {
        let bytes = [0xEF, 0xBB, 0xBF, 0xFF];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "\u{00EF}\u{00BB}\u{00BF}\u{00FF}");
        assert_eq!(decoded.encoding, SourceEncoding::Latin1Fallback);
        assert_eq!(
            decoded.selection_reason,
            DecodeSelectionReason::InvalidUtf8BomPayloadLatin1Fallback
        );
        assert_eq!(decoded.bom, SourceBomDisposition::Utf8PreservedByFallback);
    }

    #[test]
    fn invalid_utf8_uses_explicit_latin1_fallback() {
        let bytes = [0x63, 0x61, 0x66, 0xE9];
        let decoded = decode_source_bytes(&bytes);

        assert_eq!(decoded.text, "café");
        assert_eq!(decoded.encoding, SourceEncoding::Latin1Fallback);
        assert_eq!(decoded.selection_reason, DecodeSelectionReason::InvalidUtf8Latin1Fallback);
        assert_eq!(decoded.fidelity, DecodeFidelity::FallbackDecode);
        assert_eq!(decoded.original_mapping, OriginalByteMapping::ReencodedUnavailable);
        assert_ne!(decoded.original_byte_len, decoded.decoded_utf8_byte_len);
    }

    #[test]
    fn bom_only_payloads_have_typed_empty_results() {
        let utf8 = decode_source_bytes(&[0xEF, 0xBB, 0xBF]);
        let utf16le = decode_source_bytes(&[0xFF, 0xFE]);
        let utf16be = decode_source_bytes(&[0xFE, 0xFF]);

        assert!(utf8.text.is_empty());
        assert!(utf16le.text.is_empty());
        assert!(utf16be.text.is_empty());
        assert_eq!(utf8.bom, SourceBomDisposition::Utf8Consumed);
        assert_eq!(utf16le.bom, SourceBomDisposition::Utf16LeConsumed);
        assert_eq!(utf16be.bom, SourceBomDisposition::Utf16BeConsumed);
    }
}
