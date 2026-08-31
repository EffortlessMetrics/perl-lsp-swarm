//! Native formatter public surface.
//!
//! The layout implementation remains in `native/implementation.rs`; the public
//! [`NativeFormatter`] is a thin lexical-safety facade over that engine. Typed
//! terminal outcomes live beside it so compatibility callers can retain
//! [`FormatResult`] while new callers consume an explicit outcome contract.
//!
//! [`native::terminal_sequence`] and [`native::edit_application`] are the
//! #8048 shift-left proof seam: pure, production-unwired policy and oracle
//! code whose integration owners are #10239/#10242.

#[path = "native/edit_application.rs"]
mod edit_application;
#[path = "native/facade.rs"]
mod facade;
#[path = "native/implementation.rs"]
mod implementation;
#[path = "native/line_ending.rs"]
mod line_ending;
#[path = "native/outcome.rs"]
mod outcome;
#[path = "native/terminal_sequence.rs"]
mod terminal_sequence;

pub use edit_application::{EditApplicationError, EditSpec, PositionEncoding, apply_edits_exact};
pub use facade::NativeFormatter;
pub use implementation::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatDiagnostic,
    FormatDiagnosticSeverity, FormatDoc, FormatResult, FormatterMode, KeywordSpacing,
    PerlFormatter, TextEdit, TextPosition, TextRange, TrailingComma,
};
pub use line_ending::inferred_line_ending;
pub use outcome::*;
pub use terminal_sequence::{
    FinalNewlinePolicy, PolicyOutcome, TerminalChange, TerminalNewlineEvidence, TerminalRun,
    TerminalSequence, apply_terminal_sequence_policy, source_convention, trailing_run,
};
