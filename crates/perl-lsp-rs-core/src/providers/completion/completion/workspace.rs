//! Workspace symbol completion for Perl
//!
//! Provides completion for symbols from other files in the workspace using the workspace index.
//! Includes module name completion for `use`/`require` statements, workspace-aware method
//! completion for `->` expressions, and general cross-file symbol completion.

use super::{
    context::CompletionContext,
    items::{CompletionItem, CompletionItemKind, InsertTextFormat},
};
use crate::providers::completion::module_scan_cache::{ModuleCompletionScanCache, ScanCacheKey};
use perl_module::module_name_to_path;
use perl_semantic_analyzer::{
    semantic::SemanticModel, symbol::SymbolTable, type_inference::TypeInferenceEngine,
};

pub(super) use super::receiver::{
    ReceiverEvidence, classify_receiver_with_symbol_table, detail_with_evidence,
};
#[cfg(test)]
pub(super) use super::receiver::{
    classify_receiver, classify_text_pattern_receiver, receiver_package_from_context_or_source,
    receiver_package_from_symbol_table_or_source, source_package_fallback,
};
use perl_semantic_facts::{
    Confidence, DefinitionCandidate, EntityKind, FileId, PackageEdge, PackageEdgeKind, Provenance,
    VisibleSymbol, VisibleSymbolSource,
};
use perl_workspace::position::{Position, Range};
use perl_workspace::semantic::{
    imports::ImportExportIndex,
    package_graph::PackageGraphIndex,
    queries::{SemanticQueries, WorkspaceSemanticQueries},
    references::ReferenceIndex,
};
use perl_workspace::workspace_index::{
    SymbolKind as WsSymbolKind, WorkspaceIndex, WorkspaceSymbol,
};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Add workspace symbol completions for functions and variables
///
/// Queries the workspace index to provide completions for symbols from other files.
/// Uses the `import_map` to promote imported symbols and downrank explicitly
/// not-imported symbols for import-aware sort ordering.
///
pub fn add_workspace_symbol_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    workspace_index: &Option<Arc<WorkspaceIndex>>,
    import_map: &HashMap<String, HashSet<String>>,
    used_modules: &HashSet<String>,
) {
    // Only proceed if we have a workspace index
    let Some(index) = workspace_index else {
        return;
    };

    // Only provide workspace completions if there's a reasonable prefix
    // to avoid overwhelming the user with all workspace symbols
    if context.prefix.is_empty() {
        return;
    }

    // Check if the workspace index has any symbols
    if !index.has_symbols() {
        return;
    }

    // Search for symbols matching the prefix
    let matching_symbols = index.find_symbols(&context.prefix);

    for symbol in matching_symbols {
        // Skip symbols that don't match the prefix
        if !symbol.name.starts_with(&context.prefix)
            && !symbol.qualified_name.as_ref().is_some_and(|qn| qn.contains(&context.prefix))
        {
            continue;
        }

        match symbol.kind {
            WsSymbolKind::Subroutine | WsSymbolKind::Method => {
                // Determine sort priority and detail based on import map
                let label = symbol.qualified_name.as_ref().unwrap_or(&symbol.name).clone();
                let module = symbol.container_name.as_deref().unwrap_or("");
                // Bare insertion is only truthful when the defining namespace
                // is already visible. Import synthesis is withdrawn (#11158).
                if !workspace_symbol_visible_without_import(
                    &symbol,
                    context,
                    import_map,
                    used_modules,
                ) {
                    continue;
                }

                let (sort_prefix, detail) = match import_map.get(module) {
                    None => {
                        // Module not in import_map: not used or `use Module` (import all).
                        // Rank at tier 4 (after core builtins at tier 3).
                        let det = symbol
                            .container_name
                            .clone()
                            .unwrap_or_else(|| "workspace".to_string());
                        ("4_", det)
                    }
                    Some(imported_set) if imported_set.is_empty() => {
                        // Explicit empty import `use Module qw()` — not in namespace.
                        // Rank at tier 5 (lowest, after all useful completions).
                        ("5_", "not imported".to_string())
                    }
                    Some(imported_set) if imported_set.contains(&symbol.name) => {
                        // Symbol is explicitly imported — boost priority to tier 2
                        // (treated like a file-scope symbol).
                        let det = format!("imported from {module}");
                        ("2_", det)
                    }
                    Some(_) => {
                        // Module used with explicit list, but this symbol wasn't in it.
                        // Rank at tier 4 (workspace, after core builtins).
                        let det = symbol
                            .container_name
                            .clone()
                            .unwrap_or_else(|| "workspace".to_string());
                        ("4_", det)
                    }
                };

                completions.push(CompletionItem {
                    insert_text: Some(Cow::Owned(symbol.name.clone())),
                    sort_text: Some(Cow::Owned(format!("{sort_prefix}{label}"))),
                    filter_text: Some(Cow::Owned(label.clone())),
                    label: Cow::Owned(label),
                    kind: CompletionItemKind::Function,
                    detail: Some(Cow::Owned(detail)),
                    documentation: symbol.documentation.clone().map(Cow::Owned),
                    additional_edits: vec![],
                    text_edit_range: Some((context.prefix_start, context.position)),
                    commit_characters: None,
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
            WsSymbolKind::Variable(_) => {
                // Unreachable, and deliberately left empty rather than gated.
                //
                // Every workspace variable candidate is spelled with a leading
                // sigil, so only a sigil-prefixed request could ever match one.
                // `complete_sigil_context` (`request/dispatch.rs`) intercepts
                // every `$`/`@`/`%` prefix and always returns, so
                // `complete_general_context` — the sole caller of this function
                // — never runs for one; an empty prefix returns above. The arm
                // that used to stand here could therefore only fire when called
                // directly, and did so wrongly: `symbol.name` already carries
                // its sigil, so the unqualified branch emitted `$$name`.
                //
                // Variable candidates are owned by the sigil path (in-file and
                // `Pkg::`-qualified) and by the runtime workspace fallback,
                // which qualifies cross-file package variables and withdraws
                // cross-file lexicals (issue #11937).
                // `sigil_prefixed_requests_never_reach_the_workspace_symbol_pass`
                // pins that interception: if it fails, add a gated arm here
                // rather than restoring the previous emission.
            }
            WsSymbolKind::Package => {
                // Add package completion — tier 4 (workspace, after core builtins)
                let name = &symbol.name;
                completions.push(CompletionItem {
                    label: Cow::Owned(name.clone()),
                    kind: CompletionItemKind::Module,
                    detail: Some(Cow::Borrowed("package")),
                    documentation: symbol.documentation.clone().map(Cow::Owned),
                    insert_text: Some(Cow::Owned(name.clone())),
                    sort_text: Some(Cow::Owned(format!("4_{name}"))),
                    filter_text: Some(Cow::Owned(name.clone())),
                    additional_edits: vec![],
                    text_edit_range: Some((context.prefix_start, context.position)),
                    commit_characters: Some(vec![":".to_string(), ";".to_string()]),
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
            WsSymbolKind::Constant => {
                // Add constant completion — tier 4 (workspace, after core builtins)
                if !workspace_symbol_visible_without_import(
                    &symbol,
                    context,
                    import_map,
                    used_modules,
                ) {
                    continue;
                }
                let name = &symbol.name;
                completions.push(CompletionItem {
                    label: Cow::Owned(name.clone()),
                    kind: CompletionItemKind::Constant,
                    detail: symbol
                        .container_name
                        .clone()
                        .or_else(|| Some("workspace".to_string()))
                        .map(Cow::Owned),
                    documentation: symbol.documentation.clone().map(Cow::Owned),
                    insert_text: Some(Cow::Owned(name.clone())),
                    sort_text: Some(Cow::Owned(format!("4_{name}"))),
                    filter_text: Some(Cow::Owned(name.clone())),
                    additional_edits: vec![],
                    text_edit_range: Some((context.prefix_start, context.position)),
                    commit_characters: None,
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
            WsSymbolKind::Export => {
                // An export is only safe as a bare insertion when its defining
                // module is already visible in the current document.
                if !workspace_symbol_visible_without_import(
                    &symbol,
                    context,
                    import_map,
                    used_modules,
                ) {
                    continue;
                }
                let name = &symbol.name;
                completions.push(CompletionItem {
                    label: Cow::Owned(name.clone()),
                    kind: CompletionItemKind::Function,
                    detail: Some(Cow::Borrowed("exported")),
                    documentation: symbol.documentation.clone().map(Cow::Owned),
                    insert_text: Some(Cow::Owned(name.clone())),
                    sort_text: Some(Cow::Owned(format!("2_{name}"))), // Prioritize exports
                    filter_text: Some(Cow::Owned(name.clone())),
                    additional_edits: vec![],
                    text_edit_range: Some((context.prefix_start, context.position)),
                    commit_characters: None,
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
            _ => {
                // Skip other symbol types
            }
        }
    }
}

fn workspace_symbol_visible_without_import(
    symbol: &WorkspaceSymbol,
    context: &CompletionContext,
    import_map: &HashMap<String, HashSet<String>>,
    used_modules: &HashSet<String>,
) -> bool {
    let module = symbol
        .container_name
        .as_deref()
        .or_else(|| {
            symbol.qualified_name.as_deref().and_then(|qualified| {
                qualified.strip_suffix(&symbol.name).and_then(|prefix| prefix.strip_suffix("::"))
            })
        })
        .unwrap_or("");
    module.is_empty()
        || module == "main"
        || module == context.current_package
        || (used_modules.contains(module)
            && import_map.get(module).is_some_and(|symbols| symbols.contains(&symbol.name)))
}

/// Add live compiler visible-symbol completions for imported/exported symbols.
///
/// This is intentionally narrower than the shadow/cutover proof helpers:
/// only high-confidence import/export visibility facts are promoted into the
/// live completion list. Generated members, local symbols, external fallback
/// symbols, and dynamic-boundary candidates remain gated by their existing
/// provider-specific proof lanes.
pub fn add_visible_symbol_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    workspace_index: &Option<Arc<WorkspaceIndex>>,
    filepath: Option<&str>,
    import_map: &HashMap<String, HashSet<String>>,
    used_modules: &HashSet<String>,
) {
    if context.prefix.is_empty() || context.prefix.starts_with(['$', '@', '%', '&']) {
        return;
    }

    let Some(index) = workspace_index else {
        return;
    };
    let Some(uri) = filepath else {
        return;
    };
    let Ok(byte_offset) = u32::try_from(context.position) else {
        return;
    };

    let Some(visible_symbols) = index.with_semantic_queries_for_uri(uri, |file_id, queries| {
        queries.visible_symbols_at(file_id, byte_offset, None)
    }) else {
        return;
    };

    for symbol in visible_symbols
        .into_iter()
        .filter(is_live_visible_completion_candidate)
        .filter(|symbol| visible_symbol_has_import_authority(symbol, import_map, used_modules))
        .filter(|symbol| symbol.name.starts_with(&context.prefix))
    {
        let source_module = symbol.context.as_ref().and_then(|context| {
            context.source_module.as_deref().filter(|module| !module.is_empty())
        });
        let label = symbol.name.clone();
        completions.push(CompletionItem {
            label: Cow::Owned(label.clone()),
            kind: CompletionItemKind::Function,
            detail: Some(Cow::Owned(visible_symbol_completion_detail(&symbol, source_module))),
            documentation: Some(Cow::Owned(visible_symbol_completion_documentation(
                &symbol,
                source_module,
            ))),
            insert_text: Some(Cow::Owned(label.clone())),
            sort_text: Some(Cow::Owned(format!("2z_visible_{label}"))),
            filter_text: Some(Cow::Owned(label)),
            additional_edits: vec![],
            text_edit_range: Some((context.prefix_start, context.position)),
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        });
    }
}

/// A compiler visibility fact with a module origin is usable as a bare
/// completion only when the request document makes that module visible.
///
/// The semantic query can expose export facts from an indexed module even when
/// the current file has not imported that module. Qualified workspace
/// completions remain responsible for that unimported case; this producer must
/// not promote the same symbol to a bare insertion without import authority.
fn visible_symbol_has_import_authority(
    symbol: &VisibleSymbol,
    import_map: &HashMap<String, HashSet<String>>,
    used_modules: &HashSet<String>,
) -> bool {
    let Some(module) = symbol.context.as_ref().and_then(|context| context.source_module.as_deref())
    else {
        return true;
    };

    if !used_modules.contains(module) {
        return false;
    }

    // `visible_symbols_at` emits `DefaultExport` facts only when the request
    // document's import of this module is a bare `use module;` form and the
    // symbol is a member of that module's `@EXPORT`, position-gated to the
    // query point (perl-workspace semantic visibility + ExportSet facts).
    // That compiler fact already proves default-import visibility, and the
    // buffer import map deliberately records no row for a bare `use`. An
    // explicit list (`use module qw(...)`, including `qw()`) replaces those
    // defaults, so only exact membership from an explicit row can still
    // authorize.
    if matches!(symbol.source, VisibleSymbolSource::DefaultExport) {
        return import_map.get(module).is_none_or(|imported| imported.contains(&symbol.name));
    }

    import_map.get(module).is_some_and(|symbols| symbols.contains(&symbol.name))
}

fn is_live_visible_completion_candidate(symbol: &VisibleSymbol) -> bool {
    symbol.confidence == Confidence::High
        && matches!(
            symbol.source,
            VisibleSymbolSource::ExplicitImport
                | VisibleSymbolSource::DefaultExport
                | VisibleSymbolSource::ExportTag
        )
}

fn visible_symbol_completion_detail(symbol: &VisibleSymbol, source_module: Option<&str>) -> String {
    let source = match symbol.source {
        VisibleSymbolSource::ExplicitImport => "imported",
        VisibleSymbolSource::DefaultExport => "default export",
        VisibleSymbolSource::ExportTag => "tag export",
        _ => "visible symbol",
    };

    match source_module {
        Some(module) => format!("{source} from {module} - compiler fact, high confidence"),
        None => format!("{source} - compiler fact, high confidence"),
    }
}

fn visible_symbol_completion_documentation(
    symbol: &VisibleSymbol,
    source_module: Option<&str>,
) -> String {
    let source = match symbol.source {
        VisibleSymbolSource::ExplicitImport => "explicit import",
        VisibleSymbolSource::DefaultExport => "default export",
        VisibleSymbolSource::ExportTag => "export tag",
        _ => "visible symbol",
    };
    let module = source_module.map(|module| format!("\nModule: `{module}`")).unwrap_or_default();

    format!(
        "Compiler visible-symbol completion.\n\nSource: {source}\nProvenance: ImportExportInference\nConfidence: High\nFreshness: Fresh{module}"
    )
}

/// Ultra-common Perl pragmas and core modules that should surface first in `use` completions.
///
/// Tier 0: always-used pragmas and critical infrastructure modules.
const COMMON_MODULES_TIER_0: &[&str] = &[
    "strict",
    "warnings",
    "Carp",
    "Exporter",
    "File::Path",
    "File::Spec",
    "List::Util",
    "Scalar::Util",
    "Data::Dumper",
    "JSON",
    "POSIX",
    "Getopt::Long",
];

/// Common CPAN modules that are frequently used but less universal than tier-0.
///
/// Tier 1: widely-used libraries (DB, OOP, testing, filesystem).
const COMMON_MODULES_TIER_1: &[&str] =
    &["DBI", "Moo", "Moose", "Try::Tiny", "Path::Tiny", "Test::More", "Test::Exception"];

/// Returns the sort-text tier prefix for a module name.
///
/// Returns `"0"` for tier-0 (ultra-common), `"1"` for tier-1 (common), and `"9"` for
/// all other modules so they sort after the well-known ones.
fn module_sort_tier(name: &str) -> &'static str {
    if COMMON_MODULES_TIER_0.contains(&name) {
        "0"
    } else if COMMON_MODULES_TIER_1.contains(&name) {
        "1"
    } else {
        "9"
    }
}

const MAX_MODULE_SCAN_ROOTS: usize = 16;
const MAX_MODULES_PER_SCAN: usize = 512;
const MAX_SCAN_DEPTH: usize = 8;

/// Convert a module file path under `root` to a Perl module name.
///
/// Example: `lib/File/Spec.pm` under `lib` => `File::Spec`.
pub fn path_to_module_name(root: &Path, file_path: &Path) -> Option<String> {
    let rel = file_path.strip_prefix(root).ok()?;
    if rel.extension().and_then(|ext| ext.to_str()) != Some("pm") {
        return None;
    }

    let stem = rel.with_extension("");
    let mut parts: Vec<String> = Vec::new();
    for component in stem.components() {
        let part = component.as_os_str().to_str()?;
        if part.is_empty() {
            continue;
        }
        parts.push(part.to_string());
    }

    if parts.is_empty() { None } else { Some(parts.join("::")) }
}

/// Split a module prefix like `"Foo::Bar::Ba"` into a subdir and a leaf prefix.
///
/// Returns `(scan_dir, leaf_prefix, depth_consumed)` where:
/// - `scan_dir` is `root` joined with the path components before the last `::`,
/// - `leaf_prefix` is the last `::` segment (the partial name being typed),
/// - `depth_consumed` is the number of directory levels already descended into.
///
/// For a single-segment prefix such as `"Foo"`, `scan_dir == root`,
/// `leaf_prefix == "Foo"`, and `depth_consumed == 0` — the caller falls back
/// to a normal root scan.
///
/// For `"Foo::Bar::Ba"`:
/// - `scan_dir  == root.join("Foo/Bar")`
/// - `leaf_prefix == "Ba"`
/// - `depth_consumed == 2`  (two directory levels have been consumed)
///
/// This lets `scan_directory_for_modules` start the BFS at the narrowest
/// possible directory instead of scanning from the include root and filtering,
/// which is a significant speedup on large vendor / local / system `@INC` trees.
pub fn root_and_leaf_prefix(root: &Path, module_prefix: &str) -> (PathBuf, String, usize) {
    if module_prefix.is_empty() {
        return (root.to_path_buf(), String::new(), 0);
    }

    let mut parts: Vec<&str> = module_prefix.split("::").collect();
    if parts.len() <= 1 {
        // Single segment — scan from root, filter by that segment.
        return (root.to_path_buf(), module_prefix.to_string(), 0);
    }

    // The last element is the partial leaf being typed; everything before it
    // is a fully-typed namespace segment that maps to a real directory.
    let leaf = parts.pop().unwrap_or_default().to_string();
    let depth_consumed = parts.len(); // number of dir levels consumed
    let subdir = parts.iter().fold(root.to_path_buf(), |p, part| p.join(part));
    (subdir, leaf, depth_consumed)
}

/// Recursively scan a directory for `.pm` files and return module names.
///
/// When `prefix` contains `::` separators the scan starts from the narrowest
/// matching subdirectory (`prefix_dir`) instead of from `root`, which avoids
/// traversing unrelated subtrees on large include trees.  `path_to_module_name`
/// is still called with the original `root` so that the full qualified name
/// (`Foo::Bar::Baz`) is reconstructed correctly and the existing `starts_with`
/// filter continues to work unchanged.
pub fn scan_directory_for_modules(root: &Path, prefix: &str) -> Vec<String> {
    let mut modules = Vec::new();
    if !root.is_dir() {
        return modules;
    }

    // Prefix-directed optimisation: for namespaced prefixes like "Foo::Bar::Ba"
    // start BFS at `root/Foo/Bar/` rather than `root/`.  If that subdir does
    // not exist we fall back to a root scan (the prefix simply has no matches).
    let (scan_dir, _leaf_prefix, depth_consumed) = root_and_leaf_prefix(root, prefix);
    let start_dir = if scan_dir.is_dir() { scan_dir } else { root.to_path_buf() };
    let start_depth = if start_dir == root { 0 } else { depth_consumed };

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::from([(start_dir, start_depth)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if modules.len() >= MAX_MODULES_PER_SCAN {
            break;
        }

        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            if modules.len() >= MAX_MODULES_PER_SCAN {
                break;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();

            if file_type.is_dir() {
                // Use path.is_symlink() rather than file_type.is_symlink() because
                // DirEntry::file_type() returns the entry's own type: on Unix a
                // symlinked directory has is_symlink()=true AND is_dir()=false,
                // so the file_type.is_symlink() guard inside is_dir() would be
                // dead code. path.is_symlink() correctly detects symlinks via lstat.
                if depth < MAX_SCAN_DEPTH && !path.is_symlink() {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let Some(module_name) = path_to_module_name(root, &path) else {
                continue;
            };

            if prefix.is_empty() || module_name.starts_with(prefix) {
                modules.push(module_name);
            }
        }
    }

    modules
}

/// Add module name completions for `use` and `require` statements.
///
/// When the cursor is after `use ` or `require `, suggests package names from the
/// workspace index. This enables discovering available modules as you type.
///
/// For example, typing `use My` will suggest `MyApp`, `MyApp::Config`, etc.
fn workspace_module_symbol_matches_roots(symbol: &WorkspaceSymbol, active_roots: &[&Path]) -> bool {
    if active_roots.is_empty() {
        return true;
    }

    let Some(symbol_path) = perl_workspace::workspace_index::uri_to_fs_path(&symbol.uri) else {
        return false;
    };
    let module_path = module_name_to_path(&symbol.name);
    let symbol_key = normalized_path_key(&symbol_path);

    active_roots.iter().any(|root| {
        let candidate = root.join(&module_path);
        normalized_path_key(&candidate) == symbol_key
    })
}

fn normalized_path_key(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        if component == Component::CurDir {
            continue;
        }
        normalized.push(component.as_os_str());
    }

    let path = if normalized.as_os_str().is_empty() { PathBuf::from(".") } else { normalized };
    let key = path.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) { key.to_ascii_lowercase() } else { key }
}

/// Collect module names from include roots using the same bounded directory
/// scanner and optional short-TTL cache used by completion-list module lookup.
pub fn collect_module_names_from_roots_with_cache(
    prefix: &str,
    include_paths: &[PathBuf],
    system_inc_paths: &[PathBuf],
    include_system_inc: bool,
    scan_cache: Option<&ModuleCompletionScanCache>,
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<String> {
    let mut modules = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let cache_key = |root: &Path| -> ScanCacheKey {
        let (scan_dir, _, _) = root_and_leaf_prefix(root, prefix);
        let prefix_dir = match scan_dir.strip_prefix(root) {
            Ok(path) => path.to_path_buf(),
            Err(_) => scan_dir.to_path_buf(),
        };
        ScanCacheKey {
            // Inline completion runs on frequent keystrokes; keep this helper free
            // of canonicalization I/O and rely on stable include-root paths.
            canonical_root: root.to_path_buf(),
            prefix_dir,
            module_prefix: prefix.to_string(),
        }
    };

    let mut add_external_modules = |roots: &[PathBuf]| -> bool {
        for root in roots.iter().take(MAX_MODULE_SCAN_ROOTS) {
            if is_cancelled() {
                return false;
            }

            let scanned_modules = match scan_cache {
                Some(cache) => {
                    let key = cache_key(root);
                    if let Some(cached) = cache.get(&key) {
                        if is_cancelled() {
                            return false;
                        }
                        cached
                    } else {
                        let scanned = scan_directory_for_modules(root, prefix);
                        if is_cancelled() {
                            return false;
                        }
                        cache.insert(key, scanned.clone());
                        scanned
                    }
                }
                None => scan_directory_for_modules(root, prefix),
            };

            if is_cancelled() {
                return false;
            }

            for name in scanned_modules {
                if !seen.contains(&name) {
                    seen.insert(name.clone());
                    modules.push(name);
                }
            }
        }

        true
    };

    if !add_external_modules(include_paths) {
        return modules;
    }
    if include_system_inc {
        let _ = add_external_modules(system_inc_paths);
    }

    modules
}

/// Add module name completions for `use` and `require` statements.
///
/// Thin backward-compatible wrapper around [`add_use_module_completions_with_cache`]
/// that passes `None` for the cache.  Prefer the `_with_cache` variant when a
/// runtime-owned [`ModuleCompletionScanCache`] is available.
#[allow(dead_code)] // Public backward-compatibility API; callers in perl-lsp-rs use _with_cache
pub fn add_use_module_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    workspace_index: &Option<Arc<WorkspaceIndex>>,
    include_paths: &[PathBuf],
    system_inc_paths: &[PathBuf],
    include_system_inc: bool,
) {
    add_use_module_completions_with_cache(
        completions,
        context,
        workspace_index,
        include_paths,
        system_inc_paths,
        include_system_inc,
        None,
        &|| false,
    );
}

/// Add module name completions for `use` and `require` statements, optionally
/// using a runtime-owned TTL cache to avoid repeated filesystem scans on each
/// keystroke (issue #8514).
///
/// ## Cache contract
///
/// - When `scan_cache` is `Some`, each
///   `(canonical_root, prefix_dir, full_module_prefix)` tuple is looked up before
///   scanning. On a miss the scan proceeds normally and the result is stored in
///   the cache. On a hit the cached `Vec<String>` is used directly.
/// - `is_cancelled` is checked **before returning any cached hit** so that a
///   cancelled LSP request does not deliver results to the editor.
/// - The workspace-index path is not cached — it is already in-memory.
/// - Cache population uses the canonical form of the root path when available
///   (`std::fs::canonicalize`); falls back to the raw path on error.
pub fn add_use_module_completions_with_cache(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    workspace_index: &Option<Arc<WorkspaceIndex>>,
    include_paths: &[PathBuf],
    system_inc_paths: &[PathBuf],
    include_system_inc: bool,
    scan_cache: Option<&ModuleCompletionScanCache>,
    is_cancelled: &dyn Fn() -> bool,
) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut active_module_roots: Vec<&Path> = include_paths.iter().map(PathBuf::as_path).collect();
    if include_system_inc {
        active_module_roots.extend(system_inc_paths.iter().map(PathBuf::as_path));
    }

    if let Some(index) = workspace_index
        && index.has_symbols()
    {
        // Search for package symbols matching the prefix
        let all_symbols = if context.prefix.is_empty() {
            index.all_symbols()
        } else {
            index.find_symbols(&context.prefix)
        };

        for symbol in all_symbols {
            if symbol.kind != WsSymbolKind::Package {
                continue;
            }

            // Match against the module name prefix
            if !context.prefix.is_empty() && !symbol.name.starts_with(&context.prefix) {
                continue;
            }

            if !workspace_module_symbol_matches_roots(&symbol, &active_module_roots) {
                continue;
            }

            if !seen.insert(symbol.name.clone()) {
                continue;
            }

            let name = &symbol.name;
            completions.push(CompletionItem {
                label: Cow::Owned(name.clone()),
                kind: CompletionItemKind::Module,
                detail: Some(Cow::Borrowed("module")),
                documentation: symbol
                    .documentation
                    .clone()
                    .or_else(|| Some(format!("Package `{name}`")))
                    .map(Cow::Owned),
                insert_text: Some(Cow::Owned(name.clone())),
                sort_text: Some(Cow::Owned(format!("1{}_{name}", module_sort_tier(name)))),
                filter_text: Some(Cow::Owned(name.clone())),
                additional_edits: vec![],
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }

    // Helper: resolve the canonical form of `root` for use as a cache key.
    // Falls back to the raw path when canonicalization fails (e.g. non-existent dir).
    let canonical_root = |root: &Path| -> PathBuf {
        std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
    };

    // Helper: build the ScanCacheKey for (root, prefix).
    //
    // The prefix_dir is the subdirectory that the scan starts from
    // (e.g. `Mojo/` for prefix `Mojo::Controller`). Using a relative
    // path under the canonical root keeps the key stable across callers
    // that might pass in slightly different root representations. The full
    // prefix is part of the key because cached values are prefix-filtered.
    let cache_key = |root: &Path| -> ScanCacheKey {
        let (scan_dir, _, _) = root_and_leaf_prefix(root, &context.prefix);
        let prefix_dir = scan_dir.strip_prefix(root).unwrap_or(&scan_dir).to_path_buf();
        ScanCacheKey {
            canonical_root: canonical_root(root),
            prefix_dir,
            module_prefix: context.prefix.clone(),
        }
    };

    let mut add_external_modules = |roots: &[PathBuf], detail: &str| -> bool {
        for root in roots.iter().take(MAX_MODULE_SCAN_ROOTS) {
            if is_cancelled() {
                return false;
            }

            let modules = match scan_cache {
                Some(cache) => {
                    let key = cache_key(root);
                    if let Some(cached) = cache.get(&key) {
                        // Cancellation check before returning cached result.
                        if is_cancelled() {
                            return false;
                        }
                        cached
                    } else {
                        let scanned = scan_directory_for_modules(root, &context.prefix);
                        if is_cancelled() {
                            return false;
                        }
                        cache.insert(key, scanned.clone());
                        scanned
                    }
                }
                None => scan_directory_for_modules(root, &context.prefix),
            };

            if is_cancelled() {
                return false;
            }

            for name in modules {
                if !seen.insert(name.clone()) {
                    continue;
                }

                completions.push(CompletionItem {
                    label: Cow::Owned(name.clone()),
                    kind: CompletionItemKind::Module,
                    detail: Some(Cow::Owned(detail.to_string())),
                    documentation: Some(Cow::Owned(format!("Package `{name}`"))),
                    insert_text: Some(Cow::Owned(name.clone())),
                    sort_text: Some(Cow::Owned(format!("2{}_{name}", module_sort_tier(&name)))),
                    filter_text: Some(Cow::Owned(name)),
                    additional_edits: vec![],
                    text_edit_range: Some((context.prefix_start, context.position)),
                    commit_characters: Some(vec![":".to_string(), ";".to_string()]),
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
        }
        true
    };

    if !add_external_modules(include_paths, "external module") {
        return;
    }
    if include_system_inc {
        let _ = add_external_modules(system_inc_paths, "system module");
    }
}

/// Add import completions for symbols inside `use Module qw(...)`.
///
/// When the cursor is inside the `qw()` import list of a `use` statement,
/// queries the workspace index for symbols exported by or defined in that
/// module and suggests matching function/variable/constant names.
///
/// For example, typing `use File::Basename qw(bas` will suggest `basename`,
/// `fileparse`, `dirname`, etc.
pub fn add_use_qw_import_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    workspace_index: &Option<Arc<WorkspaceIndex>>,
    module_name: &str,
    qw_prefix: &str,
) {
    let Some(index) = workspace_index else {
        return;
    };

    if !index.has_symbols() {
        return;
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let members = index.get_package_members(module_name);

    for symbol in &members {
        match symbol.kind {
            WsSymbolKind::Subroutine
            | WsSymbolKind::Method
            | WsSymbolKind::Export
            | WsSymbolKind::Constant => {}
            _ => continue,
        }

        // Filter by prefix typed inside qw()
        if !qw_prefix.is_empty() && !symbol.name.starts_with(qw_prefix) {
            continue;
        }

        // Deduplicate
        if !seen.insert(&symbol.name) {
            continue;
        }

        let kind_label = match symbol.kind {
            WsSymbolKind::Constant => "constant",
            WsSymbolKind::Export => "exported",
            _ => "function",
        };

        let name = &symbol.name;
        completions.push(CompletionItem {
            label: Cow::Owned(name.clone()),
            kind: match symbol.kind {
                WsSymbolKind::Constant => CompletionItemKind::Constant,
                _ => CompletionItemKind::Function,
            },
            detail: Some(Cow::Owned(format!("{module_name} {kind_label}"))),
            documentation: symbol
                .documentation
                .clone()
                .or_else(|| Some(format!("`{module_name}::{name}`")))
                .map(Cow::Owned),
            insert_text: Some(Cow::Owned(name.clone())),
            sort_text: Some(Cow::Owned(format!("1_{name}"))),
            filter_text: Some(Cow::Owned(name.clone())),
            additional_edits: vec![],
            text_edit_range: Some((context.prefix_start, context.position)),
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        });
    }
}

/// Add method completions from the workspace index for `->` expressions.
///
/// When the user types `$obj->` or `Package->`, queries the workspace index for
/// methods defined in the receiver's package and suggests them.
///
/// Auto-import edits are attached when the receiver package is not yet imported.
///
/// When receiver inference returns [`ReceiverEvidence::Unknown`] (no exact
/// receiver evidence found), the bounded low-confidence fallback added in
/// #7929 fires: methods from imported / visible packages plus the current
/// file's package and its `@ISA` chain are offered with a low-confidence
/// detail label and a sort tier that puts them below all exact-receiver
/// completions. [`ReceiverEvidence::Dynamic`] (positively-detected dynamic
/// `bless` forms) is *not* fallback-eligible and stays fail-closed.
pub fn add_workspace_method_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    source: &str,
    symbol_table: &SymbolTable,
    type_engine: Option<&TypeInferenceEngine>,
    workspace_index: &Option<Arc<WorkspaceIndex>>,
    used_modules: &HashSet<String>,
) {
    let Some(index) = workspace_index else {
        return;
    };

    // Exact current-document method facts must run even when the persisted
    // workspace index is still empty (#16809). Unknown-receiver fallback stays
    // index-gated so an empty index cannot become an all-workspace scan.

    // Prefer semantic receiver facts only when they meet the narrow live pilot
    // bar. Medium, dynamic, unknown, and unsupported facts fall back through the
    // existing receiver classifier instead of suppressing legacy behavior.
    let evidence =
        classify_receiver_with_symbol_table(context, source, type_engine, Some(symbol_table));

    // Union receivers: offer methods from every candidate package (#9500).
    // Methods shared across arms are deduplicated; the first arm's definition wins.
    // Use the `candidate_packages` accessor so the dispatch stays decoupled from
    // the enum variant's internals.
    let union_packages = evidence.candidate_packages();
    if !union_packages.is_empty() {
        add_union_receiver_method_completions(completions, context, source, index, union_packages);
        return;
    }

    let Some(package_name) = evidence.package().map(str::to_string) else {
        // No exact receiver package. Trigger bounded Unknown-receiver
        // fallback (#7929) only for `Unknown` evidence; `Dynamic` stays
        // fail-closed. An empty index must not open a workspace-wide scan.
        if evidence.is_unknown_fallback_eligible() && index.has_symbols() {
            add_unknown_receiver_fallback(completions, context, source, index, used_modules);
        }
        return;
    };

    // Collect labels already present to avoid duplicates with local method completions
    let method_prefix = context.prefix.rsplit("->").next().unwrap_or("");

    // Collect all methods from the receiver package AND its ancestor chain
    // (parents + roles). Child methods take priority.
    let members = collect_all_package_members_with_source(index, &package_name, source);
    drop_generic_local_methods_rebound_from_composition(completions, &package_name, &members);

    let method_symbols = {
        let existing_labels: HashSet<&str> =
            completions.iter().map(|item| item.label.as_ref()).collect();
        workspace_method_symbols(&members, &existing_labels, method_prefix)
    };
    let method_text_edit_range = (context.method_text_edit_start(source), context.position);

    if add_semantic_method_completions(
        completions,
        method_text_edit_range,
        index,
        &package_name,
        method_prefix,
        &method_symbols,
        &evidence,
    ) {
        return;
    }

    for symbol in method_symbols {
        // Show which package actually defines the method for inherited completions
        let defining_pkg = symbol.container_name.as_deref().unwrap_or(package_name.as_str());
        let base_detail = if defining_pkg == package_name {
            format!("{package_name} method")
        } else {
            format!("{package_name} method (from {defining_pkg})")
        };
        // Append receiver-evidence suffix from #7918. Detail-only — no
        // change to label, insert_text, filter_text, sort_text, or the
        // candidate set.
        let detail = detail_with_evidence(base_detail, &evidence);

        // Own-class methods rank above inherited: tier 2 for own, tier 3 for inherited.
        // This ensures $obj->zoom (own) sorts before $obj->abstract_method (inherited)
        // even when the own method name is alphabetically after the inherited name.
        let method_tier = if defining_pkg == package_name { "2" } else { "3" };

        completions.push(CompletionItem {
            label: Cow::Owned(symbol.name.clone()),
            kind: CompletionItemKind::Function,
            detail: Some(Cow::Owned(detail)),
            documentation: symbol
                .documentation
                .clone()
                .or_else(|| {
                    Some(format!(
                        "Method `{}::{}` from workspace index.",
                        defining_pkg, symbol.name
                    ))
                })
                .map(Cow::Owned),
            insert_text: Some(Cow::Owned(format!("{}()", symbol.name))),
            sort_text: Some(Cow::Owned(format!("{method_tier}_{}", symbol.name))), // tier 2=own, 3=inherited, after local (tier 1)
            filter_text: Some(Cow::Owned(symbol.name.clone())),
            additional_edits: vec![],
            text_edit_range: Some((context.method_text_edit_start(source), context.position)),
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        });
    }
}

/// Union-aware method completion for [`ReceiverEvidence::UnionCandidates`] (#9500).
///
/// When the receiver type is a union (e.g. `my $obj : Foo | Bar`), the
/// `candidate_packages` field of the underlying [`ReceiverFact`] exposes every
/// distinct object package from the union.  This function queries each package
/// and its `@ISA` ancestor chain, deduplicates methods by name (first
/// occurrence wins), and offers them all with a sort tier that reflects
/// source-backed high-confidence evidence.
///
/// Sort tiers used:
/// - `2u_<name>` — method found in every union arm (shared interface)
/// - `3u_<name>` — method found in at least one arm (partial interface)
///
/// This preserves the proven tier-ordering invariant (tiers 1–4 for
/// exact-receiver, tier 5–6 for low-confidence fallback).
fn add_union_receiver_method_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    source: &str,
    index: &WorkspaceIndex,
    packages: &[String],
) {
    let method_prefix = context.prefix.rsplit("->").next().unwrap_or("");
    // Snapshot existing labels before any push to avoid borrow conflicts.
    let existing_labels: HashSet<String> =
        completions.iter().map(|item| item.label.as_ref().to_string()).collect();
    let mut emitted: HashSet<String> = HashSet::new();

    // Gather methods per package so we can determine "shared across all arms".
    let per_package_methods: Vec<HashSet<String>> = packages
        .iter()
        .map(|pkg| {
            collect_all_package_members(index, pkg)
                .into_iter()
                .filter(|s| matches!(s.kind, WsSymbolKind::Subroutine | WsSymbolKind::Method))
                .filter(|s| method_prefix.is_empty() || s.name.starts_with(method_prefix))
                .map(|s| s.name)
                .collect()
        })
        .collect();

    let all_method_names: HashSet<String> =
        per_package_methods.iter().flat_map(|set| set.iter().cloned()).collect();

    let shared_methods: HashSet<&String> = all_method_names
        .iter()
        .filter(|name| per_package_methods.iter().all(|set| set.contains(*name)))
        .collect();

    // Collect into pending first (mirrors add_unknown_receiver_fallback pattern)
    // so we do not hold any borrow on `completions` while pushing.
    let mut pending: Vec<CompletionItem> = Vec::new();

    // Emit one completion per method, iterating packages in declaration order
    // so the first arm's definition wins for the detail label.
    for package_name in packages {
        let members = collect_all_package_members_with_source(index, package_name, source);
        for symbol in &members {
            if !matches!(symbol.kind, WsSymbolKind::Subroutine | WsSymbolKind::Method) {
                continue;
            }
            if !method_prefix.is_empty() && !symbol.name.starts_with(method_prefix) {
                continue;
            }
            if existing_labels.contains(symbol.name.as_str()) {
                continue;
            }
            if !emitted.insert(symbol.name.clone()) {
                continue;
            }

            let defining_pkg = symbol.container_name.as_deref().unwrap_or(package_name.as_str());
            let arms_label = packages.join(" | ");
            let detail = if shared_methods.contains(&symbol.name) {
                format!("shared method ({arms_label}) — receiver: union candidates")
            } else {
                format!("method from {defining_pkg} ({arms_label}) — receiver: union candidates")
            };

            // Shared-interface methods rank above partial-interface ones.
            let sort_tier = if shared_methods.contains(&symbol.name) { "2u" } else { "3u" };

            pending.push(CompletionItem {
                label: Cow::Owned(symbol.name.clone()),
                kind: CompletionItemKind::Function,
                detail: Some(Cow::Owned(detail)),
                documentation: symbol
                    .documentation
                    .clone()
                    .or_else(|| {
                        Some(format!(
                            "Method `{}::{}` — union receiver `{}`.",
                            defining_pkg,
                            symbol.name,
                            packages.join(" | ")
                        ))
                    })
                    .map(Cow::Owned),
                insert_text: Some(Cow::Owned(format!("{}()", symbol.name))),
                sort_text: Some(Cow::Owned(format!("{sort_tier}_{}", symbol.name))),
                filter_text: Some(Cow::Owned(symbol.name.clone())),
                additional_edits: vec![],
                text_edit_range: Some((context.method_text_edit_start(source), context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }
    completions.extend(pending);
}

/// Bounded low-confidence fallback for method completion when receiver
/// evidence is [`ReceiverEvidence::Unknown`] (#7929 outcome A).
///
/// Sources are restricted to:
/// - imported / visible packages from the current file's `import_map`
/// - the current package and its `@ISA` chain (via
///   [`collect_all_package_members_with_source`]) when `current_package`
///   is set and not `main`
///
/// Current-document source is required so an unindexed child package can
/// still follow `use parent` / role composition to persisted generated
/// members. Allowed packages and unknown/low-confidence labelling are
/// unchanged (#7929).
///
/// All-workspace fallback is intentionally not used. Fallback candidates
/// carry a `receiver: unknown, low confidence` detail suffix and use sort
/// tier 6 so they always sort below exact-receiver completions (which
/// use tiers 1–4).
fn add_unknown_receiver_fallback(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    source: &str,
    index: &WorkspaceIndex,
    used_modules: &HashSet<String>,
) {
    let mut allowed_packages: HashSet<String> = used_modules.clone();
    if !context.current_package.is_empty() && context.current_package != "main" {
        allowed_packages.insert(context.current_package.clone());
    }
    if allowed_packages.is_empty() {
        return;
    }

    let method_prefix = context.prefix.rsplit("->").next().unwrap_or("");
    let existing_labels: HashSet<&str> =
        completions.iter().map(|item| item.label.as_ref()).collect();
    let mut emitted: HashSet<String> = HashSet::new();
    let mut pending = Vec::new();

    for package_name in &allowed_packages {
        let members = collect_all_package_members_with_source(index, package_name, source);
        for symbol in members {
            if !matches!(symbol.kind, WsSymbolKind::Subroutine | WsSymbolKind::Method) {
                continue;
            }
            if !method_prefix.is_empty() && !symbol.name.starts_with(method_prefix) {
                continue;
            }
            if existing_labels.contains(symbol.name.as_str()) {
                continue;
            }
            if !emitted.insert(symbol.name.clone()) {
                continue;
            }

            let defining_pkg = symbol.container_name.as_deref().unwrap_or(package_name.as_str());
            let detail = format!(
                "workspace method — receiver: unknown, low confidence (from {defining_pkg})"
            );

            // Auto-insert `use <defining_pkg>;` when the method comes from a
            // package other than the current one. Symbols from already-imported,
            // `main`, or current-package namespaces yield no edit.
            pending.push(CompletionItem {
                label: Cow::Owned(symbol.name.clone()),
                kind: CompletionItemKind::Function,
                detail: Some(Cow::Owned(detail)),
                documentation: symbol.documentation.clone().or_else(|| {
                    Some(format!(
                        "Workspace method `{}::{}` (low-confidence fallback for unknown receiver).",
                        defining_pkg, symbol.name
                    ))
                }).map(Cow::Owned),
                insert_text: Some(Cow::Owned(format!("{}()", symbol.name))),
                // Tier 6: below all exact-receiver completion tiers
                // (existing tiers 1–4) and below other tier-5 catch-alls.
                sort_text: Some(Cow::Owned(format!("6_{}", symbol.name))),
                filter_text: Some(Cow::Owned(symbol.name.clone())),
                additional_edits: vec![],
                text_edit_range: Some((context.method_text_edit_start(source), context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }
    completions.extend(pending);
}

/// Drop in-file generic method rows that composed-role / ancestor facts will
/// rebind. Consumer-defined methods keep local precedence (#16809).
fn drop_generic_local_methods_rebound_from_composition(
    completions: &mut Vec<CompletionItem>,
    receiver_package: &str,
    members: &[WorkspaceSymbol],
) {
    let rebound: HashSet<&str> = members
        .iter()
        .filter(|symbol| {
            symbol.container_name.as_deref().unwrap_or(receiver_package) != receiver_package
        })
        .map(|symbol| symbol.name.as_str())
        .collect();
    if rebound.is_empty() {
        return;
    }
    completions.retain(|item| {
        !(rebound.contains(item.label.as_ref())
            && item.detail.as_deref() == Some("method")
            && item.sort_text.as_deref().is_some_and(|sort| sort.starts_with("1_")))
    });
}

fn workspace_method_symbols<'a>(
    members: &'a [WorkspaceSymbol],
    existing_labels: &HashSet<&str>,
    method_prefix: &str,
) -> Vec<&'a WorkspaceSymbol> {
    members
        .iter()
        .filter(|symbol| matches!(symbol.kind, WsSymbolKind::Subroutine | WsSymbolKind::Method))
        .filter(|symbol| method_prefix.is_empty() || symbol.name.starts_with(method_prefix))
        .filter(|symbol| !existing_labels.contains(symbol.name.as_str()))
        .collect()
}

fn add_semantic_method_completions(
    completions: &mut Vec<CompletionItem>,
    method_text_edit_range: (usize, usize),
    index: &WorkspaceIndex,
    package_name: &str,
    method_prefix: &str,
    method_symbols: &[&WorkspaceSymbol],
    evidence: &ReceiverEvidence,
) -> bool {
    let legacy_names = method_symbol_names(method_symbols);
    if legacy_names.is_empty() {
        return false;
    }

    let Some(candidates) = semantic_method_candidates_for_legacy_methods(
        index,
        package_name,
        &legacy_names,
        method_symbols,
    ) else {
        return false;
    };

    let mut candidate_names: HashSet<String> = HashSet::new();
    for candidate in &candidates {
        candidate_names.insert(candidate.display_name.clone());
    }

    // Cut over only when semantic candidates cover the current legacy workspace
    // method set for this prefix. Otherwise the provider keeps the proven
    // fallback path and avoids dropping completions.
    if !legacy_names.iter().all(|name| candidate_names.contains(name)) {
        return false;
    }

    let mut seen = HashSet::new();
    for candidate in candidates {
        if !method_prefix.is_empty() && !candidate.display_name.starts_with(method_prefix) {
            continue;
        }
        if !seen.insert(candidate.display_name.clone()) {
            continue;
        }

        completions.push(CompletionItem {
            label: Cow::Owned(candidate.display_name.clone()),
            kind: CompletionItemKind::Function,
            detail: Some(Cow::Owned(semantic_method_detail(package_name, &candidate, evidence))),
            documentation: Some(Cow::Owned(semantic_method_documentation(
                package_name,
                &candidate,
            ))),
            insert_text: Some(Cow::Owned(format!("{}()", candidate.display_name))),
            sort_text: Some(Cow::Owned(format!(
                "{}_{}",
                semantic_method_sort_tier(package_name, &candidate),
                candidate.display_name
            ))),
            filter_text: Some(Cow::Owned(candidate.display_name.clone())),
            additional_edits: vec![],
            text_edit_range: Some(method_text_edit_range),
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        });
    }

    true
}

fn method_symbol_names(method_symbols: &[&WorkspaceSymbol]) -> Vec<String> {
    let mut names: Vec<String> = method_symbols.iter().map(|symbol| symbol.name.clone()).collect();
    names.sort();
    names.dedup();
    names
}

fn semantic_method_candidates_for_legacy_methods(
    index: &WorkspaceIndex,
    package_name: &str,
    method_names: &[String],
    method_symbols: &[&WorkspaceSymbol],
) -> Option<Vec<DefinitionCandidate>> {
    let mut shards = HashMap::new();
    let mut source_uris = HashSet::new();
    let legacy_defining_packages = method_symbol_defining_packages(method_symbols, package_name);

    if let Some(package_location) = index.find_definition(package_name) {
        source_uris.insert(package_location.uri);
    }

    for symbol in method_symbols {
        if let Some(shard) = index.file_fact_shard(&symbol.uri) {
            source_uris.insert(symbol.uri.clone());
            shards.entry(shard.source_uri.clone()).or_insert(shard);
        }
    }

    if shards.is_empty() {
        return None;
    }

    let package_graph = build_completion_package_graph(index, &source_uris);
    let reference_index = ReferenceIndex::new();
    let import_export_index = ImportExportIndex::new();
    let queries = WorkspaceSemanticQueries::with_package_graph(
        &reference_index,
        &import_export_index,
        &shards,
        &package_graph,
    );

    let mut candidates = Vec::new();
    for method_name in method_names {
        let expected_package = legacy_defining_packages.get(method_name)?;
        candidates.extend(
            queries
                .method_candidates(package_name, method_name)
                .into_iter()
                .filter(is_confident_method_candidate)
                .filter(|candidate| {
                    candidate.package.as_deref() == Some(expected_package.as_str())
                }),
        );
    }

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|left, right| {
        semantic_method_candidate_sort_key(package_name, left)
            .cmp(&semantic_method_candidate_sort_key(package_name, right))
    });
    candidates.dedup_by(|left, right| {
        left.display_name == right.display_name && left.package == right.package
    });

    Some(candidates)
}

fn method_symbol_defining_packages(
    method_symbols: &[&WorkspaceSymbol],
    receiver_package: &str,
) -> HashMap<String, String> {
    let mut packages = HashMap::new();
    for symbol in method_symbols {
        packages.entry(symbol.name.clone()).or_insert_with(|| {
            symbol.container_name.clone().unwrap_or_else(|| receiver_package.to_string())
        });
    }
    packages
}

fn is_confident_method_candidate(candidate: &DefinitionCandidate) -> bool {
    match candidate.kind {
        // Generated accessors are emitted with medium confidence because they
        // are inferred from framework declarations rather than explicit Perl
        // subroutines.  The workspace-index path is still authoritative enough
        // for inherited completion when the entity kind is preserved.
        EntityKind::GeneratedMember => {
            matches!(candidate.confidence, Confidence::Medium | Confidence::High)
        }
        EntityKind::Method | EntityKind::Subroutine => candidate.confidence == Confidence::High,
        _ => false,
    }
}

fn semantic_method_candidate_sort_key(
    receiver_package: &str,
    candidate: &DefinitionCandidate,
) -> (u8, String, String) {
    (
        semantic_method_sort_rank(receiver_package, candidate),
        candidate.display_name.clone(),
        candidate.package.clone().unwrap_or_default(),
    )
}

fn semantic_method_sort_tier(
    receiver_package: &str,
    candidate: &DefinitionCandidate,
) -> &'static str {
    match semantic_method_sort_rank(receiver_package, candidate) {
        0 => "2",
        1 => "3",
        _ => "4",
    }
}

fn semantic_method_sort_rank(receiver_package: &str, candidate: &DefinitionCandidate) -> u8 {
    if candidate.package.as_deref() == Some(receiver_package) {
        match candidate.kind {
            EntityKind::GeneratedMember => 1,
            _ => 0,
        }
    } else {
        2
    }
}

fn semantic_method_detail(
    receiver_package: &str,
    candidate: &DefinitionCandidate,
    evidence: &ReceiverEvidence,
) -> String {
    let defining_pkg = candidate.package.as_deref().unwrap_or(receiver_package);
    let base = match candidate.kind {
        EntityKind::GeneratedMember => format!("generated accessor from {defining_pkg}"),
        _ if defining_pkg == receiver_package => format!("method from {receiver_package}"),
        _ => format!("inherited method from {defining_pkg}"),
    };
    detail_with_evidence(base, evidence)
}

fn semantic_method_documentation(
    receiver_package: &str,
    candidate: &DefinitionCandidate,
) -> String {
    let defining_pkg = candidate.package.as_deref().unwrap_or(receiver_package);
    match candidate.kind {
        EntityKind::GeneratedMember => {
            format!("Generated method `{}` from `{defining_pkg}`.", candidate.display_name)
        }
        _ => format!("Method `{}::{}`.", defining_pkg, candidate.display_name),
    }
}

fn build_completion_package_graph(
    index: &WorkspaceIndex,
    source_uris: &HashSet<String>,
) -> PackageGraphIndex {
    const MAX_DISCOVERED_ROLE_FILES: usize = 32;

    let mut graph = PackageGraphIndex::new();
    // `source_uris` is a `HashSet`; make the bounded discovery order stable so
    // the cap cannot select different role files across hash iterations.
    let mut initial_uris: Vec<_> = source_uris.iter().cloned().collect();
    initial_uris.sort_unstable();
    let mut pending_uris = VecDeque::from(initial_uris);
    let mut visited_uris = HashSet::new();
    let max_files = source_uris.len().saturating_add(MAX_DISCOVERED_ROLE_FILES);

    while let Some(uri) = pending_uris.pop_front() {
        if visited_uris.len() >= max_files {
            break;
        }
        if !visited_uris.insert(uri.clone()) {
            continue;
        }
        let Some(text) = workspace_text_for_uri(index, &uri) else {
            continue;
        };
        let Ok(ast) = parse_workspace_source(&text) else {
            continue;
        };
        let model = SemanticModel::build(&ast, &text);
        let edges: Vec<PackageEdge> = model
            .package_edges()
            .iter()
            .filter(|edge| {
                matches!(edge.confidence, Confidence::High | Confidence::Medium)
                    && !matches!(edge.provenance, Provenance::DynamicBoundary)
            })
            .cloned()
            .collect();

        for edge in &edges {
            if edge.kind != PackageEdgeKind::ComposesRole {
                continue;
            }
            let Some(location) = index.find_definition(&edge.to_package) else {
                continue;
            };
            if !visited_uris.contains(&location.uri) {
                pending_uris.push_back(location.uri);
            }
        }

        if !edges.is_empty() {
            graph.add_edges(&uri, semantic_file_id(&uri), edges);
        }
    }

    graph
}

fn parse_workspace_source(text: &str) -> Result<perl_parser_core::ast::Node, String> {
    let mut parser = perl_semantic_analyzer::Parser::new(text);
    parser.parse().map_err(|err| err.to_string())
}

fn workspace_text_for_uri(index: &WorkspaceIndex, uri: &str) -> Option<String> {
    index.document_store().get_text(uri).or_else(|| {
        perl_workspace::workspace_index::uri_to_fs_path(uri)
            .and_then(|path| std::fs::read_to_string(path).ok())
    })
}

fn semantic_file_id(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}

#[cfg(test)]
mod visible_symbol_completion_tests {
    use super::{
        VisibleSymbol, VisibleSymbolSource, is_live_visible_completion_candidate,
        visible_symbol_has_import_authority,
    };
    use perl_semantic_facts::{Confidence, EntityId, VisibleSymbolContext};
    use std::collections::{HashMap, HashSet};

    fn visible(source: VisibleSymbolSource, confidence: Confidence) -> VisibleSymbol {
        VisibleSymbol {
            name: "candidate".to_string(),
            entity_id: Some(EntityId(1)),
            source,
            confidence,
            context: None,
        }
    }

    #[test]
    fn live_visible_completion_filter_accepts_only_high_confidence_import_export_sources() {
        assert!(is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::ExplicitImport,
            Confidence::High,
        )));
        assert!(is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::DefaultExport,
            Confidence::High,
        )));
        assert!(is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::ExportTag,
            Confidence::High,
        )));

        assert!(!is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::ExplicitImport,
            Confidence::Medium,
        )));
        assert!(!is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::Generated,
            Confidence::High,
        )));
        assert!(!is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::DynamicUnknown,
            Confidence::High,
        )));
        assert!(!is_live_visible_completion_candidate(&visible(
            VisibleSymbolSource::LocalLexical,
            Confidence::High,
        )));
    }

    #[test]
    fn explicit_runtime_import_without_use_module_entry_is_not_authority() {
        let symbol = VisibleSymbol {
            name: "bar".to_string(),
            entity_id: Some(EntityId(1)),
            source: VisibleSymbolSource::ExplicitImport,
            confidence: Confidence::High,
            context: Some(VisibleSymbolContext::new(Some("Foo".to_string()), None, None)),
        };
        let import_map = HashMap::from([("Foo".to_string(), HashSet::from(["bar".to_string()]))]);

        assert!(!visible_symbol_has_import_authority(&symbol, &import_map, &HashSet::new()));
    }

    fn default_export_symbol(name: &str) -> VisibleSymbol {
        VisibleSymbol {
            name: name.to_string(),
            entity_id: Some(EntityId(1)),
            source: VisibleSymbolSource::DefaultExport,
            confidence: Confidence::High,
            context: Some(VisibleSymbolContext::new(Some("Foo".to_string()), None, None)),
        }
    }

    /// A bare `use Foo;` records no import-map row; the compiler's
    /// `DefaultExport` fact is the authority for default-import visibility
    /// (#11158 preservation row).
    #[test]
    fn default_export_with_bare_use_is_authorized() {
        assert!(visible_symbol_has_import_authority(
            &default_export_symbol("defaulted"),
            &HashMap::new(),
            &HashSet::from(["Foo".to_string()]),
        ));
    }

    /// `use Foo ();` records an explicit empty-set row that replaces default
    /// imports; a `DefaultExport` fact must not survive it.
    #[test]
    fn default_export_with_explicit_empty_list_is_denied() {
        let import_map = HashMap::from([("Foo".to_string(), HashSet::new())]);

        assert!(!visible_symbol_has_import_authority(
            &default_export_symbol("defaulted"),
            &import_map,
            &HashSet::from(["Foo".to_string()]),
        ));
    }

    /// An explicit list without this symbol replaces default imports.
    #[test]
    fn default_export_with_explicit_list_excluding_symbol_is_denied() {
        let import_map = HashMap::from([("Foo".to_string(), HashSet::from(["other".to_string()]))]);

        assert!(!visible_symbol_has_import_authority(
            &default_export_symbol("defaulted"),
            &import_map,
            &HashSet::from(["Foo".to_string()]),
        ));
    }

    /// An explicit list naming this symbol still authorizes it.
    #[test]
    fn default_export_with_exact_membership_is_authorized() {
        let import_map =
            HashMap::from([("Foo".to_string(), HashSet::from(["defaulted".to_string()]))]);

        assert!(visible_symbol_has_import_authority(
            &default_export_symbol("defaulted"),
            &import_map,
            &HashSet::from(["Foo".to_string()]),
        ));
    }

    /// The document must reference the module at all: export facts from an
    /// indexed but unimported module stay non-authoritative (#11158).
    #[test]
    fn default_export_without_module_reference_is_denied() {
        assert!(!visible_symbol_has_import_authority(
            &default_export_symbol("defaulted"),
            &HashMap::new(),
            &HashSet::new(),
        ));
    }

    /// Non-default sources keep exact-row membership as their authority;
    /// module presence alone never authorizes an arbitrary member (#11158).
    #[test]
    fn explicit_import_still_requires_exact_row_membership() {
        let symbol = VisibleSymbol {
            name: "imported_fn".to_string(),
            entity_id: Some(EntityId(2)),
            source: VisibleSymbolSource::ExplicitImport,
            confidence: Confidence::High,
            context: Some(VisibleSymbolContext::new(Some("Foo".to_string()), None, None)),
        };

        assert!(!visible_symbol_has_import_authority(
            &symbol,
            &HashMap::new(),
            &HashSet::from(["Foo".to_string()]),
        ));
        let import_map =
            HashMap::from([("Foo".to_string(), HashSet::from(["imported_fn".to_string()]))]);
        assert!(visible_symbol_has_import_authority(
            &symbol,
            &import_map,
            &HashSet::from(["Foo".to_string()]),
        ));
    }
}

/// Collect all method symbols accessible from a package, following parent/role chains.
///
/// Traverses the inheritance graph starting at `package_name`, collecting
/// subroutine and method symbols from each package in MRO order.
/// Child-defined methods shadow parent methods — the first occurrence of each name wins.
///
/// MRO handling:
/// - Default (DFS): leftmost-depth-first @ISA traversal, matching Perl's
///   default method resolution order.
/// - C3 (`use mro 'c3'`): C3 linearization of @ISA ancestors, matching
///   Perl's C3 MRO pragma (#6326).
/// - Role composition: roles are appended after @ISA ancestors in BFS order,
///   distinct from @ISA MRO ordering per the issue's non-goals.
///
/// Edge-case handling:
/// - Diamond inheritance: visited-set prevents duplicate traversal.
/// - Circular `@ISA`: visited-set + depth bound prevents infinite loops.
/// - Package not indexed: `get_package_members` returns `Vec::new()` gracefully.
/// - `use parent -norequire`: already handled by `ClassModelBuilder`; model.parents
///   contains the parent names regardless.
pub(super) fn collect_all_package_members(
    index: &WorkspaceIndex,
    package_name: &str,
) -> Vec<WorkspaceSymbol> {
    collect_all_package_members_with_source(index, package_name, "")
}

/// Collect package members and use the current open document as a model source
/// when the receiver package has not been indexed yet. This keeps completion
/// useful during editing while retaining the workspace index as the authority
/// for persisted members and inherited packages.
///
/// Current-document class models are parsed once and preferred over persisted
/// index members for packages declared in the open buffer (#16809). Indexed
/// parents and roles are still unioned so an unsaved buffer that omits
/// `use parent` / `with` does not drop the persisted ISA or role graph.
/// This adapter retires when [`WorkspaceSemanticQueries`] can consume a
/// source-only shard for the accepted document generation.
fn collect_all_package_members_with_source(
    index: &WorkspaceIndex,
    package_name: &str,
    source: &str,
) -> Vec<WorkspaceSymbol> {
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut result: Vec<WorkspaceSymbol> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let (current_models, current_methods) = current_document_package_facts(source);
    let mut model_cache: HashMap<String, SourcePackageFacts> = HashMap::new();

    let load_model = |pkg: &str, cache: &mut HashMap<String, SourcePackageFacts>| {
        cache
            .entry(pkg.to_string())
            .or_insert_with(|| {
                load_source_package_facts(pkg, index, &current_models, &current_methods)
            })
            .clone()
    };

    // DFS traversal honoring MRO: visit receiver first, then @ISA ancestors
    // in MRO order, then roles. This ensures child definitions shadow parents.
    fn visit_mro(
        pkg: &str,
        index: &WorkspaceIndex,
        load_model: &impl Fn(&str, &mut HashMap<String, SourcePackageFacts>) -> SourcePackageFacts,
        model_cache: &mut HashMap<String, SourcePackageFacts>,
        visited: &mut HashSet<String>,
        seen_names: &mut HashSet<String>,
        result: &mut Vec<WorkspaceSymbol>,
        depth: usize,
    ) {
        const MAX_DEPTH: usize = 50;
        if depth >= MAX_DEPTH || !visited.insert(pkg.to_string()) {
            return;
        }

        let facts = load_model(pkg, model_cache);
        if facts.from_current_document {
            for symbol in facts.methods {
                if seen_names.insert(symbol.name.clone()) {
                    result.push(symbol);
                }
            }
            // Current-buffer source wins on name collision. Still consume
            // persisted generated members and non-colliding explicit index
            // methods so a split-file package does not drop workspace facts
            // (#16809 combined with defining-class invocant completion).
            push_index_method_symbols(index.get_generated_package_members(pkg), seen_names, result);
            push_index_method_symbols(index.get_package_members(pkg), seen_names, result);
        } else {
            push_index_method_symbols(
                index
                    .get_package_members(pkg)
                    .into_iter()
                    .chain(index.get_generated_package_members(pkg)),
                seen_names,
                result,
            );
        }

        // Traverse @ISA ancestors in MRO order. C3 uses the same parent walk as
        // DFS here: completion only needs consistent visitation, not a second
        // linearization algorithm (#6326).
        for parent in &facts.parents {
            visit_mro(
                parent,
                index,
                load_model,
                model_cache,
                visited,
                seen_names,
                result,
                depth + 1,
            );
        }

        // Traverse roles after @ISA (role composition is distinct from MRO)
        for role in &facts.roles {
            visit_mro(role, index, load_model, model_cache, visited, seen_names, result, depth + 1);
        }
    }

    visit_mro(
        package_name,
        index,
        &load_model,
        &mut model_cache,
        &mut visited,
        &mut seen_names,
        &mut result,
        0,
    );

    result
}

#[derive(Clone)]
struct SourcePackageFacts {
    parents: Vec<String>,
    roles: Vec<String>,
    methods: Vec<WorkspaceSymbol>,
    from_current_document: bool,
}

fn empty_source_package_facts() -> SourcePackageFacts {
    SourcePackageFacts {
        parents: Vec::new(),
        roles: Vec::new(),
        methods: Vec::new(),
        from_current_document: false,
    }
}

fn push_index_method_symbols(
    symbols: impl IntoIterator<Item = WorkspaceSymbol>,
    seen_names: &mut HashSet<String>,
    result: &mut Vec<WorkspaceSymbol>,
) {
    for symbol in symbols {
        match symbol.kind {
            WsSymbolKind::Subroutine | WsSymbolKind::Method => {}
            _ => continue,
        }
        if seen_names.insert(symbol.name.clone()) {
            result.push(symbol);
        }
    }
}

fn current_document_package_facts(
    source: &str,
) -> (
    HashMap<String, perl_semantic_analyzer::class_model::ClassModel>,
    HashMap<String, Vec<WorkspaceSymbol>>,
) {
    if source.is_empty() {
        return (HashMap::new(), HashMap::new());
    }
    let mut parser = perl_semantic_analyzer::Parser::new(source);
    let Ok(ast) = parser.parse() else {
        return (HashMap::new(), HashMap::new());
    };
    let models = perl_semantic_analyzer::class_model::ClassModelBuilder::new()
        .build(&ast)
        .into_iter()
        .map(|model| (model.name.clone(), model))
        .collect();
    (models, current_document_methods_from_ast(&ast, source))
}

fn current_document_methods_from_ast(
    ast: &perl_semantic_analyzer::Node,
    source: &str,
) -> HashMap<String, Vec<WorkspaceSymbol>> {
    let table =
        perl_semantic_analyzer::symbol::SymbolExtractor::new_with_source(source).extract(ast);
    let mut methods: HashMap<String, Vec<WorkspaceSymbol>> = HashMap::new();
    for symbols in table.symbols.values() {
        for symbol in symbols {
            if !matches!(
                symbol.kind,
                perl_semantic_analyzer::symbol::SymbolKind::Subroutine
                    | perl_semantic_analyzer::symbol::SymbolKind::Method
            ) {
                continue;
            }
            if matches!(symbol.declaration.as_deref(), Some("my") | Some("state")) {
                continue;
            }
            let package =
                package_name_from_qualified(&symbol.qualified_name).unwrap_or("main").to_string();
            if let Some(member) = current_document_method_symbol_from_parts(
                &package,
                &symbol.name,
                symbol.location.start,
                symbol.location.end,
            ) {
                methods.entry(package).or_default().push(member);
            }
        }
    }
    methods
}

fn package_name_from_qualified(qualified_name: &str) -> Option<&str> {
    qualified_name.rsplit_once("::").map(|(package, _)| package)
}

fn load_source_package_facts(
    pkg: &str,
    index: &WorkspaceIndex,
    current_models: &HashMap<String, perl_semantic_analyzer::class_model::ClassModel>,
    current_methods: &HashMap<String, Vec<WorkspaceSymbol>>,
) -> SourcePackageFacts {
    if let Some(model) = current_models.get(pkg) {
        let methods = current_methods.get(pkg).cloned().unwrap_or_else(|| {
            model
                .methods
                .iter()
                .filter_map(|method| current_document_method_symbol(&model.name, method))
                .collect()
        });
        let indexed = load_indexed_source_package_facts(pkg, index);
        return SourcePackageFacts {
            parents: merge_named_edges(model.parents.clone(), indexed.parents),
            roles: merge_named_edges(model.roles.clone(), indexed.roles),
            methods,
            from_current_document: true,
        };
    }

    if let Some(methods) = current_methods.get(pkg) {
        let indexed = load_indexed_source_package_facts(pkg, index);
        return SourcePackageFacts {
            parents: indexed.parents,
            roles: indexed.roles,
            methods: methods.clone(),
            from_current_document: true,
        };
    }

    load_indexed_source_package_facts(pkg, index)
}

fn merge_named_edges(mut preferred: Vec<String>, extra: Vec<String>) -> Vec<String> {
    for name in extra {
        if !preferred.iter().any(|existing| existing == &name) {
            preferred.push(name);
        }
    }
    preferred
}

fn load_indexed_source_package_facts(pkg: &str, index: &WorkspaceIndex) -> SourcePackageFacts {
    let indexed_text = index.find_definition(pkg).and_then(|pkg_location| {
        index.document_store().get_text(&pkg_location.uri).or_else(|| {
            perl_workspace::workspace_index::uri_to_fs_path(&pkg_location.uri)
                .and_then(|path| std::fs::read_to_string(path).ok())
        })
    });
    let Some(text) = indexed_text else {
        return empty_source_package_facts();
    };
    let mut parser = perl_semantic_analyzer::Parser::new(&text);
    let Ok(ast) = parser.parse() else {
        return empty_source_package_facts();
    };
    perl_semantic_analyzer::class_model::ClassModelBuilder::new()
        .build(&ast)
        .into_iter()
        .find(|model| model.name == pkg)
        .map(|model| source_package_facts_from_model(&model, false))
        .unwrap_or_else(empty_source_package_facts)
}

fn source_package_facts_from_model(
    model: &perl_semantic_analyzer::class_model::ClassModel,
    from_current_document: bool,
) -> SourcePackageFacts {
    let methods = if from_current_document {
        model
            .methods
            .iter()
            .filter_map(|method| current_document_method_symbol(&model.name, method))
            .collect()
    } else {
        Vec::new()
    };
    SourcePackageFacts {
        parents: model.parents.clone(),
        roles: model.roles.clone(),
        methods,
        from_current_document,
    }
}

fn current_document_method_symbol(
    package: &str,
    method: &perl_semantic_analyzer::class_model::MethodInfo,
) -> Option<WorkspaceSymbol> {
    if matches!(method.declarator.as_deref(), Some("my") | Some("state")) {
        return None;
    }
    current_document_method_symbol_from_parts(
        package,
        &method.name,
        method.location.start,
        method.location.end,
    )
}

fn current_document_method_symbol_from_parts(
    package: &str,
    name: &str,
    start: usize,
    end: usize,
) -> Option<WorkspaceSymbol> {
    if name.is_empty() {
        return None;
    }
    Some(WorkspaceSymbol {
        name: name.to_string(),
        kind: WsSymbolKind::Method,
        uri: String::new(),
        range: Range {
            start: Position { byte: start, line: 1, column: 1 },
            end: Position { byte: end.max(start), line: 1, column: 1 },
        },
        qualified_name: Some(format!("{package}::{name}")),
        documentation: None,
        container_name: Some(package.to_string()),
        has_body: true,
        workspace_folder_uri: None,
        is_lexical: false,
    })
}

#[cfg(test)]
mod collect_all_tests {
    use super::*;
    use perl_tdd_support::must;
    use perl_workspace::workspace::workspace_index::WorkspaceIndex;
    use std::sync::Arc;
    use url::Url;

    fn inherited_moo_parent_index() -> Arc<WorkspaceIndex> {
        let index = Arc::new(WorkspaceIndex::new());
        let parent_uri = must(Url::parse("file:///workspace/Parent.pm"));
        must(
            index.index_file(
                parent_uri,
                r#"package Parent;
use Moo;
has 'name' => (is => 'ro', isa => 'Str');
has 'status' => (
    is => 'rw',
    predicate => 1,
    builder => 1,
    clearer => 1,
);
1;
"#
                .to_string(),
            ),
        );
        index
    }

    #[test]
    fn collect_all_keeps_indexed_parents_when_current_document_omits_isa() {
        let index = Arc::new(WorkspaceIndex::new());
        must(index.index_file(
            must(Url::parse("file:///workspace/Parent.pm")),
            "package Parent;\nsub parent_method { 1 }\n1;\n".to_string(),
        ));
        must(index.index_file(
            must(Url::parse("file:///workspace/Child.pm")),
            "package Child;\nuse parent 'Parent';\nsub child_method { 1 }\n1;\n".to_string(),
        ));
        let child_source = "package Child;\nsub do_thing {\n    my ($obj) = @_;\n    $obj->\n";
        let members =
            collect_all_package_members_with_source(index.as_ref(), "Child", child_source);
        let names: Vec<_> = members.iter().map(|member| member.name.as_str()).collect();
        assert!(
            names.contains(&"child_method"),
            "current-document Child method must remain, got {names:?}"
        );
        assert!(
            names.contains(&"parent_method"),
            "indexed Child ISA must survive an open buffer that omits use parent, got {names:?}"
        );
    }

    #[test]
    fn collect_all_follows_parent_generated_members() {
        let index = inherited_moo_parent_index();
        assert!(index.has_symbols(), "parent-only Moo index should be populated");
        let child_source = r#"
package Child;
use Moo;
use parent 'Parent';

sub greet {
    my $self = shift;
    $self->
}
"#;
        let members =
            collect_all_package_members_with_source(index.as_ref(), "Child", child_source);
        let names: Vec<_> = members.iter().map(|member| member.name.as_str()).collect();
        assert!(
            names.contains(&"name"),
            "expected inherited generated reader from Parent, got {names:?}"
        );
    }

    #[test]
    fn collect_all_uses_current_source_role_methods_when_index_is_empty() {
        let index = Arc::new(WorkspaceIndex::new());
        assert!(!index.has_symbols(), "empty index is the #16809 startup fixture");
        let source = r#"
package Printable;
use Moo::Role;
sub stringify { "ok" }
package User;
use Moo;
with 'Printable';
"#;
        let members = collect_all_package_members_with_source(index.as_ref(), "User", source);
        let names: Vec<_> = members.iter().map(|member| member.name.as_str()).collect();
        assert!(
            names.contains(&"stringify"),
            "same-file composed-role method must be collected before indexing, got {names:?}"
        );
        let stringify = members.iter().find(|member| member.name == "stringify");
        assert_eq!(
            stringify.and_then(|member| member.container_name.as_deref()),
            Some("Printable")
        );
    }

    #[test]
    fn collect_all_keeps_indexed_generated_members_for_current_document_package() {
        let source = r#"
package User;
use Moo;
has 'name' => (is => 'ro', isa => 'Str');
sub own_method { 1 }
"#;
        let index = Arc::new(WorkspaceIndex::new());
        must(
            index.index_initial_file(
                must(Url::parse("file:///workspace/User.pm")),
                source.to_string(),
            ),
        );
        assert!(index.has_symbols(), "indexed current-document package must publish symbols");
        let members = collect_all_package_members_with_source(index.as_ref(), "User", source);
        let names: Vec<_> = members.iter().map(|member| member.name.as_str()).collect();
        assert!(
            names.contains(&"own_method"),
            "current-document method must still be collected, got {names:?}"
        );
        assert!(
            names.contains(&"name"),
            "indexed generated reader must remain after current-document composition, got {names:?}"
        );
    }

    #[test]
    fn collect_all_keeps_indexed_explicit_members_for_current_document_package() {
        let indexed = r#"
package User;
sub indexed_only { 1 }
sub own_method { "index" }
"#;
        let current = r#"
package User;
sub own_method { "current" }
sub consume { 1 }
"#;
        let index = Arc::new(WorkspaceIndex::new());
        must(index.index_file(must(Url::parse("file:///workspace/User.pm")), indexed.to_string()));
        let members = collect_all_package_members_with_source(index.as_ref(), "User", current);
        let names: Vec<_> = members.iter().map(|member| member.name.as_str()).collect();
        assert!(
            names.contains(&"consume"),
            "current-document method must still be collected, got {names:?}"
        );
        assert!(
            names.contains(&"indexed_only"),
            "non-colliding explicit index method must remain after current-document composition, got {names:?}"
        );
        assert!(
            names.contains(&"own_method"),
            "colliding own_method must still be collected, got {names:?}"
        );
        let own_uri = members
            .iter()
            .find(|member| member.name == "own_method")
            .map(|member| member.uri.as_str());
        assert_eq!(
            own_uri,
            Some(""),
            "current-document own_method must win the name collision, got {own_uri:?}"
        );
    }
}

/// Tests for union-receiver method completion (#9500).
///
/// These tests exercise `add_union_receiver_method_completions` directly.
/// The discriminating test `union_receiver_surfaces_methods_from_second_arm`
/// verifies that dropping any union arm would cause a test failure — the
/// contract required by #9500.
#[cfg(test)]
mod union_receiver_method_completion_tests {
    use super::*;
    use perl_tdd_support::must;
    use perl_workspace::workspace::workspace_index::WorkspaceIndex;
    use std::sync::Arc;
    use url::Url;

    /// Index with `Foo` (has `shared_method` + `foo_only`) and
    /// `Bar` (has `shared_method` + `bar_only`).
    fn two_package_index() -> Arc<WorkspaceIndex> {
        let index = Arc::new(WorkspaceIndex::new());

        let foo_uri = must(Url::parse("file:///workspace/Foo.pm"));
        must(index.index_file(
            foo_uri,
            "package Foo;\nsub shared_method { }\nsub foo_only { }\n1;\n".to_string(),
        ));

        let bar_uri = must(Url::parse("file:///workspace/Bar.pm"));
        must(index.index_file(
            bar_uri,
            "package Bar;\nsub shared_method { }\nsub bar_only { }\n1;\n".to_string(),
        ));

        index
    }

    fn arrow_context(source: &str) -> CompletionContext {
        let position = source.len();
        CompletionContext {
            position,
            trigger_character: Some('>'),
            in_string: false,
            in_regex: false,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: source.to_string(),
            prefix_start: 0,
            cursor_scope_id: 0,
        }
    }

    /// Discriminating test for #9500: if the second union arm is dropped,
    /// `bar_only` would be absent from the completions.
    #[test]
    fn union_receiver_surfaces_methods_from_second_arm() {
        let index = two_package_index();
        let source = "$obj->";
        let context = arrow_context(source);
        let mut completions: Vec<CompletionItem> = Vec::new();

        add_union_receiver_method_completions(
            &mut completions,
            &context,
            source,
            &index,
            &["Foo".to_string(), "Bar".to_string()],
        );

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_ref()).collect();

        assert!(labels.contains(&"shared_method"), "shared_method should appear; got {labels:?}");
        assert!(labels.contains(&"foo_only"), "foo_only (Foo arm) should appear; got {labels:?}");
        // This assertion would FAIL if the second union candidate were dropped.
        assert!(
            labels.contains(&"bar_only"),
            "bar_only (Bar arm, second candidate) should appear; got {labels:?}"
        );
    }

    /// Shared methods must not appear more than once even though both arms define them.
    #[test]
    fn union_receiver_deduplicates_shared_method() {
        let index = two_package_index();
        let source = "$obj->";
        let context = arrow_context(source);
        let mut completions: Vec<CompletionItem> = Vec::new();

        add_union_receiver_method_completions(
            &mut completions,
            &context,
            source,
            &index,
            &["Foo".to_string(), "Bar".to_string()],
        );

        let count = completions.iter().filter(|c| c.label.as_ref() == "shared_method").count();
        assert_eq!(count, 1, "shared_method should appear exactly once, not duplicated");
    }

    /// Shared-interface methods (in every arm) must rank above partial-interface
    /// methods (in at least one arm) via sort tiers `2u_` vs `3u_`.
    #[test]
    fn shared_method_gets_shared_sort_tier_and_partial_gets_partial_tier() {
        let index = two_package_index();
        let source = "$obj->";
        let context = arrow_context(source);
        let mut completions: Vec<CompletionItem> = Vec::new();

        add_union_receiver_method_completions(
            &mut completions,
            &context,
            source,
            &index,
            &["Foo".to_string(), "Bar".to_string()],
        );

        let shared = completions.iter().find(|c| c.label.as_ref() == "shared_method");
        let partial = completions.iter().find(|c| c.label.as_ref() == "foo_only");

        let shared_sort = shared.and_then(|c| c.sort_text.as_ref()).map(|s| s.as_ref().to_string());
        let partial_sort =
            partial.and_then(|c| c.sort_text.as_ref()).map(|s| s.as_ref().to_string());

        assert!(
            shared_sort.as_deref().is_some_and(|s| s.starts_with("2u_")),
            "shared_method should have sort tier 2u_, got {shared_sort:?}"
        );
        assert!(
            partial_sort.as_deref().is_some_and(|s| s.starts_with("3u_")),
            "foo_only should have sort tier 3u_, got {partial_sort:?}"
        );
    }
}
