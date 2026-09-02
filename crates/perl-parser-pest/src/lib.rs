#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Legacy Pest-based Perl parser (v2).
//!
//! This crate provides a pure Rust Perl parser built with the Pest parser generator.
//! It outputs tree-sitter compatible S-expressions and requires no C dependencies.
//!
//! **Note:** This is maintained as a learning tool and historical reference.
//! For production parsing and LSP, use `perl-parser` (v3).

//!
//! # Example
//!
//! ```ignore
//! use perl_parser_pest::PureRustPerlParser;
//!
//! let mut parser = PureRustPerlParser::new();
//! let code = r#"
//!     sub hello {
//!         my $name = shift;
//!         print "Hello, $name!\n";
//!     }
//! "#;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ast = parser.parse(code)?;
//! # Ok(())
//! # }
//! let sexp = parser.to_sexp(&ast);
//! println!("{}", sexp);
//! ```
//!
//! # Architecture
//!
//! The parser uses a three-stage pipeline:
//! 1. **Pest Parsing**: PEG grammar processes input into parse tree
//! 2. **AST Building**: Type-safe AST construction with position tracking
//! 3. **S-Expression Output**: Tree-sitter compatible format generation
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

pub mod error;
pub mod heredoc;
pub mod outcome;
pub mod pratt_parser;
pub mod pure_rust_parser;
pub mod sexp_formatter;

// Re-export the main types for convenience
pub use error::{ParseError, ParseResult};
pub use heredoc::{
    HeredocCapture, HeredocDefect, HeredocDelimiterForm, HeredocScan, MAX_HEREDOC_BODY_BYTES,
    MAX_HEREDOC_DEPTH,
};
pub use outcome::{
    OutcomeError, PARSE_OUTCOME_SCHEMA, PARSER_FAILURE_SCHEMA, ParseAttempt, ParseCompleteness,
    ParseDiagnostic, ParseDiagnosticKind, ParseOutcome, ParseOutcomeVocabulary, ParserFailure,
    ParserFailureKind, RecoveryAction, STRICT_PARSE_ERROR_SCHEMA, SourceLineColumn, SourceRange,
    StrictParseError,
};
pub use pratt_parser::PrattParser;
pub use pure_rust_parser::{AstNode, PerlParser, PureRustPerlParser};
pub use sexp_formatter::SexpFormatter;
