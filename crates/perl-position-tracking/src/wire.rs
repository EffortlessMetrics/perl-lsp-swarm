//! LSP wire types and conversion helpers.
//!
//! This module defines **structural** protocol coordinates: `(line, character)`
//! pairs carried on the wire. Both fields are 0-based. The `character` field has
//! **no intrinsic encoding** in this crate — its meaning is supplied by the active
//! session [`PositionEncodingKind`](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/types/#positionEncodingKind)
//! (UTF-16 by default in LSP 3.17, UTF-8 when negotiated).
//!
//! Conversions between [`WirePosition`] / [`WireRange`] and [`lsp_types::Position`] /
//! [`lsp_types::Range`] copy those fields only; they never consult source text or
//! infer an encoding.
//!
//! Legacy convenience helpers ([`WirePosition::from_byte_offset`],
//! [`WirePosition::to_byte_offset`], and the byte-offset range builders) interpret
//! source bytes using the crate's UTF-16 line/column helpers. They remain for
//! existing callers but are not wire-type semantics; retirement is tracked in #8727
//! after consumers migrate to the canonical generation-bound mapper (#8491).
use crate::{offset_to_utf16_line_col, utf16_line_col_to_offset};
use serde::{Deserialize, Serialize};

/// A protocol-facing LSP position.
///
/// Both fields are 0-based structural coordinates. The `character` unit is defined
/// by the active session encoding when the position is interpreted against source
/// text; this type alone does not embed UTF-16 or UTF-8 semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WirePosition {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based column offset from the start of the line, in wire column units.
    pub character: u32,
}
impl WirePosition {
    /// Creates a new wire position from explicit line and character values.
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    /// Converts a byte offset in `source` into wire coordinates using the legacy
    /// UTF-16 line/column helpers in this crate.
    ///
    /// Prefer the canonical generation-bound mapper (#8491) for new call sites.
    pub fn from_byte_offset(source: &str, byte_offset: usize) -> Self {
        let (line, character) = offset_to_utf16_line_col(source, byte_offset);
        Self { line, character }
    }

    /// Converts this wire position back into a byte offset using the legacy UTF-16
    /// line/column helpers in this crate.
    pub fn to_byte_offset(&self, source: &str) -> usize {
        utf16_line_col_to_offset(source, self.line, self.character)
    }
}

/// A protocol-facing LSP range with inclusive start and exclusive end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WireRange {
    /// Start position of the range (inclusive).
    pub start: WirePosition,
    /// End position of the range (exclusive).
    pub end: WirePosition,
}
impl WireRange {
    /// Creates a new range from start and end positions.
    pub fn new(start: WirePosition, end: WirePosition) -> Self {
        Self { start, end }
    }

    /// Builds a wire range from start/end byte offsets via [`WirePosition::from_byte_offset`].
    pub fn from_byte_offsets(source: &str, start_byte: usize, end_byte: usize) -> Self {
        Self {
            start: WirePosition::from_byte_offset(source, start_byte),
            end: WirePosition::from_byte_offset(source, end_byte),
        }
    }

    /// Creates an empty (cursor) range at `pos`.
    pub fn empty(pos: WirePosition) -> Self {
        Self { start: pos, end: pos }
    }

    /// Creates a range that spans the full document.
    pub fn whole_document(source: &str) -> Self {
        Self {
            start: WirePosition::new(0, 0),
            end: WirePosition::from_byte_offset(source, source.len()),
        }
    }
}

