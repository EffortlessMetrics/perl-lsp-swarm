//! Parity between the structural registry and production FieldId / traversal facts.
//!
//! The checker compares a supplied registry against observed walkers so tests can
//! inject mutated rows. Production walkers live in [`super::visit`].

use super::{
    ChildFieldSpec, FieldCardinality, GrammarNameSpec, KIND_SCHEMA_MODE, KIND_SCHEMA_VERSION,
    KindBody, KindStructuralRow, TraversalObservation, observe_kind_traversal,
};
use crate::{FieldId, Node};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Owned pair of nodes that differ by one runtime grammar-name input.
#[derive(Debug, Clone, PartialEq)]
pub struct GrammarInputWitness {
    /// Variant whose grammar name is runtime-derived.
    pub kind_name: &'static str,
    /// Payload or child-field name that must be declared on the row.
    pub input: &'static str,
    /// Representative whose grammar name should differ from [`Self::right`].
    pub left: Node,
    /// Representative that differs from [`Self::left`] in `input`.
    pub right: Node,
}

/// Inputs consumed by [`check_kind_schema`].
#[derive(Debug, Clone, Copy)]
pub struct KindSchemaEvidence<'a> {
    /// Shadow registry rows in claimed declaration order.
    pub registry: &'a [KindStructuralRow<'a>],
    /// Canonical [`NodeKind::ALL_KIND_NAMES`].
    pub kind_names: &'a [&'static str],
    /// Canonical [`NodeKind::RECOVERY_KIND_NAMES`].
    pub recovery_names: &'a [&'static str],
    /// Canonical [`FieldId::ALL`].
    pub field_ids: &'a [FieldId],
    /// Exhaustive representative bank, including #7754 fixtures.
    pub representatives: &'a [Node],
    /// Optional-absent and repeated-empty forms.
    pub cardinality_forms: &'a [Node],
    /// Grammar-name input pairs for runtime-derived rows.
    pub grammar_witnesses: &'a [GrammarInputWitness],
}

