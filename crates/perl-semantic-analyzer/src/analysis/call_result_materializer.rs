//! Pure callsite materialization for canonical callable-result relations.
//!
//! This module resolves no names and reads no source. It combines one already
//! resolved callable-result relation with one exact callsite's receiver and
//! argument bindings, preserving optionality, context, currentness, ambiguity,
//! and deterministic budgets for later receiver and key queries.

use std::collections::BTreeSet;

use perl_semantic_facts::{
    CallableResultFact, CallableResultRelation, EntityId, FactId, SemanticFactStatus,
    SemanticFreshness, SourceAnchor, SourceGeneration, ValueShape,
};

/// Perl value context at one callsite.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallValueContext {
    /// Scalar context.
    Scalar,
    /// List context.
    List,
    /// Void context.
    Void,
    /// The callsite context has not been established.
    Unknown,
}

/// One current value bound to a receiver or argument at the callsite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallBoundValue {
    /// Bounded value shape established by canonical facts.
    pub value: ValueShape,
    /// Source anchor that established this bound value, when available.
    pub source_anchor: Option<SourceAnchor>,
    /// Generation of the source or semantic input that established the value.
    pub generation: SourceGeneration,
    /// Freshness of the value relative to the current call request.
    pub freshness: SemanticFreshness,
}

impl CallBoundValue {
    /// Construct one current source-backed bound value.
    #[must_use]
    pub fn current(
        value: ValueShape,
        source_anchor: Option<SourceAnchor>,
        generation: SourceGeneration,
    ) -> Self {
        Self { value, source_anchor, generation, freshness: SemanticFreshness::Fresh }
    }
}

/// One canonical argument-to-parameter binding value for the exact call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgumentValue {
    /// Canonical parameter binding entity.
    pub parameter_entity_id: EntityId,
    /// Zero-based parameter position retained for explanation and fallback.
    pub position: u16,
    /// Value bound to this parameter for the current call.
    pub bound: CallBoundValue,
}

/// Exact callsite inputs supplied to [`materialize_call_result`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallResultMaterializationInput {
    /// Canonical call occurrence or call expression entity.
    pub call_entity_id: EntityId,
    /// Exact callable target identity selected for this call.
    pub callable_entity_id: EntityId,
    /// Callsite source anchor.
    pub call_anchor: SourceAnchor,
    /// Current callsite source generation.
    pub call_generation: SourceGeneration,
    /// Freshness of the call identity and binding set.
    pub call_freshness: SemanticFreshness,
    /// Receiver value for method calls, when present.
    pub receiver: Option<CallBoundValue>,
    /// Canonical argument-to-parameter bindings for this call.
    pub arguments: Vec<CallArgumentValue>,
    /// Perl scalar/list/void context at the callsite.
    pub value_context: CallValueContext,
}

/// Deterministic work and result limits for call-result materialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallResultMaterializationBudget {
    /// Maximum relation nodes traversed.
    pub max_relation_nodes: usize,
    /// Maximum distinct possible values retained.
    pub max_values: usize,
}

impl Default for CallResultMaterializationBudget {
    fn default() -> Self {
        Self { max_relation_nodes: 64, max_values: 16 }
    }
}

/// Whether every relation alternative materialized from current exact inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallResultMaterializationCompleteness {
    /// Every admitted relation alternative materialized.
    Complete,
    /// One or more relation alternatives or inputs remained unavailable.
    Partial,
}

/// Typed limitation preventing complete materialization.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallResultMaterializationLimitation {
    /// The callable-result fact itself was not exact and current.
    RelationNotExact(SemanticFactStatus),
    /// The call identity or callsite binding generation was not current.
    CallNotCurrent,
    /// The callsite source anchor was malformed.
    InvalidCallAnchor,
    /// A receiver-self relation had no receiver input.
    MissingReceiver,
    /// A receiver or argument value was stale, unknown-generation, or otherwise not current.
    InputNotCurrent,
    /// An argument relation had no matching canonical parameter binding.
    MissingArgument,
    /// Several incompatible values were bound to the requested parameter identity.
    AmbiguousArgument,
    /// A concrete or bound value shape was unknown.
    UnknownValue,
    /// The relation was explicitly unknown.
    UnknownRelation,
    /// The fact's callable subject identity was absent or did not match the
    /// resolved callable identity for this call.
    CallableIdentityMismatch,
    /// Bare return could not be represented exactly in the current scalar/list context.
    BareReturnContext,
    /// Deterministic relation-node or value-cardinality budget was exhausted.
    BudgetExhausted,
}

