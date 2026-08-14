//! LSP wire types: encoding-neutral structural protocol values.
//!
//! This module defines the protocol-facing position, range, and location types
//! used in the LSP wire format.
//!
//! ## Encoding neutrality
//!
//! [`WirePosition`] and [`WireRange`] are **structural** protocol values.
//! The `character` field carries the value transmitted over the wire; its meaning
//! (UTF-16 code units per the base LSP specification, UTF-8 code units per the
//! `positionEncoding` negotiated capability, or another agreed encoding) is
//! determined by the **active session encoding** held in the surrounding context,
//! not by these types themselves.
//!
//! This means:
//! - Round-tripping a `WirePosition` through serialization is always safe.
//! - Converting a `WirePosition` to/from a byte offset requires the caller to
//!   supply the source text **and** know the active encoding. The deprecated
//!   helpers in this module assumed UTF-16 unconditionally; callers should
//!   migrate to [`crate::offset_to_utf16_line_col`] /
//!   [`crate::utf16_line_col_to_offset`] (or the corresponding UTF-8 variants)
//!   and select the correct one based on the negotiated `positionEncoding`.
//!
//! ## URI validity
//!
//! [`WireLocation`] holds a raw URI string received from the wire.  Converting
//! it into an [`lsp_types::Location`] requires the URI to parse successfully.
//! Conversions that cannot return an error must not silently substitute a
//! different resource URI – that would name the wrong document.
//!
//! Use [`WireLocation::try_into_lsp_location`] or
//! `TryFrom<WireLocation> for lsp_types::Location` instead of the old
//! infallible `From` impl.

use serde::{Deserialize, Serialize};

/// A protocol-facing LSP position.
///
/// Both fields are 0-based.  The `character` field is a wire-level integer
/// whose interpretation (UTF-16 code units, UTF-8 code units, etc.) is
/// determined by the **active session position encoding**, not by this type.
///
/// The LSP base specification uses UTF-16 code units.  Servers that negotiate
/// `positionEncoding = "utf-8"` use UTF-8 code units instead.  Always consult
/// the owning context for the correct encoding before converting `character` to
/// or from a byte offset.
///
/// # Structural conversions
///
/// The `lsp-compat` feature provides `From<WirePosition> for lsp_types::Position`
/// and the reverse.  These conversions are **structural only** (field-for-field
/// copy); they carry no semantic guarantee about encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WirePosition {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based code-unit offset within the line.
    ///
    /// The unit (UTF-16 code unit, UTF-8 byte, …) is determined by the
    /// active session position encoding – see the module documentation.
    pub character: u32,
}

impl WirePosition {
    /// Creates a new wire position from explicit line and character values.
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    /// Converts a byte offset in `source` into an LSP wire position, **assuming
    /// UTF-16 character encoding**.
    ///
    /// # Deprecation
    ///
    /// This method unconditionally assumes UTF-16 code-unit counting, which is
    /// correct only when the active session encoding is UTF-16.  When the
    /// negotiated `positionEncoding` is `"utf-8"`, or when no encoding has been
    /// negotiated yet, this method produces wrong results.
    ///
    /// Migrate to [`crate::offset_to_utf16_line_col`] (UTF-16) or
    /// `crate::LineStartsCache::offset_to_position` (UTF-8) and select the
    /// correct variant based on the encoding held in the owning context.
    ///
    /// This method will be removed in v0.15.
    #[deprecated(
        since = "0.12.3",
        note = "Assumes UTF-16 unconditionally. Use offset_to_utf16_line_col or a \
                UTF-8 equivalent with the encoding from the owning session context. \
                Scheduled for removal in v0.15."
    )]
    pub fn from_byte_offset(source: &str, byte_offset: usize) -> Self {
        let (line, character) = crate::offset_to_utf16_line_col(source, byte_offset);
        Self { line, character }
    }

    /// Converts this LSP wire position back into a byte offset in `source`,
    /// **assuming UTF-16 character encoding**.
    ///
    /// # Deprecation
    ///
    /// This method unconditionally assumes UTF-16 code-unit counting.  See
    /// [`Self::from_byte_offset`] for the migration guidance.
    ///
    /// This method will be removed in v0.15.
    #[deprecated(
        since = "0.12.3",
        note = "Assumes UTF-16 unconditionally. Use utf16_line_col_to_offset or a \
                UTF-8 equivalent with the encoding from the owning session context. \
                Scheduled for removal in v0.15."
    )]
    pub fn to_byte_offset(&self, source: &str) -> usize {
        crate::utf16_line_col_to_offset(source, self.line, self.character)
    }
}