/// One discriminating mismatch between the shadow registry and production.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindSchemaMismatch {
    /// A canonical kind has no registry row.
    MissingRow {
        /// Missing kind name.
        kind_name: String,
    },
    /// A registry row names a kind that is not canonical.
    ExtraRow {
        /// Unexpected kind name.
        kind_name: String,
    },
    /// Registry order does not match canonical declaration order.
    OrderDrift {
        /// Canonical names.
        expected: Vec<String>,
        /// Registry names.
        actual: Vec<String>,
    },
    /// The same kind appears more than once.
    DuplicateKind {
        /// Duplicated kind name.
        kind_name: String,
    },
    /// A row lists the same child field twice.
    DuplicateChildField {
        /// Kind that declared the duplicate.
        kind_name: String,
        /// Duplicated field name.
        field: String,
    },
    /// A row names a field outside [`FieldId::ALL`].
    UnknownFieldId {
        /// Kind that named the field.
        kind_name: String,
        /// Unknown field name.
        field: String,
    },
    /// `FieldId::ALL` itself contains a duplicate name.
    DuplicateFieldIdInventory {
        /// Duplicated field name.
        field: String,
    },
    /// A `FieldId` inventory entry is never named by a registry row.
    UnusedFieldIdInventory {
        /// Unused field name.
        field: String,
    },
    /// Observed first-occurrence fields disagree with the declared present set.
    ChildFieldSet {
        /// Kind under comparison.
        kind_name: String,
        /// Declared present field names.
        declared: Vec<String>,
        /// Observed present field names.
        observed: Vec<String>,
    },
    /// Present fields match but canonical order does not.
    ChildFieldOrder {
        /// Kind under comparison.
        kind_name: String,
        /// Declared order of present fields.
        expected: Vec<String>,
        /// Observed first-occurrence order.
        actual: Vec<String>,
    },
    /// Declared cardinality does not match the observed emission counts.
    Cardinality {
        /// Kind under comparison.
        kind_name: String,
        /// Field whose cardinality drifted.
        field: String,
        /// Declared cardinality token.
        declared: String,
        /// Why the observation is incompatible.
        detail: String,
    },
    /// A leaf row declared child fields.
    LeafDeclaresChildren {
        /// Kind marked leaf.
        kind_name: String,
    },
    /// A child-bearing row declared no child fields.
    ChildBearingDeclaresNoChildren {
        /// Kind marked child-bearing.
        kind_name: String,
    },
    /// A child-bearing representative was tagged as a leaf.
    ChildBearingMarkedLeaf {
        /// Kind that actually emitted children.
        kind_name: String,
    },
    /// Recovery tags do not equal the canonical recovery inventory.
    RecoverySet {
        /// Canonical recovery names.
        expected: Vec<String>,
        /// Registry recovery tags.
        actual: Vec<String>,
    },
    /// A canonical kind has no representative constructor.
    MissingRepresentative {
        /// Kind lacking a representative.
        kind_name: String,
    },
    /// Immutable and mutable walkers visited children in different order.
    ImmutableMutableDivergence {
        /// Kind under comparison.
        kind_name: String,
        /// Immutable visit ids.
        immutable: Vec<usize>,
        /// Mutable visit ids.
        mutable: Vec<usize>,
    },
    /// Static versus runtime grammar-name class does not match production.
    GrammarClass {
        /// Kind under comparison.
        kind_name: String,
        /// What drifted.
        detail: String,
    },
    /// A runtime-derived row omits an input that a witness proves is live.
    MissingGrammarInput {
        /// Kind under comparison.
        kind_name: String,
        /// Missing input name.
        input: String,
    },
    /// A witness input is not declared on the runtime-derived row.
    UndeclaredGrammarInput {
        /// Kind under comparison.
        kind_name: String,
        /// Witness input name.
        input: String,
    },
    /// A grammar witness pair produced the same grammar name.
    VacuousGrammarWitness {
        /// Kind under comparison.
        kind_name: String,
        /// Input that failed to change the grammar name.
        input: String,
    },
    /// A runtime-derived row has no witness for a declared input.
    UnwitnessedGrammarInput {
        /// Kind under comparison.
        kind_name: String,
        /// Declared input without a witness pair.
        input: String,
    },
}

impl fmt::Display for KindSchemaMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRow { kind_name } => {
                write!(f, "missing structural row for {kind_name}")
            }
            Self::ExtraRow { kind_name } => {
                write!(f, "extra structural row for {kind_name}")
            }
            Self::OrderDrift { expected, actual } => {
                write!(f, "registry order {actual:?} != canonical {expected:?}")
            }
            Self::DuplicateKind { kind_name } => {
                write!(f, "duplicate structural row for {kind_name}")
            }
            Self::DuplicateChildField { kind_name, field } => {
                write!(f, "{kind_name}: duplicate child field {field}")
            }
            Self::UnknownFieldId { kind_name, field } => {
                write!(f, "{kind_name}: field {field} is not a FieldId")
            }
            Self::DuplicateFieldIdInventory { field } => {
                write!(f, "FieldId inventory contains duplicate {field}")
            }
            Self::UnusedFieldIdInventory { field } => {
                write!(f, "FieldId inventory contains unused {field}")
            }
            Self::ChildFieldSet { kind_name, declared, observed } => {
                write!(
                    f,
                    "{kind_name}: declared child fields {declared:?} != observed {observed:?}"
                )
            }
            Self::ChildFieldOrder { kind_name, expected, actual } => {
                write!(f, "{kind_name}: child field order {actual:?} != {expected:?}")
            }
            Self::Cardinality { kind_name, field, declared, detail } => {
                write!(f, "{kind_name}.{field}: {declared} cardinality failed: {detail}")
            }
            Self::LeafDeclaresChildren { kind_name } => {
                write!(f, "{kind_name}: leaf row declares child fields")
            }
            Self::ChildBearingDeclaresNoChildren { kind_name } => {
                write!(f, "{kind_name}: child-bearing row declares no child fields")
            }
            Self::ChildBearingMarkedLeaf { kind_name } => {
                write!(f, "{kind_name}: child-bearing variant marked as a leaf")
            }
            Self::RecoverySet { expected, actual } => {
                write!(f, "recovery tags {actual:?} != canonical {expected:?}")
            }
            Self::MissingRepresentative { kind_name } => {
                write!(f, "no representative constructor for {kind_name}")
            }
            Self::ImmutableMutableDivergence { kind_name, immutable, mutable } => {
                write!(f, "{kind_name}: immutable visit {immutable:?} != mutable visit {mutable:?}")
            }
            Self::GrammarClass { kind_name, detail } => {
                write!(f, "{kind_name}: grammar class drift: {detail}")
            }
            Self::MissingGrammarInput { kind_name, input } => {
                write!(f, "{kind_name}: runtime grammar input {input} is not declared")
            }
            Self::UndeclaredGrammarInput { kind_name, input } => {
                write!(f, "{kind_name}: witness input {input} is not declared")
            }
            Self::VacuousGrammarWitness { kind_name, input } => {
                write!(f, "{kind_name}: grammar witness for {input} did not change the name")
            }
            Self::UnwitnessedGrammarInput { kind_name, input } => {
                write!(f, "{kind_name}: declared grammar input {input} has no witness")
            }
        }
    }
}

