//! Scoped role-composition graph builder for PL303 cross-file diagnostics.
//!
//! The persistent workspace `PackageGraphIndex` carries only `Inherits` edges
//! (built from HIR stash data). `ComposesRole` edges require `SemanticModel`
//! from `perl-semantic-analyzer`, which cannot be a dependency of
//! `perl-workspace` without creating a cycle
//! (`perl-semantic-analyzer → perl-module → perl-workspace`).
//!
//! This module builds a request-scoped `PackageGraphIndex` with `ComposesRole`
//! edges by re-parsing role definition files on demand, bounded by a file-count
//! cap. The resulting graph is passed to
//! `WorkspaceIndex::with_semantic_queries_for_uri_and_graph` so that
//! `transitive_role_methods` can traverse cross-file role composition.

use std::collections::{HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

use perl_parser_core::ast::Node;
use perl_semantic_analyzer::{class_model::ClassModelBuilder, semantic::SemanticModel};
use perl_semantic_facts::{Confidence, FileId, PackageEdgeKind, Provenance};
use perl_workspace::{semantic::package_graph::PackageGraphIndex, workspace_index::WorkspaceIndex};

/// Maximum number of role-definition files parsed while building the scoped
/// graph. Prevents an unbounded parse sweep on deep/wide role hierarchies.
const MAX_ROLE_GRAPH_FILES: usize = 32;

/// Return the distinct role package names consumed by any class model in `node`.
///
/// Uses `ClassModelBuilder` to enumerate `with` clauses across all packages
/// declared in the file. Returns an empty vec when no roles are consumed,
/// enabling a fast-path skip of the graph build in callers.
pub fn consumed_role_names(node: &Node) -> Vec<String> {
    let mut seen = HashSet::new();
    for model in ClassModelBuilder::new().build(node) {
        for role in model.roles {
            seen.insert(role);
        }
    }
    seen.into_iter().collect()
}

/// Build a `PackageGraphIndex` scoped to the roles consumed by the file being
/// diagnosed plus their transitively composed roles.
///
/// Algorithm:
/// 1. Start with `seed_roles` (the consumed role names from `with` clauses).
/// 2. For each role not yet visited: resolve its definition URI via
///    `index.find_definition`, get the file text, parse it with `SemanticModel`,
///    extract `ComposesRole` (and `Inherits`) edges, add them to the graph, and
///    enqueue newly discovered `ComposesRole` targets.
/// 3. Stop after `MAX_ROLE_GRAPH_FILES` files to bound per-request parse cost.
///
/// Conservative failure modes: if a role's definition cannot be found or the
/// file cannot be parsed, that role contributes no edges (no guessed conflict).
pub fn build_role_scoped_package_graph(
    index: &WorkspaceIndex,
    seed_roles: &[String],
) -> PackageGraphIndex {
    let mut graph = PackageGraphIndex::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = seed_roles.iter().cloned().collect();
    let mut file_count = 0usize;

    while let Some(role_name) = queue.pop_front() {
        if !visited.insert(role_name.clone()) {
            continue;
        }
        if file_count >= MAX_ROLE_GRAPH_FILES {
            break;
        }

        let Some(location) = index.find_definition(&role_name) else {
            continue;
        };

        let Some(text) = workspace_text_for_uri(index, &location.uri) else {
            continue;
        };

        let Ok(ast) = parse_role_source(&text) else {
            continue;
        };

        let model = SemanticModel::build(&ast, &text);
        let edges: Vec<_> = model
            .package_edges()
            .iter()
            .filter(|edge| {
                matches!(edge.confidence, Confidence::High | Confidence::Medium)
                    && !matches!(edge.provenance, Provenance::DynamicBoundary)
            })
            .cloned()
            .collect();

        for edge in &edges {
            if edge.kind == PackageEdgeKind::ComposesRole {
                queue.push_back(edge.to_package.clone());
            }
        }

        if !edges.is_empty() {
            graph.add_edges(&location.uri, uri_to_file_id(&location.uri), edges);
        }

        file_count += 1;
    }

    graph
}

fn parse_role_source(text: &str) -> Result<perl_parser_core::ast::Node, String> {
    let mut parser = perl_semantic_analyzer::Parser::new(text);
    parser.parse().map_err(|e| e.to_string())
}

fn workspace_text_for_uri(index: &WorkspaceIndex, uri: &str) -> Option<String> {
    index.document_store().get_text(uri).or_else(|| {
        perl_workspace::workspace_index::uri_to_fs_path(uri)
            .and_then(|path| std::fs::read_to_string(path).ok())
    })
}

fn uri_to_file_id(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}