/// A protocol-facing LSP range with inclusive start and exclusive end.
///
/// Like [`WirePosition`], this is a structural protocol value.  The `character`
/// field inside each endpoint carries an encoding-dependent wire integer; see
/// [`WirePosition`] for details.
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

    /// Builds a wire range from start/end byte offsets in `source`, **assuming
    /// UTF-16 character encoding**.
    ///
    /// # Deprecation
    ///
    /// This method unconditionally assumes UTF-16 code-unit counting.  See
    /// [`WirePosition::from_byte_offset`] for the migration guidance.
    ///
    /// This method will be removed in v0.15.
    #[deprecated(
        since = "0.12.3",
        note = "Assumes UTF-16 unconditionally. Build WireRange::new from \
                encoding-aware positions instead. Scheduled for removal in v0.15."
    )]
    pub fn from_byte_offsets(source: &str, start_byte: usize, end_byte: usize) -> Self {
        #[allow(deprecated)]
        Self {
            start: WirePosition::from_byte_offset(source, start_byte),
            end: WirePosition::from_byte_offset(source, end_byte),
        }
    }

    /// Creates an empty (cursor) range at `pos`.
    pub fn empty(pos: WirePosition) -> Self {
        Self { start: pos, end: pos }
    }

    /// Creates a range that spans the full document, **assuming UTF-16 character
    /// encoding**.
    ///
    /// # Deprecation
    ///
    /// This method unconditionally assumes UTF-16 code-unit counting.  See
    /// [`WirePosition::from_byte_offset`] for the migration guidance.
    ///
    /// This method will be removed in v0.15.
    #[deprecated(
        since = "0.12.3",
        note = "Assumes UTF-16 unconditionally. Build the end position using an \
                encoding-aware helper. Scheduled for removal in v0.15."
    )]
    pub fn whole_document(source: &str) -> Self {
        #[allow(deprecated)]
        Self {
            start: WirePosition::new(0, 0),
            end: WirePosition::from_byte_offset(source, source.len()),
        }
    }
}

/// A protocol-facing location that combines a URI and a range.
///
/// The `uri` field holds the raw URI string as received from or sent over the
/// wire.  It is not validated on construction; callers that need an
/// [`lsp_types::Location`] must perform explicit, fallible conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireLocation {
    /// Document URI as received from the wire.
    pub uri: String,
    /// Range within the referenced document.
    pub range: WireRange,
}

impl WireLocation {
    /// Creates a new wire location.
    pub fn new(uri: String, range: WireRange) -> Self {
        Self { uri, range }
    }

    /// Converts this wire location into an [`lsp_types::Location`].
    ///
    /// Returns `Err` if the stored URI is not a valid URI.  The error includes
    /// the raw URI string so callers can log or propagate it.  This method
    /// never substitutes a different resource URI for an invalid one.
    ///
    /// # Errors
    ///
    /// Returns [`WireLocationError::InvalidUri`] when the URI field cannot be
    /// parsed.
    #[cfg(feature = "lsp-compat")]
    pub fn try_into_lsp_location(self) -> Result<lsp_types::Location, WireLocationError> {
        let uri = self
            .uri
            .parse::<lsp_types::Uri>()
            .map_err(|_| WireLocationError::InvalidUri { raw: self.uri.clone() })?;
        Ok(lsp_types::Location { uri, range: self.range.into() })
    }
}

/// Errors that can occur when converting a [`WireLocation`] to a protocol type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireLocationError {
    /// The URI stored in the [`WireLocation`] is not a valid URI.
    ///
    /// The `raw` field contains the original string for diagnostic purposes.
    InvalidUri {
        /// The raw URI string that failed to parse.
        raw: String,
    },
}

impl std::fmt::Display for WireLocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireLocationError::InvalidUri { raw } => {
                write!(f, "invalid URI in WireLocation: {raw:?}")
            }
        }
    }
}

impl std::error::Error for WireLocationError {}

// ── lsp-compat structural conversions ────────────────────────────────────────

#[cfg(feature = "lsp-compat")]
impl From<WirePosition> for lsp_types::Position {
    /// Structural field-for-field conversion.
    ///
    /// This is a pure field copy: `line → line`, `character → character`.
    /// No encoding transformation is performed; the caller is responsible for
    /// ensuring that both sides agree on what `character` means.
    fn from(p: WirePosition) -> Self {
        Self { line: p.line, character: p.character }
    }
}

