//! Byte-level identity of a corpus member.
//!
//! Corpus assertions are expressed as `(line, character)` positions, so the
//! bytes those positions are measured against are part of the contract. This
//! module classifies a member from its **raw bytes** — never from a decoded
//! `String` — so that a line-ending conversion, an added or removed final
//! newline, an introduced byte-order mark, or an undecodable byte sequence is
//! an observable difference rather than a silent one.
//!
//! Classification is deliberately fail-closed and lossless: invalid UTF-8 is
//! reported with the offset of the first offending byte instead of being
//! replaced with `U+FFFD`.

use std::fmt;

/// UTF-8 byte-order mark.
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// How a member's line terminators are represented in its raw bytes.
///
/// The classification is over the terminators that are actually present. A
/// member with no terminator at all is [`NewlineStyle::None`]; a member that
/// mixes representations is [`NewlineStyle::Mixed`], which is never the same
/// as any single style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewlineStyle {
    /// The member contains no line terminator.
    None,
    /// Every terminator is a bare line feed (`\n`).
    Lf,
    /// Every terminator is a carriage return + line feed pair (`\r\n`).
    Crlf,
    /// Every terminator is a bare carriage return (`\r`).
    Cr,
    /// More than one terminator representation is present.
    Mixed,
}

impl fmt::Display for NewlineStyle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::None => "none",
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
            Self::Cr => "CR",
            Self::Mixed => "mixed",
        };
        formatter.write_str(name)
    }
}

/// Whether a member's raw bytes decode as UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// The bytes are valid UTF-8.
    Utf8,
    /// The bytes are not valid UTF-8.
    ///
    /// `valid_up_to` is the offset of the first byte that could not be
    /// decoded, so a failure names a position instead of being absorbed into a
    /// replacement character.
    InvalidUtf8 {
        /// Byte offset of the first undecodable byte.
        valid_up_to: usize,
    },
}

impl fmt::Display for Encoding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8 => formatter.write_str("UTF-8"),
            Self::InvalidUtf8 { valid_up_to } => {
                write!(formatter, "invalid UTF-8 at byte offset {valid_up_to}")
            }
        }
    }
}

/// The byte-level identity of one corpus member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ByteFidelity {
    /// Terminator representation used by the member.
    pub newline_style: NewlineStyle,
    /// Whether the final byte terminates a line.
    ///
    /// A member whose last byte is `\n` or `\r` ends with a terminator. An
    /// empty member does not.
    pub final_newline: bool,
    /// Whether the member begins with a UTF-8 byte-order mark.
    pub byte_order_mark: bool,
    /// Whether the member decodes as UTF-8.
    pub encoding: Encoding,
}

impl ByteFidelity {
    /// Classify a member from its exact bytes.
    ///
    /// The input is never decoded, normalized, or copied: every field is
    /// derived from `bytes` as stored.
    #[must_use]
    pub fn classify(bytes: &[u8]) -> Self {
        Self {
            newline_style: classify_newline_style(bytes),
            final_newline: matches!(bytes.last(), Some(b'\n' | b'\r')),
            byte_order_mark: bytes.starts_with(&UTF8_BOM),
            encoding: classify_encoding(bytes),
        }
    }

    /// Whether the member is valid UTF-8.
    #[must_use]
    pub fn is_utf8(&self) -> bool {
        matches!(self.encoding, Encoding::Utf8)
    }
}

impl fmt::Display for ByteFidelity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "newlines={}, final newline={}, BOM={}, encoding={}",
            self.newline_style, self.final_newline, self.byte_order_mark, self.encoding
        )
    }
}

