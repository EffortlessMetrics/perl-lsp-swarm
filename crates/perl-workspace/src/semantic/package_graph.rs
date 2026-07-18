//! Package graph index for cross-file inheritance and role-composition lookups.
//!
//! Maintains a directed graph of [`PackageNode`] entries connected by
//! [`PackageEdge`] entries with kinds [`PackageEdgeKind::Inherits`],
//! [`PackageEdgeKind::ComposesRole`], and [`PackageEdgeKind::DependsOn`].
//!
//! Supports incremental add/remove via [`PackageGraphIndex::add_edges`] and
//! [`PackageGraphIndex::remove_edges_for_file`], keyed by the file's source URI.
//!
//! # Cycle Detection
//!
//! The [`ancestors`](PackageGraphIndex::ancestors) method traverses the
//! inheritance chain with a visited set. When a cycle is detected the
//! traversal terminates and the result carries a `cycle_detected` flag
//! rather than looping indefinitely.
//!
//! # Requirements
//!
//! - **Req 14.1**: Maintain `PackageNode` entries for each known package, class, and role.
//! - **Req 14.2**: Record `PackageEdge` entries with Inherits, ComposesRole, DependsOn kinds.
//! - **Req 14.4**: Cycle detection — terminate traversal and report cycle rather than looping.

use perl_semantic_facts::{
    AnchorId, EntityId, FileId, PackageEdge, PackageEdgeKind, PackageKind, PackageNode,
};
use std::collections::{HashMap, HashSet};

/// Result of an ancestor traversal through the package graph.
///
/// Contains the ordered list of ancestor package names and a flag indicating
/// whether a cycle was detected during traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorResult {
    /// Ancestor package names in traversal order (breadth-first).
    pub ancestors: Vec<String>,
    /// `true` when a circular inheritance chain was detected.
    pub cycle_detected: bool,
}

/// Result of a transitive role-composition traversal through the package graph.
///
/// Contains the ordered list of transitively composed role names (excluding
/// the starting package) and a flag indicating whether a composition cycle was
/// detected during traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleCompositionResult {
    /// Composed role names in deterministic DFS pre-order (excludes the
    /// starting package).
    pub roles: Vec<String>,
    /// `true` when a circular role-composition chain was detected.
    pub cycle_detected: bool,
}

/// Cross-file package graph index.
///
/// Populated from [`PackageEdge`] data extracted by the package graph
/// extractor during workspace indexing. Supports incremental updates:
/// call [`remove_edges_for_file`](Self::remove_edges_for_file) to purge
/// stale entries, then [`add_edges`](Self::add_edges) to insert fresh ones.
#[derive(Debug, Default)]
pub struct PackageGraphIndex {
    /// Package name → node metadata.
    nodes: HashMap<String, PackageNode>,

    /// Package name → outgoing edges (edges where `from_package` == key).
    outgoing_edges: HashMap<String, Vec<PackageEdge>>,

    /// Source URI for each outgoing edge, kept in lockstep with
    /// [`Self::outgoing_edges`].  Equal edges can be contributed by multiple
    /// files, so edge equality alone is not sufficient for replacement.
    outgoing_edge_sources: HashMap<String, Vec<String>>,

    /// Tracks which file URIs have contributed edges so that
    /// [`remove_edges_for_file`](Self::remove_edges_for_file) can purge
    /// stale entries.
    file_edges: HashMap<String, Vec<PackageEdge>>,

    /// File identity for each URI's source-package contribution.
    file_edge_ids: HashMap<String, FileId>,
}

impl PackageGraphIndex {
    /// Create an empty package graph index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a batch of [`PackageEdge`] entries from a single file.
    ///
    /// For each edge, the source and target packages are ensured to have
    /// [`PackageNode`] entries. Unknown target packages are recorded with
    /// [`PackageKind::External`] and [`Confidence::Low`].
    pub fn add_edges(&mut self, source_uri: &str, file_id: FileId, edges: Vec<PackageEdge>) {
        // Store edges for later removal.
        self.file_edges.insert(source_uri.to_string(), edges.clone());
        self.file_edge_ids.insert(source_uri.to_string(), file_id);

        for edge in &edges {
            // Ensure the source package node exists.
            self.ensure_node_from_edge_source(edge, file_id);

            // Ensure the target package node exists (may be external).
            self.ensure_node_from_edge_target(edge);

            // Record the outgoing edge.
            self.outgoing_edges.entry(edge.from_package.clone()).or_default().push(edge.clone());
            self.outgoing_edge_sources
                .entry(edge.from_package.clone())
                .or_default()
                .push(source_uri.to_string());
        }
    }