/// Collected check-mode result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KindSchemaReport {
    /// Discriminating mismatches; empty means the supplied registry is in parity.
    pub mismatches: Vec<KindSchemaMismatch>,
}

impl KindSchemaReport {
    /// Whether the supplied registry matched every checked production surface.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.mismatches.is_empty()
    }
}

impl fmt::Display for KindSchemaReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mismatches.is_empty() {
            return write!(f, "kind schema parity is clean");
        }
        writeln!(f, "kind schema parity found {} mismatch(es):", self.mismatches.len())?;
        for mismatch in &self.mismatches {
            writeln!(f, "- {mismatch}")?;
        }
        Ok(())
    }
}

/// Deterministic serialization of a registry. Order is the slice order.
#[must_use]
pub fn serialize_kind_schema(registry: &[KindStructuralRow<'_>]) -> String {
    let mut out = format!(
        "# perl-ast NodeKind structural schema v{KIND_SCHEMA_VERSION}\n# mode={KIND_SCHEMA_MODE}\n"
    );
    for (index, row) in registry.iter().enumerate() {
        out.push_str(row.kind_name);
        out.push('\t');
        out.push_str(&index.to_string());
        out.push('\t');
        out.push_str(match row.body {
            KindBody::Leaf => "leaf",
            KindBody::ChildBearing => "child-bearing",
        });
        out.push('\t');
        if row.children.is_empty() {
            out.push('-');
        } else {
            for (i, child) in row.children.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(child.field.name());
                out.push(':');
                out.push_str(child.cardinality.token());
            }
        }
        out.push('\t');
        match row.grammar {
            GrammarNameSpec::Static(name) => {
                out.push_str("static:");
                out.push_str(name);
            }
            GrammarNameSpec::RuntimeDerived { inputs } => {
                out.push_str("runtime:");
                out.push_str(&inputs.join("+"));
            }
        }
        out.push('\t');
        out.push_str(if row.recovery { "recovery" } else { "source" });
        out.push('\t');
        out.push_str(if row.source_boundary { "boundary" } else { "interior" });
        out.push('\t');
        out.push_str(row.compatibility.token());
        out.push('\n');
    }
    out
}

/// Compare a shadow registry with current production observations.
#[must_use]
pub fn check_kind_schema(evidence: &KindSchemaEvidence<'_>) -> KindSchemaReport {
    let mut report = KindSchemaReport::default();
    check_field_id_inventory(evidence, &mut report);
    check_kind_inventory(evidence, &mut report);
    check_recovery_inventory(evidence, &mut report);
    check_row_self_consistency(evidence, &mut report);
    check_representatives(evidence, &mut report);
    check_grammar(evidence, &mut report);
    report
}