#[cfg(feature = "lsp-compat")]
impl From<lsp_types::Position> for WirePosition {
    /// Structural field-for-field conversion.
    ///
    /// See [`From<WirePosition> for lsp_types::Position`] for the encoding
    /// caveat.
    fn from(p: lsp_types::Position) -> Self {
        Self { line: p.line, character: p.character }
    }
}

#[cfg(feature = "lsp-compat")]
impl From<WireRange> for lsp_types::Range {
    /// Structural conversion: delegates to the `WirePosition` conversion.
    fn from(r: WireRange) -> Self {
        Self { start: r.start.into(), end: r.end.into() }
    }
}

#[cfg(feature = "lsp-compat")]
impl From<lsp_types::Range> for WireRange {
    /// Structural conversion: delegates to the `WirePosition` conversion.
    fn from(r: lsp_types::Range) -> Self {
        Self { start: r.start.into(), end: r.end.into() }
    }
}

/// Fallible conversion from [`WireLocation`] to [`lsp_types::Location`].
///
/// Returns `Err(WireLocationError::InvalidUri { … })` when the URI field
/// cannot be parsed.  Never substitutes another resource URI.
#[cfg(feature = "lsp-compat")]
impl TryFrom<WireLocation> for lsp_types::Location {
    type Error = WireLocationError;

    fn try_from(l: WireLocation) -> Result<Self, Self::Error> {
        l.try_into_lsp_location()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must;

    // ── WirePosition structural behaviour ───────────────────────────────────

    #[test]
    fn wire_position_new_and_default() {
        let p = WirePosition::new(3, 7);
        assert_eq!(p.line, 3);
        assert_eq!(p.character, 7);

        let d = WirePosition::default();
        assert_eq!(d.line, 0);
        assert_eq!(d.character, 0);
    }

    #[test]
    fn wire_position_serde_round_trip() {
        let p = WirePosition::new(10, 42);
        let json = must(serde_json::to_string(&p));
        let back: WirePosition = must(serde_json::from_str(&json));
        assert_eq!(p, back);
    }

    // ── WireRange structural behaviour ──────────────────────────────────────

    #[test]
    fn wire_range_empty_cursor() {
        let pos = WirePosition::new(5, 3);
        let r = WireRange::empty(pos);
        assert_eq!(r.start, r.end);
        assert_eq!(r.start.line, 5);
    }

    #[test]
    fn wire_range_serde_round_trip() {
        let r = WireRange::new(WirePosition::new(0, 0), WirePosition::new(1, 10));
        let json = must(serde_json::to_string(&r));
        let back: WireRange = must(serde_json::from_str(&json));
        assert_eq!(r, back);
    }

    // ── WireLocation construction ────────────────────────────────────────────

    #[test]
    fn wire_location_stores_raw_uri() {
        let loc = WireLocation::new("file:///tmp/foo.pl".to_string(), WireRange::default());
        assert_eq!(loc.uri, "file:///tmp/foo.pl");
    }

    #[test]
    fn wire_location_accepts_invalid_uri_on_construction() {
        // Construction is always allowed; validation is deferred to conversion.
        let loc = WireLocation::new("not a uri!!".to_string(), WireRange::default());
        assert_eq!(loc.uri, "not a uri!!");
    }
}

#[cfg(all(test, feature = "lsp-compat"))]
mod lsp_compat_tests {
    use super::*;
    use perl_test_must::{must, must_err};

    // ── Structural position / range round-trips ──────────────────────────────

    #[test]
    fn wire_position_to_lsp_position_is_structural() {
        let wire = WirePosition::new(2, 8);
        let lsp: lsp_types::Position = wire.into();
        assert_eq!(lsp.line, 2);
        assert_eq!(lsp.character, 8);
    }

    #[test]
    fn lsp_position_to_wire_position_is_structural() {
        let lsp = lsp_types::Position { line: 5, character: 13 };
        let wire: WirePosition = lsp.into();
        assert_eq!(wire.line, 5);
        assert_eq!(wire.character, 13);
    }

    #[test]
    fn wire_position_structural_round_trip() {
        let original = WirePosition::new(7, 99);
        let lsp: lsp_types::Position = original.into();
        let back: WirePosition = lsp.into();
        assert_eq!(original, back, "structural round-trip must preserve every field");
    }