    /// Remove all edges and orphaned nodes that originated from the given file URI.
    ///
    /// This is the "remove" half of incremental re-indexing: call this before
    /// [`add_edges`](Self::add_edges) with the updated edges.
    pub fn remove_edges_for_file(&mut self, source_uri: &str) {
        let removed_edges = match self.file_edges.remove(source_uri) {
            Some(edges) => edges,
            None => return,
        };
        self.file_edge_ids.remove(source_uri);

        // Collect the package names whose outgoing edges need pruning.
        let affected_packages: HashSet<String> =
            removed_edges.iter().map(|e| e.from_package.clone()).collect();

        // Remove only the edges owned by this file.  Two files may contribute
        // equal HIR edges (which commonly have no anchor), so comparing edge
        // values alone would incorrectly delete another file's contribution.
        for pkg in &affected_packages {
            if let (Some(edges), Some(sources)) =
                (self.outgoing_edges.get_mut(pkg), self.outgoing_edge_sources.get_mut(pkg))
            {
                // The two vectors are a lockstep ownership projection. If a
                // future mutation corrupts that invariant, preserve the
                // existing graph rather than silently dropping unmatched
                // entries through `zip`.
                if edges.len() != sources.len() {
                    debug_assert_eq!(
                        edges.len(),
                        sources.len(),
                        "package graph edge/source ownership vectors diverged"
                    );
                    continue;
                }
                let mut kept_edges = Vec::with_capacity(edges.len());
                let mut kept_sources = Vec::with_capacity(sources.len());
                for (edge, owner) in edges.drain(..).zip(sources.drain(..)) {
                    if owner != source_uri {
                        kept_edges.push(edge);
                        kept_sources.push(owner);
                    }
                }
                *edges = kept_edges;
                *sources = kept_sources;
            }
        }

        // Clean up empty edge buckets.
        self.outgoing_edges.retain(|_, v| !v.is_empty());
        self.outgoing_edge_sources.retain(|_, v| !v.is_empty());

        // Rebuild source metadata from the remaining owners. A package can be
        // contributed by multiple files; retaining the original node would
        // leave a removed file's FileId attached to the surviving package.
        self.rebuild_node_metadata();
    }

    /// Traverse the inheritance chain (Inherits edges) starting from
    /// `package_name` and return all ancestor packages.
    ///
    /// Uses breadth-first traversal with a visited set for cycle detection.
    /// When a cycle is detected the traversal terminates immediately and
    /// [`AncestorResult::cycle_detected`] is set to `true`.
    pub fn ancestors(&self, package_name: &str) -> AncestorResult {
        let mut visited = HashSet::new();
        let mut ancestors = Vec::new();
        let mut cycle_detected = false;

        visited.insert(package_name.to_string());
        self.collect_ancestors(package_name, &mut visited, &mut ancestors, &mut cycle_detected);

        AncestorResult { ancestors, cycle_detected }
    }

    /// Recursively collect ancestors via DFS, using `visited` to track the
    /// current traversal path for cycle detection.
    ///
    /// Nodes are added to `visited` before recursing and removed after, so
    /// only back-edges (true cycles) trigger `cycle_detected`. Diamond
    /// convergence is handled by checking `ancestors` to avoid duplicates
    /// without flagging a cycle.
    fn collect_ancestors(
        &self,
        package_name: &str,
        on_stack: &mut HashSet<String>,
        ancestors: &mut Vec<String>,
        cycle_detected: &mut bool,
    ) {
        if *cycle_detected {
            return;
        }

        for parent in self.direct_parents(package_name) {
            if on_stack.contains(&parent) {
                // Back-edge to a node on the current DFS path — true cycle.
                *cycle_detected = true;
                return;
            }

            // Skip already-collected ancestors (diamond convergence).
            if ancestors.contains(&parent) {
                continue;
            }

            ancestors.push(parent.clone());
            on_stack.insert(parent.clone());
            self.collect_ancestors(&parent, on_stack, ancestors, cycle_detected);
            on_stack.remove(&parent);

            if *cycle_detected {
                return;
            }
        }
    }

    /// Traverse the role-composition chain (`ComposesRole` edges) starting from
    /// `package_name` and return every transitively composed role.
    ///
    /// Mirrors [`ancestors`](Self::ancestors): depth-first traversal with an
    /// on-path set for cycle detection. Roles are returned in deterministic
    /// DFS pre-order and de-duplicated; the starting package is **not**
    /// included. When a composition cycle is detected,
    /// [`RoleCompositionResult::cycle_detected`] is set and only the cyclic
    /// branch is abandoned — unrelated sibling roles are still collected, so
    /// the result stays complete for every acyclic path.
    pub fn transitive_composed_roles(&self, package_name: &str) -> RoleCompositionResult {
        let mut on_stack = HashSet::new();
        let mut collected = HashSet::new();
        let mut roles = Vec::new();
        let mut cycle_detected = false;

        on_stack.insert(package_name.to_string());
        self.collect_composed_roles(
            package_name,
            &mut on_stack,
            &mut collected,
            &mut roles,
            &mut cycle_detected,
        );

        RoleCompositionResult { roles, cycle_detected }
    }

