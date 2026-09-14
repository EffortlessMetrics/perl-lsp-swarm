//! LSP transport layer for perl-lsp.
//!
//! This module provides the transport layer implementation for the Perl Language Server,
//! handling message framing according to the LSP Base Protocol specification.
//!
//! Previously the standalone `perl-lsp-transport` crate; absorbed into
//! `perl-lsp-rs-core::transport` in Wave G3 (#4535).

pub mod framing;
pub mod incoming;

pub use framing::{
    ContentLengthFramer, FramingError, MAX_FRAME_SIZE, frame, log_response, write_message,
    write_notification,
};
pub use incoming::{
    ContentLengthMessageReader, IncomingMessageError, IncomingMessageStage, decode_incoming_body,
    read_message, read_message_outcome,
};