fn check_field_id_inventory(evidence: &KindSchemaEvidence<'_>, report: &mut KindSchemaReport) {
    let mut seen = BTreeSet::new();
    for field in evidence.field_ids {
        let name = field.name();
        if !seen.insert(name) {
            report
                .mismatches
                .push(KindSchemaMismatch::DuplicateFieldIdInventory { field: name.to_string() });
        }
    }
    let used: BTreeSet<&str> = evidence
        .registry
        .iter()
        .flat_map(|row| row.children.iter().map(|child| child.field.name()))
        .collect();
    for field in evidence.field_ids {
        let name = field.name();
        if !used.contains(name) {
            report
                .mismatches
                .push(KindSchemaMismatch::UnusedFieldIdInventory { field: name.to_string() });
        }
    }
}

fn check_kind_inventory(evidence: &KindSchemaEvidence<'_>, report: &mut KindSchemaReport) {
    let expected: Vec<String> =
        evidence.kind_names.iter().map(|name| (*name).to_string()).collect();
    let actual: Vec<String> =
        evidence.registry.iter().map(|row| row.kind_name.to_string()).collect();

    let mut seen = BTreeSet::new();
    for name in &actual {
        if !seen.insert(name.clone()) {
            report.mismatches.push(KindSchemaMismatch::DuplicateKind { kind_name: name.clone() });
        }
    }

    let expected_set: BTreeSet<&str> = evidence.kind_names.iter().copied().collect();
    let actual_set: BTreeSet<&str> = evidence.registry.iter().map(|row| row.kind_name).collect();
    for name in expected_set.difference(&actual_set) {
        report.mismatches.push(KindSchemaMismatch::MissingRow { kind_name: (*name).to_string() });
    }
    for name in actual_set.difference(&expected_set) {
        report.mismatches.push(KindSchemaMismatch::ExtraRow { kind_name: (*name).to_string() });
    }
    if expected != actual {
        report.mismatches.push(KindSchemaMismatch::OrderDrift { expected, actual });
    }
}

fn check_recovery_inventory(evidence: &KindSchemaEvidence<'_>, report: &mut KindSchemaReport) {
    let expected: BTreeSet<String> =
        evidence.recovery_names.iter().map(|name| (*name).to_string()).collect();
    let actual: BTreeSet<String> = evidence
        .registry
        .iter()
        .filter(|row| row.recovery)
        .map(|row| row.kind_name.to_string())
        .collect();
    if expected != actual {
        report.mismatches.push(KindSchemaMismatch::RecoverySet {
            expected: expected.into_iter().collect(),
            actual: actual.into_iter().collect(),
        });
    }
}

fn check_row_self_consistency(evidence: &KindSchemaEvidence<'_>, report: &mut KindSchemaReport) {
    let field_names: BTreeSet<&str> = evidence.field_ids.iter().map(|field| field.name()).collect();
    for row in evidence.registry {
        let mut seen_fields = BTreeSet::new();
        for child in row.children {
            let name = child.field.name();
            if !field_names.contains(name) {
                report.mismatches.push(KindSchemaMismatch::UnknownFieldId {
                    kind_name: row.kind_name.to_string(),
                    field: name.to_string(),
                });
            }
            if !seen_fields.insert(name) {
                report.mismatches.push(KindSchemaMismatch::DuplicateChildField {
                    kind_name: row.kind_name.to_string(),
                    field: name.to_string(),
                });
            }
        }
        match row.body {
            KindBody::Leaf if !row.children.is_empty() => {
                report.mismatches.push(KindSchemaMismatch::LeafDeclaresChildren {
                    kind_name: row.kind_name.to_string(),
                });
            }
            KindBody::ChildBearing if row.children.is_empty() => {
                report.mismatches.push(KindSchemaMismatch::ChildBearingDeclaresNoChildren {
                    kind_name: row.kind_name.to_string(),
                });
            }
            _ => {}
        }
    }
}

