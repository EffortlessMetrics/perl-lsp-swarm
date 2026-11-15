//! Shared dancer2 hooks contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::dancer2_hooks`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::hook::HookDeclaration;

use crate::FileId;

/// Versioned identity of the reviewed Dancer2 hook/alias contract.
///
/// The alias table and canonical name set follow Dancer2 1.1.1
/// (`Dancer2::Core::App::hook_aliases`, `Dancer2::Core::App::supported_hooks`,
/// and the `Dancer2::Core::Hook` name coerce); the workspace
/// `dancer2_skeleton` fixture carries `1.1.1`.
pub const DANCER2_HOOK_CONTRACT_VERSION: &str = "dancer2-hooks.1-1.v1";

/// Hook keyword of the reviewed Dancer2 DSL contract.
pub const DANCER2_HOOK_KEYWORD: &str = "hook";

/// Reviewed alias table: literal hook name → canonical hook name.
///
/// Verbatim from Dancer2 1.1.1 `Dancer2::Core::App::hook_aliases`, including
/// the Dancer 1 compatibility spellings. The `before_template` entry is the
/// composed two-stage normalization (the `Dancer2::Core::Hook` name coerce to
/// `before_template_render`, then the alias to `engine.template.before_render`).
pub const DANCER2_HOOK_ALIASES: &[(&str, &str)] = &[
    ("before", "core.app.before_request"),
    ("before_request", "core.app.before_request"),
    ("after", "core.app.after_request"),
    ("after_request", "core.app.after_request"),
    ("init_error", "core.error.init"),
    ("before_error", "core.error.before"),
    ("after_error", "core.error.after"),
    ("on_route_exception", "core.app.route_exception"),
    ("before_file_render", "core.app.before_file_render"),
    ("after_file_render", "core.app.after_file_render"),
    ("before_handler_file_render", "handler.file.before_render"),
    ("after_handler_file_render", "handler.file.after_render"),
    // Compatibility spellings from Dancer 1.
    ("before_error_render", "core.error.before"),
    ("after_error_render", "core.error.after"),
    ("before_error_init", "core.error.init"),
    // Engine hooks reachable through reviewed aliases.
    ("before_template_render", "engine.template.before_render"),
    ("after_template_render", "engine.template.after_render"),
    ("before_layout_render", "engine.template.before_layout_render"),
    ("after_layout_render", "engine.template.after_layout_render"),
    ("before_serializer", "engine.serializer.before"),
    ("after_serializer", "engine.serializer.after"),
    // `Dancer2::Core::Hook` name coerce followed by the alias table:
    // `before_template` → `before_template_render` →
    // `engine.template.before_render`.
    ("before_template", "engine.template.before_render"),
];

/// Reviewed canonical hook names.
///
/// The application hook positions of Dancer2 1.1.1
/// (`Dancer2::Core::App::supported_hooks`) plus every canonical alias target
/// of [`DANCER2_HOOK_ALIASES`] (engine/serializer/handler names are valid
/// statically-nameable hook positions upstream; their registration depends on
/// engine candidates but their identity is version-authoritative).
pub const DANCER2_CANONICAL_HOOK_NAMES: &[&str] = &[
    "core.app.before_request",
    "core.app.after_request",
    "core.app.route_exception",
    "core.app.before_file_render",
    "core.app.after_file_render",
    "core.error.before",
    "core.error.after",
    "core.error.init",
    "engine.template.before_render",
    "engine.template.after_render",
    "engine.template.before_layout_render",
    "engine.template.after_layout_render",
    "engine.serializer.before",
    "engine.serializer.after",
    "handler.file.before_render",
    "handler.file.after_render",
];

/// Reviewed canonical hook positions whose semantics establish request
/// context (#13604).
///
/// These are the application hook positions that Dancer2 1.1.1 dispatches
/// from `Dancer2::Core::App::_dispatch_route`/`response_internal` with the
/// current request already installed on the app, so the request-scoped DSL
/// keywords (`request`, `params`, `redirect`, `cookie`, `session`, ...) are
/// meaningful inside the handler body.
///
/// Every other reviewed canonical position is deliberately absent rather
/// than assumed:
///
/// - `core.error.*` can run for an error constructed outside any request;
/// - `engine.template.*` runs wherever `template` is called, including
///   outside a request;
/// - `engine.serializer.*` and `handler.file.*`/`core.app.*_file_render` are
///   engine/handler positions whose request scoping depends on the calling
///   site rather than on the hook position itself.
///
/// Absence here means *not established*, never *proven absent*: a consumer
/// must not offer request-scoped keywords in those positions, and must
/// equally not report using one there as wrong.
pub const DANCER2_REQUEST_CONTEXT_HOOKS: &[&str] =
    &["core.app.before_request", "core.app.after_request", "core.app.route_exception"];

/// One source-extracted Dancer2 hook declaration awaiting minting.
///
/// Produced by the AST extractor in
/// `perl_semantic_analyzer::analysis::dancer2_hooks`; this carrier adds the
/// package/file/declaration identity around the canonical payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2HookDeclaration {
    /// Package the declaration appears in (activation scope).
    pub package: Option<String>,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Full declaration range (keyword start to last operand end).
    pub declaration_start_byte: u32,
    /// End of the full declaration range.
    pub declaration_end_byte: u32,
    /// Canonical hook payload (name/handler).
    pub hook: HookDeclaration,
}
