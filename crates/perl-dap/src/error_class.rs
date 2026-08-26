//! Fixed-origin DAP operational error classification (#8739).
//!
//! Domain error types keep their own shape and typed sources. This module owns
//! the #4979 category policy for variants whose origin is already determined by
//! the variant itself.
//!
//! Context-dependent stack/variable parse variants are not classified here.
//! [`crate::stack::StackParseError::UnrecognizedFormat`] and
//! [`crate::variables::VariableParseError`]'s unrecognized/unterminated variants
//! wait on the debugger-output origin wrapper in #8746.

use crate::eval::ValidationError;
use crate::peer_protocol::PeerFrameError;
use crate::security::SecurityError;
use crate::stack::FixedOriginStackParseError;
use crate::variables::FixedOriginVariableParseError;
use perl_parser_core::{ErrorCategory, ErrorClass};

const fn assert_error_class<T: ErrorClass>() {}

const _: () = {
    assert_error_class::<ValidationError>();
    assert_error_class::<SecurityError>();
    assert_error_class::<PeerFrameError>();
    assert_error_class::<FixedOriginStackParseError<'static>>();
    assert_error_class::<FixedOriginVariableParseError<'static>>();
};

impl ErrorClass for ValidationError {
    fn error_class(&self) -> ErrorCategory {
        // Every variant is a safe-evaluation policy refusal of user/client input.
        match self {
            Self::DangerousOperation(_)
            | Self::AssignmentOperator(_)
            | Self::IncrementDecrement
            | Self::Backticks
            | Self::RegexMutation(_)
            | Self::ContainsNewlines => ErrorCategory::UserError,
        }
    }
}

impl ErrorClass for SecurityError {
    fn error_class(&self) -> ErrorCategory {
        match self {
            Self::PathTraversalAttempt(_)
            | Self::PathOutsideWorkspace(_)
            | Self::SymlinkOutsideWorkspace(_)
            | Self::InvalidPathCharacters
            | Self::InvalidExpression => ErrorCategory::UserError,
            Self::ExcessiveTimeout(_) => ErrorCategory::ResourceLimit,
        }
    }
}

impl ErrorClass for PeerFrameError {
    fn error_class(&self) -> ErrorCategory {
        match self {
            Self::Framing(error) => error.error_class(),
            Self::Json(_) => ErrorCategory::Protocol,
        }
    }
}

impl ErrorClass for FixedOriginStackParseError<'_> {
    fn error_class(&self) -> ErrorCategory {
        match self {
            Self::RegexError(_) => ErrorCategory::Bug,
        }
    }
}

impl ErrorClass for FixedOriginVariableParseError<'_> {
    fn error_class(&self) -> ErrorCategory {
        match self {
            Self::MaxDepthExceeded(_) => ErrorCategory::ResourceLimit,
            Self::RegexError(_) => ErrorCategory::Bug,
        }
    }
}
