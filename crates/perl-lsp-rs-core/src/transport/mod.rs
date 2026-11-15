//! LSP transport layer for perl-lsp.
//!
//! This module provides the transport layer implementation for the Perl Language Server,
//! handling message framing according to the LSP Base Protocol specification.
//!
//! Previously the standalone `perl-lsp-transport` crate; absorbed into
//! `perl-lsp-rs-core::transport` in Wave G3 (#4535).

pub mod framing;
mod framing_error;
pub mod incoming;
mod incoming_error;
mod incoming_message;

pub use framing::{
    ContentLengthFramer, FramingError, MAX_FRAME_SIZE, frame, log_response, write_message,
    write_notification,
};
pub use incoming::{
    ContentLengthMessageReader, IncomingMessageError, IncomingMessageStage, decode_incoming_body,
    read_incoming_message_outcome, read_message, read_message_outcome,
};
pub use incoming_message::{
    IncomingMessage, IncomingNotification, IncomingRequest, IncomingResponse,
    IncomingResponsePayload, decode_incoming_message,
};