/// A protocol-facing location that combines a URI and a range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireLocation {
    /// Document URI.
    pub uri: String,
    /// Range within the referenced document.
    pub range: WireRange,
}
impl WireLocation {
    /// Creates a new wire location.
    pub fn new(uri: String, range: WireRange) -> Self {
        Self { uri, range }
    }
}
#[cfg(feature = "lsp-compat")]
impl From<WirePosition> for lsp_types::Position {
    fn from(p: WirePosition) -> Self {
        Self { line: p.line, character: p.character }
    }
}
#[cfg(feature = "lsp-compat")]
impl From<lsp_types::Position> for WirePosition {
    fn from(p: lsp_types::Position) -> Self {
        Self { line: p.line, character: p.character }
    }
}
#[cfg(feature = "lsp-compat")]
impl From<WireRange> for lsp_types::Range {
    fn from(r: WireRange) -> Self {
        Self { start: r.start.into(), end: r.end.into() }
    }
}
#[cfg(feature = "lsp-compat")]
impl From<lsp_types::Range> for WireRange {
    fn from(r: lsp_types::Range) -> Self {
        Self { start: r.start.into(), end: r.end.into() }
    }
}
/// A [`WireLocation`] URI that is not a valid protocol URI.
///
/// This crate never substitutes a different resource when conversion fails. The
/// rejected input is carried so the caller can make an evidence-bearing decision
/// at the layer that owns degradation policy.
///
/// Callers that write the rejected value into durable evidence are responsible for
/// bounding and redacting it; the value is reproduced here exactly as supplied.
#[cfg(feature = "lsp-compat")]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("wire location URI is not a valid protocol URI: {uri:?}")]
pub struct WireLocationUriError {
    /// The rejected URI, exactly as supplied by the caller.
    pub uri: String,
}

#[cfg(feature = "lsp-compat")]
impl TryFrom<WireLocation> for lsp_types::Location {
    type Error = WireLocationUriError;

    /// Converts structurally, preserving a valid URI exactly.
    ///
    /// Returns [`WireLocationUriError`] when the URI does not parse or lacks a scheme. No synthetic,
    /// default, or substitute URI is ever produced: naming a resource the caller
    /// never requested is the failure this conversion exists to prevent.
    ///
    /// An LSP `DocumentUri` is an *absolute* URI, but `lsp_types::Uri` parses the
    /// wider URI-**reference** grammar, in which a scheme-less relative reference
    /// (`""`, `"foo.pl"`, `"./rel.pl"`, `"/abs/path.pl"`, `"//host/x"`) parses
    /// successfully without identifying an absolute resource. Parse success alone is
    /// therefore not validity here: the scheme is required, so an emitted
    /// `Location` always carries a scheme-bearing URI. This does not establish that
    /// the resource exists or can be resolved.
    fn try_from(location: WireLocation) -> Result<Self, Self::Error> {
        let WireLocation { uri, range } = location;
        match uri.parse::<lsp_types::Uri>() {
            Ok(parsed) if parsed.scheme().is_some() => {
                Ok(Self { uri: parsed, range: range.into() })
            }
            _ => Err(WireLocationUriError { uri }),
        }
    }
}

#[cfg(all(test, feature = "lsp-compat"))]
mod tests {
    use super::*;

    #[test]
    fn wire_position_lsp_types_round_trip_copies_fields_without_source() {
        let wire = WirePosition::new(2, 5);
        let lsp: lsp_types::Position = wire.into();
        assert_eq!(lsp.line, 2);
        assert_eq!(lsp.character, 5);
        let back: WirePosition = lsp.into();
        assert_eq!(back, wire);
    }

    #[test]
    fn wire_range_lsp_types_round_trip_copies_fields_without_source() {
        let wire = WireRange::new(WirePosition::new(0, 1), WirePosition::new(3, 4));
        let lsp: lsp_types::Range = wire.into();
        assert_eq!(lsp.start.line, 0);
        assert_eq!(lsp.start.character, 1);
        assert_eq!(lsp.end.line, 3);
        assert_eq!(lsp.end.character, 4);
        let back: WireRange = lsp.into();
        assert_eq!(back, wire);
    }

    #[test]
    fn wire_location_to_lsp_location_preserves_valid_uri() -> Result<(), String> {
        let wire_location = WireLocation::new(
            "file:///tmp/example.pl".to_string(),
            WireRange::new(WirePosition::new(1, 2), WirePosition::new(3, 4)),
        );

        let location = lsp_types::Location::try_from(wire_location)
            .map_err(|error| format!("a valid URI must convert without error: {error}"))?;

        assert_eq!(location.uri.as_str(), "file:///tmp/example.pl");
        assert_eq!(location.range.start.line, 1);
        assert_eq!(location.range.start.character, 2);
        assert_eq!(location.range.end.line, 3);
        assert_eq!(location.range.end.character, 4);
        Ok(())
    }

