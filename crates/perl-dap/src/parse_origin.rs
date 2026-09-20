//! Caller-supplied origin for stack and variable parser inputs (#8746).
//!
//! Classification of context-dependent parse failures (`UnrecognizedFormat`,
//! unterminated string/collection) is determined by this origin, not by the
//! payload text. Fixed-origin variants (resource bounds, internal constant
//! regex failure) keep their #8739 categories across every origin.
//!
//! Callers must supply origin. Parsers must not infer it from `Display`,
//! `Debug`, debugger prompts, or message substrings.

use perl_parser_core::ErrorCategory;
use std::error::Error as StdError;
use std::fmt;

/// How the caller obtained the bytes being parsed.
///
/// This enum is exhaustive and has no [`Default`]. A missing origin is a
/// type error, not a silent fallback to [`ErrorCategory::Protocol`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebuggerOutputOrigin {
    /// Negotiated debugger-control channel payload (for example a framed `T` dump).
    DebuggerControlPayload,
    /// Negotiated Perl Debugger Peer Protocol payload.
    PeerProtocolPayload,
    /// Best-effort scan of debuggee stdout/stderr or mixed recent output.
    BestEffortDebuggeeOutput,
    /// Test fixture or instrument input that is not a live peer contract.
    FixtureOrInstrumentInput,
}

impl DebuggerOutputOrigin {
    /// Stable machine token for this origin.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DebuggerControlPayload => "debugger_control_payload",
            Self::PeerProtocolPayload => "peer_protocol_payload",
            Self::BestEffortDebuggeeOutput => "best_effort_debuggee_output",
            Self::FixtureOrInstrumentInput => "fixture_or_instrument_input",
        }
    }

    /// Category for origin-ambiguous parse variants once the caller has named origin.
    ///
    /// Resource limits and internal regex failures do not use this table.
    pub(crate) const fn context_dependent_category(self) -> ErrorCategory {
        match self {
            Self::DebuggerControlPayload | Self::PeerProtocolPayload => ErrorCategory::Protocol,
            Self::BestEffortDebuggeeOutput | Self::FixtureOrInstrumentInput => {
                ErrorCategory::Advisory
            }
        }
    }
}

/// Operation, session, and suspension identity preserved across wrapping.
///
/// All fields are optional so a caller that only knows origin can still wrap
/// input. Empty identity is not a default origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ParseIdentity {
    operation_id: Option<u64>,
    session_id: Option<u64>,
    suspension_generation: Option<u64>,
}

impl ParseIdentity {
    /// Empty identity; origin must still be supplied separately.
    #[must_use]
    pub const fn new() -> Self {
        Self { operation_id: None, session_id: None, suspension_generation: None }
    }

    /// Attaches a caller-owned operation identity.
    #[must_use]
    pub const fn with_operation_id(mut self, operation_id: u64) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    /// Attaches a caller-owned session identity.
    #[must_use]
    pub const fn with_session_id(mut self, session_id: u64) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Attaches a caller-owned stopped-suspension generation.
    #[must_use]
    pub const fn with_suspension_generation(mut self, suspension_generation: u64) -> Self {
        self.suspension_generation = Some(suspension_generation);
        self
    }

    /// Operation identity, if the caller supplied one.
    #[must_use]
    pub const fn operation_id(self) -> Option<u64> {
        self.operation_id
    }

    /// Session identity, if the caller supplied one.
    #[must_use]
    pub const fn session_id(self) -> Option<u64> {
        self.session_id
    }

    /// Suspension generation, if the caller supplied one.
    #[must_use]
    pub const fn suspension_generation(self) -> Option<u64> {
        self.suspension_generation
    }

    /// Attaches a DAP/request identity when it is a non-negative integer.
    #[must_use]
    pub fn with_operation_id_from_i64(self, operation_id: i64) -> Self {
        match u64::try_from(operation_id) {
            Ok(id) => self.with_operation_id(id),
            Err(_) => self,
        }
    }
}

/// Parser input with caller-supplied origin and optional identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginatedParseInput<'a> {
    origin: DebuggerOutputOrigin,
    identity: ParseIdentity,
    text: &'a str,
}

impl<'a> OriginatedParseInput<'a> {
    /// Wraps `text` with a required origin and optional identity.
    #[must_use]
    pub const fn new(origin: DebuggerOutputOrigin, identity: ParseIdentity, text: &'a str) -> Self {
        Self { origin, identity, text }
    }

    /// Origin the caller declared for these bytes.
    #[must_use]
    pub const fn origin(self) -> DebuggerOutputOrigin {
        self.origin
    }

    /// Identity the caller attached to this parse.
    #[must_use]
    pub const fn identity(self) -> ParseIdentity {
        self.identity
    }

    /// Underlying payload text. Parsers must not infer origin from this.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }

    /// Same origin and identity with a different text slice (for example one line).
    #[must_use]
    pub const fn with_text<'b>(self, text: &'b str) -> OriginatedParseInput<'b> {
        OriginatedParseInput { origin: self.origin, identity: self.identity, text }
    }

    /// Attaches this origin and identity to a parse error without inspecting text.
    #[must_use]
    pub const fn attach<E>(self, source: E) -> OriginatedParseError<E> {
        OriginatedParseError { origin: self.origin, identity: self.identity, source }
    }
}

/// Parse error carrying the origin and identity that produced it.
///
/// [`Display`] delegates to the underlying parse/regex source so wrapping does
/// not invent a second message. Classification uses origin plus variant, never
/// this rendered text.
#[derive(Debug)]
pub struct OriginatedParseError<E> {
    origin: DebuggerOutputOrigin,
    identity: ParseIdentity,
    source: E,
}

impl<E> OriginatedParseError<E> {
    /// Origin the caller declared for the failed parse.
    #[must_use]
    pub const fn origin(&self) -> DebuggerOutputOrigin {
        self.origin
    }

    /// Identity preserved from the originated input.
    #[must_use]
    pub const fn identity(&self) -> ParseIdentity {
        self.identity
    }

    /// Underlying parse/regex error.
    #[must_use]
    pub const fn parse_error(&self) -> &E {
        &self.source
    }
}

impl<E: fmt::Display> fmt::Display for OriginatedParseError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(f)
    }
}

impl<E: StdError + 'static> StdError for OriginatedParseError<E> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}
