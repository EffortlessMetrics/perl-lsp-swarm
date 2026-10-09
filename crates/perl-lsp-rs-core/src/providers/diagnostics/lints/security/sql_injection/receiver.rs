//! Same-file DBI receiver evidence for PL607 (#5035 / #16864).
//!
//! A security warning never guesses DB-ness. Qualification is structural: a
//! name whose pre-sink assignments are not all `DBI->connect` calls (shadowed
//! inner `my $dbh = Engine->new`, list/signature rebinding, connect introduced
//! after the sink) is unproven and stays silent. Handles reached through
//! aliases, parameters, or DBD-specific class names stay unproven likewise;
//! the binding-precise statement-handle identity model is owned by #7471.

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

#[derive(Default)]
struct ReceiverNameHistory {
    assignments: Vec<ReceiverAssignment>,
    /// After [`ReceiverAssignmentIndex::finish`], index of the first
    /// non-connect assignment in source order, if any.
    first_non_connect: Option<usize>,
}

/// Pre-indexed receiver-assignment evidence, keyed by receiver name.
///
/// One pass over the document builds this index; every SQL sink then resolves
/// its receiver by name plus a source-ordered partition of only that
/// receiver's assignments. After `finish`, each lookup is O(log A_name): the
/// first non-connect index is precomputed, so a crafted document cannot
/// multiply sinks against assignment prefixes to exhaust CPU through an
/// open-document diagnostic request (#5035 review / #16864).
pub(super) struct ReceiverAssignmentIndex {
    by_name: HashMap<String, ReceiverNameHistory>,
}

impl ReceiverAssignmentIndex {
    pub(super) fn new() -> Self {
        Self { by_name: HashMap::new() }
    }

    fn record(&mut self, name: String, offset: usize, is_connect: bool) {
        self.by_name
            .entry(name)
            .or_default()
            .assignments
            .push(ReceiverAssignment { offset, is_connect });
    }

    /// Freeze the index after collection, putting every receiver's
    /// assignments in source order and recording the first non-connect.
    pub(super) fn finish(mut self) -> Self {
        for bucket in self.by_name.values_mut() {
            bucket.assignments.sort_by_key(|assignment| assignment.offset);
            bucket.first_non_connect =
                bucket.assignments.iter().position(|assignment| !assignment.is_connect);
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
        let prior =
            bucket.assignments.partition_point(|assignment| assignment.offset < sink_offset);
        prior > 0 && bucket.first_non_connect.is_none_or(|index| index >= prior)
    }
}

/// Collect same-file scalar assignments and list/signature bindings per
/// receiver name.
///
/// This is the AST-provable form of the repository's DBI receiver-classification
/// precedent (`providers/completion/completion/methods.rs`,
/// `infer_receiver_type`: "check if variable was assigned from DBI->connect"):
/// the canonical `my $dbh = DBI->connect(...)` (or plain assignment) idiom.
/// A later `my ($dbh) = @_` or signature `$dbh` is mixed evidence and stays
/// silent — the warning never guesses which binding the sink sees.
pub(super) fn collect_receiver_assignments(node: &Node, index: &mut ReceiverAssignmentIndex) {
    walk_node(node, &mut |visited| match &visited.kind {
        NodeKind::VariableDeclaration { variable, initializer: Some(init), .. } => {
            if let Some(name) = scalar_variable_name(variable) {
                index.record(name, visited.location.start, is_dbi_connect(init));
            }
        }
        NodeKind::VariableListDeclaration { variables, initializer, .. } => {
            let is_connect = initializer.as_deref().is_some_and(is_dbi_connect);
            for declared in variables {
                record_declared_scalars(declared, index, visited.location.start, is_connect);
            }
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            if let Some(name) = scalar_variable_name(lhs) {
                index.record(name, visited.location.start, is_dbi_connect(rhs));
            }
        }
        NodeKind::MandatoryParameter { variable }
        | NodeKind::SlurpyParameter { variable }
        | NodeKind::NamedParameter { variable, .. }
        | NodeKind::OptionalParameter { variable, .. } => {
            record_declared_scalars(variable, index, visited.location.start, false);
        }
        _ => {}
    });
}

fn is_dbi_connect(value: &Node) -> bool {
    match &value.kind {
        NodeKind::MethodCall { object, method, .. } => {
            matches!(&object.kind, NodeKind::Identifier { name } if name == "DBI")
                && method == "connect"
        }
        _ => false,
    }
}

fn record_declared_scalars(
    node: &Node,
    index: &mut ReceiverAssignmentIndex,
    offset: usize,
    is_connect: bool,
) {
    match &node.kind {
        NodeKind::Variable { .. } => {
            if let Some(name) = scalar_variable_name(node) {
                index.record(name, offset, is_connect);
            }
        }
        NodeKind::VariableWithAttributes { variable, .. } => {
            record_declared_scalars(variable, index, offset, is_connect);
        }
        NodeKind::NestedVariableList { items } => {
            for item in items {
                record_declared_scalars(item, index, offset, is_connect);
            }
        }
        _ => {}
    }
}

/// The bare name of a scalar variable node (`$dbh` -> `dbh`), if this node is
/// one.
pub(super) fn scalar_variable_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => Some(name.clone()),
        _ => None,
    }
}
