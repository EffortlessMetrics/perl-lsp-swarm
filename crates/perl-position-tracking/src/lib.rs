//! Position tracking, byte-span types, and encoding-neutral LSP wire values.
//!
//! This crate provides foundational types for source location tracking in the
//! Perl LSP ecosystem:
//!
//! - [`ByteSpan`]: Byte-offset based spans for parser/AST use
//! - [`LineStartsCache`]: Efficient line index for offset-to-position conversion
//! - [`WirePosition`]/[`WireRange`]: Encoding-neutral LSP protocol position types
//! - [`WireLocation`]: URI + range; URI conversion is explicitly fallible
//!
//! ## Wire types and position encoding
//!
//! [`WirePosition`] and [`WireRange`] are **structural** protocol values.
//! The `character` field carries the integer transmitted over the wire; its
//! meaning (UTF-16 code units per the base LSP specification, UTF-8 code
//! units when `positionEncoding = "utf-8"` is negotiated, etc.) is determined
//! by the **active session encoding** in the surrounding context.
//!
//! Converting between a wire `character` value and a byte offset therefore
//! requires knowing both the source text **and** the active encoding.  Use
//! [`offset_to_utf16_line_col`] / [`utf16_line_col_to_offset`] for UTF-16
//! sessions or [`LineStartsCache::offset_to_position`] for UTF-8 sessions.
//! Do not call the deprecated `WirePosition::from_byte_offset` /
//! `to_byte_offset` helpers, which assumed UTF-16 unconditionally.
//!
//! ## URI validity
//!
//! [`WireLocation`] stores a raw URI string.  Converting it to an
//! `lsp_types::Location` is **fallible**: use [`WireLocation::try_into_lsp_location`]
//! or `TryFrom<WireLocation> for lsp_types::Location` (both require the
//! `lsp-compat` feature).  Invalid URIs produce [`wire::WireLocationError`]
//! rather than silently naming the wrong resource.
//!
//! # Example
//!
//! ```
//! use perl_position_tracking::{ByteSpan, LineStartsCache};
//!
//! let source = "line 1\nline 2\nline 3";
//! let cache = LineStartsCache::new(source);
//!
//! // Create a span covering "line 2"
//! let span = ByteSpan::new(7, 13);
//! assert_eq!(span.slice(source), "line 2");
//!
//! // Convert to line/column for LSP (UTF-8 offsets via the cache)
//! let (line, col) = cache.offset_to_position(source, span.start);
//! assert_eq!(line, 1); // 0-indexed
//! assert_eq!(col, 0);
//! ```

#![warn(missing_docs)]

pub use convert::{offset_to_utf16_line_col, utf16_line_col_to_offset};
pub use line_index::{LineIndex, LineStartsCache};
pub use mapper::{
    LineEnding, PositionMapper, apply_edit_utf8, json_to_position, last_line_column_utf8,
    newline_count, position_to_json,
};
pub use position::{Position, Range};
pub use span::{ByteSpan, SourceLocation};

mod convert;
mod line_index;
pub mod mapper;
mod position;
mod span;

pub mod wire;
pub use wire::{WireLocation, WireLocationError, WirePosition, WireRange};
