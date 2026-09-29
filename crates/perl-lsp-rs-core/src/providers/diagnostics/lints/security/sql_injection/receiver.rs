//! Same-file DBI receiver evidence for PL607 (#5035 / #16864).
//!
//! A security warning never guesses DB-ness. Qualification is structural: a
//! name whose pre-sink assignments are not all `DBI->connect` calls (shadowed
//! inner `my $dbh = Engine->new`, rebinding, connect introduced after the
//! sink) is unproven and stays silent. Handles reached through aliases,
//! parameters, or DBD-specific class names stay unproven likewise; the
//! binding-precise statement-handle identity model is owned by #7471.

use std::collections::HashMap;

use perl_parser_core::ast::{Node, NodeKind};

use crate::providers::diagnostics::walker::walk_node;

/// One same-file scalar assignment observed for a receiver name.
struct ReceiverAssignment {
    /// Byte offset of the assignment site, for source-order qualification.
    offset: usize,
    /// Whether the assigned value is a `DBI->connect(...)` call.
    is_connect: bool,
}

/// Pre-indexed receiver-assignment evidence, keyed by receiver name.
///
/// One pass over the document builds this index; every SQL sink then resolves
/// its receiver in O(1) by name plus a source-ordered prefix scan of only that
/// receiver's assignments. A document with A assignments and S sinks costs
/// O(A + S log A) per diagnostic pass instead of O(A × S), so a crafted
/// document cannot multiply sinks against assignments to exhaust CPU through
/// an open-document diagnostic request (#5035 review).
pub(super) struct ReceiverAssignmentIndex {
    by_name: HashMap<String, Vec<ReceiverAssignment>>,
}

impl ReceiverAssignmentIndex {
    pub(super) fn new() -> Self {
        Self { by_name: HashMap::new() }
    }

    fn record(&mut self, name: String, offset: usize, is_connect: bool) {
        self.by_name.entry(name).or_default().push(ReceiverAssignment { offset, is_connect });
    }

    /// Freeze the index after collection, putting every receiver's
    /// assignments in source order. Pre-order AST traversal is source-ordered
    /// in practice; sorting each bucket by offset makes that a guarantee
    /// instead of an assumption.
    pub(super) fn finish(mut self) -> Self {
        for bucket in self.by_name.values_mut() {
            bucket.sort_by_key(|assignment| assignment.offset);
        }
        self
    }

    /// Whether `name` is a proven DBI handle at a sink starting at
    /// `sink_offset`: at least one same-file assignment before the sink must
    /// exist, and every such assignment must come from `DBI->connect(...)`.
    pub(super) fn is_proven_dbh(&self, name: &str, sink_offset: usize) -> bool {
        let Some(bucket) = self.by_name.get(name) else {
            return false;
        };
        let prior = bucket.partition_point(|assignment| assignment.offset < sink_offset);
        prior > 0 && bucket[..prior].iter().all(|assignment| assignment.is_connect)
    }
}

/// Collect same-file scalar assignments per receiver name.
///
/// This is the AST-provable form of the repository's DBI receiver-classification
/// precedent (`providers/completion/completion/methods.rs`,
/// `infer_receiver_type`: "check if variable was assigned from DBI->connect"):
/// the canonical `my $dbh = DBI->connect(...)` (or plain assignment) idiom.
pub(super) fn collect_receiver_assignments(node: &Node, index: &mut ReceiverAssignmentIndex) {
    walk_node(node, &mut |visited| {
        let connect_assigned = |value: &Node| match &value.kind {
            NodeKind::MethodCall { object, method, .. } => {
                matches!(&object.kind, NodeKind::Identifier { name } if name == "DBI")
                    && method == "connect"
            }
            _ => false,
        };

        match &visited.kind {
            // Declarations without an initializer are neutral: they carry no
            // evidence about the receiver's origin either way.
            NodeKind::VariableDeclaration { variable, initializer: Some(init), .. } => {
                if let Some(name) = scalar_variable_name(variable) {
                    index.record(name, visited.location.start, connect_assigned(init));
                }
            }
            NodeKind::Assignment { lhs, rhs, .. } => {
                if let Some(name) = scalar_variable_name(lhs) {
                    index.record(name, visited.location.start, connect_assigned(rhs));
                }
            }
            _ => {}
        }
    });
}

/// The bare name of a scalar variable node (`$dbh` -> `dbh`), if this node is
/// one.
pub(super) fn scalar_variable_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => Some(name.clone()),
        _ => None,
    }
}