/// Count each terminator representation and reduce it to one style.
fn classify_newline_style(bytes: &[u8]) -> NewlineStyle {
    let mut saw_lf = false;
    let mut saw_crlf = false;
    let mut saw_cr = false;

    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    saw_crlf = true;
                    index += 2;
                } else {
                    saw_cr = true;
                    index += 1;
                }
            }
            b'\n' => {
                saw_lf = true;
                index += 1;
            }
            _ => index += 1,
        }
    }

    match (saw_lf, saw_crlf, saw_cr) {
        (false, false, false) => NewlineStyle::None,
        (true, false, false) => NewlineStyle::Lf,
        (false, true, false) => NewlineStyle::Crlf,
        (false, false, true) => NewlineStyle::Cr,
        _ => NewlineStyle::Mixed,
    }
}

/// Report UTF-8 validity without replacing anything.
fn classify_encoding(bytes: &[u8]) -> Encoding {
    match std::str::from_utf8(bytes) {
        Ok(_) => Encoding::Utf8,
        Err(error) => Encoding::InvalidUtf8 { valid_up_to: error.valid_up_to() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classification the gold corpus requires today.
    fn lf_utf8(bytes: &[u8]) -> bool {
        let fidelity = ByteFidelity::classify(bytes);
        fidelity.newline_style == NewlineStyle::Lf
            && fidelity.final_newline
            && !fidelity.byte_order_mark
            && fidelity.is_utf8()
    }

    #[test]
    fn lf_source_with_a_final_newline_is_the_default_class() {
        let fidelity = ByteFidelity::classify(b"my $x = 1;\nmy $y = 2;\n");
        assert_eq!(fidelity.newline_style, NewlineStyle::Lf);
        assert!(fidelity.final_newline);
        assert!(!fidelity.byte_order_mark);
        assert_eq!(fidelity.encoding, Encoding::Utf8);
    }

    #[test]
    fn converting_lf_to_crlf_changes_the_class() {
        let lf = b"my $x = 1;\nmy $y = 2;\n";
        let crlf = b"my $x = 1;\r\nmy $y = 2;\r\n";

        assert!(lf_utf8(lf));
        assert!(!lf_utf8(crlf), "a CRLF conversion must not satisfy the LF class");
        assert_eq!(ByteFidelity::classify(crlf).newline_style, NewlineStyle::Crlf);
        assert_ne!(ByteFidelity::classify(lf), ByteFidelity::classify(crlf));
    }

    #[test]
    fn adding_or_removing_a_final_newline_changes_the_class() {
        let with_newline = b"my $x = 1;\n";
        let without_newline = b"my $x = 1;";

        assert!(ByteFidelity::classify(with_newline).final_newline);
        assert!(!ByteFidelity::classify(without_newline).final_newline);
        assert!(!lf_utf8(without_newline), "a stripped final newline must not satisfy the class");

        // Removing the only terminator also removes the style evidence, which
        // must not be reported as LF.
        assert_eq!(ByteFidelity::classify(without_newline).newline_style, NewlineStyle::None);
    }

    #[test]
    fn a_lone_carriage_return_is_not_reported_as_lf_or_crlf() {
        let fidelity = ByteFidelity::classify(b"my $x = 1;\rmy $y = 2;\r");
        assert_eq!(fidelity.newline_style, NewlineStyle::Cr);
        assert!(fidelity.final_newline, "a trailing bare CR still terminates the last line");
    }

    #[test]
    fn mixed_terminators_are_never_confused_with_a_single_style() {
        for bytes in [
            b"a\nb\r\n".as_slice(),
            b"a\r\nb\n".as_slice(),
            b"a\rb\n".as_slice(),
            b"a\r\nb\rc\n".as_slice(),
        ] {
            assert_eq!(
                ByteFidelity::classify(bytes).newline_style,
                NewlineStyle::Mixed,
                "mixed input {bytes:?} must classify as mixed"
            );
        }
    }

    #[test]
    fn crlf_is_one_terminator_not_a_cr_next_to_an_lf() {
        // The naive reading counts `\r\n` as both a bare CR and a bare LF and
        // therefore reports mixed newlines for an ordinary CRLF file.
        assert_eq!(ByteFidelity::classify(b"a\r\nb\r\n").newline_style, NewlineStyle::Crlf);
    }

    #[test]
    fn a_byte_order_mark_is_reported_and_does_not_hide_the_newline_style() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice(b"my $x = 1;\n");

        let fidelity = ByteFidelity::classify(&bytes);
        assert!(fidelity.byte_order_mark);
        assert_eq!(fidelity.newline_style, NewlineStyle::Lf);
        assert!(fidelity.is_utf8(), "a BOM is valid UTF-8, it is just not wanted here");
        assert!(!lf_utf8(&bytes), "a BOM must not satisfy the default class");
    }

    #[test]
    fn invalid_utf8_is_named_by_offset_rather_than_replaced() {
        // `0xFF` is never a legal UTF-8 byte.
        let bytes = b"use strict;\n\xFFmy $x = 1;\n";
        let fidelity = ByteFidelity::classify(bytes);

        assert_eq!(fidelity.encoding, Encoding::InvalidUtf8 { valid_up_to: 12 });
        assert!(!fidelity.is_utf8());
        assert!(
            fidelity.to_string().contains("invalid UTF-8 at byte offset 12"),
            "the failure must name the offending offset: {fidelity}"
        );

        // Negative control: the lossy decode that this classification exists to
        // avoid silently produces a valid string instead of a failure.
        assert!(String::from_utf8_lossy(bytes).contains('\u{FFFD}'));
    }

    #[test]
    fn a_truncated_multibyte_sequence_reports_the_start_of_the_sequence() {
        // The lead byte of a three-byte sequence with its continuation bytes cut off.
        let fidelity = ByteFidelity::classify(b"ok\xE2\x82");
        assert_eq!(fidelity.encoding, Encoding::InvalidUtf8 { valid_up_to: 2 });
    }

    #[test]
    fn multibyte_utf8_is_valid_and_does_not_disturb_the_newline_style() {
        // Two-, three-, and four-byte scalars.
        let fidelity = ByteFidelity::classify("my $n = 'é 中 𝄞';\n".as_bytes());
        assert_eq!(fidelity.encoding, Encoding::Utf8);
        assert_eq!(fidelity.newline_style, NewlineStyle::Lf);
        assert!(fidelity.final_newline);
    }

    #[test]
    fn an_empty_member_has_no_style_and_no_final_newline() {
        let fidelity = ByteFidelity::classify(b"");
        assert_eq!(fidelity.newline_style, NewlineStyle::None);
        assert!(!fidelity.final_newline);
        assert!(!fidelity.byte_order_mark);
        assert_eq!(fidelity.encoding, Encoding::Utf8);
        assert!(!lf_utf8(b""), "an empty member must not satisfy the default class");
    }

    /// Each style must be reachable and land on its own variant. A classifier
    /// that collapses two representations — bare CR read as LF, or `Mixed`
    /// never returned — still satisfies every single-style test above, but
    /// fails this partition.
    #[test]
    fn every_newline_style_is_reachable_and_pairwise_distinct() {
        let representatives: [(&[u8], NewlineStyle); 5] = [
            (b"no terminator", NewlineStyle::None),
            (b"a\nb\n", NewlineStyle::Lf),
            (b"a\r\nb\r\n", NewlineStyle::Crlf),
            (b"a\rb\r", NewlineStyle::Cr),
            (b"a\nb\r\n", NewlineStyle::Mixed),
        ];

        let mut observed = Vec::new();
        for (bytes, expected) in representatives {
            let style = ByteFidelity::classify(bytes).newline_style;
            assert_eq!(style, expected, "{bytes:?} must classify as {expected}");
            assert!(!observed.contains(&style), "{style} was produced by two different inputs");
            observed.push(style);
        }

        assert_eq!(observed.len(), 5, "every newline style must be reachable");
    }
}
