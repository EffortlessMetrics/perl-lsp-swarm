//! Typed incoming-message failure facts.
//!
//! Reader and wire-response policy remain outside this module. The public
//! paths in `transport` and `transport::incoming` forward these exact types.
//! Full dependency closure also requires the framing-error boundary (#13996);
//! separating these definitions does not make the surrounding crate neutral.

use super::framing::FramingError;
use crate::protocol::JsonRpcId;
use std::fmt;

/// Stage of the incoming pipeline that rejected a complete frame or header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IncomingMessageStage {
    /// `Content-Length` framing failed before a body could be decoded.
    Framing,
    /// The frame body was not valid UTF-8.
    Encoding,
    /// The UTF-8 body was not valid JSON.
    Json,
    /// The JSON value was not a current JSON-RPC request/notification shape.
    JsonRpcShape,
}

impl IncomingMessageStage {
    /// Stable log/label token for this stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Framing => "framing",
            Self::Encoding => "encoding",
            Self::Json => "json",
            Self::JsonRpcShape => "jsonrpc-shape",
        }
    }
}

impl fmt::Display for IncomingMessageStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed failure for one incoming frame. Ordinary diagnostics are payload-private.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IncomingMessageError {
    /// Header/frame extraction failed.
    Framing(FramingError),
    /// Body bytes were not valid UTF-8.
    InvalidUtf8 {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// First invalid UTF-8 offset.
        valid_up_to: usize,
    },
    /// Body was valid UTF-8 but not JSON.
    MalformedJson {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// serde_json 1-based line of the syntax error.
        line: usize,
        /// serde_json 1-based column of the syntax error.
        column: usize,
    },
    /// JSON parsed but was not a JSON-RPC object (scalar, `null`, or object
    /// without a current request/notification/response shape).
    InvalidMessageShape {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// Request id when the protocol legally exposes one.
        recoverable_id: Option<JsonRpcId>,
    },
    /// JSON parsed as an array. JSON-RPC batch is unsupported here.
    UnsupportedBatch {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
    },
    /// Object had a `method` but `jsonrpc` was missing or not a string.
    InvalidJsonRpc {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// Request id when the protocol legally exposes one.
        recoverable_id: Option<JsonRpcId>,
    },
}

impl IncomingMessageError {
    /// Pipeline stage that failed.
    #[must_use]
    pub const fn stage(&self) -> IncomingMessageStage {
        match self {
            Self::Framing(_) => IncomingMessageStage::Framing,
            Self::InvalidUtf8 { .. } => IncomingMessageStage::Encoding,
            Self::MalformedJson { .. } => IncomingMessageStage::Json,
            Self::InvalidMessageShape { .. }
            | Self::UnsupportedBatch { .. }
            | Self::InvalidJsonRpc { .. } => IncomingMessageStage::JsonRpcShape,
        }
    }

    /// Body or claimed-frame size when that metadata exists.
    #[must_use]
    pub const fn payload_bytes(&self) -> Option<usize> {
        match self {
            Self::Framing(FramingError::FrameTooLarge { len }) => Some(*len),
            Self::Framing(_) => None,
            Self::InvalidUtf8 { payload_bytes, .. }
            | Self::MalformedJson { payload_bytes, .. }
            | Self::InvalidMessageShape { payload_bytes, .. }
            | Self::UnsupportedBatch { payload_bytes }
            | Self::InvalidJsonRpc { payload_bytes, .. } => Some(*payload_bytes),
        }
    }

    /// Recovered JSON-RPC id when the value legally supplied one.
    #[must_use]
    pub fn recoverable_id(&self) -> Option<&JsonRpcId> {
        match self {
            Self::InvalidMessageShape { recoverable_id, .. }
            | Self::InvalidJsonRpc { recoverable_id, .. } => recoverable_id.as_ref(),
            Self::Framing(_)
            | Self::InvalidUtf8 { .. }
            | Self::MalformedJson { .. }
            | Self::UnsupportedBatch { .. } => None,
        }
    }
}

impl fmt::Display for IncomingMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "incoming framing error: {error}"),
            Self::InvalidUtf8 { payload_bytes, valid_up_to } => write!(
                f,
                "invalid UTF-8 in incoming JSON-RPC body (payload_bytes={payload_bytes}, valid_up_to={valid_up_to})"
            ),
            Self::MalformedJson { payload_bytes, line, column } => write!(
                f,
                "malformed JSON in incoming JSON-RPC body (payload_bytes={payload_bytes}, line={line}, column={column})"
            ),
            Self::InvalidMessageShape { payload_bytes, .. } => {
                write!(f, "incoming JSON is not a JSON-RPC object (payload_bytes={payload_bytes})")
            }
            Self::UnsupportedBatch { payload_bytes } => {
                write!(f, "unsupported JSON-RPC batch array (payload_bytes={payload_bytes})")
            }
            Self::InvalidJsonRpc { payload_bytes, .. } => write!(
                f,
                "incoming JSON-RPC object is missing a string jsonrpc field (payload_bytes={payload_bytes})"
            ),
        }
    }
}

impl fmt::Debug for IncomingMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => f.debug_tuple("Framing").field(error).finish(),
            Self::InvalidUtf8 { payload_bytes, valid_up_to } => f
                .debug_struct("InvalidUtf8")
                .field("payload_bytes", payload_bytes)
                .field("valid_up_to", valid_up_to)
                .finish(),
            Self::MalformedJson { payload_bytes, line, column } => f
                .debug_struct("MalformedJson")
                .field("payload_bytes", payload_bytes)
                .field("line", line)
                .field("column", column)
                .finish(),
            Self::InvalidMessageShape { payload_bytes, recoverable_id } => f
                .debug_struct("InvalidMessageShape")
                .field("payload_bytes", payload_bytes)
                .field("recoverable_id", recoverable_id)
                .finish(),
            Self::UnsupportedBatch { payload_bytes } => {
                f.debug_struct("UnsupportedBatch").field("payload_bytes", payload_bytes).finish()
            }
            Self::InvalidJsonRpc { payload_bytes, recoverable_id } => f
                .debug_struct("InvalidJsonRpc")
                .field("payload_bytes", payload_bytes)
                .field("recoverable_id", recoverable_id)
                .finish(),
        }
    }
}

impl std::error::Error for IncomingMessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(error) => Some(error),
            Self::InvalidUtf8 { .. }
            | Self::MalformedJson { .. }
            | Self::InvalidMessageShape { .. }
            | Self::UnsupportedBatch { .. }
            | Self::InvalidJsonRpc { .. } => None,
        }
    }
}