    /// Recursively collect composed roles via DFS, using `on_stack` to track
    /// the current traversal path for cycle detection.
    ///
    /// Roles are added to `on_stack` before recursing and removed after, so
    /// only back-edges (true composition cycles) trigger `cycle_detected`.
    /// Convergence via multiple composition paths is handled by checking
    /// `roles` to avoid duplicates without flagging a cycle.
    ///
    /// A detected cycle abandons **only** the offending branch (`continue`),
    /// never the whole traversal: sibling roles reached through other,
    /// acyclic edges are unrelated to the cycle and must still be collected.
    /// Aborting the entire DFS on the first back-edge would silently drop
    /// those siblings depending on edge-insertion order.
    ///
    /// `collected` mirrors `roles` as a set so convergent-composition
    /// de-duplication is O(1) per edge rather than an O(n) scan of the ordered
    /// output vector; `roles` remains the deterministic DFS pre-order list.
    fn collect_composed_roles(
        &self,
        package_name: &str,
        on_stack: &mut HashSet<String>,
        collected: &mut HashSet<String>,
        roles: &mut Vec<String>,
        cycle_detected: &mut bool,
    ) {
        for role in self.composed_roles(package_name) {
            if on_stack.contains(&role) {
                // Back-edge to a role on the current DFS path — a true cycle.
                // Skip this branch only; keep collecting siblings.
                *cycle_detected = true;
                continue;
            }

            // Skip already-collected roles (convergent composition).
            if !collected.insert(role.clone()) {
                continue;
            }

            roles.push(role.clone());
            on_stack.insert(role.clone());
            self.collect_composed_roles(&role, on_stack, collected, roles, cycle_detected);
            on_stack.remove(&role);
        }
    }