/// Provider-neutral materialized result for one exact callsite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedCallResult {
    /// Canonical call identity.
    pub call_entity_id: EntityId,
    /// Canonical callable target identity.
    pub callable_entity_id: EntityId,
    /// Callable-result fact identity consumed by the materializer.
    pub relation_fact_id: FactId,
    /// Callsite source anchor.
    pub call_anchor: SourceAnchor,
    /// Callsite source generation.
    pub call_generation: SourceGeneration,
    /// Deterministically ordered possible value shapes.
    pub possible_values: Vec<ValueShape>,
    /// Whether the relation permits absence in addition to the possible values.
    pub optional: bool,
    /// Whether a bare return contributed to this result.
    pub bare_return: bool,
    /// Perl scalar/list/void context retained from the callsite.
    pub value_context: CallValueContext,
    /// Whether every relation alternative materialized.
    pub completeness: CallResultMaterializationCompleteness,
    /// Typed limitations preventing complete exact materialization.
    pub limitations: Vec<CallResultMaterializationLimitation>,
    /// Source anchors of receiver or argument inputs actually consumed.
    pub input_anchors: Vec<SourceAnchor>,
    /// Canonical parameter identities consumed by argument relations.
    pub argument_entities: Vec<EntityId>,
    /// Number of relation nodes inspected.
    pub work_units: usize,
}

impl MaterializedCallResult {
    /// Classify the materialized result without applying receiver-, key-, or provider-specific policy.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        if !matches!(self.completeness, CallResultMaterializationCompleteness::Complete)
            || !self.limitations.is_empty()
            || self.possible_values.iter().any(is_unknown_shape)
        {
            SemanticFactStatus::Degraded
        } else if self.possible_values.is_empty()
            && !(self.bare_return && matches!(self.value_context, CallValueContext::Void))
        {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Materialize one callable-result relation against one already-resolved callsite.
#[must_use]
pub fn materialize_call_result(
    fact: &CallableResultFact,
    input: &CallResultMaterializationInput,
    budget: CallResultMaterializationBudget,
) -> MaterializedCallResult {
    let mut state = MaterializationState::new(fact, input, budget);

    let fact_status = fact.status();
    if !matches!(fact_status, SemanticFactStatus::Exact) {
        state.limit(CallResultMaterializationLimitation::RelationNotExact(fact_status));
    }
    if fact.envelope.entity_id != Some(input.callable_entity_id) {
        state.limit(CallResultMaterializationLimitation::CallableIdentityMismatch);
    }
    if !input.call_generation.is_known()
        || !matches!(input.call_freshness, SemanticFreshness::Fresh)
    {
        state.limit(CallResultMaterializationLimitation::CallNotCurrent);
    }
    if input.call_anchor.start_byte > input.call_anchor.end_byte {
        state.limit(CallResultMaterializationLimitation::InvalidCallAnchor);
    }

    state.materialize_relation(&fact.relation);
    state.finish()
}

struct MaterializationState<'a> {
    fact: &'a CallableResultFact,
    input: &'a CallResultMaterializationInput,
    budget: CallResultMaterializationBudget,
    values: Vec<ValueShape>,
    optional: bool,
    bare_return: bool,
    limitations: BTreeSet<CallResultMaterializationLimitation>,
    input_anchors: BTreeSet<SourceAnchor>,
    argument_entities: BTreeSet<EntityId>,
    work_units: usize,
}

impl<'a> MaterializationState<'a> {
    fn new(
        fact: &'a CallableResultFact,
        input: &'a CallResultMaterializationInput,
        budget: CallResultMaterializationBudget,
    ) -> Self {
        Self {
            fact,
            input,
            budget,
            values: Vec::new(),
            optional: false,
            bare_return: false,
            limitations: BTreeSet::new(),
            input_anchors: BTreeSet::new(),
            argument_entities: BTreeSet::new(),
            work_units: 0,
        }
    }