    #[test]
    fn wire_range_to_lsp_range_is_structural() {
        let wire = WireRange::new(WirePosition::new(1, 2), WirePosition::new(3, 4));
        let lsp: lsp_types::Range = wire.into();
        assert_eq!(lsp.start.line, 1);
        assert_eq!(lsp.start.character, 2);
        assert_eq!(lsp.end.line, 3);
        assert_eq!(lsp.end.character, 4);
    }

    #[test]
    fn lsp_range_to_wire_range_is_structural() {
        let lsp = lsp_types::Range {
            start: lsp_types::Position { line: 0, character: 5 },
            end: lsp_types::Position { line: 2, character: 0 },
        };
        let wire: WireRange = lsp.into();
        assert_eq!(wire.start.line, 0);
        assert_eq!(wire.start.character, 5);
        assert_eq!(wire.end.line, 2);
        assert_eq!(wire.end.character, 0);
    }

    // ── WireLocation → lsp_types::Location (fallible) ───────────────────────

    #[test]
    fn valid_uri_conversion_preserves_uri_and_range() {
        let wire = WireLocation::new(
            "file:///tmp/example.pl".to_string(),
            WireRange::new(WirePosition::new(1, 2), WirePosition::new(3, 4)),
        );

        let loc: lsp_types::Location = must(wire.try_into());

        assert_eq!(loc.uri.as_str(), "file:///tmp/example.pl");
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(loc.range.start.character, 2);
        assert_eq!(loc.range.end.line, 3);
        assert_eq!(loc.range.end.character, 4);
    }

    #[test]
    fn invalid_uri_returns_error_not_fallback() {
        let raw_uri = "not a uri".to_string();
        let wire = WireLocation::new(
            raw_uri.clone(),
            WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 1)),
        );

        let result: Result<lsp_types::Location, _> = wire.try_into();

        assert!(
            result.is_err(),
            "invalid URI must fail rather than substitute a fallback resource"
        );
        match must_err(result) {
            WireLocationError::InvalidUri { raw } => {
                assert_eq!(raw, raw_uri, "error must carry the original URI string");
            }
        }
    }

    #[test]
    fn invalid_uri_error_is_displayable() {
        let err = WireLocationError::InvalidUri { raw: "bad::uri".to_string() };
        let msg = err.to_string();
        assert!(msg.contains("bad::uri"), "display should include the raw URI");
    }

    #[test]
    fn try_into_lsp_location_method_is_consistent_with_tryfrom() {
        let wire = WireLocation::new(
            "file:///foo.pm".to_string(),
            WireRange::new(WirePosition::new(0, 0), WirePosition::new(2, 5)),
        );
        let via_method = must(wire.clone().try_into_lsp_location());
        let via_trait: lsp_types::Location = must(wire.try_into());

        assert_eq!(via_method.uri.as_str(), via_trait.uri.as_str());
        assert_eq!(via_method.range.start.line, via_trait.range.start.line);
    }

    #[test]
    fn wire_location_serde_preserves_uri_without_validation() {
        let wire = WireLocation::new(
            "file:///scripts/test.t".to_string(),
            WireRange::new(WirePosition::new(5, 0), WirePosition::new(5, 20)),
        );
        let json = must(serde_json::to_string(&wire));
        let back: WireLocation = must(serde_json::from_str(&json));
        assert_eq!(wire, back);
    }

    // ── Encoding-neutrality (caller-context encoding) ────────────────────────
    //
    // Wire types carry the integer that was on the wire.  The same integer
    // means different things depending on the session's positionEncoding.
    // These tests show that the structural round-trip preserves the integer
    // regardless of how the caller interprets it.

    #[test]
    fn wire_position_preserves_character_value_independent_of_encoding() {
        // Simulate a server that sent character=4 with positionEncoding=utf-8.
        let utf8_context_character: u32 = 4;
        let wire = WirePosition::new(0, utf8_context_character);

        // Round-trip through lsp_types and back – value must be preserved.
        let lsp: lsp_types::Position = wire.into();
        let back: WirePosition = lsp.into();
        assert_eq!(back.character, utf8_context_character);

        // Same integer, now interpreted as UTF-16 code units by a different
        // session – the structural value is identical.
        let utf16_context_character: u32 = 4;
        assert_eq!(back.character, utf16_context_character);
    }
}
