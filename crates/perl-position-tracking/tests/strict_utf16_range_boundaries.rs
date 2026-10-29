//! Strict range admission protects byte replacements, not grapheme clusters.
//! These literal expectations exercise the reference API, not live text sync.

use perl_position_tracking::{
    PositionEncoding, PositionMappingDisposition as Disposition, WirePosition, WireRange,
    byte_range_to_wire_range, wire_range_to_bytes,
};

// Bytes: a=0, 😀=1..5, e=5, ◌́=6..8, CRLF=8..10, β=10..12, LF=12, EOF=13.
// Scalar indices: a=0, 😀=1, e=2, ◌́=3, CR=4, LF=5, β=6, LF=7, EOF=8.
const SOURCE: &str = "a😀e\u{301}\r\nβ\n";

fn range(start: (u32, u32), end: (u32, u32)) -> WireRange {
    WireRange {
        start: WirePosition { line: start.0, character: start.1 },
        end: WirePosition { line: end.0, character: end.1 },
    }
}

#[test]
fn exact_utf16_ranges_select_literal_bytes_and_scalars_for_replacement() {
    let cases = [
        (range((0, 1), (0, 3)), 1..5, 1..2, "*", "a*e\u{301}\r\nβ\n"),
        // The combining mark is independently addressable: do not snap to e.
        (range((0, 4), (0, 5)), 6..8, 3..4, "!", "a😀e!\r\nβ\n"),
        // End-exclusive ranges include the complete separator via next-line 0.
        (range((0, 5), (1, 0)), 8..10, 4..6, "\n", "a😀e\u{301}\nβ\n"),
        (range((1, 1), (2, 0)), 12..13, 7..8, "", "a😀e\u{301}\r\nβ"),
        (range((2, 0), (2, 0)), 13..13, 8..8, "!", "a😀e\u{301}\r\nβ\n!"),
    ];

    for (wire, bytes, chars, replacement, expected) in cases {
        let incoming = wire_range_to_bytes(SOURCE, wire, PositionEncoding::Utf16);
        assert_eq!(incoming.disposition(), Disposition::Exact, "{wire:?}");
        assert_eq!(incoming.byte_range(), Some(bytes.clone()), "{wire:?}");
        assert_eq!(incoming.char_range(), Some(chars), "{wire:?}");

        let outgoing = byte_range_to_wire_range(SOURCE, bytes, PositionEncoding::Utf16);
        assert_eq!(outgoing.disposition(), Disposition::Exact, "{wire:?}");
        assert_eq!(outgoing.wire_range(), Some(wire));

        let mut edited = SOURCE.to_owned();
        if let Some(admitted) = incoming.byte_range() {
            edited.replace_range(admitted, replacement);
        }
        assert_eq!(edited, expected, "{wire:?}");
    }
}

#[test]
fn invalid_utf16_range_endpoint_never_authorizes_a_replacement() {
    let cases = [
        (range((0, 2), (0, 3)), Disposition::InvalidCodeUnitBoundary),
        (range((0, 1), (0, 2)), Disposition::InvalidCodeUnitBoundary),
        (range((0, 2), (0, 2)), Disposition::InvalidCodeUnitBoundary),
        (range((3, 0), (3, 0)), Disposition::InvalidLine),
        (range((0, 0), (3, 0)), Disposition::InvalidLine),
        (range((0, 5), (0, 4)), Disposition::InvalidRangeOrder),
        (range((1, 0), (0, 5)), Disposition::InvalidRangeOrder),
    ];

    for (wire, expected) in cases {
        let mapping = wire_range_to_bytes(SOURCE, wire, PositionEncoding::Utf16);
        assert_eq!(mapping.disposition(), expected, "{wire:?}");
        assert_eq!(mapping.byte_range(), None, "{wire:?}");
        assert_eq!(mapping.char_range(), None, "{wire:?}");
    }
}

#[test]
fn incoming_line_end_normalization_stays_before_the_separator() {
    for (wire, bytes, chars, normalized_start, normalized_end) in [
        (range((0, 99), (1, 0)), 8..10, 4..6, (0, 5), (1, 0)),
        (range((0, 4), (0, 99)), 6..8, 3..4, (0, 4), (0, 5)),
        (range((0, 99), (0, 100)), 8..8, 4..4, (0, 5), (0, 5)),
        (range((2, 0), (2, 99)), 13..13, 8..8, (2, 0), (2, 0)),
    ] {
        let mapping = wire_range_to_bytes(SOURCE, wire, PositionEncoding::Utf16);
        let normalized = range(normalized_start, normalized_end);
        assert_eq!(mapping.disposition(), Disposition::LineEndNormalized, "{wire:?}");
        assert_eq!(mapping.byte_range(), Some(bytes));
        assert_eq!(mapping.char_range(), Some(chars));
        assert_eq!(mapping.start().normalized(), Some(normalized.start));
        assert_eq!(mapping.end().normalized(), Some(normalized.end));
    }
}

#[test]
fn outgoing_ranges_reject_either_scalar_or_crlf_interior_endpoint() {
    for (bytes, expected) in [
        (2..5, Disposition::InvalidCodeUnitBoundary),
        (1..2, Disposition::InvalidCodeUnitBoundary),
        (7..8, Disposition::InvalidCodeUnitBoundary),
        (6..7, Disposition::InvalidCodeUnitBoundary),
        (9..10, Disposition::InvalidNewlineBoundary),
        (8..9, Disposition::InvalidNewlineBoundary),
        (9..9, Disposition::InvalidNewlineBoundary),
        (13..14, Disposition::OverflowOrResourceBoundary),
        (14..14, Disposition::OverflowOrResourceBoundary),
        (std::ops::Range { start: 10, end: 8 }, Disposition::InvalidRangeOrder),
    ] {
        let mapping = byte_range_to_wire_range(SOURCE, bytes.clone(), PositionEncoding::Utf16);
        assert_eq!(mapping.disposition(), expected, "{bytes:?}");
        assert_eq!(mapping.wire_range(), None, "{bytes:?}");
    }
}
