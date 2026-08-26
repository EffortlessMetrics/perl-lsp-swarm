//! Runtime wiring for the canonical Dancer2 provider slice (#8928).
//!
//! One request-scoped helper resolves the `Dancer2` module through the
//! request's effective `@INC`, reads its declared version (cached by file
//! identity), and builds the canonical activation and file facts for the
//! current snapshot. Every promoted cell consumes this single context, so
//! each request selects exactly one authority: canonical facts under exact
//! activation, otherwise zero framework output (never a legacy union).

use perl_lsp_rs_core::providers::dancer2::RuntimeDancer2Module;
use perl_lsp_rs_core::providers::dancer2::current_package_at;
use perl_lsp_rs_core::providers::dancer2::{
    canonical_file_facts, file_activations, read_declared_module_version,
};
use perl_semantic_facts::FileId;
use perl_semantic_facts::SourceGeneration;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

use super::super::LspServer;

/// One request-scoped Dancer2 context (activation + canonical facts).
pub(crate) struct Dancer2RequestContext {
    /// Canonical activation facts per package for this snapshot.
    pub activations: perl_lsp_rs_core::providers::dancer2::Dancer2FileActivations,
    /// Canonical minted facts for this snapshot.
    pub facts: perl_lsp_rs_core::providers::dancer2::CanonicalDancer2FileFacts,
}

/// Cached declared-version observation for one resolved module file.
struct VersionCacheEntry {
    modified_secs: u64,
    len: u64,
    version: Option<String>,
}

fn version_cache() -> &'static Mutex<HashMap<PathBuf, VersionCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, VersionCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn observe_dancer2_module(resolved_uri: &str) -> Option<RuntimeDancer2Module> {
    let path = crate::runtime::source_path_from_uri(resolved_uri)?;
    let metadata = std::fs::metadata(&path).ok()?;
    let modified_secs = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let len = metadata.len();

    let cache_key = path.clone();
    {
        let cache = version_cache().lock().ok()?;
        if let Some(entry) = cache.get(&cache_key)
            && entry.modified_secs == modified_secs
            && entry.len == len
        {
            return entry
                .version
                .clone()
                .map(|version| RuntimeDancer2Module::new(path.display().to_string(), version));
        }
    }

    // Bounded read: module version declarations live in the header; a cap
    // keeps this honest even for very large distributions.
    let version = std::fs::read_to_string(&path).ok().and_then(|source| {
        let header: String = source.lines().take(400).collect::<Vec<&str>>().join("\n");
        read_declared_module_version(&header)
    });

    if let Ok(mut cache) = version_cache().lock() {
        cache.insert(cache_key, VersionCacheEntry { modified_secs, len, version: version.clone() });
    }
    version.map(|version| RuntimeDancer2Module::new(path.display().to_string(), version))
}

impl LspServer {
    /// Build the request-scoped Dancer2 context for one document snapshot.
    pub(crate) fn dancer2_request_context(
        &self,
        uri: &str,
        text: &str,
        content_hash: u64,
        ast: &perl_parser::ast::Node,
    ) -> Dancer2RequestContext {
        let generation = SourceGeneration::known(format!("lsp-doc:{content_hash:016x}"));
        let file_id = FileId(content_hash & 0xFFFF_FFFF);
        // Resolve at the first activation site's offset so position-aware
        // `@INC` state (`use lib` / `no lib` relative to the import) is
        // honored for the activation evidence.
        let activation_offset =
            perl_lsp_rs_core::providers::dancer2::first_activation_site_offset(ast);
        let module = self
            .resolve_module_to_path_with_doc_at_offset(
                "Dancer2",
                Some(text),
                Some(uri),
                activation_offset,
            )
            .as_deref()
            .and_then(observe_dancer2_module);
        let activations = file_activations(ast, file_id, module.as_ref(), &generation);
        let facts = canonical_file_facts(ast, file_id, &activations);
        Dancer2RequestContext { activations, facts }
    }

    /// The Dancer2 package scope at `offset` for this snapshot, if the
    /// document is Dancer2-relevant at all.
    pub(crate) fn dancer2_package_at(
        &self,
        uri: &str,
        text: &str,
        content_hash: u64,
        ast: &perl_parser::ast::Node,
        offset: usize,
    ) -> Option<(Dancer2RequestContext, String)> {
        let context = self.dancer2_request_context(uri, text, content_hash, ast);
        if context.activations.packages.is_empty() {
            return None;
        }
        let package = current_package_at(ast, offset).to_string();
        context.activations.for_package(&package)?;
        Some((context, package))
    }
}
