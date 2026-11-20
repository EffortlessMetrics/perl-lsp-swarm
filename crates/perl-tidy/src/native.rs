//! Native formatter public surface.
//!
//! The implementation remains in `native/implementation.rs`; typed terminal outcomes live
//! beside it so compatibility callers can retain [`FormatResult`] while new
//! callers consume an explicit outcome contract.
//!
//! Source admission and edit application use the exact predecessor bytes.
//! Protocol positions are converted by callers before reaching this library.

#[path = "native/bytes.rs"]
mod bytes;
#[path = "native/implementation.rs"]
mod implementation;
pub use bytes::*;
#[path = "native/line_ending.rs"]
mod line_ending;
#[path = "native/outcome.rs"]
mod outcome;
#[path = "native/terminal_sequence.rs"]
mod terminal_sequence;

pub use implementation::counters::{
    COUNTER_CLOCK_TAG, COUNTER_SCHEMA_V1, MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1,
    NativePipelineCounters, PipelineCollectorScope, SCALING_ABSOLUTE_SLACK_V1,
    SCALING_RATIO_BOUND_V1, exceeds_replacement_envelope_v1,
};
pub use implementation::*;
pub use line_ending::{inferred_line_ending, inferred_line_ending_at};
pub use outcome::*;
pub use terminal_sequence::{
    FinalNewlinePolicy, PolicyOutcome, TerminalChange, TerminalNewlineEvidence, TerminalRun,
    TerminalSequence, apply_terminal_sequence_policy, source_convention, trailing_run,
};
