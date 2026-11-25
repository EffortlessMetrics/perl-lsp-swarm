//! LSP inlay hints provider for Perl
//!
//! This crate provides inlay hint generation for type information.
//!
//! ## Features
//!
//! - Type inference
//! - Parameter hints
//! - LSP protocol compatibility
//!
//! ## Usage
//!
//! ```rust,ignore
//! use perl_lsp_inlay_hints::InlayHintsProvider;
//!
//! let provider = InlayHintsProvider::new();
//! let hints = provider.generate_hints(&ast, source, &symbol_table)?;
//! ```

mod impl_inlay_hints;
mod output_builtin_roles;
mod resolve_subject;

pub use impl_inlay_hints::{
    InlayHint, InlayHintKind, extract_param_names, inlay_hint_label_str, trivial_type_hints,
};
pub use output_builtin_roles::{
    InlayHintsProvider, parameter_hints, parameter_hints_with_resolver,
};
pub use resolve_subject::InlayHintResolveSubjectV1;
