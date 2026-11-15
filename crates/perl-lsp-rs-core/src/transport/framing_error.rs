//! Language-neutral Content-Length framing failure facts (#13996).
//!
//! Framing mechanics and limits remain in `framing`; Perl operational
//! categories belong to `protocol::framing_error_class`, not this type.

use std::fmt;

/// Framing errors for `Content-Length` transport parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramingError {
    /// Header bytes could not be interpreted as a valid frame header.
    InvalidHeader,
    /// Header bytes were not valid UTF-8.
    InvalidHeaderUtf8,
    /// `Content-Length` header was missing from a complete header block.
    MissingContentLength,
    /// `Content-Length` value was not a valid non-negative integer.
    InvalidContentLength,
    /// `Content-Length` exceeded the maximum frame size enforced by the framer.
    FrameTooLarge {
        /// The actual frame size that was too large.
        len: usize,
    },
}

impl fmt::Display for FramingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHeader => write!(f, "invalid Content-Length header"),
            Self::InvalidHeaderUtf8 => write!(f, "header contains invalid UTF-8"),
            Self::MissingContentLength => write!(f, "missing Content-Length header"),
            Self::InvalidContentLength => write!(f, "invalid Content-Length value"),
            Self::FrameTooLarge { len } => write!(f, "frame too large: {len} bytes"),
        }
    }
}

impl std::error::Error for FramingError {}
