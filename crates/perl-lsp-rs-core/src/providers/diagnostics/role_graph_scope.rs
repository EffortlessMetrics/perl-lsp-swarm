//! Build a request-scoped [`PackageGraphIndex`] covering the roles consumed by
//! the file under analysis.
//!
//! The workspace's persistent [`PackageGraphIndex`] holds only `Inherits` edges
//! (populated from HIR); the HIR lowerer never emits `ComposesRole` edges.
//! This module builds a lightweight, per-request graph that does contain those
//! edges so that [`transitive_role_methods`] can resolve cross-file role
//! providers for PL303 diagnostics.
//!
//! The BFS is bounded to [`MAX_ROLE_GRAPH_FILES`] files and skips roles whose
//! source cannot be found — the lint stays conservative (fail-closed).
//!
//! [`transitive_role_methods`]: perl_workspace::semantic::queries::SemanticQueries::transitive_role_methods

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use perl_parser_core::ast::Node;
use perl_semantic_analyzer::{Parser, class_model::ClassModelBuilder, semantic::SemanticModel};
use perl_semantic_facts::{Confidence, FileId, PackageEdge, PackageEdgeKind, Provenance};
use perl_workspace::semantic::package_graph::PackageGraphIndex;
use perl_workspace::workspace_index::{WorkspaceIndex, uri_to_fs_path};

/// Maximum number of source files parsed during a single role-graph BFS.
/// Keeps per-request cost bounded even for deep role hierarchies.
const MAX_ROLE_GRAPH_FILES: usize = 32;

/// Return all role names directly consumed by classes in `ast` (the `.roles`
/// fields of every [`ClassModel`] in the file, flattened and deduplicated).
///
/// Returns an empty `Vec` for files that contain no `with '...'` clauses.
/// The caller uses this as a fast-path gate: if empty, the existing persistent
/// graph path is used unchanged and no extra parsing occurs.
///
/// [`ClassModel`]: perl_semantic_analyzer::class_model::ClassModel
pub fn consumed_role_names(ast: &Node) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for model in ClassModelBuilder::new().build(ast) {
        for role in model.roles {
            if seen.insert(role.clone()) {
                result.push(role);
            }
        }
    }
    result
}

/// Build a [`PackageGraphIndex`] containing `ComposesRole` edges for the
/// transitive closure of `seed_roles`, bounded by [`MAX_ROLE_GRAPH_FILES`].
///
/// The BFS resolves each role name to its definition URI via the workspace
/// index, reads the source, parses it, and extracts High/Medium-confidence
/// non-`DynamicBoundary` `ComposesRole` edges.  Newly discovered role packages
/// are enqueued for the next BFS round.  Roles that cannot be resolved or whose
/// source cannot be read are silently skipped.
///
/// The returned graph is intended to be passed to
/// [`WorkspaceIndex::with_semantic_queries_for_uri_and_graph`] to enable
/// [`transitive_role_methods`] in the diagnostics path.
///
/// [`transitive_role_methods`]: perl_workspace::semantic::queries::SemanticQueries::transitive_role_methods
pub fn build_role_scoped_package_graph(
    index: &WorkspaceIndex,
    seed_roles: &[String],
) -> PackageGraphIndex {
    let mut graph = PackageGraphIndex::new();
    let mut visited_uris: HashSet<String> = HashSet::new();
    let mut visited_packages: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    for role in seed_roles {
        if visited_packages.insert(role.clone()) {
            queue.push_back(role.clone());
        }
    }

    let mut files_parsed: usize = 0;

    while let Some(role_name) = queue.pop_front() {
        if files_parsed >= MAX_ROLE_GRAPH_FILES {
            break;
        }

        let location = match index.find_definition(&role_name) {
            Some(loc) => loc,
            None => continue,
        };
        let uri = location.uri;

        if !visited_uris.insert(uri.clone()) {
            continue;
        }
        files_parsed += 1;

        let text = match read_source_for_uri(index, &uri) {
            Some(t) => t,
            None => continue,
        };

        let ast = match parse_source(&text) {
            Ok(a) => a,
            Err(_) => continue,
        };

        let model = SemanticModel::build(&ast, &text);
        let edges: Vec<PackageEdge> = model
            .package_edges()
            .iter()
            .filter(|edge| {
                matches!(edge.kind, PackageEdgeKind::ComposesRole)
                    && matches!(edge.confidence, Confidence::High | Confidence::Medium)
                    && !matches!(edge.provenance, Provenance::DynamicBoundary)
            })
            .cloned()
            .collect();

        for edge in &edges {
            if visited_packages.insert(edge.to_package.clone()) {
                queue.push_back(edge.to_package.clone());
            }
        }

        if !edges.is_empty() {
            graph.add_edges(&uri, file_id_for_uri(&uri), edges);
        }
    }

    graph
}

fn read_source_for_uri(index: &WorkspaceIndex, uri: &str) -> Option<String> {
    index
        .document_store()
        .get_text(uri)
        .or_else(|| uri_to_fs_path(uri).and_then(|path| std::fs::read_to_string(path).ok()))
}

fn parse_source(text: &str) -> Result<Node, String> {
    let mut parser = Parser::new(text);
    parser.parse().map_err(|err| err.to_string())
}

fn file_id_for_uri(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}