    /// Return the roles composed by `package_name` (ComposesRole edges).
    pub fn composed_roles(&self, package_name: &str) -> Vec<String> {
        self.outgoing_edges
            .get(package_name)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|e| e.kind == PackageEdgeKind::ComposesRole)
                    .map(|e| e.to_package.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return the direct dependencies of `package_name` (DependsOn edges).
    pub fn dependencies(&self, package_name: &str) -> Vec<String> {
        self.outgoing_edges
            .get(package_name)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|e| e.kind == PackageEdgeKind::DependsOn)
                    .map(|e| e.to_package.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Look up a [`PackageNode`] by name.
    pub fn get_node(&self, package_name: &str) -> Option<&PackageNode> {
        self.nodes.get(package_name)
    }

    /// Return all outgoing edges for a package.
    pub fn get_edges(&self, package_name: &str) -> &[PackageEdge] {
        self.outgoing_edges.get(package_name).map(Vec::as_slice).unwrap_or_default()
    }

    /// Return the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Return the total number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.outgoing_edges.values().map(Vec::len).sum()
    }

    // ── Private helpers ──

    /// Return the direct parent package names (Inherits edges) for a package.
    fn direct_parents(&self, package_name: &str) -> Vec<String> {
        self.outgoing_edges
            .get(package_name)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|e| e.kind == PackageEdgeKind::Inherits)
                    .map(|e| e.to_package.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Ensure a node exists for the source package of an edge.
    fn ensure_node_from_edge_source(&mut self, edge: &PackageEdge, file_id: FileId) {
        self.nodes.entry(edge.from_package.clone()).or_insert_with(|| {
            // Infer the package kind from the edge kind.
            let kind = match edge.kind {
                PackageEdgeKind::ComposesRole => PackageKind::Class,
                _ => PackageKind::Package,
            };
            PackageNode::new(
                EntityId(0), // placeholder — will be refined when entity facts are available
                edge.from_package.clone(),
                kind,
                edge.anchor_id,
                Some(file_id),
            )
        });
    }

    /// Ensure a node exists for the target package of an edge.
    fn ensure_node_from_edge_target(&mut self, edge: &PackageEdge) {
        self.nodes.entry(edge.to_package.clone()).or_insert_with(|| {
            let kind = match edge.kind {
                PackageEdgeKind::ComposesRole => PackageKind::Role,
                _ => PackageKind::External,
            };
            PackageNode::new(
                EntityId(0), // placeholder
                edge.to_package.clone(),
                kind,
                None,
                None,
            )
        });
    }

    /// Reconstruct node metadata from the currently live edge ownership.
    fn rebuild_node_metadata(&mut self) {
        let mut source_nodes: HashMap<String, (PackageKind, Option<AnchorId>, FileId)> =
            HashMap::new();
        let mut target_kinds: HashMap<String, PackageKind> = HashMap::new();

        for (source_uri, edges) in &self.file_edges {
            let Some(file_id) = self.file_edge_ids.get(source_uri).copied() else {
                continue;
            };
            for edge in edges {
                let source_kind = match edge.kind {
                    PackageEdgeKind::ComposesRole => PackageKind::Class,
                    _ => PackageKind::Package,
                };
                source_nodes
                    .entry(edge.from_package.clone())
                    .and_modify(|(kind, anchor_id, existing_file_id)| {
                        if source_kind == PackageKind::Class {
                            *kind = PackageKind::Class;
                        }
                        if file_id < *existing_file_id {
                            *anchor_id = edge.anchor_id;
                            *existing_file_id = file_id;
                        } else if file_id == *existing_file_id && anchor_id.is_none() {
                            // An anchor from the same file can improve an
                            // earlier anchor-less edge; never borrow an anchor
                            // from a different file while retaining its owner.
                            *anchor_id = edge.anchor_id;
                        }
                    })
                    .or_insert((source_kind, edge.anchor_id, file_id));

                let target_kind = match edge.kind {
                    PackageEdgeKind::ComposesRole => PackageKind::Role,
                    _ => PackageKind::External,
                };
                target_kinds
                    .entry(edge.to_package.clone())
                    .and_modify(|kind| {
                        if target_kind == PackageKind::Role {
                            *kind = PackageKind::Role;
                        }
                    })
                    .or_insert(target_kind);
            }
        }

        let mut nodes = HashMap::with_capacity(source_nodes.len() + target_kinds.len());
        for (name, kind) in target_kinds {
            nodes.insert(name.clone(), PackageNode::new(EntityId(0), name, kind, None, None));
        }
        for (name, (kind, anchor_id, file_id)) in source_nodes {
            nodes.insert(
                name.clone(),
                PackageNode::new(EntityId(0), name, kind, anchor_id, Some(file_id)),
            );
        }
        self.nodes = nodes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::{AnchorId, Confidence, Provenance};

    /// Helper: build a simple `PackageEdge` with Inherits kind.
    fn inherits_edge(from: &str, to: &str) -> PackageEdge {
        PackageEdge::new(
            from.to_string(),
            to.to_string(),
            PackageEdgeKind::Inherits,
            Some(AnchorId(1)),
            Provenance::ExactAst,
            Confidence::High,
        )
    }

    /// Helper: build a ComposesRole edge.
    fn composes_edge(from: &str, to: &str) -> PackageEdge {
        PackageEdge::new(
            from.to_string(),
            to.to_string(),
            PackageEdgeKind::ComposesRole,
            Some(AnchorId(2)),
            Provenance::ExactAst,
            Confidence::High,
        )
    }

    /// Helper: build a DependsOn edge.
    fn depends_edge(from: &str, to: &str) -> PackageEdge {
        PackageEdge::new(
            from.to_string(),
            to.to_string(),
            PackageEdgeKind::DependsOn,
            None,
            Provenance::NameHeuristic,
            Confidence::Low,
        )
    }

    #[test]
    fn empty_graph_has_no_nodes_or_edges() -> Result<(), Box<dyn std::error::Error>> {
        let index = PackageGraphIndex::new();
        assert_eq!(index.node_count(), 0);
        assert_eq!(index.edge_count(), 0);
        Ok(())
    }

    #[test]
    fn add_edges_creates_nodes_and_edges() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        let edges = vec![inherits_edge("Child", "Parent")];
        index.add_edges("file:///lib/Child.pm", FileId(1), edges);

        assert_eq!(index.node_count(), 2);
        assert_eq!(index.edge_count(), 1);

        let child_node = index.get_node("Child").ok_or("expected Child node")?;
        assert_eq!(child_node.name, "Child");

        let parent_node = index.get_node("Parent").ok_or("expected Parent node")?;
        assert_eq!(parent_node.name, "Parent");
        assert_eq!(parent_node.kind, PackageKind::External);
        Ok(())
    }

    #[test]
    fn ancestors_returns_linear_chain() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // Child -> Parent -> GrandParent
        index.add_edges("file:///lib/Child.pm", FileId(1), vec![inherits_edge("Child", "Parent")]);
        index.add_edges(
            "file:///lib/Parent.pm",
            FileId(2),
            vec![inherits_edge("Parent", "GrandParent")],
        );

        let result = index.ancestors("Child");
        assert!(!result.cycle_detected);
        assert_eq!(result.ancestors, vec!["Parent", "GrandParent"]);
        Ok(())
    }

    #[test]
    fn ancestors_returns_empty_for_no_parents() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/Root.pm", FileId(1), vec![depends_edge("Root", "SomeDep")]);

        let result = index.ancestors("Root");
        assert!(!result.cycle_detected);
        assert!(result.ancestors.is_empty());
        Ok(())
    }

    #[test]
    fn ancestors_returns_empty_for_unknown_package() -> Result<(), Box<dyn std::error::Error>> {
        let index = PackageGraphIndex::new();
        let result = index.ancestors("Unknown");
        assert!(!result.cycle_detected);
        assert!(result.ancestors.is_empty());
        Ok(())
    }