    fn materialize_relation(&mut self, relation: &CallableResultRelation) {
        if !self.reserve_relation_node() {
            return;
        }

        match relation {
            CallableResultRelation::Concrete(value) => self.push_value(value.clone()),
            CallableResultRelation::ReceiverSelf => {
                let Some(receiver) = &self.input.receiver else {
                    self.limit(CallResultMaterializationLimitation::MissingReceiver);
                    return;
                };
                self.consume_bound_value(receiver);
            }
            CallableResultRelation::Argument { parameter_entity_id, position } => {
                self.materialize_argument(*parameter_entity_id, *position)
            }
            CallableResultRelation::BareReturn => {
                self.bare_return = true;
                if !matches!(self.input.value_context, CallValueContext::Void) {
                    self.limit(CallResultMaterializationLimitation::BareReturnContext);
                }
            }
            CallableResultRelation::Optional(inner) => {
                self.optional = true;
                self.materialize_relation(inner);
            }
            CallableResultRelation::FiniteUnion(relations) => {
                for relation in relations {
                    if self
                        .limitations
                        .contains(&CallResultMaterializationLimitation::BudgetExhausted)
                    {
                        break;
                    }
                    self.materialize_relation(relation);
                }
            }
            CallableResultRelation::Unknown => {
                self.limit(CallResultMaterializationLimitation::UnknownRelation);
            }
            _ => self.limit(CallResultMaterializationLimitation::UnknownRelation),
        }
    }

    fn materialize_argument(&mut self, parameter_entity_id: EntityId, position: u16) {
        let matches = self
            .input
            .arguments
            .iter()
            .filter(|argument| {
                argument.parameter_entity_id == parameter_entity_id && argument.position == position
            })
            .collect::<Vec<_>>();

        if matches.is_empty() {
            self.limit(CallResultMaterializationLimitation::MissingArgument);
            return;
        }

        self.argument_entities.insert(parameter_entity_id);
        let mut distinct_values = Vec::<ValueShape>::new();
        let mut all_current = true;
        for argument in matches {
            if !bound_value_is_current(&argument.bound) {
                all_current = false;
            }
            if let Some(anchor) = argument.bound.source_anchor {
                self.input_anchors.insert(anchor);
            }
            if !distinct_values.contains(&argument.bound.value) {
                distinct_values.push(argument.bound.value.clone());
            }
        }

        if !all_current {
            self.limit(CallResultMaterializationLimitation::InputNotCurrent);
        }
        if distinct_values.len() > 1 {
            self.limit(CallResultMaterializationLimitation::AmbiguousArgument);
        }
        for value in distinct_values {
            self.push_value(value);
        }
    }

    fn consume_bound_value(&mut self, bound: &CallBoundValue) {
        if !bound_value_is_current(bound) {
            self.limit(CallResultMaterializationLimitation::InputNotCurrent);
        }
        if let Some(anchor) = bound.source_anchor {
            self.input_anchors.insert(anchor);
        }
        self.push_value(bound.value.clone());
    }

    fn push_value(&mut self, value: ValueShape) {
        if is_unknown_shape(&value) {
            self.limit(CallResultMaterializationLimitation::UnknownValue);
        }
        if self.values.contains(&value) {
            return;
        }
        if self.values.len() >= self.budget.max_values {
            self.limit(CallResultMaterializationLimitation::BudgetExhausted);
            return;
        }
        self.values.push(value);
    }

    fn reserve_relation_node(&mut self) -> bool {
        if self.work_units >= self.budget.max_relation_nodes {
            self.limit(CallResultMaterializationLimitation::BudgetExhausted);
            return false;
        }
        self.work_units = self.work_units.saturating_add(1);
        true
    }

    fn limit(&mut self, limitation: CallResultMaterializationLimitation) {
        self.limitations.insert(limitation);
    }

