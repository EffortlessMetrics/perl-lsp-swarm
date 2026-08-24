//! Type conversions between parser engine types and LSP protocol types.
//!
//! This module provides conversion utilities to translate between the internal parser
//! representation (from `perl-parser`) and the LSP protocol types (from `lsp-types`).
//!
//! # Conversion Categories
//!
//! - **Position & Range** - Converting between byte offsets and LSP Position/Range
//! - **Symbols** - Converting parser symbols to LSP SymbolInformation/DocumentSymbol
//! - **Diagnostics** - Converting parser errors to LSP Diagnostic messages
//! - **Completions** - Converting parser results to LSP CompletionItem
//! - **Locations** - Converting parser locations to LSP Location/LocationLink
//!
//! # UTF-16 Safety
//!
//! LSP uses UTF-16 code units for positions, while Rust strings use UTF-8.
//! All conversions must properly handle multi-byte characters and surrogate pairs.
//!
//! # Wire Types
//!
//! Wire types (`WirePosition`, `WireRange`, `WireLocation`) are the canonical types
//! for LSP JSON serialization. These types:
//!
//! - Use 0-based line numbers (as required by LSP)
//! - Use UTF-16 code units for character offsets
//! - Convert through byte offsets for correctness
//!
//! Always use wire types when serializing to LSP JSON, not engine types.

// Re-export wire types from perl-position-tracking (canonical implementation)
pub use perl_position_tracking::{WireLocation, WirePosition, WireRange};

use gen_lsp_types::Uri;

/// Validate a URI string and wrap it in the substrate's String-backed `Uri`,
/// mirroring the wire-crate's parse-or-fallback semantics.
fn substrate_uri(s: &str) -> Uri {
    match url::Url::parse(s) {
        Ok(parsed) => Uri(parsed.as_str().to_string()),
        Err(_) => {
            // Same last-resort fallback contract as the wire crate.
            for candidate in ["file:///unknown", "file:///", "about:blank"] {
                if url::Url::parse(candidate).is_ok() {
                    return Uri(candidate.to_string());
                }
            }
            Uri("http://localhost/0".to_string())
        }
    }
}

/// Convert a wire location into the selected substrate's `Location`.
///
/// The equivalent `From<WireLocation>` impl stays behind the doomed
/// `lsp-compat` edge of perl-position-tracking (#9632 owns its removal), so
/// the final adapter carries its own conversion onto the selected substrate
/// (#11802 matrix, LT02 population).
#[must_use]
pub fn wire_location_to_location(l: &WireLocation) -> gen_lsp_types::Location {
    gen_lsp_types::Location {
        uri: substrate_uri(&l.uri),
        range: gen_lsp_types::Range {
            start: gen_lsp_types::Position {
                line: l.range.start.line,
                character: l.range.start.character,
            },
            end: gen_lsp_types::Position {
                line: l.range.end.line,
                character: l.range.end.character,
            },
        },
    }
}
