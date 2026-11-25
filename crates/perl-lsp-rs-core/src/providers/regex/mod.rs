//! Canonical regex-family hover projection (#7048).
//!
//! Presentation over parser-retained [`perl_parser_core::RegexAnalysisTable`] records
//! (#7018). No regex rescanning or message parsing lives here.

mod hover;

pub use hover::hover_projection_at;