    fn finish(mut self) -> MaterializedCallResult {
        self.values.sort_by_key(value_shape_sort_key);
        self.values.dedup();
        MaterializedCallResult {
            call_entity_id: self.input.call_entity_id,
            callable_entity_id: self.input.callable_entity_id,
            relation_fact_id: self.fact.envelope.fact_id,
            call_anchor: self.input.call_anchor,
            call_generation: self.input.call_generation.clone(),
            possible_values: self.values,
            optional: self.optional,
            bare_return: self.bare_return,
            value_context: self.input.value_context,
            completeness: if self.limitations.is_empty() {
                CallResultMaterializationCompleteness::Complete
            } else {
                CallResultMaterializationCompleteness::Partial
            },
            limitations: self.limitations.into_iter().collect(),
            input_anchors: self.input_anchors.into_iter().collect(),
            argument_entities: self.argument_entities.into_iter().collect(),
            work_units: self.work_units,
        }
    }
}

fn bound_value_is_current(bound: &CallBoundValue) -> bool {
    bound.generation.is_known() && matches!(bound.freshness, SemanticFreshness::Fresh)
}

fn is_unknown_shape(shape: &ValueShape) -> bool {
    matches!(shape, ValueShape::Unknown)
}

fn value_shape_sort_key(shape: &ValueShape) -> String {
    match shape {
        ValueShape::Unknown => "9:unknown".to_string(),
        ValueShape::Scalar => "0:scalar".to_string(),
        ValueShape::ArrayRef => "1:array-ref".to_string(),
        ValueShape::HashRef => "2:hash-ref".to_string(),
        ValueShape::CodeRef => "3:code-ref".to_string(),
        ValueShape::PackageName { package } => format!("4:package:{package}"),
        ValueShape::Object { package, confidence } => format!("5:object:{package}:{confidence:?}"),
        _ => "8:future-shape".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use perl_semantic_facts::{
        AnchorId, CallableResultCompleteness, Confidence, FileId, InvalidationDependency,
        LifecyclePhase, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
        SemanticProducer, SemanticProvenance, SemanticReasonCode,
    };

    use super::*;

    fn exact_fact(relation: CallableResultRelation) -> CallableResultFact {
        CallableResultFact::new(
            SemanticFactEnvelope::new(
                FactId(1),
                Some(EntityId(2)),
                SemanticFactKind::CallableResult,
                SourceAnchor::new(Some(AnchorId(3)), FileId(4), 10, 20),
                SourceGeneration::known("callee-generation"),
                None,
                Some("Example".to_string()),
                LifecyclePhase::Runtime,
                SemanticProducer::SemanticAnalyzer,
                SemanticProvenance::Known(perl_semantic_facts::Provenance::ExactAst),
                SemanticConfidence::Known(Confidence::High),
                SemanticFreshness::Fresh,
                None,
                Vec::<InvalidationDependency>::new(),
                SemanticReasonCode::ExactSource,
            ),
            relation,
            vec![SourceAnchor::new(Some(AnchorId(5)), FileId(4), 30, 40)],
            CallableResultCompleteness::Complete,
            Vec::new(),
        )
    }

    fn input() -> CallResultMaterializationInput {
        CallResultMaterializationInput {
            call_entity_id: EntityId(10),
            callable_entity_id: EntityId(2),
            call_anchor: SourceAnchor::new(Some(AnchorId(11)), FileId(12), 50, 60),
            call_generation: SourceGeneration::known("call-generation"),
            call_freshness: SemanticFreshness::Fresh,
            receiver: None,
            arguments: Vec::new(),
            value_context: CallValueContext::Scalar,
        }
    }

    fn object(package: &str) -> ValueShape {
        ValueShape::Object { package: package.to_string(), confidence: Confidence::High }
    }

    #[test]
    fn concrete_result_materializes_exactly() {
        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Concrete(object("App::Client"))),
            &input(),
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Exact);
        assert_eq!(result.possible_values, vec![object("App::Client")]);
        assert!(result.limitations.is_empty());
    }

