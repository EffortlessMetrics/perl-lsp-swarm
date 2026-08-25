//! Observe production traversal without becoming a second child walker.
//!
//! Field identities come from [`crate::Node::try_for_each_child_with_field`].
//! Mutable visit order comes from [`crate::Node::for_each_child_mut`]. The
//! mutable walker still has no `FieldId` surface; visit-order parity uses
//! stamped direct-child locations.

use crate::{FieldId, Node, SourceLocation};
use std::collections::BTreeMap;
use std::ops::ControlFlow;

/// Direct-child observation of one representative node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraversalObservation {
    /// [`crate::NodeKind::kind_name`] of the observed node.
    pub kind_name: &'static str,
    /// First-occurrence field order from the immutable walker.
    pub fields_in_first_occurrence_order: Vec<&'static str>,
    /// Emission counts keyed by canonical field name.
    pub field_counts: BTreeMap<&'static str, usize>,
    /// Stamped visit ids from the immutable walker.
    pub immutable_visit_ids: Vec<usize>,
    /// Stamped visit ids from the mutable walker.
    pub mutable_visit_ids: Vec<usize>,
}

/// Stamp each direct child with a unique location so mutable visit order is observable.
fn stamp_direct_child_visit_ids(node: &mut Node) {
    let mut next = 1_usize;
    node.for_each_child_mut(|child| {
        child.location = SourceLocation { start: next, end: next };
        next = next.saturating_add(1);
    });
}

/// Observe one node's production immutable and mutable child walks.
#[must_use]
pub fn observe_kind_traversal(node: &Node) -> TraversalObservation {
    let mut stamped = node.clone();
    stamp_direct_child_visit_ids(&mut stamped);

    let mut fields_in_first_occurrence_order = Vec::new();
    let mut field_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut immutable_visit_ids = Vec::new();
    let _ = stamped.try_for_each_child_with_field(|field, child| {
        if let Some(field) = field {
            let name = FieldId::name(field);
            let count = field_counts.entry(name).or_insert(0);
            if *count == 0 {
                fields_in_first_occurrence_order.push(name);
            }
            *count = count.saturating_add(1);
        }
        immutable_visit_ids.push(child.location.start);
        ControlFlow::<()>::Continue(())
    });

    let mut mutable_visit_ids = Vec::new();
    stamped.for_each_child_mut(|child| {
        mutable_visit_ids.push(child.location.start);
    });

    TraversalObservation {
        kind_name: stamped.kind.kind_name(),
        fields_in_first_occurrence_order,
        field_counts,
        immutable_visit_ids,
        mutable_visit_ids,
    }
}
