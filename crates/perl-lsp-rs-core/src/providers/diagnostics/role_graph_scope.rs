//! Scoped package-graph construction for PL303 cross-file role-conflict detection.
//!
//! The production `WorkspaceIndex` holds a persistent `PackageGraphIndex` that
//! contains only `Inherits` edges emitted by the HIR lowerer. `ComposesRole`
//! edges are extracted by `perl-semantic-analyzer`'s
//! `PackageGraphExtractor`, which `perl-workspace` cannot depend on (it is a
//! downstream crate). This module builds a *request-scoped* graph containing
//! `ComposesRole` edges for the roles consumed by a single source file, bounded
//! by a BFS fanout cap so it never parses the whole workspace on every
//! diagnostics run.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use perl_parser_core::ast::Node;
use perl_semantic_analyzer::{
    Parser, analysis::class_model::ClassModelBuilder, semantic::SemanticModel,
};
use perl_semantic_facts::{Confidence, FileId, PackageEdge, PackageEdgeKind, Provenance};
use perl_workspace::semantic::package_graph::PackageGraphIndex;
use perl_workspace::workspace_index::{WorkspaceIndex, uri_to_fs_path};

/// Maximum number of files parsed when building the role-scoped graph.
/// Acts as a hard BFS cap to prevent unbounded parsing on large role hierarchies.
const MAX_ROLE_GRAPH_FILES: usize = 32;

/// Extract all role names consumed by any class or role declared in `ast`.
///
/// Returns an empty `Vec` for files with no `with '...'` clauses — the common
/// case — allowing the caller to skip graph construction entirely.
pub fn consumed_role_names(ast: &Node) -> Vec<String> {
    ClassModelBuilder::new()
        .build(ast)
        .into_iter()
        .flat_map(|model| model.roles.into_iter())
        .collect()
}

/// Build a `PackageGraphIndex` scoped to the roles consumed by one source file.
///
/// Starting from `seed_roles` (the roles the current file consumes), performs
/// a bounded BFS: resolves each role name to its defining URI via the workspace
/// index, parses that file, extracts `High|Medium` confidence `ComposesRole`
/// edges, and enqueues any transitively composed roles. Halts when
/// `MAX_ROLE_GRAPH_FILES` distinct files have been parsed or the queue is empty.
///
/// Roles that cannot be resolved (external, unindexed, or dynamically composed)
/// are silently skipped — the lint stays conservative and never guesses.
///
/// The `current_uri` is pre-inserted into the visited set so the current file
/// is never re-parsed by this function.
pub fn build_role_scoped_package_graph(
    index: &WorkspaceIndex,
    seed_roles: &[String],
    current_uri: &str,
) -> PackageGraphIndex {
    let mut graph = PackageGraphIndex::new();
    let mut visited_uris: HashSet<String> = HashSet::new();
    let mut queued_roles: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    visited_uris.insert(current_uri.to_string());

    for role in seed_roles {
        if queued_roles.insert(role.clone()) {
            queue.push_back(role.clone());
        }
    }

    while let Some(role_name) = queue.pop_front() {
        if visited_uris.len() > MAX_ROLE_GRAPH_FILES {
            break;
        }

        let Some(location) = index.find_definition(&role_name) else {
            continue;
        };
        let uri = location.uri.clone();

        if !visited_uris.insert(uri.clone()) {
            continue;
        }

        let Some(text) = text_for_uri(index, &uri) else {
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

        if !edges.is_empty() {
            graph.add_edges(&uri, file_id_for_uri(&uri), edges.clone());
        }

        for edge in &edges {
            if edge.kind == PackageEdgeKind::ComposesRole
                && queued_roles.insert(edge.to_package.clone())
            {
                queue.push_back(edge.to_package.clone());
            }
        }
    }

    graph
}

fn text_for_uri(index: &WorkspaceIndex, uri: &str) -> Option<String> {
    index
        .document_store()
        .get_text(uri)
        .or_else(|| uri_to_fs_path(uri).and_then(|path| std::fs::read_to_string(path).ok()))
}

fn parse_source(text: &str) -> Result<Node, String> {
    let mut parser = Parser::new(text);
    parser.parse().map_err(|e| e.to_string())
}

fn file_id_for_uri(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}
