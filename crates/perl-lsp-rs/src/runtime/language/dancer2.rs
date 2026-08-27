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
    CanonicalDancer2FileFacts, Dancer2FileActivations, canonical_file_facts, file_activations,
    read_declared_module_version,
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
        // Cheap in-memory gate first: a document with no `use Dancer2`
        // activation site pays no filesystem module resolution at all —
        // the resolution walk is I/O and must not run on every request
        // for Dancer2-free files.
        if !perl_lsp_rs_core::providers::dancer2::has_activation_site(ast) {
            return Dancer2RequestContext {
                activations: Dancer2FileActivations::default(),
                facts: CanonicalDancer2FileFacts::default(),
            };
        }
        let generation = SourceGeneration::known(format!("lsp-doc:{content_hash:016x}"));
        let file_id = FileId(content_hash & 0xFFFF_FFFF);
        // Whole-file `@INC` state: position-aware activation-site anchoring
        // was tried and reverted — the per-folder relative-root semantics
        // it requires (resolving `use lib 'lib'` against the owning
        // workspace folder rather than the server root) are not provided by
        // the shared resolution layer today and broke multi-root
        // workspaces. Recorded as the boundary; revisit with folder-scoped
        // @INC resolution.
        let module = self
            .resolve_module_to_path_with_doc_at_offset("Dancer2", Some(text), Some(uri), None)
            .as_deref()
            .and_then(observe_dancer2_module);
        let activations = file_activations(ast, file_id, module.as_ref(), &generation);
        let facts = canonical_file_facts(ast, file_id, &activations);
        Dancer2RequestContext { activations, facts }
    }

    /// Resolve the `Dancer2` module anchored at the document's first
    /// activation site, binding every relative include root to the owning
    /// workspace folder (#12776).
    ///
    /// This is the provider-side consumer seam only. Position-aware
    /// `@INC` evaluation (`use lib` / `no lib` scoping) comes from the shared
    /// [`LspServer::effective_inc_context_for_doc`] evaluated at the
    /// activation-site byte offset; the folder scoping reuses the shared
    /// candidate collector with only the owning folder's URI so relative
    /// lexical/configured roots bind to their project instead of fanning out
    /// across every registered folder (multi-root same-name isolation,
    /// ux scenario 69). Absolute include roots, `PERL5LIB`, and interpreter
    /// startup `@INC` are folder-independent and pass through unchanged.
    ///
    /// General per-folder relative-root semantics remain owned by the
    /// module-resolution train (#4240; #10575, #8112, #10569); other
    /// consumers of `resolve_module_to_path_*` keep their shared-layer
    /// behavior.
    fn resolve_dancer2_module_at_activation(
        &self,
        uri: &str,
        text: &str,
        activation_offset: usize,
    ) -> Option<String> {
        let context =
            self.effective_inc_context_for_doc(Some(uri), Some(text), Some(activation_offset))?;
        let scoped_folder_uris: Vec<String> = match context.folder_uri.as_ref() {
            Some(owned_folder_uri) => vec![owned_folder_uri.clone()],
            None => {
                // No owning folder was detected: keep the shared all-folders
                // view rather than silently dropping configured roots.
                let folders = self.workspace_folders.lock().clone();
                folders.iter().map(|folder| folder.uri.clone()).collect()
            }
        };
        let open_document_uris: Vec<String> = {
            let documents = self.documents.lock();
            documents
                .keys()
                .filter(|open_uri| context.symbol_uri_reachable(open_uri))
                .cloned()
                .collect()
        };
        let timeout = std::time::Duration::from_millis(context.resolution_timeout_ms);
        match perl_module::resolve_module_uri_with_effective_inc(
            "Dancer2",
            &open_document_uris,
            &scoped_folder_uris,
            &context.effective_roots,
            timeout,
        ) {
            perl_module::ModuleUriResolution::Resolved(resolved_uri) => Some(resolved_uri),
            _ => None,
        }
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

#[cfg(test)]
mod activation_anchoring_tests {
    //! Activation-site anchoring discriminators (#12776).
    //!
    //! Red-first proofs over the real request-context seam:
    //! - a `use lib` appearing after `use Dancer2` must not retroactively
    //!   make the earlier activation exact;
    //! - a preceding `no lib` must cancel configured include roots;
    //! - a relative lexical root must resolve against the owning workspace
    //!   folder only (multi-root same-name isolation, ux scenario 69);
    //! - a preceding lexical root keeps anchoring the import (stability
    //!   control that passes before and after the fix).

    use super::*;
    use crate::runtime::workspace_folder::WorkspaceFolderState;
    use perl_lsp_rs_core::config::WorkspaceConfig;
    use std::path::{Path, PathBuf};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn parse_ast(source: &str) -> perl_parser::ast::Node {
        let mut parser = perl_parser::Parser::new(source);
        match parser.parse() {
            Ok(ast) => ast,
            Err(error) => panic!("fixture must parse: {error}"),
        }
    }

    fn stub_module(version: &str) -> String {
        format!("package Dancer2;\nour $VERSION = '{version}';\n1;\n")
    }

    /// Stub Dancer2 distribution layout: one folder root with its own
    /// `lib/Dancer2.pm`.
    fn make_app_folder(base: &Path, name: &str, version: &str) -> PathBuf {
        let folder = base.join(name);
        let lib = folder.join("lib");
        std::fs::create_dir_all(&lib).expect("create lib dir");
        std::fs::write(lib.join("Dancer2.pm"), stub_module(version)).expect("write stub module");
        folder
    }

    fn isolated_workspace_config() -> WorkspaceConfig {
        let mut config = WorkspaceConfig::default();
        config.use_system_inc = false;
        config.use_perl5lib = false;
        config
    }

    fn server_with_folders(folders: &[(&Path, Option<WorkspaceConfig>)]) -> LspServer {
        let server = LspServer::new();
        let mut states = Vec::new();
        for (path, config) in folders {
            let mut state =
                WorkspaceFolderState::new(path_uri(path)).with_path(path.to_path_buf());
            if let Some(config) = config {
                state = state.with_effective_workspace_config(config.clone());
            }
            states.push(state);
        }
        *server.workspace_folders.lock() = states;
        if let Some((first, _)) = folders.first() {
            *server.root_path.lock() = Some(first.to_path_buf());
        }
        server
    }

    fn path_uri(path: &Path) -> String {
        match url::Url::from_file_path(path) {
            Ok(url) => url.to_string(),
            Err(()) => panic!("fixture path must convert to a URI: {}", path.display()),
        }
    }

    fn normalized(path: &str) -> String {
        path.replace('\\', "/")
    }

    fn request_context(
        server: &LspServer,
        doc_path: &Path,
        source: &str,
    ) -> Dancer2RequestContext {
        let content_hash = perl_lsp_rs_core::tooling::perl_critic::hash_content(source);
        let ast = parse_ast(source);
        server.dancer2_request_context(&path_uri(doc_path), source, content_hash, &ast)
    }

    // A `use lib` placed after the import belongs to later code: it must not
    // retroactively make the earlier activation exact (#12654 review P2).
    #[test]
    fn use_lib_after_activation_does_not_exact_the_earlier_import() -> TestResult {
        let temp = tempfile::tempdir()?;
        let ws = temp.path().join("ws");
        let vendor = ws.join("vendor");
        std::fs::create_dir_all(&vendor)?;
        std::fs::write(vendor.join("Dancer2.pm"), stub_module("1.300.0"))?;

        let config = isolated_workspace_config();
        let server = server_with_folders(&[(&ws, Some(config))]);
        let doc = ws.join("bin").join("app.pl");
        let source = "use Dancer2;\nget '/x' => sub {1};\nuse lib 'vendor';\n";

        let context = request_context(&server, &doc, source);

        assert!(
            !context.activations.has_exact(),
            "later `use lib 'vendor'` must not retroactively exact the earlier import"
        );
        assert!(
            context.activations.module.is_none(),
            "activation-site @INC state excludes the later add: no module observation expected"
        );
        Ok(())
    }

    // Stability control: a lexical root declared BEFORE the import anchors it.
    #[test]
    fn preceding_use_lib_still_anchors_the_import() -> TestResult {
        let temp = tempfile::tempdir()?;
        let ws = temp.path().join("ws");
        let vendor = ws.join("vendor");
        std::fs::create_dir_all(&vendor)?;
        std::fs::write(vendor.join("Dancer2.pm"), stub_module("1.300.0"))?;

        let config = isolated_workspace_config();
        let server = server_with_folders(&[(&ws, Some(config))]);
        let doc = ws.join("bin").join("app.pl");
        let source = "use lib 'vendor';\nuse Dancer2;\n";

        let context = request_context(&server, &doc, source);

        assert!(context.activations.has_exact(), "preceding root still activates exactly");
        assert_eq!(
            context.activations.module.as_ref().map(|m| m.declared_version.as_str()),
            Some("1.300.0"),
            "exactness comes from the resolvable versioned module"
        );
        Ok(())
    }

    // A `no lib` before the import cancels configured include roots for the
    // activation site's resolution too.
    #[test]
    fn preceding_no_lib_cancels_configured_roots_for_the_activation() -> TestResult {
        let temp = tempfile::tempdir()?;
        let ws = temp.path().join("ws");
        let lib = ws.join("lib_ok");
        std::fs::create_dir_all(&lib)?;
        std::fs::write(lib.join("Dancer2.pm"), stub_module("7.7.7"))?;

        let mut config = isolated_workspace_config();
        config.include_paths = vec!["lib_ok".to_string()];
        let server = server_with_folders(&[(&ws, Some(config))]);
        let doc = ws.join("bin").join("app.pl");
        let source = "no lib 'lib_ok';\nuse Dancer2;\n";

        let context = request_context(&server, &doc, source);

        assert!(
            !context.activations.has_exact(),
            "a preceding `no lib` must cancel the configured root at the activation site"
        );
        assert!(
            context.activations.module.is_none(),
            "cancelled configured root leaves no module observation"
        );

        // Control: without the cancellation the configured root activates.
        let kept = "use Dancer2;\n";
        let context = request_context(&server, &doc, kept);
        assert!(
            context.activations.has_exact(),
            "without `no lib`, the configured root still resolves the module"
        );
        Ok(())
    }

    #[test]
    fn relative_lexical_root_resolves_against_owning_folder_only() -> TestResult {
        let temp = tempfile::tempdir()?;
        let svc_a = make_app_folder(temp.path(), "svc-a", "0.0.1");
        let svc_b = make_app_folder(temp.path(), "svc-b", "0.0.2");

        // Registration order matters: the owning folder must NOT be first.
        let server = server_with_folders(&[(&svc_a, None), (&svc_b, None)]);
        let doc_b = svc_b.join("bin").join("app.pl");
        let source = "use lib 'lib';\nuse Dancer2;\n";

        let context = request_context(&server, &doc_b, source);

        let module = context
            .activations
            .module
            .as_ref()
            .ok_or("expected an owning-folder module resolution")?;
        assert_eq!(module.declared_version, "0.0.2", "the owning folder's stub wins");
        assert_eq!(
            normalized(&module.resolved_path),
            normalized(svc_b.join("lib").join("Dancer2.pm").to_string_lossy().as_ref()),
            "relative lexical roots bind to the owning workspace folder, not folder order"
        );
        assert!(context.activations.has_exact());
        Ok(())
    }

    // Multi-root same-name route/app isolation law (ux scenario 69): each
    // root's app resolves its own versioned module even when names collide.
    #[test]
    fn multi_root_apps_each_resolve_within_their_own_folder() -> TestResult {
        let temp = tempfile::tempdir()?;
        let svc_a = make_app_folder(temp.path(), "svc-a", "0.0.1");
        let svc_b = make_app_folder(temp.path(), "svc-b", "0.0.2");

        let server = server_with_folders(&[(&svc_a, None), (&svc_b, None)]);
        let doc_a = svc_a.join("bin").join("app.pl");
        let doc_b = svc_b.join("bin").join("app.pl");
        let source = "use lib 'lib';\nuse Dancer2;\n";

        let context_a = request_context(&server, &doc_a, source);
        let context_b = request_context(&server, &doc_b, source);

        let module_a =
            context_a.activations.module.as_ref().ok_or("folder A must resolve its module")?;
        let module_b =
            context_b.activations.module.as_ref().ok_or("folder B must resolve its module")?;
        assert!(context_a.activations.has_exact() && context_b.activations.has_exact());
        assert_ne!(
            normalized(&module_a.resolved_path),
            normalized(&module_b.resolved_path),
            "same-name modules must stay isolated per root"
        );
        assert_eq!(module_a.declared_version, "0.0.1");
        assert_eq!(module_b.declared_version, "0.0.2");
        Ok(())
    }
}