fn check_representatives(evidence: &KindSchemaEvidence<'_>, report: &mut KindSchemaReport) {
    let rows: BTreeMap<&str, &KindStructuralRow<'_>> =
        evidence.registry.iter().map(|row| (row.kind_name, row)).collect();

    let mut observations_by_kind: BTreeMap<&str, Vec<TraversalObservation>> = BTreeMap::new();
    let all_nodes = evidence.representatives.iter().chain(evidence.cardinality_forms.iter()).chain(
        evidence.grammar_witnesses.iter().flat_map(|witness| [&witness.left, &witness.right]),
    );

    for node in all_nodes {
        let observation = observe_kind_traversal(node);
        if observation.immutable_visit_ids != observation.mutable_visit_ids
            || observation.immutable_field_sequence != observation.mutable_field_sequence
        {
            report.mismatches.push(KindSchemaMismatch::ImmutableMutableDivergence {
                kind_name: observation.kind_name.to_string(),
                immutable: observation.immutable_visit_ids.clone(),
                mutable: observation.mutable_visit_ids.clone(),
            });
        }
        observations_by_kind.entry(observation.kind_name).or_default().push(observation);
    }

    for kind_name in evidence.kind_names {
        if !observations_by_kind.contains_key(kind_name) {
            report.mismatches.push(KindSchemaMismatch::MissingRepresentative {
                kind_name: (*kind_name).to_string(),
            });
        }
    }

    for (kind_name, observations) in &observations_by_kind {
        let Some(row) = rows.get(kind_name) else {
            continue;
        };
        reconcile_kind(row, observations, report);
    }
}

fn reconcile_kind(
    row: &KindStructuralRow<'_>,
    observations: &[TraversalObservation],
    report: &mut KindSchemaReport,
) {
    let mut counts_by_field: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for child in row.children {
        counts_by_field.insert(child.field.name(), Vec::new());
    }

    for observation in observations {
        if row.is_leaf() && !observation.fields_in_first_occurrence_order.is_empty() {
            report.mismatches.push(KindSchemaMismatch::ChildBearingMarkedLeaf {
                kind_name: row.kind_name.to_string(),
            });
        }

        let declared_present: Vec<String> = row
            .children
            .iter()
            .filter(|child| {
                observation.field_counts.get(child.field.name()).copied().unwrap_or(0) > 0
            })
            .map(|child| child.field.name().to_string())
            .collect();
        let observed_present: Vec<String> = observation
            .fields_in_first_occurrence_order
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        let declared_set: BTreeSet<&str> = declared_present.iter().map(String::as_str).collect();
        let observed_set: BTreeSet<&str> = observed_present.iter().map(String::as_str).collect();
        if declared_set != observed_set {
            report.mismatches.push(KindSchemaMismatch::ChildFieldSet {
                kind_name: row.kind_name.to_string(),
                declared: declared_present,
                observed: observed_present,
            });
        } else if declared_present != observed_present {
            report.mismatches.push(KindSchemaMismatch::ChildFieldOrder {
                kind_name: row.kind_name.to_string(),
                expected: declared_present,
                actual: observed_present,
            });
        }

        for child in row.children {
            let count = observation.field_counts.get(child.field.name()).copied().unwrap_or(0);
            if let Some(counts) = counts_by_field.get_mut(child.field.name()) {
                counts.push(count);
            }
            if child.cardinality == FieldCardinality::Optional && count > 1 {
                report.mismatches.push(KindSchemaMismatch::Cardinality {
                    kind_name: row.kind_name.to_string(),
                    field: child.field.name().to_string(),
                    declared: child.cardinality.token().to_string(),
                    detail: format!("observed {count} emissions"),
                });
            }
        }
    }

    for child in row.children {
        let counts = counts_by_field.get(child.field.name()).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(detail) = cardinality_failure(child, counts) {
            report.mismatches.push(KindSchemaMismatch::Cardinality {
                kind_name: row.kind_name.to_string(),
                field: child.field.name().to_string(),
                declared: child.cardinality.token().to_string(),
                detail,
            });
        }
    }
}

