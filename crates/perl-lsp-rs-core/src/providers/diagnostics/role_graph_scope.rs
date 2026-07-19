//! Scoped role-provider graph for cross-file PL303 detection.
//!
//! Builds a [`PackageGraphIndex`] containing `ComposesRole` edges scoped to
//! the roles consumed by a single file.  This graph is passed to
//! [`WorkspaceIndex::with_semantic_queries_for_uri_and_graph`] so that
//! [`SemanticQueries::transitive_role_methods`] can walk cross-file role
//! composition for PL303 diagnostics.
//!
//! The persistent `WorkspaceIndex` graph only contains `Inherits` edges (from
//! HIR lowering); `ComposesRole` edges come from `perl-semantic-analyzer`'s
//! `SemanticModel::package_edges()`, which cannot be wired into
//! `perl-workspace` without creating a dependency cycle.  Building a
//! request-scoped graph here solves the cycle without per-request whole-workspace
//! re-parsing.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use perl_parser_core::ast::Node;
use perl_semantic_analyzer::class_model::ClassModelBuilder;
use perl_semantic_facts::{Confidence, FileId, PackageEdge, PackageEdgeKind, Provenance};
use perl_workspace::semantic::package_graph::PackageGraphIndex;
use perl_workspace::workspace_index::{WorkspaceIndex, uri_to_fs_path};

/// Maximum number of role-definition files parsed per diagnostic run.
///
/// Limits per-request parse cost for deeply-nested role hierarchies.  Files
/// beyond this cap are skipped; `transitive_role_methods` degrades
/// conservatively (returns empty) for roles reachable only through un-parsed
/// files.
const MAX_ROLE_GRAPH_FILES: usize = 32;

/// Extract the unique set of role names consumed by any package in `node` via
/// `with` clauses.  Returns an empty vec for files that have no role consumers
/// (the common case) so callers can skip the graph build entirely.
pub fn consumed_role_names(node: &Node) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut roles = Vec::new();
    for model in ClassModelBuilder::new().build(node) {
        for role in &model.roles {
            if seen.insert(role.clone()) {
                roles.push(role.clone());
            }
        }
    }
    roles
}

/// Build a [`PackageGraphIndex`] containing `ComposesRole` edges for the
/// transitive providers of `seed_roles`, bounded to [`MAX_ROLE_GRAPH_FILES`]
/// files.
///
/// Mirrors `build_completion_package_graph` in the completion provider but
/// performs a BFS expansion from seed roles rather than a fixed URI set, and
/// skips `current_uri` (same-file roles are already handled by the local
/// [`ClassModel`] path in `check_role_conflicts`).
///
/// The returned graph supplies the **topology** for
/// [`SemanticQueries::transitive_role_methods`]; method bodies come from the
/// persistent fact shards via [`WorkspaceIndex::with_semantic_queries_for_uri_and_graph`].
pub fn build_role_scoped_package_graph(
    index: &WorkspaceIndex,
    seed_roles: &[String],
    current_uri: &str,
) -> PackageGraphIndex {
    let mut graph = PackageGraphIndex::new();
    let mut queued: VecDeque<String> = seed_roles.iter().cloned().collect();
    let mut visited: HashSet<String> = seed_roles.iter().cloned().collect();
    let mut files_parsed: usize = 0;

    while let Some(role_name) = queued.pop_front() {
        if files_parsed >= MAX_ROLE_GRAPH_FILES {
            break;
        }

        // Resolve the role package name to a definition URI.  Returns None for
        // external, unindexed, or dynamically-generated roles — skip those to
        // stay conservative (no guessed PL303).
        let Some(location) = index.find_definition(&role_name) else {
            continue;
        };
        let uri = location.uri;

        // Same-file definitions are already handled by check_role_conflicts'
        // local ClassModel path; skip them here to avoid redundant parsing.
        if uri.is_empty() || uri == current_uri {
            continue;
        }

        // Prefer live document text (open buffer); fall back to filesystem.
        let Some(text) = index
            .document_store()
            .get_text(&uri)
            .or_else(|| uri_to_fs_path(&uri).and_then(|path| std::fs::read_to_string(path).ok()))
        else {
            continue;
        };

        files_parsed += 1;

        let mut parser = perl_semantic_analyzer::Parser::new(&text);
        let Ok(ast) = parser.parse() else { continue };
        let model = perl_semantic_analyzer::semantic::SemanticModel::build(&ast, &text);

        let file_id = uri_to_file_id(&uri);
        let edges: Vec<PackageEdge> = model
            .package_edges()
            .iter()
            .filter(|edge| {
                matches!(edge.confidence, Confidence::High | Confidence::Medium)
                    && !matches!(edge.provenance, Provenance::DynamicBoundary)
            })
            .cloned()
            .collect();

        // Enqueue newly-seen roles that this role in turn composes, so
        // transitive_role_methods can follow the full composition chain.
        for edge in &edges {
            if edge.kind == PackageEdgeKind::ComposesRole && visited.insert(edge.to_package.clone())
            {
                queued.push_back(edge.to_package.clone());
            }
        }

        if !edges.is_empty() {
            graph.add_edges(&uri, file_id, edges);
        }
    }

    graph
}

fn uri_to_file_id(uri: &str) -> FileId {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    FileId(hasher.finish())
}
