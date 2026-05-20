//! Single-line Perl import head parsing and literal require/import extraction.
//!
//! Parse a single source line that starts with `use` or `require` and return
//! the first import token with stable byte offsets.
//!
//! Also provides [`extract_require_import_symbols`], a text-level extractor
//! that recognises the literal `require Module; Module->import(...)` adjacency
//! pattern in multi-line source without requiring AST construction.

mod export_tags;
mod import_head;
mod require_import_extractor;

pub use export_tags::resolve_known_export_tag;
pub use import_head::{
    DispatchSemantics, ImportBehavior, ImportListForm, LoadTiming, ModuleImportHead,
    ModuleImportKind, RequireForm, parse_module_import_head,
};
pub use require_import_extractor::{RequireImportEntry, extract_require_import_symbols};