    /// The load-bearing control: an invalid URI must fail, and must not be laundered
    /// into some other resource. Restoring the removed substitution helper makes this
    /// test fail at the `Err` assertion.
    #[test]
    fn wire_location_to_lsp_location_rejects_invalid_uri_without_substituting() -> Result<(), String>
    {
        let wire_location = WireLocation::new(
            "not a uri".to_string(),
            WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 1)),
        );

        match lsp_types::Location::try_from(wire_location) {
            Ok(location) => Err(format!(
                "an invalid URI must not yield a Location, but it became {:?}",
                location.uri.as_str()
            )),
            Err(error) => {
                // The rejected input is carried verbatim so the caller can report it.
                assert_eq!(error.uri, "not a uri");
                assert!(error.to_string().contains("not a uri"));
                Ok(())
            }
        }
    }

    /// Negative control against a weakened fix: rejection must be driven by the URI
    /// actually being invalid, not by rejecting everything. Paired with the test
    /// above, collapsing the conversion to always-`Ok` or always-`Err` fails one side.
    #[test]
    fn wire_location_conversion_discriminates_valid_from_invalid() {
        let range = WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 1));

        for valid in
            ["file:///a.pl", "untitled:Untitled-1", "file:///c%20d/e.pm", "http://h/p", "urn:x:y"]
        {
            assert!(
                lsp_types::Location::try_from(WireLocation::new(valid.to_string(), range)).is_ok(),
                "{valid} must convert"
            );
        }

        // Established empirically against `lsp_types::Uri`, not assumed. The second
        // group parses successfully as URI *references* and is rejected for having
        // no scheme, not for failing to parse — that is the distinction the
        // conversion has to make, so both groups belong in this control.
        for invalid in [
            // malformed: fails to parse at all
            "not a uri",
            "  ",
            "://",
            "http://[",
            "%%",
            // parses, but is a scheme-less relative reference naming no document
            "",
            "foo/bar.pl",
            "./rel.pl",
            "/abs/path.pl",
            "//host/x",
            "a",
        ] {
            let result =
                lsp_types::Location::try_from(WireLocation::new(invalid.to_string(), range));
            assert!(result.is_err(), "{invalid:?} must be rejected");
        }
    }

    /// A scheme-less reference parses but names no resolvable document, so it must
    /// be rejected rather than emitted. Dropping the `scheme().is_some()` guard
    /// makes every row here convert successfully.
    #[test]
    fn wire_location_rejects_scheme_less_relative_references() -> Result<(), String> {
        let range = WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 1));

        for reference in ["", "foo/bar.pl", "./rel.pl", "/abs/path.pl", "//host/x"] {
            // Precondition: these really do parse; the guard, not the parser, rejects them.
            assert!(
                reference.parse::<lsp_types::Uri>().is_ok(),
                "{reference:?} is expected to parse as a URI reference"
            );

            match lsp_types::Location::try_from(WireLocation::new(reference.to_string(), range)) {
                Ok(location) => {
                    return Err(format!(
                        "scheme-less {reference:?} must not yield a Location, but it became {:?}",
                        location.uri.as_str()
                    ));
                }
                Err(error) => assert_eq!(error.uri, reference),
            }
        }
        Ok(())
    }

    /// Pins the removal of the substitution helper itself, not just its call site.
    ///
    /// The needles are assembled at runtime rather than written as literals, so this
    /// test does not match its own source text.
    #[test]
    fn module_source_contains_no_synthetic_uri_substitution() {
        let source = include_str!("wire.rs");
        let forbidden = [
            format!("file:///{}", "unknown"),
            format!("about:{}", "blank"),
            format!("urn:perl-lsp:{}", "unknown"),
            format!("fallback_lsp{}", "_uri"),
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "`{needle}` must not reappear as a substitute URI in position tracking"
            );
        }
    }
}
