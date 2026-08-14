//! Token stream and trivia utilities for parser workflows.
//!
//! This module hosts parser-facing token infrastructure that depends on parser
//! and AST types and therefore cannot live in `perl-lexer` without creating a
//! dependency cycle:
//!
//! - [`token_stream`] — buffered [`TokenStream`](token_stream::TokenStream)
//!   over the raw lexer with lookahead, trivia skipping, and mode resets.
//! - [`trivia`] — low-level whitespace/comment/POD values and compatibility lexer.
//! - [`trivia_parser`] — the sole public parser-backed trivia surface. It uses
//!   the canonical [`crate::Parser`] for AST and recovery output and retains
//!   exact source plus a source-ordered trivia inventory.
//!
//! Complete per-node source geometry is deliberately not claimed here; #7101
//! owns that follow-on contract.

/// Buffered token stream over the raw lexer with trivia skipping.
pub mod token_stream;
/// Token wrapper utilities for preserving original lexemes.
pub use perl_lexer::tokenizer::token_wrapper;
/// Trivia tokens and compatibility lexing utilities.
pub mod trivia;
/// Canonical parser-backed trivia preservation surface.
pub mod trivia_parser;