fn cardinality_failure(child: &ChildFieldSpec, counts: &[usize]) -> Option<String> {
    match child.cardinality {
        FieldCardinality::Required => {
            if counts.contains(&0) {
                Some("a representative omitted a required field".to_string())
            } else {
                None
            }
        }
        FieldCardinality::Optional => {
            let has_absent = counts.contains(&0);
            let has_present = counts.contains(&1);
            if has_absent && has_present {
                None
            } else {
                Some(format!("optional field needs absent and present forms, observed {counts:?}"))
            }
        }
        FieldCardinality::Repeated => {
            let has_empty = counts.contains(&0);
            let has_many = counts.iter().any(|count| *count >= 2);
            if has_empty && has_many {
                None
            } else {
                Some(format!("repeated field needs empty and multi forms, observed {counts:?}"))
            }
        }
    }
}

fn check_grammar(evidence: &KindSchemaEvidence<'_>, report: &mut KindSchemaReport) {
    let rows: BTreeMap<&str, &KindStructuralRow<'_>> =
        evidence.registry.iter().map(|row| (row.kind_name, row)).collect();

    let mut samples: BTreeMap<&str, &Node> = BTreeMap::new();
    for node in evidence.representatives.iter().chain(evidence.cardinality_forms.iter()) {
        samples.entry(node.kind.kind_name()).or_insert(node);
    }

    for row in evidence.registry {
        let Some(sample) = samples.get(row.kind_name) else {
            continue;
        };
        match row.grammar {
            GrammarNameSpec::Static(expected) => match sample.kind.grammar_kind_name_static() {
                Some(actual) if actual == expected => {}
                Some(actual) => report.mismatches.push(KindSchemaMismatch::GrammarClass {
                    kind_name: row.kind_name.to_string(),
                    detail: format!("declared static {expected}, production {actual}"),
                }),
                None => report.mismatches.push(KindSchemaMismatch::GrammarClass {
                    kind_name: row.kind_name.to_string(),
                    detail: format!("declared static {expected}, production is runtime-derived"),
                }),
            },
            GrammarNameSpec::RuntimeDerived { .. } => {
                if let Some(actual) = sample.kind.grammar_kind_name_static() {
                    report.mismatches.push(KindSchemaMismatch::GrammarClass {
                        kind_name: row.kind_name.to_string(),
                        detail: format!("declared runtime-derived, production static {actual}"),
                    });
                }
            }
        }
    }

    let mut witnessed: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for witness in evidence.grammar_witnesses {
        if witness.left.kind.kind_name() != witness.kind_name
            || witness.right.kind.kind_name() != witness.kind_name
        {
            report.mismatches.push(KindSchemaMismatch::VacuousGrammarWitness {
                kind_name: witness.kind_name.to_string(),
                input: witness.input.to_string(),
            });
            continue;
        }
        if witness.left.kind.grammar_kind_name() == witness.right.kind.grammar_kind_name() {
            report.mismatches.push(KindSchemaMismatch::VacuousGrammarWitness {
                kind_name: witness.kind_name.to_string(),
                input: witness.input.to_string(),
            });
        }
        let Some(row) = rows.get(witness.kind_name) else {
            continue;
        };
        match row.grammar {
            GrammarNameSpec::RuntimeDerived { inputs } => {
                if inputs.contains(&witness.input) {
                    witnessed.entry(witness.kind_name).or_default().insert(witness.input);
                } else {
                    report.mismatches.push(KindSchemaMismatch::MissingGrammarInput {
                        kind_name: witness.kind_name.to_string(),
                        input: witness.input.to_string(),
                    });
                }
            }
            GrammarNameSpec::Static(_) => {
                report.mismatches.push(KindSchemaMismatch::UndeclaredGrammarInput {
                    kind_name: witness.kind_name.to_string(),
                    input: witness.input.to_string(),
                });
            }
        }
    }

    for row in evidence.registry {
        if let GrammarNameSpec::RuntimeDerived { inputs } = row.grammar {
            let seen = witnessed.get(row.kind_name);
            for input in inputs {
                let witnessed_input = seen.is_some_and(|set| set.contains(input));
                if !witnessed_input {
                    report.mismatches.push(KindSchemaMismatch::UnwitnessedGrammarInput {
                        kind_name: row.kind_name.to_string(),
                        input: (*input).to_string(),
                    });
                }
            }
        }
    }
}
