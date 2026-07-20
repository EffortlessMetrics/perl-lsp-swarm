//! Scoped package-graph builder for PL303 cross-file/transitive role-conflict diagnostics.
//!
//! Builds a bounded `PackageGraphIndex` with `ComposesRole` edges covering
//! the roles consumed by the file under analysis and their transitively-composed
//! roles. Used by the production diagnostics path to enable cross-file and
//! transitive PL303 detection without naively parsing the whole workspace.

use std::collections::{HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

use perl_semantic_analyzer::{Parser, class_model::ClassModelBuilder, semantic::SemanticModel};
use perl_semantic_facts::{Confidence, FileId, PackageEdgeKind, Provenance};
use perl_workspace::{
    semantic::package_graph::PackageGraphIndex,
    workspace_index::{WorkspaceIndex, uri_to_fs_path},
};

/// Maximum number of role files to parse during one diagnostics run.
const MAX_ROLE_GRAPH_FILES: usize = 32;

/// Extract the names of all roles consumed via `with 'Role'` across all
/// packages declared in `ast`.
///
/// Returns an empty vec when the AST has no `with` declarations (the common
/// case — no role graph parsing is needed).
pub fn consumed_role_names(ast: &perl_parser_core::ast::Node) -> Vec<String> {
    ClassModelBuilder::new().build(ast).into_iter().flat_map(|model| model.roles).collect()
}

/// Build a scoped [`PackageGraphIndex`] covering `seed_roles` and their
/// transitively-composed roles.
///
/// For each seed role, resolves its defining URI via the workspace symbol
/// index, parses that file, extracts [`SemanticModel::package_edges()`]
/// (which emits `ComposesRole` edges), and enqueues any new `ComposesRole`
/// targets for further BFS traversal. Bounded to [`MAX_ROLE_GRAPH_FILES`]
/// parsed files.
///
/// Unresolved/external/dynamically-composed roles contribute no edges and
/// therefore no conflict — conservative behaviour is preserved.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_role_scoped_package_graph(
    index: &WorkspaceIndex,
    seed_roles: &[String],
    current_uri: &str,
) -> PackageGraphIndex {
    let mut graph = PackageGraphIndex::new();
    let mut visited_uris: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    // Seed: resolve each consumed role name to its definition URI.
    for role in seed_roles {
        if let Some(location) = index.find_definition(role) {
            if location.uri != current_uri && !visited_uris.contains(&location.uri) {
                queue.push_back(location.uri);
            }
        }
    }

    while let Some(uri) = queue.pop_front() {
        if visited_uris.len() >= MAX_ROLE_GRAPH_FILES {
            break;
        }
        if visited_uris.contains(&uri) {
            continue;
        }
        visited_uris.insert(uri.clone());

        let Some(text) = text_for_uri(index, &uri) else { continue };
        let Some(ast) = parse_source(&text) else { continue };

        let model = SemanticModel::build(&ast, &text);
        let file_id = uri_to_file_id(&uri);

        let edges: Vec<_> = model
            .package_edges()
            .iter()
            .filter(|edge| {
                matches!(edge.confidence, Confidence::High | Confidence::Medium)
                    && !matches!(edge.provenance, Provenance::DynamicBoundary)
            })
            .cloned()
            .collect();

        // Enqueue new ComposesRole targets for BFS.
        for edge in &edges {
            if edge.kind == PackageEdgeKind::ComposesRole {
                if let Some(loc) = index.find_definition(&edge.to_package) {
                    if !visited_uris.contains(&loc.uri) {
                        queue.push_back(loc.uri);
                    }
                }
            }
        }

        if !edges.is_empty() {
            graph.add_edges(&uri, file_id, edges);
        }
    }

    graph
}

#[cfg(not(target_arch = "wasm32"))]
fn text_for_uri(index: &WorkspaceIndex, uri: &str) -> Option<String> {
    index
        .document_store()
        .get_text(uri)
        .or_else(|| uri_to_fs_path(uri).and_then(|path| std::fs::read_to_string(path).ok()))
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_source(text: &str) -> Option<perl_parser_core::ast::Node> {
    let mut parser = Parser::new(text);
    parser.parse().ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn uri_to_file_id(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}
