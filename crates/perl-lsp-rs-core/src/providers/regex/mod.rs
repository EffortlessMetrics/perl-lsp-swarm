//! Canonical regex read-only projections over parser-retained analysis.
//!
//! Navigation (#7030) and hover (#7048) share one generation-aligned table;
//! neither layer re-parses pattern text.

mod navigation;

pub use navigation::{
    RegexCaptureDefinitionLink, SourceByteSpan, definition_links_at, record_at_pattern_body_offset,
    reference_spans_at,
};