    #[test]
    fn ancestors_detects_direct_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // A -> B -> A (cycle)
        index.add_edges("file:///lib/A.pm", FileId(1), vec![inherits_edge("A", "B")]);
        index.add_edges("file:///lib/B.pm", FileId(2), vec![inherits_edge("B", "A")]);

        let result = index.ancestors("A");
        assert!(result.cycle_detected, "should detect cycle A -> B -> A");
        // B should be in ancestors before cycle is detected.
        assert!(result.ancestors.contains(&"B".to_string()));
        Ok(())
    }

    #[test]
    fn ancestors_detects_longer_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // A -> B -> C -> A (cycle)
        index.add_edges("file:///lib/A.pm", FileId(1), vec![inherits_edge("A", "B")]);
        index.add_edges("file:///lib/B.pm", FileId(2), vec![inherits_edge("B", "C")]);
        index.add_edges("file:///lib/C.pm", FileId(3), vec![inherits_edge("C", "A")]);

        let result = index.ancestors("A");
        assert!(result.cycle_detected, "should detect cycle A -> B -> C -> A");
        assert!(result.ancestors.contains(&"B".to_string()));
        assert!(result.ancestors.contains(&"C".to_string()));
        Ok(())
    }

    #[test]
    fn ancestors_self_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // A -> A (self-cycle)
        index.add_edges("file:///lib/A.pm", FileId(1), vec![inherits_edge("A", "A")]);

        let result = index.ancestors("A");
        assert!(result.cycle_detected, "should detect self-cycle A -> A");
        Ok(())
    }

    #[test]
    fn ancestors_diamond_no_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // Diamond: D -> B, D -> C, B -> A, C -> A
        index.add_edges(
            "file:///lib/D.pm",
            FileId(1),
            vec![inherits_edge("D", "B"), inherits_edge("D", "C")],
        );
        index.add_edges("file:///lib/B.pm", FileId(2), vec![inherits_edge("B", "A")]);
        index.add_edges("file:///lib/C.pm", FileId(3), vec![inherits_edge("C", "A")]);

        let result = index.ancestors("D");
        assert!(!result.cycle_detected, "diamond is not a cycle");
        // All ancestors should be present.
        assert!(result.ancestors.contains(&"B".to_string()));
        assert!(result.ancestors.contains(&"C".to_string()));
        assert!(result.ancestors.contains(&"A".to_string()));
        Ok(())
    }

    #[test]
    fn composed_roles_returns_role_names() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/MyClass.pm",
            FileId(1),
            vec![
                composes_edge("MyClass", "Printable"),
                composes_edge("MyClass", "Serializable"),
                inherits_edge("MyClass", "Base"),
            ],
        );

        let roles = index.composed_roles("MyClass");
        assert_eq!(roles.len(), 2);
        assert!(roles.contains(&"Printable".to_string()));
        assert!(roles.contains(&"Serializable".to_string()));
        Ok(())
    }

    #[test]
    fn composed_roles_returns_empty_for_no_roles() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/Plain.pm", FileId(1), vec![inherits_edge("Plain", "Base")]);

        let roles = index.composed_roles("Plain");
        assert!(roles.is_empty());
        Ok(())
    }

    #[test]
    fn transitive_composed_roles_follows_role_chain() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // MyClass composes RoleA; RoleA composes RoleB; RoleB composes RoleC.
        index.add_edges(
            "file:///lib/MyClass.pm",
            FileId(1),
            vec![composes_edge("MyClass", "RoleA")],
        );
        index.add_edges("file:///lib/RoleA.pm", FileId(2), vec![composes_edge("RoleA", "RoleB")]);
        index.add_edges("file:///lib/RoleB.pm", FileId(3), vec![composes_edge("RoleB", "RoleC")]);

        let result = index.transitive_composed_roles("MyClass");
        assert!(!result.cycle_detected);
        // Starting package excluded; transitive roles in DFS pre-order.
        assert_eq!(result.roles, vec!["RoleA", "RoleB", "RoleC"]);
        Ok(())
    }

    #[test]
    fn transitive_composed_roles_excludes_start_and_empty_for_leaf()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/Plain.pm", FileId(1), vec![inherits_edge("Plain", "Base")]);

        let result = index.transitive_composed_roles("Plain");
        assert!(!result.cycle_detected);
        assert!(result.roles.is_empty(), "a package composing no roles yields no transitive roles");
        Ok(())
    }

    #[test]
    fn transitive_composed_roles_detects_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // RoleA composes RoleB; RoleB composes RoleA (composition cycle).
        index.add_edges("file:///lib/RoleA.pm", FileId(1), vec![composes_edge("RoleA", "RoleB")]);
        index.add_edges("file:///lib/RoleB.pm", FileId(2), vec![composes_edge("RoleB", "RoleA")]);

        let result = index.transitive_composed_roles("RoleA");
        assert!(result.cycle_detected, "role-composition cycle must be detected");
        // Traversal still terminates and collects the reachable role before the
        // back-edge is found.
        assert!(result.roles.contains(&"RoleB".to_string()));
        Ok(())
    }

    #[test]
    fn transitive_composed_roles_convergent_paths_no_cycle()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // Diamond over ComposesRole: Top -> L, Top -> R, L -> Shared, R -> Shared.
        index.add_edges(
            "file:///lib/Top.pm",
            FileId(1),
            vec![composes_edge("Top", "L"), composes_edge("Top", "R")],
        );
        index.add_edges("file:///lib/L.pm", FileId(2), vec![composes_edge("L", "Shared")]);
        index.add_edges("file:///lib/R.pm", FileId(3), vec![composes_edge("R", "Shared")]);

        let result = index.transitive_composed_roles("Top");
        assert!(!result.cycle_detected, "convergent composition paths are not a cycle");
        assert!(result.roles.contains(&"L".to_string()));
        assert!(result.roles.contains(&"R".to_string()));
        assert!(result.roles.contains(&"Shared".to_string()));
        // "Shared" is collected exactly once despite two paths reaching it.
        assert_eq!(result.roles.iter().filter(|r| *r == "Shared").count(), 1);
        Ok(())
    }

    #[test]
    fn transitive_composed_roles_cycle_preserves_unrelated_siblings()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        // Top composes RoleCyclic (first edge) and RoleClean (second edge).
        // RoleCyclic composes back to Top (a genuine cycle); RoleClean is an
        // unrelated acyclic sibling. The cycle must not drop RoleClean, even
        // though RoleCyclic is visited first.
        index.add_edges(
            "file:///lib/Top.pm",
            FileId(1),
            vec![composes_edge("Top", "RoleCyclic"), composes_edge("Top", "RoleClean")],
        );
        index.add_edges(
            "file:///lib/RoleCyclic.pm",
            FileId(2),
            vec![composes_edge("RoleCyclic", "Top")],
        );

        let result = index.transitive_composed_roles("Top");
        assert!(result.cycle_detected, "the RoleCyclic -> Top back-edge is a cycle");
        assert!(
            result.roles.contains(&"RoleCyclic".to_string()),
            "the cyclic role itself is still reached before the back-edge"
        );
        assert!(
            result.roles.contains(&"RoleClean".to_string()),
            "an unrelated sibling must survive a cycle in another branch: {:?}",
            result.roles
        );
        Ok(())
    }

    #[test]
    fn transitive_composed_roles_reflects_stale_index_removal()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/MyClass.pm",
            FileId(1),
            vec![composes_edge("MyClass", "RoleA")],
        );
        index.add_edges("file:///lib/RoleA.pm", FileId(2), vec![composes_edge("RoleA", "RoleB")]);

        assert_eq!(index.transitive_composed_roles("MyClass").roles, vec!["RoleA", "RoleB"]);

        // Re-index RoleA.pm with the RoleB composition removed (incremental update).
        index.remove_edges_for_file("file:///lib/RoleA.pm");

        let result = index.transitive_composed_roles("MyClass");
        assert_eq!(
            result.roles,
            vec!["RoleA"],
            "stale transitive role must disappear after re-index"
        );
        Ok(())
    }

    #[test]
    fn dependencies_returns_depends_on_targets() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/App.pm",
            FileId(1),
            vec![
                depends_edge("App", "DBI"),
                depends_edge("App", "JSON"),
                inherits_edge("App", "Base"),
            ],
        );

        let deps = index.dependencies("App");
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"DBI".to_string()));
        assert!(deps.contains(&"JSON".to_string()));
        Ok(())
    }

    #[test]
    fn remove_edges_for_file_clears_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/Child.pm", FileId(1), vec![inherits_edge("Child", "Parent")]);

        assert_eq!(index.node_count(), 2);
        assert_eq!(index.edge_count(), 1);

        index.remove_edges_for_file("file:///lib/Child.pm");

        assert_eq!(index.node_count(), 0);
        assert_eq!(index.edge_count(), 0);
        Ok(())
    }

    #[test]
    fn remove_edges_for_file_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/Child.pm", FileId(1), vec![inherits_edge("Child", "Parent")]);

        index.remove_edges_for_file("file:///lib/Child.pm");
        // Second remove should be a no-op.
        index.remove_edges_for_file("file:///lib/Child.pm");

        assert_eq!(index.node_count(), 0);
        assert_eq!(index.edge_count(), 0);
        Ok(())
    }

    #[test]
    fn remove_unknown_file_is_noop() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/Child.pm", FileId(1), vec![inherits_edge("Child", "Parent")]);

        index.remove_edges_for_file("file:///nonexistent.pm");

        // Original entries should still be present.
        assert_eq!(index.node_count(), 2);
        assert_eq!(index.edge_count(), 1);
        Ok(())
    }

    #[test]
    fn multiple_files_coexist() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges("file:///lib/A.pm", FileId(1), vec![inherits_edge("A", "Base")]);
        index.add_edges("file:///lib/B.pm", FileId(2), vec![inherits_edge("B", "Base")]);

        assert_eq!(index.node_count(), 3); // A, B, Base
        assert_eq!(index.edge_count(), 2);

        // Remove one file — only its edges should disappear.
        index.remove_edges_for_file("file:///lib/A.pm");

        assert_eq!(index.edge_count(), 1);
        // A is no longer referenced, but B and Base remain.
        assert_eq!(index.node_count(), 2);
        assert!(index.get_node("A").is_none());
        assert!(index.get_node("B").is_some());
        assert!(index.get_node("Base").is_some());
        Ok(())
    }

    #[test]
    fn removing_one_file_preserves_equal_edge_owned_by_another_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        let edge = inherits_edge("Child", "Base");
        index.add_edges("file:///lib/Child-one.pm", FileId(1), vec![edge.clone()]);
        index.add_edges("file:///lib/Child-two.pm", FileId(2), vec![edge]);

        assert_eq!(index.edge_count(), 2);
        assert_eq!(index.ancestors("Child").ancestors, vec!["Base"]);

        index.remove_edges_for_file("file:///lib/Child-one.pm");

        assert_eq!(index.edge_count(), 1);
        assert_eq!(index.ancestors("Child").ancestors, vec!["Base"]);
        assert!(index.get_node("Child").is_some());
        assert!(index.get_node("Base").is_some());
        Ok(())
    }

    #[test]
    fn removing_one_file_reassigns_shared_source_node_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        let edge = inherits_edge("Child", "Base");
        index.add_edges("file:///lib/Child-one.pm", FileId(1), vec![edge.clone()]);
        index.add_edges("file:///lib/Child-two.pm", FileId(2), vec![edge]);

        assert_eq!(index.get_node("Child").and_then(|node| node.file_id), Some(FileId(1)));
        index.remove_edges_for_file("file:///lib/Child-one.pm");
        assert_eq!(index.get_node("Child").and_then(|node| node.file_id), Some(FileId(2)));
        Ok(())
    }

    #[test]
    fn rebuild_node_metadata_keeps_anchor_and_file_owner_paired()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        let mut anchorless = inherits_edge("Child", "Base");
        anchorless.anchor_id = None;
        let anchored = inherits_edge("Child", "Base");
        index.add_edges("file:///lib/Child-one.pm", FileId(1), vec![anchorless]);
        index.add_edges("file:///lib/Child-two.pm", FileId(2), vec![anchored]);

        let child = index.get_node("Child").ok_or("expected Child node")?;
        assert_eq!(child.file_id, Some(FileId(1)));
        assert_eq!(child.anchor_id, None);
        Ok(())
    }

    #[test]
    fn incremental_reindex_replaces_edges() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/Child.pm",
            FileId(1),
            vec![inherits_edge("Child", "OldParent")],
        );

        let result = index.ancestors("Child");
        assert_eq!(result.ancestors, vec!["OldParent"]);

        // Simulate re-indexing: remove old, add updated edges.
        index.remove_edges_for_file("file:///lib/Child.pm");
        index.add_edges(
            "file:///lib/Child.pm",
            FileId(1),
            vec![inherits_edge("Child", "NewParent")],
        );

        let result = index.ancestors("Child");
        assert_eq!(result.ancestors, vec!["NewParent"]);
        assert!(index.get_node("OldParent").is_none());
        Ok(())
    }

    #[test]
    fn role_target_gets_role_kind() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/MyClass.pm",
            FileId(1),
            vec![composes_edge("MyClass", "MyRole")],
        );

        let role_node = index.get_node("MyRole").ok_or("expected MyRole node")?;
        assert_eq!(role_node.kind, PackageKind::Role);

        let class_node = index.get_node("MyClass").ok_or("expected MyClass node")?;
        assert_eq!(class_node.kind, PackageKind::Class);
        Ok(())
    }

    #[test]
    fn external_target_gets_low_confidence_kind() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/Child.pm",
            FileId(1),
            vec![inherits_edge("Child", "Unknown::External")],
        );

        let ext_node = index.get_node("Unknown::External").ok_or("expected external node")?;
        assert_eq!(ext_node.kind, PackageKind::External);
        assert!(ext_node.file_id.is_none());
        Ok(())
    }

    #[test]
    fn get_edges_returns_all_outgoing() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = PackageGraphIndex::new();
        index.add_edges(
            "file:///lib/MyClass.pm",
            FileId(1),
            vec![
                inherits_edge("MyClass", "Base"),
                composes_edge("MyClass", "Role1"),
                depends_edge("MyClass", "Dep1"),
            ],
        );

        let edges = index.get_edges("MyClass");
        assert_eq!(edges.len(), 3);
        Ok(())
    }

    #[test]
    fn get_edges_returns_empty_for_unknown() -> Result<(), Box<dyn std::error::Error>> {
        let index = PackageGraphIndex::new();
        let edges = index.get_edges("Unknown");
        assert!(edges.is_empty());
        Ok(())
    }

    // ── Property-based tests ──

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;
        use proptest::test_runner::Config as ProptestConfig;

        /// Strategy to generate a Perl-like package name (e.g. "Pkg0", "Pkg3").
        ///
        /// Uses a small fixed pool so that random edges frequently collide on
        /// the same node, naturally producing cycles and diamonds.
        fn arb_package_name(node_count: usize) -> impl Strategy<Value = String> {
            (0..node_count).prop_map(|i| format!("Pkg{i}"))
        }

        /// Strategy to generate a random directed graph as a list of
        /// `(from, to)` Inherits edges.
        ///
        /// `node_count` controls the pool size (small pools → more cycles).
        /// `edge_count` controls how many edges are generated.
        fn arb_edge_list(
            node_count: usize,
            max_edges: usize,
        ) -> impl Strategy<Value = Vec<(String, String)>> {
            prop::collection::vec(
                (arb_package_name(node_count), arb_package_name(node_count)),
                0..=max_edges,
            )
        }

        /// Build a [`PackageGraphIndex`] from a list of `(from, to)` pairs,
        /// all treated as [`PackageEdgeKind::Inherits`].
        fn build_graph(edge_list: &[(String, String)]) -> PackageGraphIndex {
            let mut index = PackageGraphIndex::new();
            let edges: Vec<PackageEdge> =
                edge_list.iter().map(|(from, to)| inherits_edge(from, to)).collect();
            if !edges.is_empty() {
                index.add_edges("file:///lib/generated.pm", FileId(1), edges);
            }
            index
        }

        /// Return `true` when the graph (represented as an edge list)
        /// contains a cycle reachable from `start` via Inherits edges.
        ///
        /// Uses DFS with an on-stack set, mirroring the production algorithm.
        fn has_reachable_cycle(edge_list: &[(String, String)], start: &str) -> bool {
            // Build adjacency list.
            let mut adj: std::collections::HashMap<&str, Vec<&str>> =
                std::collections::HashMap::new();
            for (from, to) in edge_list {
                adj.entry(from.as_str()).or_default().push(to.as_str());
            }

            let mut on_stack = std::collections::HashSet::new();
            on_stack.insert(start);
            dfs_has_cycle(&adj, start, &mut on_stack)
        }

        fn dfs_has_cycle<'a>(
            adj: &std::collections::HashMap<&'a str, Vec<&'a str>>,
            node: &'a str,
            on_stack: &mut std::collections::HashSet<&'a str>,
        ) -> bool {
            if let Some(neighbors) = adj.get(node) {
                for &next in neighbors {
                    if on_stack.contains(next) {
                        return true;
                    }
                    on_stack.insert(next);
                    if dfs_has_cycle(adj, next, on_stack) {
                        return true;
                    }
                    on_stack.remove(next);
                }
            }
            false
        }

        // **Validates: Requirements 14.4**
        //
        // Property 12: Package Graph Cycle Termination — For any package
        // graph with circular inheritance chains, the `ancestors` traversal
        // terminates in finite time and reports the cycle rather than
        // looping indefinitely.
        proptest! {
            #![proptest_config(ProptestConfig {
                failure_persistence: None,
                ..ProptestConfig::default()
            })]

            #[test]
            fn prop_ancestors_terminates_and_detects_cycles(
                edge_list in arb_edge_list(6, 12),
            ) {
                let graph = build_graph(&edge_list);

                // Collect all node names present in the graph.
                let mut all_nodes: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                for (from, to) in &edge_list {
                    all_nodes.insert(from.clone());
                    all_nodes.insert(to.clone());
                }

                // For every node, `ancestors` must terminate (the test
                // completing is proof) and must report cycle_detected when
                // a cycle is reachable from that node.
                for node in &all_nodes {
                    let result = graph.ancestors(node);

                    let oracle_has_cycle = has_reachable_cycle(&edge_list, node);

                    if oracle_has_cycle {
                        prop_assert!(
                            result.cycle_detected,
                            "ancestors('{}') should detect a cycle (edges: {:?})",
                            node,
                            edge_list,
                        );
                    }
                    // When the oracle says no cycle, the implementation
                    // should agree.
                    if !oracle_has_cycle {
                        prop_assert!(
                            !result.cycle_detected,
                            "ancestors('{}') should NOT detect a cycle (edges: {:?})",
                            node,
                            edge_list,
                        );
                    }
                }
            }
        }
    }
}
