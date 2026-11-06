//! Ordered `Foreach` binder projection for the F01 prep train (#13320).
//!
//! Production ASTs still store one iterator in [`NodeKind::Foreach::variable`].
//! F02 (#13322) will replace that field with a canonical ordered binder list.
//! Until then, every consumer that cares about binder cardinality should route
//! through this adapter instead of inventing a second durable binder model.

use crate::{Node, NodeKind};

/// Current production binder count for [`NodeKind::Foreach`].
pub const CURRENT_FOREACH_BINDER_CARDINALITY: usize = 1;

/// Migration disposition for a live `Foreach.variable` consumer (#13320).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForeachBinderMigrationDisposition {
    /// Must observe every binder once F02 lands; silent first-binder-only behavior is invalid.
    BinderCardinalitySensitive,
    /// Ignores binders entirely or only needs loop body/list structure.
    BinderCardinalityAgnostic,
    /// Projects the legacy single field through a bounded helper until F02.
    CompatibilityProjection,
    /// Must change when the canonical AST binder list lands (F02 / #13322).
    MustMigrateWithF02,
    /// Semantic/HIR/LSP projection deferred to F03/F04 (#13324 / #13325).
    MustMigrateWithF03F04,
    /// Touches `Foreach` for non-binder reasons with independent proof.
    UnrelatedWithProof,
}
/// Read-only view over the current single-binder `Foreach` shape.
#[derive(Debug, Clone, Copy)]
pub struct ForeachBinderView<'a> {
    variable: &'a Node,
    list: &'a Node,
    body: &'a Node,
    continue_block: Option<&'a Node>,
}

impl<'a> ForeachBinderView<'a> {
    /// Build a view when the kind is already known to be `Foreach`.
    #[must_use]
    pub fn from_parts(
        variable: &'a Node,
        list: &'a Node,
        body: &'a Node,
        continue_block: Option<&'a Node>,
    ) -> Self {
        Self { variable, list, body, continue_block }
    }

    /// Try to construct a view from any [`NodeKind`].
    #[must_use]
    pub fn try_from_kind(kind: &'a NodeKind) -> Option<Self> {
        match kind {
            NodeKind::Foreach { variable, list, body, continue_block } => {
                Some(Self::from_parts(variable, list, body, continue_block.as_deref()))
            }
            _ => None,
        }
    }

    /// Binders in source order for the current AST shape (length is always
    /// [`CURRENT_FOREACH_BINDER_CARDINALITY`] until F02).
    #[must_use]
    pub fn ordered_binder_nodes(&self) -> [&'a Node; CURRENT_FOREACH_BINDER_CARDINALITY] {
        [self.variable]
    }

    /// Number of production binders represented by the current AST field shape.
    #[must_use]
    pub fn production_binder_count(&self) -> usize {
        CURRENT_FOREACH_BINDER_CARDINALITY
    }

    /// Direct access to the legacy single iterator field (F02 removes this path).
    #[must_use]
    pub fn legacy_single_binder(&self) -> &'a Node {
        self.variable
    }

    #[must_use]
    pub fn list(&self) -> &'a Node {
        self.list
    }

    #[must_use]
    pub fn body(&self) -> &'a Node {
        self.body
    }

    #[must_use]
    pub fn continue_block(&self) -> Option<&'a Node> {
        self.continue_block
    }

    /// Structural child order for bounded test/subtest walkers: every binder,
    /// then list, then body (continue blocks remain parent-owned elsewhere).
    #[must_use]
    pub fn structural_test_children(&self) -> Vec<&'a Node> {
        let mut children = Vec::with_capacity(3);
        children.extend_from_slice(&self.ordered_binder_nodes());
        children.push(self.list);
        children.push(self.body);
        children
    }
}

/// Test-only probe that simulates a second binder without mutating production AST shape.
#[derive(Debug, Clone)]
pub struct ForeachSyntheticBinderProbe<'a> {
    view: ForeachBinderView<'a>,
    extra_binder: &'a Node,
}

impl<'a> ForeachSyntheticBinderProbe<'a> {
    /// Attach a synthetic binder to expose cardinality-sensitive consumers in tests.
    #[must_use]
    pub fn new(view: ForeachBinderView<'a>, extra_binder: &'a Node) -> Self {
        Self { view, extra_binder }
    }

    /// Simulated binder count once F02 allows N binders.
    #[must_use]
    pub fn simulated_binder_count(&self) -> usize {
        self.view.production_binder_count() + 1
    }

    /// All binders visible to a cardinality-tolerant consumer under the probe.
    #[must_use]
    pub fn ordered_binder_nodes(&self) -> Vec<&'a Node> {
        let mut binders: Vec<&'a Node> = self.view.ordered_binder_nodes().into();
        binders.push(self.extra_binder);
        binders
    }

    /// Consumers that only read [`ForeachBinderView::legacy_single_binder`] stay at one.
    #[must_use]
    pub fn legacy_only_binder_count(&self) -> usize {
        self.view.production_binder_count()
    }
}