    #[test]
    fn receiver_self_uses_the_current_receiver() {
        let mut call = input();
        let receiver_anchor = SourceAnchor::new(Some(AnchorId(13)), FileId(12), 20, 30);
        call.receiver = Some(CallBoundValue::current(
            object("App::Builder"),
            Some(receiver_anchor),
            SourceGeneration::known("receiver-generation"),
        ));

        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::ReceiverSelf),
            &call,
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Exact);
        assert_eq!(result.possible_values, vec![object("App::Builder")]);
        assert_eq!(result.input_anchors, vec![receiver_anchor]);
    }

    #[test]
    fn argument_relation_selects_canonical_parameter_identity() {
        let mut call = input();
        call.arguments = vec![
            CallArgumentValue {
                parameter_entity_id: EntityId(100),
                position: 0,
                bound: CallBoundValue::current(
                    object("Wrong::Value"),
                    None,
                    SourceGeneration::known("argument-generation"),
                ),
            },
            CallArgumentValue {
                parameter_entity_id: EntityId(101),
                position: 0,
                bound: CallBoundValue::current(
                    object("Right::Value"),
                    None,
                    SourceGeneration::known("argument-generation"),
                ),
            },
        ];

        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Argument {
                parameter_entity_id: EntityId(101),
                position: 0,
            }),
            &call,
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Exact);
        assert_eq!(result.possible_values, vec![object("Right::Value")]);
        assert_eq!(result.argument_entities, vec![EntityId(101)]);
    }

    #[test]
    fn optional_relation_preserves_optionality() {
        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Optional(Box::new(
                CallableResultRelation::Concrete(object("App::Row")),
            ))),
            &input(),
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Exact);
        assert!(result.optional);
        assert_eq!(result.possible_values, vec![object("App::Row")]);
    }

    #[test]
    fn finite_union_is_deterministic_and_deduplicated() {
        let relation = CallableResultRelation::finite_union(vec![
            CallableResultRelation::Concrete(object("App::B")),
            CallableResultRelation::Concrete(object("App::A")),
            CallableResultRelation::Concrete(object("App::B")),
        ]);
        let result = materialize_call_result(
            &exact_fact(relation),
            &input(),
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Exact);
        assert_eq!(result.possible_values, vec![object("App::A"), object("App::B")]);
    }

    #[test]
    fn missing_receiver_is_partial() {
        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::ReceiverSelf),
            &input(),
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert!(result.limitations.contains(&CallResultMaterializationLimitation::MissingReceiver));
    }

    #[test]
    fn conflicting_argument_values_are_partial_without_dropping_evidence() {
        let mut call = input();
        call.arguments = vec![
            CallArgumentValue {
                parameter_entity_id: EntityId(101),
                position: 0,
                bound: CallBoundValue::current(
                    object("App::A"),
                    None,
                    SourceGeneration::known("argument-generation"),
                ),
            },
            CallArgumentValue {
                parameter_entity_id: EntityId(101),
                position: 0,
                bound: CallBoundValue::current(
                    object("App::B"),
                    None,
                    SourceGeneration::known("argument-generation"),
                ),
            },
        ];

        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Argument {
                parameter_entity_id: EntityId(101),
                position: 0,
            }),
            &call,
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert_eq!(result.possible_values, vec![object("App::A"), object("App::B")]);
        assert!(
            result.limitations.contains(&CallResultMaterializationLimitation::AmbiguousArgument)
        );
    }

    #[test]
    fn unknown_relation_and_unknown_value_are_partial() {
        let unknown_relation = materialize_call_result(
            &exact_fact(CallableResultRelation::Unknown),
            &input(),
            CallResultMaterializationBudget::default(),
        );
        assert!(
            unknown_relation
                .limitations
                .contains(&CallResultMaterializationLimitation::UnknownRelation)
        );

        let unknown_value = materialize_call_result(
            &exact_fact(CallableResultRelation::Concrete(ValueShape::Unknown)),
            &input(),
            CallResultMaterializationBudget::default(),
        );
        assert!(
            unknown_value.limitations.contains(&CallResultMaterializationLimitation::UnknownValue)
        );
    }

    #[test]
    fn bare_return_is_exact_only_for_void_context() {
        let scalar = materialize_call_result(
            &exact_fact(CallableResultRelation::BareReturn),
            &input(),
            CallResultMaterializationBudget::default(),
        );
        assert_eq!(scalar.status(), SemanticFactStatus::Degraded);
        assert!(scalar.bare_return);

        let mut void_input = input();
        void_input.value_context = CallValueContext::Void;
        let void = materialize_call_result(
            &exact_fact(CallableResultRelation::BareReturn),
            &void_input,
            CallResultMaterializationBudget::default(),
        );
        assert_eq!(void.status(), SemanticFactStatus::Exact);
        assert!(void.bare_return);
        assert!(void.possible_values.is_empty());
    }

    #[test]
    fn stale_bound_input_is_partial() {
        let mut call = input();
        call.receiver = Some(CallBoundValue {
            value: object("App::Builder"),
            source_anchor: None,
            generation: SourceGeneration::known("receiver-generation"),
            freshness: SemanticFreshness::Stale,
        });

        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::ReceiverSelf),
            &call,
            CallResultMaterializationBudget::default(),
        );
        assert!(result.limitations.contains(&CallResultMaterializationLimitation::InputNotCurrent));
    }

    #[test]
    fn value_budget_widens_without_claiming_complete() {
        let relation = CallableResultRelation::finite_union(vec![
            CallableResultRelation::Concrete(object("App::A")),
            CallableResultRelation::Concrete(object("App::B")),
        ]);
        let result = materialize_call_result(
            &exact_fact(relation),
            &input(),
            CallResultMaterializationBudget { max_relation_nodes: 8, max_values: 1 },
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert_eq!(result.possible_values.len(), 1);
        assert!(result.limitations.contains(&CallResultMaterializationLimitation::BudgetExhausted));
    }

    #[test]
    fn fact_for_another_callable_is_not_exact() {
        let mut call = input();
        call.callable_entity_id = EntityId(999);

        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Concrete(object("App::Client"))),
            &call,
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert!(
            result
                .limitations
                .contains(&CallResultMaterializationLimitation::CallableIdentityMismatch)
        );
    }

    #[test]
    fn fact_without_callable_identity_is_not_exact() {
        let mut fact = exact_fact(CallableResultRelation::Concrete(object("App::Client")));
        fact.envelope.entity_id = None;

        let result =
            materialize_call_result(&fact, &input(), CallResultMaterializationBudget::default());

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert!(
            result
                .limitations
                .contains(&CallResultMaterializationLimitation::CallableIdentityMismatch)
        );
    }

    #[test]
    fn missing_argument_binding_is_partial() {
        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Argument {
                parameter_entity_id: EntityId(101),
                position: 0,
            }),
            &input(),
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert!(result.limitations.contains(&CallResultMaterializationLimitation::MissingArgument));
    }

    #[test]
    fn non_exact_relation_fact_is_partial() {
        let mut fact = exact_fact(CallableResultRelation::Concrete(object("App::Client")));
        fact.envelope.freshness = SemanticFreshness::Stale;

        let result =
            materialize_call_result(&fact, &input(), CallResultMaterializationBudget::default());

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert!(result.limitations.contains(
            &CallResultMaterializationLimitation::RelationNotExact(SemanticFactStatus::Stale)
        ));
    }

    #[test]
    fn stale_call_identity_is_partial() {
        let mut call = input();
        call.call_freshness = SemanticFreshness::Stale;

        let result = materialize_call_result(
            &exact_fact(CallableResultRelation::Concrete(object("App::Client"))),
            &call,
            CallResultMaterializationBudget::default(),
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert!(result.limitations.contains(&CallResultMaterializationLimitation::CallNotCurrent));
    }

    #[test]
    fn relation_node_budget_stops_union_traversal() {
        let relation = CallableResultRelation::finite_union(vec![
            CallableResultRelation::Concrete(object("App::A")),
            CallableResultRelation::Concrete(object("App::B")),
            CallableResultRelation::Concrete(object("App::C")),
        ]);
        let result = materialize_call_result(
            &exact_fact(relation),
            &input(),
            CallResultMaterializationBudget { max_relation_nodes: 1, max_values: 16 },
        );

        assert_eq!(result.status(), SemanticFactStatus::Degraded);
        assert_eq!(result.work_units, 1);
        assert!(result.limitations.contains(&CallResultMaterializationLimitation::BudgetExhausted));
    }
}
