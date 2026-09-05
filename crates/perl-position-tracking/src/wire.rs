//! LSP wire types and conversion helpers.
//!
//! This module defines the protocol-facing equivalents of internal span/position
//! types. The wire types use:
//!
//! - 0-based line indexes
//! - UTF-16 code unit character offsets (per LSP)
//!
//! Use [`WirePosition::from_byte_offset`] and [`WirePosition::to_byte_offset`]
//! to convert between parser byte offsets and LSP-compatible coordinates.
use crate::{offset_to_utf16_line_col, utf16_line_col_to_offset};
use serde::{Deserialize, Serialize};

/// A protocol-facing LSP position.
///
/// Both fields are 0-based. `character` is measured in UTF-16 code units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WirePosition {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based UTF-16 code-unit offset within the line.
    pub character: u32,
}
impl WirePosition {
    /// Creates a new wire position from explicit line and character values.
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    /// Converts a byte offset in `source` into an LSP wire position.
    pub fn from_byte_offset(source: &str, byte_offset: usize) -> Self {
        let (line, character) = offset_to_utf16_line_col(source, byte_offset);
        Self { line, character }
    }

    /// Converts this LSP wire position back into a byte offset in `source`.
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

    /// Builds a wire range from start/end byte offsets in `source`.
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
    /// Returns [`WireLocationUriError`] when the URI does not parse. No synthetic,
    /// default, or substitute URI is ever produced: naming a resource the caller
    /// never requested is the failure this conversion exists to prevent.
    ///
    /// Validity is exactly `lsp_types::Uri`'s parse result. Note that the empty
    /// string parses successfully, as an empty relative reference; this conversion
    /// therefore returns an empty `Uri` rather than an error for it. That is
    /// unchanged from the previous behavior — the old code also parsed `""`
    /// successfully and never reached its fallback — and whether an empty URI should
    /// be refused belongs to the URI-validity owner, not to this structural
    /// conversion.
    fn try_from(location: WireLocation) -> Result<Self, Self::Error> {
        let WireLocation { uri, range } = location;
        match uri.parse::<lsp_types::Uri>() {
            Ok(parsed) => Ok(Self { uri: parsed, range: range.into() }),
            Err(_) => Err(WireLocationUriError { uri }),
        }
    }
}

#[cfg(all(test, feature = "lsp-compat"))]
mod tests {
    use super::*;

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

        for valid in ["file:///a.pl", "untitled:Untitled-1", "file:///c%20d/e.pm"] {
            assert!(
                lsp_types::Location::try_from(WireLocation::new(valid.to_string(), range)).is_ok(),
                "{valid} must convert"
            );
        }

        // Established empirically against `lsp_types::Uri`, not assumed. Note that
        // `""` parses successfully as an empty relative reference, so it is not in
        // this list; see the module note on the empty-URI limitation.
        for invalid in ["not a uri", "  ", "://", "http://[", "%%"] {
            let result =
                lsp_types::Location::try_from(WireLocation::new(invalid.to_string(), range));
            assert!(result.is_err(), "{invalid:?} must be rejected");
        }
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
