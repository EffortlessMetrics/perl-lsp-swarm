//! Semantic analysis, symbol extraction, and type inference for Perl.
//!
//! Walks a parsed AST to build scoped symbol tables, resolve declarations and
//! references, and perform lightweight type inference. The resulting semantic
//! model powers go-to-definition, find-references, and diagnostic providers
//! in the LSP server.

#![deny(unsafe_code)]
#![deny(unreachable_pub)]
#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
#![cfg_attr(
    test,
    allow(
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::print_stderr,
        clippy::print_stdout
    )
)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![allow(
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::wildcard_imports,
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::if_not_else,
    clippy::struct_excessive_bools,
    clippy::items_after_statements,
    clippy::return_self_not_must_use,
    clippy::unused_self,
    clippy::only_used_in_recursion,
    clippy::items_after_test_module,
    clippy::while_let_loop,
    clippy::single_range_in_vec_init,
    clippy::arc_with_non_send_sync,
    clippy::needless_range_loop,
    clippy::result_large_err,
    clippy::if_same_then_else,
    clippy::should_implement_trait,
    clippy::manual_flatten,
    clippy::needless_raw_string_hashes,
    clippy::single_char_pattern,
    clippy::uninlined_format_args
)]

pub use analysis::document_analysis::DocumentDiagnosticAnalysis;
pub use analysis::index as workspace_index;
pub use perl_parser_core::{Node, NodeKind, SourceLocation};
pub use perl_parser_core::{
    Parser, ast, edit, error, parser, parser_context, position, pragma_tracker, quote_parser, util,
};

/// Semantic analysis, symbol extraction, and type inference.
pub mod analysis;

pub use analysis::argument_cardinality::known_scalar_argument_count;
pub use analysis::class_model;
#[cfg(not(target_arch = "wasm32"))]
pub use analysis::declaration;
#[cfg(not(target_arch = "wasm32"))]
pub use analysis::index;
pub use analysis::receiver_facts;
pub use analysis::scope_analyzer;
pub use analysis::semantic;
pub use analysis::symbol;
pub use analysis::type_facts;
pub use analysis::type_inference;

pub use analysis::test2::{
    ResolvedImport, Test2Facts, Test2FactsAnalysis, Test2Pragmas, is_test2_bundle, is_test2_module,
    module_default_exports, resolve_import,
};
pub use analysis::unreachable_code::{UnreachableStatement, find_unreachable_statements};
pub use analysis::unreachable_code_disposition::{
    PL406_DISPOSITION_SCHEMA_VERSION, PL406_DISPOSITIONS, Pl406ConditionalRule, Pl406Disposition,
    Pl406ProofCeiling, Pl406Scope, Pl406SemanticClass, all_pl406_dispositions, pl406_disposition,
    pl406_disposition_of,
};
