//! Scoped package-graph builder for cross-file PL303 role-conflict diagnostics.
//!
//! When a Perl file consumes roles via `with 'RoleName'` clauses, the
//! persistent workspace `PackageGraphIndex` may lack `ComposesRole` edges for
//! those roles (the HIR lowerer only emits `Inherits` edges). This module
//! builds a bounded, per-request `PackageGraphIndex` seeded from the file's
//! consumed roles, mirroring the idiom used by the completion provider's
//! `build_completion_package_graph`.
//!
//! The BFS is bounded to [`MAX_ROLE_GRAPH_FILES`] parsed files to keep
//! per-request cost proportional and avoid whole-workspace parses on every
//! diagnostics run.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use perl_parser_core::ast::Node;
use perl_semantic_analyzer::semantic::SemanticModel;
use perl_semantic_facts::{Confidence, FileId, PackageEdge, PackageEdgeKind, Provenance};
use perl_workspace::semantic::package_graph::PackageGraphIndex;
use perl_workspace::workspace_index::WorkspaceIndex;

/// Maximum number of role-defining files to parse per diagnostics request.
const MAX_ROLE_GRAPH_FILES: usize = 32;

/// Extract the flat list of role package names consumed by any package in
/// `ast` via `with 'RoleName'` (or equivalent) clauses.
///
/// Returns an empty `Vec` when the file has no `with` consumers — the caller
/// should skip the scoped-graph build as a fast path.
pub fn consumed_role_names(ast: &Arc<Node>) -> Vec<String> {
    perl_semantic_analyzer::class_model::ClassModelBuilder::new()
        .build(ast)
        .into_iter()
        .flat_map(|m| m.roles)
        .collect()
}

/// Build a bounded `PackageGraphIndex` seeded from `seed_roles`.
///
/// For each seed role the function resolves its defining URI via the workspace
/// index, fetches and re-parses the source, and adds its `ComposesRole` /
/// `Inherits` edges (High/Medium confidence, non-`DynamicBoundary`) to the
/// graph. New `ComposesRole` targets are enqueued for BFS traversal up to
/// [`MAX_ROLE_GRAPH_FILES`] total file parses.
///
/// Roles that cannot be resolved, fetched, or parsed are silently skipped —
/// the lint stays conservative and never guesses.
pub fn build_role_scoped_package_graph(
    index: &WorkspaceIndex,
    seed_roles: &[String],
) -> PackageGraphIndex {
    let mut graph = PackageGraphIndex::new();
    let mut visited_roles: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = seed_roles.to_vec();
    let mut files_parsed: usize = 0;

    while let Some(role_name) = queue.pop() {
        if !visited_roles.insert(role_name.clone()) {
            continue;
        }
        if files_parsed >= MAX_ROLE_GRAPH_FILES {
            break;
        }

        let Some(location) = index.find_definition(&role_name) else {
            continue;
        };
        let uri = &location.uri;

        let Some(text) = source_text_for_uri(index, uri) else {
            continue;
        };

        let Ok(ast) = parse_source(&text) else {
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

        // Enqueue any new ComposesRole targets for BFS.
        for edge in &edges {
            if edge.kind == PackageEdgeKind::ComposesRole
                && !visited_roles.contains(&edge.to_package)
            {
                queue.push(edge.to_package.clone());
            }
        }

        if !edges.is_empty() {
            graph.add_edges(uri, file_id_for_uri(uri), edges);
        }
        files_parsed += 1;
    }

    graph
}

fn source_text_for_uri(index: &WorkspaceIndex, uri: &str) -> Option<String> {
    index.document_store().get_text(uri).or_else(|| {
        perl_workspace::workspace_index::uri_to_fs_path(uri)
            .and_then(|path| std::fs::read_to_string(path).ok())
    })
}

fn parse_source(text: &str) -> Result<perl_parser_core::ast::Node, String> {
    let mut parser = perl_semantic_analyzer::Parser::new(text);
    parser.parse().map_err(|err| err.to_string())
}

fn file_id_for_uri(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}
