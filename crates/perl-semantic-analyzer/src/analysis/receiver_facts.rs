//! Receiver facts for trust-bounded method completion.
//!
//! This module classifies method-call receivers such as `$self->method`,
//! `Class->new`, `$hash{slot}->method`, and `$array[0]->method` without
//! changing completion behavior.  It converts existing rich [`TypeFact`] values
//! into a receiver-shaped fact that completion can later use for source-backed
//! ranking receipts.

use super::type_facts::{DynamicBoundary, ShapeFact, TypeEvidence, TypeFact};
use super::type_inference::{PerlType, TypeEnvironment};
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::Confidence;

/// Context used while extracting a receiver fact.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct ReceiverFactContext<'a> {
    /// Rich type environment available at the call site.
    pub type_environment: Option<&'a TypeEnvironment>,
    /// Source text for distinguishing syntax that the AST intentionally erases,
    /// such as `$hash{key}` versus `$hashref->{key}`.
    pub source: Option<&'a str>,
}

impl<'a> ReceiverFactContext<'a> {
    /// Creates a receiver-fact context from an optional type environment.
    pub fn new(type_environment: Option<&'a TypeEnvironment>) -> Self {
        Self { type_environment, source: None }
    }

    /// Adds source text to the receiver-fact context.
    pub fn with_source(mut self, source: &'a str) -> Self {
        self.source = Some(source);
        self
    }
}

/// Receiver shape recognized for a method call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiverKind {
    /// A `$self` or `$this` receiver.
    SelfReceiver,
    /// A scalar object receiver such as `$object`.
    ObjectVariable,
    /// A static package receiver such as `Class->new`.
    StaticPackage,
    /// A hash slot receiver such as `$hash{key}`.
    HashSlot,
    /// A hash-reference slot receiver such as `$hashref->{key}`.
    HashRefSlot,
    /// An array index receiver such as `$array[0]`.
    ArrayIndex,
    /// A receiver with a runtime-computed slot key.
    DynamicKey,
    /// A receiver that cannot be classified statically.
    Unknown,
}

/// Freshness of the evidence used for a receiver fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiverFactFreshness {
    /// The receiver fact came from the current AST or supplied type environment.
    Fresh,
    /// No fresh source-backed fact was available.
    Unknown,
}

/// Fallback posture completion must preserve for this receiver fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiverFallbackState {
    /// The receiver fact is exact enough to drive receiver-scoped completion.
    Exact,
    /// The receiver fact is not exact; completion must preserve legacy fallback.
    Fallback,
    /// The receiver fact cannot participate in completion.
    Blocked,
}

/// Trust-bounded evidence about a method-call receiver.
///
/// `package` holds the primary (first) candidate for backward compatibility.
/// `candidate_packages` holds every distinct package from the receiver's inferred
/// type, including all union branches.  For an exact single-package receiver the
/// two fields agree; for a union receiver `candidate_packages` carries the full
/// set so consumers can rank or validate against all possibilities without losing
/// information.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ReceiverFact {
    /// Classified receiver kind.
    pub kind: ReceiverKind,
    /// Primary inferred package for method ranking, when available.
    ///
    /// For union-typed receivers this is the first candidate.  Consumers that
    /// need the full set should use [`candidate_packages`](Self::candidate_packages).
    pub package: Option<String>,
    /// All package candidates derived from the receiver's inferred type.
    ///
    /// - Empty when no package can be inferred (unknown / dynamic / shape-only receivers).
    /// - One element for an exact single-package receiver.
    /// - Multiple elements when the receiver has a union type with more than one
    ///   distinct object package (e.g. `My::Foo | My::Bar`).
    ///
    /// The order matches the union's declaration order and is deterministic.
    /// Duplicates are removed; the first occurrence is kept.
    pub candidate_packages: Vec<String>,
    /// Structural shape fact associated with the receiver, when available.
    pub shape: Option<ShapeFact>,
    /// Confidence assigned to this receiver fact.
    pub confidence: Confidence,
    /// Evidence used to classify the receiver.
    pub evidence: Vec<TypeEvidence>,
    /// Freshness of the receiver evidence.
    pub freshness: ReceiverFactFreshness,
    /// Dynamic boundary that prevents precise method completion, when present.
    pub dynamic_boundary: Option<DynamicBoundary>,
    /// Source-backed byte range for the receiver expression.
    pub source_range: Option<(usize, usize)>,
    /// Fallback posture completion must preserve for this receiver.
    pub fallback_state: ReceiverFallbackState,
}

impl ReceiverFact {
    fn unknown(receiver: &Node, reason: impl Into<String>) -> Self {
        Self {
            kind: ReceiverKind::Unknown,
            package: None,
            candidate_packages: vec![],
            shape: None,
            confidence: Confidence::Low,
            evidence: vec![TypeEvidence::Heuristic { reason: reason.into() }],
            freshness: ReceiverFactFreshness::Unknown,
            dynamic_boundary: Some(DynamicBoundary::UnknownReceiver),
            source_range: Some((receiver.location.start, receiver.location.end)),
            fallback_state: ReceiverFallbackState::Fallback,
        }
    }

    fn dynamic_key(receiver: &Node, evidence: TypeEvidence) -> Self {
        Self {
            kind: ReceiverKind::DynamicKey,
            package: None,
            candidate_packages: vec![],
            shape: None,
            confidence: Confidence::Low,
            evidence: vec![evidence],
            freshness: ReceiverFactFreshness::Unknown,
            dynamic_boundary: Some(DynamicBoundary::DynamicHashKey),
            source_range: Some((receiver.location.start, receiver.location.end)),
            fallback_state: ReceiverFallbackState::Fallback,
        }
    }

    fn from_type_fact(kind: ReceiverKind, fact: TypeFact, receiver: &Node) -> Self {
        let package = package_from_type_fact(&fact);
        let candidate_packages = all_packages_from_type_fact(&fact);
        let fallback_state = fallback_state_for_fact(package.as_deref(), &fact);
        Self {
            kind,
            package,
            candidate_packages,
            shape: fact.shape,
            confidence: fact.confidence,
            evidence: fact.evidence,
            freshness: ReceiverFactFreshness::Fresh,
            dynamic_boundary: fact.dynamic_boundary,
            source_range: Some((receiver.location.start, receiver.location.end)),
            fallback_state,
        }
    }

    /// Returns `true` when the receiver has more than one distinct candidate package.
    ///
    /// A multi-candidate receiver corresponds to a union-typed variable such as
    /// `my $obj : Foo | Bar`.  Completion and diagnostics should apply fallback
    /// or union-aware logic rather than assuming a single exact package.
    pub fn is_union_receiver(&self) -> bool {
        self.candidate_packages.len() > 1
    }
}

/// Extracts a receiver fact from a method-call node.
pub fn receiver_fact_for_method_call(
    call: &Node,
    context: ReceiverFactContext<'_>,
) -> ReceiverFact {
    let NodeKind::MethodCall { object, method, .. } = &call.kind else {
        return ReceiverFact::unknown(call, "node is not a method call");
    };

    infer_receiver_fact(object, Some(method.as_str()), context)
}

/// Extracts a receiver fact from an expression used as a method-call receiver.
pub fn infer_receiver_fact(
    receiver: &Node,
    method_name: Option<&str>,
    context: ReceiverFactContext<'_>,
) -> ReceiverFact {
    match &receiver.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => {
            variable_receiver_fact(receiver, name, context)
        }
        NodeKind::VariableWithAttributes { variable, .. } => {
            infer_receiver_fact(variable, method_name, context)
        }
        NodeKind::Identifier { name } => static_package_receiver(receiver, name, method_name),
        NodeKind::String { value, .. } => {
            let normalized = normalize_package_string(value);
            match normalized {
                Some(package) => static_package_receiver(receiver, &package, method_name),
                None => ReceiverFact::unknown(receiver, "empty package string receiver"),
            }
        }
        NodeKind::Binary { op, left, right } if op == "{}" || op == "->{}" => {
            hash_receiver_fact(receiver, left, right, context)
        }
        NodeKind::Binary { op, left, right } if op == "[]" || op == "->[]" => {
            array_receiver_fact(receiver, left, right, context)
        }
        NodeKind::ArraySlice { target, indices } => {
            array_receiver_fact(receiver, target, indices, context)
        }
        NodeKind::HashSlice { target, keys } | NodeKind::KeyValueSlice { target, keys } => {
            hash_receiver_fact(receiver, target, keys, context)
        }
        NodeKind::MethodCall { .. } => ReceiverFact::unknown(
            receiver,
            "receiver is itself a method call and requires completion-chain evidence",
        ),
        _ => ReceiverFact::unknown(receiver, "receiver expression has no source-backed fact"),
    }
}

fn variable_receiver_fact(
    receiver: &Node,
    name: &str,
    context: ReceiverFactContext<'_>,
) -> ReceiverFact {
    let kind = if is_self_like_name(name) {
        ReceiverKind::SelfReceiver
    } else {
        ReceiverKind::ObjectVariable
    };

    if let Some(fact) = context.type_environment.and_then(|env| env.get_fact_at(name)) {
        return ReceiverFact::from_type_fact(kind, fact, receiver);
    }

    if is_self_like_name(name) {
        return ReceiverFact {
            kind,
            package: None,
            candidate_packages: vec![],
            shape: None,
            confidence: Confidence::Medium,
            evidence: vec![TypeEvidence::Heuristic {
                reason: "self-like receiver without package fact".to_string(),
            }],
            freshness: ReceiverFactFreshness::Unknown,
            dynamic_boundary: None,
            source_range: Some((receiver.location.start, receiver.location.end)),
            fallback_state: ReceiverFallbackState::Fallback,
        };
    }

    ReceiverFact::unknown(receiver, "object variable has no type fact")
}

fn static_package_receiver(
    receiver: &Node,
    package: &str,
    method_name: Option<&str>,
) -> ReceiverFact {
    let evidence = if method_name == Some("new") {
        TypeEvidence::ConstructorCall { package: package.to_string() }
    } else {
        TypeEvidence::Heuristic { reason: "static package receiver".to_string() }
    };

    ReceiverFact {
        kind: ReceiverKind::StaticPackage,
        package: Some(package.to_string()),
        candidate_packages: vec![package.to_string()],
        shape: None,
        confidence: Confidence::High,
        evidence: vec![evidence],
        freshness: ReceiverFactFreshness::Fresh,
        dynamic_boundary: None,
        source_range: Some((receiver.location.start, receiver.location.end)),
        fallback_state: ReceiverFallbackState::Exact,
    }
}

fn hash_receiver_fact(
    receiver: &Node,
    left: &Node,
    right: &Node,
    context: ReceiverFactContext<'_>,
) -> ReceiverFact {
    let Some(key) = static_slot_key(right) else {
        return ReceiverFact::dynamic_key(
            receiver,
            TypeEvidence::Heuristic { reason: "hash receiver key is dynamic".to_string() },
        );
    };

    let base = receiver_base_label(left);
    let kind = if matches!(&receiver.kind, NodeKind::Binary { op, .. } if op == "->{}") {
        ReceiverKind::HashRefSlot
    } else {
        ReceiverKind::HashSlot
    };
    let evidence = match kind {
        ReceiverKind::HashRefSlot => TypeEvidence::HashRefSlot { base: base.clone(), key },
        _ => TypeEvidence::HashSlot { hash: base.clone(), key },
    };

    let Some(container_fact) = receiver_container_fact(left, context) else {
        return ReceiverFact {
            kind,
            package: None,
            candidate_packages: vec![],
            shape: None,
            confidence: Confidence::Low,
            evidence: vec![evidence],
            freshness: ReceiverFactFreshness::Unknown,
            dynamic_boundary: None,
            source_range: Some((receiver.location.start, receiver.location.end)),
            fallback_state: ReceiverFallbackState::Fallback,
        };
    };

    if let Some(slot_fact) = hash_slot_type_fact(&container_fact, &evidence) {
        return ReceiverFact::from_type_fact(
            kind,
            with_access_evidence(slot_fact, &container_fact, evidence),
            receiver,
        );
    }

    fallback_receiver_fact(
        kind,
        receiver,
        container_fact,
        evidence,
        ReceiverFactFreshness::Fresh,
        None,
    )
}

fn array_receiver_fact(
    receiver: &Node,
    left: &Node,
    right: &Node,
    context: ReceiverFactContext<'_>,
) -> ReceiverFact {
    let evidence = TypeEvidence::Heuristic { reason: "array index receiver".to_string() };
    let Some(container_fact) = receiver_container_fact(left, context) else {
        return ReceiverFact {
            kind: ReceiverKind::ArrayIndex,
            package: None,
            candidate_packages: vec![],
            shape: None,
            confidence: Confidence::Low,
            evidence: vec![evidence],
            freshness: ReceiverFactFreshness::Unknown,
            dynamic_boundary: None,
            source_range: Some((receiver.location.start, receiver.location.end)),
            fallback_state: ReceiverFallbackState::Fallback,
        };
    };

    let Some(index) = static_array_index(right) else {
        return fallback_receiver_fact(
            ReceiverKind::ArrayIndex,
            receiver,
            container_fact,
            evidence,
            ReceiverFactFreshness::Unknown,
            Some(DynamicBoundary::UnknownReceiver),
        );
    };

    if let Some(index_fact) = array_index_type_fact(&container_fact, index) {
        return ReceiverFact::from_type_fact(
            ReceiverKind::ArrayIndex,
            with_access_evidence(index_fact, &container_fact, evidence),
            receiver,
        );
    }

    fallback_receiver_fact(
        ReceiverKind::ArrayIndex,
        receiver,
        container_fact,
        evidence,
        ReceiverFactFreshness::Fresh,
        None,
    )
}

fn receiver_container_fact(left: &Node, context: ReceiverFactContext<'_>) -> Option<TypeFact> {
    if let Some((_, name)) = variable_identity(left) {
        return context.type_environment.and_then(|env| env.get_fact_at(name));
    }

    match &left.kind {
        NodeKind::Binary { op, left: container, right } if op == "{}" || op == "->{}" => {
            let container_fact = receiver_container_fact(container, context)?;
            let Some(key) = static_slot_key(right) else {
                return Some(with_access_evidence(
                    TypeFact::dynamic(DynamicBoundary::DynamicHashKey),
                    &container_fact,
                    TypeEvidence::Heuristic { reason: "hash receiver key is dynamic".to_string() },
                ));
            };

            let base = receiver_base_label(container);
            let is_hashref = op == "->{}";
            let evidence = if is_hashref {
                TypeEvidence::HashRefSlot { base, key }
            } else {
                TypeEvidence::HashSlot { hash: base, key }
            };

            Some(hash_slot_type_fact(&container_fact, &evidence).map_or_else(
                || degraded_container_fact(&container_fact, evidence.clone()),
                |fact| with_access_evidence(fact, &container_fact, evidence.clone()),
            ))
        }
        NodeKind::Binary { op, left: container, right } if op == "[]" || op == "->[]" => {
            let container_fact = receiver_container_fact(container, context)?;
            let evidence = TypeEvidence::Heuristic { reason: "array index receiver".to_string() };
            let Some(index) = static_array_index(right) else {
                return Some(with_access_evidence(
                    TypeFact::dynamic(DynamicBoundary::UnknownReceiver),
                    &container_fact,
                    evidence,
                ));
            };

            Some(array_index_type_fact(&container_fact, index).map_or_else(
                || degraded_container_fact(&container_fact, evidence.clone()),
                |fact| with_access_evidence(fact, &container_fact, evidence.clone()),
            ))
        }
        _ => None,
    }
}

fn hash_slot_type_fact(container_fact: &TypeFact, evidence: &TypeEvidence) -> Option<TypeFact> {
    let key = match evidence {
        TypeEvidence::HashSlot { key, .. } | TypeEvidence::HashRefSlot { key, .. } => key,
        _ => return None,
    };
    match &container_fact.shape {
        Some(ShapeFact::Hash(shape)) => shape
            .slots
            .get(key)
            .cloned()
            .or_else(|| shape.fallback_value.as_ref().map(|fact| fact.as_ref().clone())),
        Some(ShapeFact::Object(shape)) => shape.fields.get(key).cloned(),
        _ => None,
    }
}

fn array_index_type_fact(container_fact: &TypeFact, index: usize) -> Option<TypeFact> {
    match &container_fact.shape {
        Some(ShapeFact::Array(shape)) => shape
            .indexed
            .get(&index)
            .cloned()
            .or_else(|| shape.element.as_ref().map(|fact| fact.as_ref().clone())),
        _ => None,
    }
}

fn with_access_evidence(
    mut fact: TypeFact,
    container_fact: &TypeFact,
    evidence: TypeEvidence,
) -> TypeFact {
    fact.confidence = fact.confidence.max(container_fact.confidence);
    if fact.dynamic_boundary.is_none() {
        fact.dynamic_boundary = container_fact.dynamic_boundary.clone();
    }
    for inherited in &container_fact.evidence {
        if !fact.evidence.contains(inherited) {
            fact.evidence.push(inherited.clone());
        }
    }
    if !fact.evidence.contains(&evidence) {
        fact.evidence.push(evidence);
    }
    fact
}

fn degraded_container_fact(container_fact: &TypeFact, evidence: TypeEvidence) -> TypeFact {
    let mut fact = TypeFact::unknown();
    fact.confidence = fact.confidence.max(container_fact.confidence);
    fact.dynamic_boundary = container_fact.dynamic_boundary.clone();
    for inherited in &container_fact.evidence {
        if !fact.evidence.contains(inherited) {
            fact.evidence.push(inherited.clone());
        }
    }
    if !fact.evidence.contains(&evidence) {
        fact.evidence.push(evidence);
    }
    fact
}

fn fallback_receiver_fact(
    kind: ReceiverKind,
    receiver: &Node,
    container_fact: TypeFact,
    evidence: TypeEvidence,
    freshness: ReceiverFactFreshness,
    boundary: Option<DynamicBoundary>,
) -> ReceiverFact {
    let mut inherited_evidence = container_fact.evidence.clone();
    if !inherited_evidence.contains(&evidence) {
        inherited_evidence.push(evidence);
    }
    ReceiverFact {
        kind,
        package: None,
        candidate_packages: vec![],
        shape: container_fact.shape,
        confidence: Confidence::Low.max(container_fact.confidence),
        evidence: inherited_evidence,
        freshness,
        dynamic_boundary: boundary.or(container_fact.dynamic_boundary),
        source_range: Some((receiver.location.start, receiver.location.end)),
        fallback_state: ReceiverFallbackState::Fallback,
    }
}

fn fallback_state_for_fact(package: Option<&str>, fact: &TypeFact) -> ReceiverFallbackState {
    if package.is_some_and(|package| type_fact_has_exact_package(fact, package))
        && fact.confidence == Confidence::High
        && fact.dynamic_boundary.is_none()
    {
        ReceiverFallbackState::Exact
    } else {
        ReceiverFallbackState::Fallback
    }
}

fn type_fact_has_exact_package(fact: &TypeFact, package: &str) -> bool {
    if type_has_exact_package(&fact.ty, package) {
        return true;
    }

    matches!(
        (&fact.ty, &fact.shape),
        (PerlType::Any, Some(ShapeFact::Object(shape))) if shape.package == package
    )
}

fn type_has_exact_package(ty: &PerlType, package: &str) -> bool {
    match ty {
        PerlType::Object(candidate) => candidate == package,
        PerlType::Reference(inner) => type_has_exact_package(inner, package),
        PerlType::Union(types) => {
            !types.is_empty() && types.iter().all(|ty| type_has_exact_package(ty, package))
        }
        _ => false,
    }
}

fn variable_identity(node: &Node) -> Option<(&str, &str)> {
    match &node.kind {
        NodeKind::Variable { sigil, name } => Some((sigil.as_str(), name.as_str())),
        NodeKind::VariableWithAttributes { variable, .. } => variable_identity(variable),
        _ => None,
    }
}

fn receiver_base_label(node: &Node) -> String {
    match variable_identity(node) {
        Some((sigil, name)) => format!("{sigil}{name}"),
        None => node.kind.kind_name().to_string(),
    }
}

fn static_slot_key(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::String { value, .. } => Some(normalize_literal(value)),
        NodeKind::Identifier { name } => Some(name.clone()),
        NodeKind::Number { value } => Some(value.clone()),
        _ => None,
    }
}

fn static_array_index(node: &Node) -> Option<usize> {
    match &node.kind {
        NodeKind::Number { value } => value.parse().ok(),
        _ => None,
    }
}

fn package_from_type_fact(fact: &TypeFact) -> Option<String> {
    package_from_type(&fact.ty).or_else(|| match &fact.shape {
        Some(ShapeFact::Object(shape)) => Some(shape.package.clone()),
        _ => None,
    })
}

fn package_from_type(ty: &PerlType) -> Option<String> {
    match ty {
        PerlType::Object(package) => Some(package.clone()),
        PerlType::Reference(inner) => package_from_type(inner),
        PerlType::Union(types) => types.iter().find_map(package_from_type),
        _ => None,
    }
}

/// Collects every distinct package name reachable from a type fact.
///
/// For a plain `Object(Foo)` this returns `["Foo"]`.  For a
/// `Union(Object(Foo), Object(Bar), Scalar(Int))` returns an empty vec because
/// a mixed union cannot safely dispatch object methods.
///
/// Duplicates are removed; the first occurrence wins.  When the type fact
/// carries no package (e.g. `Any`, `Scalar`, or a pure shape fact without an
/// object shape), the returned vec is empty.
fn all_packages_from_type_fact(fact: &TypeFact) -> Vec<String> {
    let mut packages: Vec<String> = all_packages_from_type(&fact.ty);

    // If no package was found from the type itself, check the shape field.
    if packages.is_empty()
        && !contains_union(&fact.ty)
        && let Some(ShapeFact::Object(shape)) = &fact.shape
    {
        packages.push(shape.package.clone());
    }

    packages
}

/// Recursively collects all distinct package names from a Perl type.
fn all_packages_from_type(ty: &PerlType) -> Vec<String> {
    let mut packages: Vec<String> = vec![];
    if !collect_packages_from_type(ty, &mut packages) {
        packages.clear();
    }
    packages
}

fn collect_packages_from_type(ty: &PerlType, packages: &mut Vec<String>) -> bool {
    match ty {
        PerlType::Object(package) => {
            if !packages.contains(package) {
                packages.push(package.clone());
            }
            true
        }
        PerlType::Reference(inner) => collect_packages_from_type(inner, packages),
        PerlType::Union(types) => {
            !types.is_empty()
                && types.iter().all(|inner| collect_packages_from_type(inner, packages))
        }
        _ => false,
    }
}

fn contains_union(ty: &PerlType) -> bool {
    match ty {
        PerlType::Reference(inner) => contains_union(inner),
        PerlType::Union(_) => true,
        _ => false,
    }
}

fn normalize_package_string(value: &str) -> Option<String> {
    let normalized = normalize_literal(value);
    if normalized.is_empty() { None } else { Some(normalized) }
}

fn normalize_literal(value: &str) -> String {
    value.trim().trim_matches('\'').trim_matches('"').trim().to_string()
}

fn is_self_like_name(name: &str) -> bool {
    matches!(name, "self" | "this")
}

#[cfg(test)]
mod tests {
    use super::super::type_inference::TypeInferenceEngine;
    use super::*;
    use crate::Parser;
    use std::collections::BTreeMap;

    fn parse_ast(code: &str) -> Result<Node, String> {
        let mut parser = Parser::new(code);
        parser.parse().map_err(|err| format!("parse failed: {err:?}"))
    }

    fn method_call_named<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
        if let NodeKind::MethodCall { method, .. } = &node.kind
            && method == name
        {
            return Some(node);
        }

        match &node.kind {
            NodeKind::Program { statements } => {
                statements.iter().find_map(|child| method_call_named(child, name))
            }
            NodeKind::ExpressionStatement { expression } => method_call_named(expression, name),
            NodeKind::VariableDeclaration { initializer, .. } => {
                initializer.as_deref().and_then(|child| method_call_named(child, name))
            }
            NodeKind::Assignment { lhs, rhs, .. } => {
                method_call_named(lhs, name).or_else(|| method_call_named(rhs, name))
            }
            NodeKind::MethodCall { object, args, .. } => method_call_named(object, name)
                .or_else(|| args.iter().find_map(|child| method_call_named(child, name))),
            NodeKind::Binary { left, right, .. } => {
                method_call_named(left, name).or_else(|| method_call_named(right, name))
            }
            _ => None,
        }
    }

    fn object_fact(package: &str, confidence: Confidence) -> TypeFact {
        TypeFact {
            ty: PerlType::Object(package.to_string()),
            confidence,
            evidence: vec![TypeEvidence::WorkspaceSymbol { package: package.to_string() }],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Object(super::super::type_facts::ObjectShape::new(
                package.to_string(),
                BTreeMap::new(),
            ))),
        }
    }

    fn hash_shape_fact(slot: &str, package: &str) -> TypeFact {
        let mut slots = BTreeMap::new();
        slots.insert(slot.to_string(), object_fact(package, Confidence::High));
        TypeFact {
            ty: PerlType::Hash { key: Box::new(PerlType::Any), value: Box::new(PerlType::Any) },
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::Literal],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Hash(super::super::type_facts::HashShape::new(slots, None))),
        }
    }

    fn hash_of_array_shape_fact(slot: &str, index: usize, package: &str) -> TypeFact {
        let mut slots = BTreeMap::new();
        slots.insert(slot.to_string(), array_shape_fact(index, package));
        TypeFact {
            ty: PerlType::Hash { key: Box::new(PerlType::Any), value: Box::new(PerlType::Any) },
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::Literal],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Hash(super::super::type_facts::HashShape::new(slots, None))),
        }
    }

    fn hash_of_hash_shape_fact(outer_slot: &str, inner_slot: &str, package: &str) -> TypeFact {
        let mut inner_slots = BTreeMap::new();
        inner_slots.insert(inner_slot.to_string(), object_fact(package, Confidence::High));
        let inner = TypeFact {
            ty: PerlType::Hash { key: Box::new(PerlType::Any), value: Box::new(PerlType::Any) },
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::Literal],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Hash(super::super::type_facts::HashShape::new(
                inner_slots,
                None,
            ))),
        };
        let mut slots = BTreeMap::new();
        slots.insert(outer_slot.to_string(), inner);
        TypeFact {
            ty: PerlType::Hash { key: Box::new(PerlType::Any), value: Box::new(PerlType::Any) },
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::Literal],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Hash(super::super::type_facts::HashShape::new(slots, None))),
        }
    }

    fn object_field_shape_fact(field: &str, field_package: &str) -> TypeFact {
        let mut fields = BTreeMap::new();
        fields.insert(field.to_string(), object_fact(field_package, Confidence::Medium));
        TypeFact {
            ty: PerlType::Object("My::Controller".to_string()),
            confidence: Confidence::Medium,
            evidence: vec![TypeEvidence::BlessLiteral { package: "My::Controller".to_string() }],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Object(super::super::type_facts::ObjectShape::new(
                "My::Controller".to_string(),
                fields,
            ))),
        }
    }

    fn array_shape_fact(index: usize, package: &str) -> TypeFact {
        let mut indexed = BTreeMap::new();
        indexed.insert(index, object_fact(package, Confidence::High));
        TypeFact {
            ty: PerlType::Array(Box::new(PerlType::Any)),
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::Literal],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Array(super::super::type_facts::ArrayShape::new(indexed, None))),
        }
    }

    fn array_union_shape_fact(index: usize, first: &str, second: &str) -> TypeFact {
        let mut indexed = BTreeMap::new();
        indexed.insert(index, union_object_fact(first, second));
        TypeFact {
            ty: PerlType::Array(Box::new(PerlType::Any)),
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::Literal],
            dynamic_boundary: None,
            shape: Some(ShapeFact::Array(super::super::type_facts::ArrayShape::new(indexed, None))),
        }
    }

    fn union_object_fact(first: &str, second: &str) -> TypeFact {
        TypeFact {
            ty: PerlType::Union(vec![
                PerlType::Object(first.to_string()),
                PerlType::Object(second.to_string()),
            ]),
            confidence: Confidence::High,
            evidence: vec![TypeEvidence::WorkspaceSymbol { package: first.to_string() }],
            dynamic_boundary: None,
            shape: None,
        }
    }

    fn receiver_fact_for(
        code: &str,
        method: &str,
        env: &TypeEnvironment,
    ) -> Result<ReceiverFact, String> {
        let ast = parse_ast(code)?;
        let call = method_call_named(&ast, method).ok_or("expected method call")?;
        Ok(receiver_fact_for_method_call(
            call,
            ReceiverFactContext::new(Some(env)).with_source(code),
        ))
    }

    fn source_derived_receiver_fact_for(code: &str, method: &str) -> Result<ReceiverFact, String> {
        let ast = parse_ast(code)?;
        let call = method_call_named(&ast, method).ok_or("expected method call")?;
        let mut engine = TypeInferenceEngine::new();
        engine.infer(&ast).map_err(|err| format!("inference failed: {err:?}"))?;
        Ok(receiver_fact_for_method_call(
            call,
            ReceiverFactContext::new(Some(engine.environment())).with_source(code),
        ))
    }

    #[test]
    fn nested_hash_of_array_slot_admits_through_recursive_container_resolution()
    -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "groups".to_string(),
            hash_of_array_shape_fact("staff", 0, "My::Group"),
        );

        let fact = receiver_fact_for("$groups{staff}[0]->render();", "render", &env)?;

        // The [0] receiver resolves through the recursive container walk:
        // $groups env fact -> {staff} slot fact -> [0] element fact. The
        // admission inherits the walked container's hash-slot evidence.
        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package.as_deref(), Some("My::Group"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));
        Ok(())
    }

    // Mirrors `method_call_named` traversal order but returns the receiver
    // object of the matching method call directly, so a found node is a
    // method-call receiver by construction.
    fn method_call_object<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
        if let NodeKind::MethodCall { method, object, .. } = &node.kind
            && method == name
        {
            return Some(object);
        }

        match &node.kind {
            NodeKind::Program { statements } => {
                statements.iter().find_map(|child| method_call_object(child, name))
            }
            NodeKind::ExpressionStatement { expression } => method_call_object(expression, name),
            NodeKind::VariableDeclaration { initializer, .. } => {
                initializer.as_deref().and_then(|child| method_call_object(child, name))
            }
            NodeKind::Assignment { lhs, rhs, .. } => {
                method_call_object(lhs, name).or_else(|| method_call_object(rhs, name))
            }
            NodeKind::MethodCall { object, args, .. } => method_call_object(object, name)
                .or_else(|| args.iter().find_map(|child| method_call_object(child, name))),
            NodeKind::Binary { left, right, .. } => {
                method_call_object(left, name).or_else(|| method_call_object(right, name))
            }
            _ => None,
        }
    }

    fn binary_receiver_parts<'a>(
        root: &'a Node,
        method: &str,
    ) -> Result<(&'a Node, &'a Node, &'a Node), String> {
        // method_call_object only yields the receiver object of a matching
        // method call, so no "not a method call" arm is reachable here.
        let object = method_call_object(root, method).ok_or("expected method call")?;
        let NodeKind::Binary { left, right, .. } = &object.kind else {
            return Err("receiver is not a subscript binary".to_string());
        };
        Ok((object, left, right))
    }

    #[test]
    fn hash_receiver_fact_call_presence_observer() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "data".to_string(),
            hash_of_hash_shape_fact("outer", "inner", "My::Leaf"),
        );
        let code = "$data{outer}{inner}->render();";
        let root = parse_ast(code)?;
        let (receiver, left, right) = binary_receiver_parts(&root, "render")?;
        let context = ReceiverFactContext::new(Some(&env)).with_source(code);

        // Direct call observation: hash_receiver_fact admitted the inner
        // slot through with_access_evidence(slot_fact, &container_fact,
        // evidence).
        let fact = super::hash_receiver_fact(receiver, left, right, context);

        assert_eq!(fact.kind, ReceiverKind::HashSlot);
        assert_eq!(fact.package.as_deref(), Some("My::Leaf"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$data" && key == "outer")
        }));
        assert!(fact.evidence.iter().any(
            |evidence| matches!(evidence, TypeEvidence::HashSlot { hash, key } if key == "inner")
        ));
        Ok(())
    }

    #[test]
    fn array_receiver_fact_call_presence_observer() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "groups".to_string(),
            hash_of_array_shape_fact("staff", 0, "My::Group"),
        );

        let code = "$groups{staff}[0]->render();";
        let ast = parse_ast(code)?;
        let (receiver, left, right) = binary_receiver_parts(&ast, "render")?;
        let context = ReceiverFactContext::new(Some(&env)).with_source(code);

        // Direct call observation: array_receiver_fact admitted the index
        // through with_access_evidence(index_fact, &container_fact, evidence).
        let fact = super::array_receiver_fact(receiver, left, right, context);

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package.as_deref(), Some("My::Group"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));
        assert!(fact
            .evidence
            .iter()
            .any(|evidence| matches!(evidence, TypeEvidence::Heuristic { reason } if reason == "array index receiver")));
        Ok(())
    }

    #[test]
    fn binary_receiver_parts_exact_error_variant() -> Result<(), String> {
        // Non-subscript receiver boundary: a plain variable receiver triggers
        // the exact "receiver is not a subscript binary" error variant named
        // by the RIPR review guidance.
        let root = parse_ast("$srv->render();")?;
        let err = binary_receiver_parts(&root, "render")
            .expect_err("expected the non-subscript-receiver error variant");
        assert_eq!(err, "receiver is not a subscript binary");
        Ok(())
    }

    #[test]
    fn binary_receiver_parts_missing_call_error_variant() -> Result<(), String> {
        // Missing-call boundary: an AST with no matching method call triggers
        // the exact "expected method call" error variant.
        let root = parse_ast("my $x = 1;")?;
        let err = binary_receiver_parts(&root, "render")
            .expect_err("expected the missing-method-call error variant");
        assert_eq!(err, "expected method call");
        Ok(())
    }

    #[test]
    fn nested_hashref_of_array_slot_admits_through_recursive_container_resolution()
    -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "config".to_string(),
            hash_of_array_shape_fact("db", 1, "My::Handle"),
        );

        let fact = receiver_fact_for("$config->{db}[1]->call();", "call", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package.as_deref(), Some("My::Handle"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashRefSlot { base, key } if base.contains("config") && key == "db")
        }));
        Ok(())
    }

    #[test]
    fn nested_hash_of_hash_slot_admits_through_recursive_container_resolution() -> Result<(), String>
    {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "data".to_string(),
            hash_of_hash_shape_fact("outer", "inner", "My::Leaf"),
        );

        let fact = receiver_fact_for("$data{outer}{inner}->render();", "render", &env)?;

        // The inner {inner} hash-slot admission runs with the container fact
        // produced by the recursive {outer} walk and inherits its evidence.
        assert_eq!(fact.kind, ReceiverKind::HashSlot);
        assert_eq!(fact.package.as_deref(), Some("My::Leaf"));
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$data" && key == "outer")
        }));
        Ok(())
    }

    #[test]
    fn method_call_container_discriminates_the_variable_identity_boundary() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("srv".to_string(), object_fact("My::Server", Confidence::High));

        // The {k} container is a method-call result, not a variable, so the
        // variable-identity base case returns None and the receiver falls
        // back without inventing a package.
        let fact = receiver_fact_for("$srv->fetch->{k}->render();", "render", &env)?;

        assert_eq!(fact.kind, ReceiverKind::HashRefSlot);
        assert_eq!(fact.package, None);
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn receiver_container_fact_boundary_discriminator() -> Result<(), String> {
        // Some boundary: the container is a variable with an environment
        // fact, so the variable-identity base case resolves it.
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("data".to_string(), hash_shape_fact("k", "My::Known"));
        let code = "$data{k}->render();";
        let root = parse_ast(code)?;
        let (_, left, _) = binary_receiver_parts(&root, "render")?;
        let context = ReceiverFactContext::new(Some(&env)).with_source(code);
        assert!(
            super::receiver_container_fact(left, context).is_some(),
            "variable container must resolve through the environment"
        );

        // None boundary: the container is a method-call result, so
        // variable_identity returns None and no binary arm applies.
        let code2 = "$srv->fetch->{k}->render();";
        let root2 = parse_ast(code2)?;
        let (_, left2, _) = binary_receiver_parts(&root2, "render")?;
        let empty = ReceiverFactContext::new(None).with_source(code2);
        assert!(
            super::receiver_container_fact(left2, empty).is_none(),
            "method-call container must miss the variable-identity boundary"
        );
        Ok(())
    }

    #[test]
    fn receiver_container_fact_boundary_discriminates_subscript_arms() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "groups".to_string(),
            hash_of_array_shape_fact("staff", 0, "My::Group"),
        );

        // Hash-slot arm boundary (op == "{}"): the {staff} subscript resolves
        // through the recursive walk and carries HashSlot access evidence.
        let hash_code = "$groups{staff}[0]->render();";
        let hash_root = parse_ast(hash_code)?;
        let (_, hash_slot_binary, _) = binary_receiver_parts(&hash_root, "render")?;
        let NodeKind::Binary { op, .. } = &hash_slot_binary.kind else {
            return Err("expected the slot subscript binary".to_string());
        };
        assert_eq!(op, "{}");
        let hash_context = ReceiverFactContext::new(Some(&env)).with_source(hash_code);
        let hash_fact = super::receiver_container_fact(hash_slot_binary, hash_context)
            .ok_or("expected the {staff} container fact")?;
        assert!(matches!(hash_fact.shape, Some(ShapeFact::Array(_))));
        assert!(hash_fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));

        // Hashref arm boundary (op == "->{}"): the is_hashref branch resolves
        // the same slot through HashRefSlot evidence instead.
        let ref_code = "$groups->{staff}[0]->render();";
        let ref_root = parse_ast(ref_code)?;
        let (_, ref_slot_binary, _) = binary_receiver_parts(&ref_root, "render")?;
        let NodeKind::Binary { op: ref_op, .. } = &ref_slot_binary.kind else {
            return Err("expected the hashref slot subscript binary".to_string());
        };
        assert_eq!(ref_op, "->{}");
        let ref_context = ReceiverFactContext::new(Some(&env)).with_source(ref_code);
        let ref_fact = super::receiver_container_fact(ref_slot_binary, ref_context)
            .ok_or("expected the ->{staff} container fact")?;
        assert!(matches!(ref_fact.shape, Some(ShapeFact::Array(_))));
        assert!(ref_fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashRefSlot { base, key } if base == "$groups" && key == "staff")
        }));

        // Array-index arm boundary (op == "[]"): the [0] subscript resolves
        // to the element fact through the recursive array arm.
        let (index_binary, _, _) = binary_receiver_parts(&hash_root, "render")?;
        let NodeKind::Binary { op: index_op, .. } = &index_binary.kind else {
            return Err("expected the index subscript binary".to_string());
        };
        assert_eq!(index_op, "[]");
        let array_context = ReceiverFactContext::new(Some(&env)).with_source(hash_code);
        let array_fact = super::receiver_container_fact(index_binary, array_context)
            .ok_or("expected the [0] container fact")?;
        assert_eq!(array_fact.ty, PerlType::Object("My::Group".to_string()));
        assert!(array_fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));
        Ok(())
    }

    #[test]
    fn unknown_variable_hash_container_discriminates_the_env_lookup_seam() -> Result<(), String> {
        let env = TypeEnvironment::new();

        let fact = receiver_fact_for("$mystery{staff}->render();", "render", &env)?;

        // The variable-identity base case runs, finds no environment fact,
        // and the hash receiver falls back without inventing a package.
        assert_eq!(fact.kind, ReceiverKind::HashSlot);
        assert_eq!(fact.package, None);
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.freshness, ReceiverFactFreshness::Unknown);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn static_constructor_receiver_records_package() -> Result<(), String> {
        let env = TypeEnvironment::new();
        let fact = receiver_fact_for("Foo::Bar->new();", "new", &env)?;

        assert_eq!(fact.kind, ReceiverKind::StaticPackage);
        assert_eq!(fact.package.as_deref(), Some("Foo::Bar"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.freshness, ReceiverFactFreshness::Fresh);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(matches!(
            fact.evidence.first(),
            Some(TypeEvidence::ConstructorCall { package }) if package == "Foo::Bar"
        ));
        Ok(())
    }

    #[test]
    fn self_receiver_uses_type_environment_fact() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("self".to_string(), object_fact("My::Controller", Confidence::High));

        let fact = receiver_fact_for("$self->render();", "render", &env)?;

        assert_eq!(fact.kind, ReceiverKind::SelfReceiver);
        assert_eq!(fact.package.as_deref(), Some("My::Controller"));
        assert_eq!(fact.confidence, Confidence::High);
        assert!(matches!(fact.shape, Some(ShapeFact::Object(_))));
        assert_eq!(fact.dynamic_boundary, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        Ok(())
    }

    #[test]
    fn object_receiver_uses_type_environment_fact() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("object".to_string(), object_fact("My::Service", Confidence::High));

        let fact = receiver_fact_for("$object->run();", "run", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ObjectVariable);
        assert_eq!(fact.package.as_deref(), Some("My::Service"));
        assert_eq!(fact.freshness, ReceiverFactFreshness::Fresh);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        Ok(())
    }

    #[test]
    fn medium_confidence_object_receiver_preserves_fallback() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("object".to_string(), object_fact("My::Service", Confidence::Medium));

        let fact = receiver_fact_for("$object->run();", "run", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ObjectVariable);
        assert_eq!(fact.package.as_deref(), Some("My::Service"));
        assert_eq!(fact.confidence, Confidence::Medium);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn union_object_receiver_preserves_fallback() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("object".to_string(), union_object_fact("My::Service", "Other"));

        let fact = receiver_fact_for("$object->run();", "run", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ObjectVariable);
        assert_eq!(fact.package.as_deref(), Some("My::Service"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        // candidate_packages must expose both union branches
        assert_eq!(fact.candidate_packages, vec!["My::Service", "Other"]);
        assert!(fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn exact_single_package_receiver_has_one_candidate() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("object".to_string(), object_fact("My::Service", Confidence::High));

        let fact = receiver_fact_for("$object->run();", "run", &env)?;

        assert_eq!(fact.candidate_packages, vec!["My::Service"]);
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn static_constructor_receiver_has_one_candidate() -> Result<(), String> {
        let env = TypeEnvironment::new();
        let fact = receiver_fact_for("Foo::Bar->new();", "new", &env)?;

        assert_eq!(fact.candidate_packages, vec!["Foo::Bar"]);
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn unknown_receiver_has_no_candidates() -> Result<(), String> {
        let env = TypeEnvironment::new();
        let fact = receiver_fact_for("$unknown->run();", "run", &env)?;

        assert!(fact.candidate_packages.is_empty());
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn dynamic_key_receiver_has_no_candidates() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("services".to_string(), hash_shape_fact("mailer", "My::Mailer"));

        let fact = receiver_fact_for("$services{$name}->send();", "send", &env)?;

        assert!(fact.candidate_packages.is_empty());
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn hash_slot_receiver_has_one_candidate() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("services".to_string(), hash_shape_fact("mailer", "My::Mailer"));

        let fact = receiver_fact_for("$services{mailer}->send();", "send", &env)?;

        assert_eq!(fact.candidate_packages, vec!["My::Mailer"]);
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn triple_union_receiver_exposes_all_candidates() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "widget".to_string(),
            TypeFact {
                ty: PerlType::Union(vec![
                    PerlType::Object("My::Button".to_string()),
                    PerlType::Object("My::Label".to_string()),
                    PerlType::Object("My::Frame".to_string()),
                ]),
                confidence: Confidence::High,
                evidence: vec![],
                dynamic_boundary: None,
                shape: None,
            },
        );

        let fact = receiver_fact_for("$widget->draw();", "draw", &env)?;

        assert_eq!(fact.candidate_packages, vec!["My::Button", "My::Label", "My::Frame"]);
        assert!(fact.is_union_receiver());
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn union_with_duplicate_packages_deduplicates() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        // Duplicate union arm: My::Foo appears twice.
        env.set_variable_fact(
            "obj".to_string(),
            TypeFact {
                ty: PerlType::Union(vec![
                    PerlType::Object("My::Foo".to_string()),
                    PerlType::Object("My::Foo".to_string()),
                    PerlType::Object("My::Bar".to_string()),
                ]),
                confidence: Confidence::High,
                evidence: vec![],
                dynamic_boundary: None,
                shape: None,
            },
        );

        let fact = receiver_fact_for("$obj->run();", "run", &env)?;

        // My::Foo should appear only once.
        assert_eq!(fact.candidate_packages, vec!["My::Foo", "My::Bar"]);
        assert!(fact.is_union_receiver());
        Ok(())
    }

    /// A union mixing object and non-object arms yields **no** candidates.
    ///
    /// #9493 introduced `candidate_packages` with no consumer and collected
    /// object arms while silently skipping non-object ones. #9500 adds the first
    /// consumer — union-receiver method completion — which makes that permissive
    /// reading unsafe: `Union(Object(Foo), Object(Bar), Scalar)` would collect two
    /// packages, satisfy `is_union_receiver()`, and offer `Foo`/`Bar` methods on a
    /// receiver that may hold a plain string.
    ///
    /// Package collection is therefore all-or-nothing: unless *every* arm resolves
    /// to an object (through references), the candidate set is empty and the
    /// receiver falls back to the existing bounded path. This tightens #9493's
    /// contract rather than weakening a proof — the field's only consumer is the
    /// completion dispatch added here, so no other behaviour depends on the old
    /// permissive reading.
    #[test]
    fn union_with_mixed_types_yields_no_candidates() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        // Union of Object + scalar — mixed arms cannot safely dispatch methods.
        env.set_variable_fact(
            "obj".to_string(),
            TypeFact {
                ty: PerlType::Union(vec![
                    PerlType::Object("My::Widget".to_string()),
                    PerlType::Scalar(super::super::type_inference::ScalarType::String),
                ]),
                confidence: Confidence::High,
                evidence: vec![],
                dynamic_boundary: None,
                shape: None,
            },
        );

        let fact = receiver_fact_for("$obj->run();", "run", &env)?;

        assert!(
            fact.candidate_packages.is_empty(),
            "mixed object/non-object union must fail closed, got {:?}",
            fact.candidate_packages
        );
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    /// The multi-object variant of the mixed union: two object arms plus a
    /// scalar. Under #9493's permissive collection this produced two candidates
    /// and would have satisfied `is_union_receiver()`, routing an unsafe receiver
    /// into union method dispatch. This is the case the single-object test above
    /// could not discriminate, because one object arm keeps the length at one.
    #[test]
    fn multi_object_union_with_non_object_arm_yields_no_candidates() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact(
            "obj".to_string(),
            TypeFact {
                ty: PerlType::Union(vec![
                    PerlType::Object("My::Widget".to_string()),
                    PerlType::Object("My::Gadget".to_string()),
                    PerlType::Scalar(super::super::type_inference::ScalarType::String),
                ]),
                confidence: Confidence::High,
                evidence: vec![],
                dynamic_boundary: None,
                shape: None,
            },
        );

        let fact = receiver_fact_for("$obj->run();", "run", &env)?;

        assert!(
            fact.candidate_packages.is_empty(),
            "multi-object mixed union must fail closed, got {:?}",
            fact.candidate_packages
        );
        assert!(!fact.is_union_receiver(), "mixed union must never present as a union receiver");
        Ok(())
    }

    #[test]
    fn array_index_receiver_has_one_candidate() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("items".to_string(), array_shape_fact(0, "My::Item"));

        let fact = receiver_fact_for("$items[0]->render();", "render", &env)?;

        assert_eq!(fact.candidate_packages, vec!["My::Item"]);
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn self_receiver_with_package_has_one_candidate() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("self".to_string(), object_fact("My::Controller", Confidence::High));

        let fact = receiver_fact_for("$self->render();", "render", &env)?;

        assert_eq!(fact.candidate_packages, vec!["My::Controller"]);
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn self_receiver_without_package_has_no_candidates() -> Result<(), String> {
        // $self with no type environment entry.
        let env = TypeEnvironment::new();
        let fact = receiver_fact_for("$self->render();", "render", &env)?;

        assert!(fact.candidate_packages.is_empty());
        assert!(!fact.is_union_receiver());
        Ok(())
    }

    #[test]
    fn hash_slot_receiver_uses_known_slot_fact() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("services".to_string(), hash_shape_fact("mailer", "My::Mailer"));

        let fact = receiver_fact_for("$services{mailer}->send();", "send", &env)?;

        assert_eq!(fact.kind, ReceiverKind::HashSlot);
        assert_eq!(fact.package.as_deref(), Some("My::Mailer"));
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$services" && key == "mailer")
        }));
        Ok(())
    }

    #[test]
    fn hashref_slot_receiver_preserves_hashref_kind() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("services".to_string(), hash_shape_fact("mailer", "My::Mailer"));

        let fact = receiver_fact_for("$services->{mailer}->send();", "send", &env)?;

        assert_eq!(fact.kind, ReceiverKind::HashRefSlot);
        assert_eq!(fact.package.as_deref(), Some("My::Mailer"));
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashRefSlot { base, key } if base == "$services" && key == "mailer")
        }));
        Ok(())
    }

    #[test]
    fn object_field_receiver_preserves_fallback() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("self".to_string(), object_field_shape_fact("db", "My::DB"));

        let fact = receiver_fact_for("$self->{db}->connect();", "connect", &env)?;

        assert_eq!(fact.kind, ReceiverKind::HashRefSlot);
        assert_eq!(fact.package.as_deref(), Some("My::DB"));
        assert_eq!(fact.confidence, Confidence::Medium);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashRefSlot { base, key } if base == "$self" && key == "db")
        }));
        Ok(())
    }

    #[test]
    fn dynamic_hash_key_marks_dynamic_boundary() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("services".to_string(), hash_shape_fact("mailer", "My::Mailer"));

        let fact = receiver_fact_for("$services{$name}->send();", "send", &env)?;

        assert_eq!(fact.kind, ReceiverKind::DynamicKey);
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::DynamicHashKey));
        assert_eq!(fact.package, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn array_index_receiver_uses_known_index_fact() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("items".to_string(), array_shape_fact(0, "My::Item"));

        let fact = receiver_fact_for("$items[0]->render();", "render", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package.as_deref(), Some("My::Item"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        Ok(())
    }

    #[test]
    fn source_derived_hash_of_arrays_receiver_resolves_element_fact() -> Result<(), String> {
        let fact = source_derived_receiver_fact_for(
            "my %groups = (staff => [My::User->new]); $groups{staff}[0]->render();",
            "render",
        )?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package.as_deref(), Some("My::User"));
        assert_eq!(fact.candidate_packages, vec!["My::User"]);
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.freshness, ReceiverFactFreshness::Fresh);
        assert_eq!(fact.dynamic_boundary, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::Heuristic { reason } if reason == "array index receiver")
        }));
        Ok(())
    }

    #[test]
    fn source_derived_hashref_of_arrays_receiver_resolves_element_fact() -> Result<(), String> {
        let fact = source_derived_receiver_fact_for(
            "my $groups = { staff => [My::User->new] }; $groups->{staff}[0]->render();",
            "render",
        )?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package.as_deref(), Some("My::User"));
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.dynamic_boundary, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashRefSlot { base, key } if base == "$groups" && key == "staff")
        }));
        Ok(())
    }

    #[test]
    fn dynamic_hash_key_inside_array_chain_preserves_dynamic_boundary() -> Result<(), String> {
        let fact = source_derived_receiver_fact_for(
            "my %groups = (staff => [My::User->new]); my $bucket = 'staff'; $groups{$bucket}[0]->render();",
            "render",
        )?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package, None);
        assert!(fact.candidate_packages.is_empty());
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::DynamicHashKey));
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::Heuristic { reason } if reason == "hash receiver key is dynamic")
        }));
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::Heuristic { reason } if reason == "array index receiver")
        }));
        Ok(())
    }

    #[test]
    fn dynamic_intermediate_boundary_survives_later_hash_lookup() -> Result<(), String> {
        let fact = source_derived_receiver_fact_for(
            "my %groups = (staff => [My::User->new]); my $bucket = 'staff'; $groups{$bucket}[0]{field}->render();",
            "render",
        )?;

        assert_eq!(fact.kind, ReceiverKind::HashSlot);
        assert_eq!(fact.package, None);
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::DynamicHashKey));
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::Heuristic { reason } if reason == "hash receiver key is dynamic")
        }));
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "Binary" && key == "field")
        }));
        Ok(())
    }

    #[test]
    fn wrong_shape_hash_slot_cannot_be_admitted_as_array_element() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("groups".to_string(), hash_shape_fact("staff", "My::Group"));

        let fact = receiver_fact_for("$groups{staff}[0]->render();", "render", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package, None);
        assert!(fact.candidate_packages.is_empty());
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));
        Ok(())
    }

    #[test]
    fn wrong_shape_array_element_cannot_be_admitted_as_hash_slot() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("items".to_string(), array_shape_fact(0, "My::Item"));

        let fact = receiver_fact_for("$items[0]{field}->render();", "render", &env)?;

        assert_eq!(fact.kind, ReceiverKind::HashSlot);
        assert_eq!(fact.package, None);
        assert!(fact.candidate_packages.is_empty());
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::Heuristic { reason } if reason == "array index receiver")
        }));
        Ok(())
    }

    #[test]
    fn ambiguous_array_element_preserves_candidates_but_not_exactness() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("groups".to_string(), {
            let mut slots = BTreeMap::new();
            slots.insert("staff".to_string(), array_union_shape_fact(0, "My::User", "My::Admin"));
            TypeFact {
                ty: PerlType::Hash { key: Box::new(PerlType::Any), value: Box::new(PerlType::Any) },
                confidence: Confidence::High,
                evidence: vec![TypeEvidence::Literal],
                dynamic_boundary: None,
                shape: Some(ShapeFact::Hash(super::super::type_facts::HashShape::new(slots, None))),
            }
        });

        let fact = receiver_fact_for("$groups{staff}[0]->render();", "render", &env)?;

        assert_eq!(fact.package.as_deref(), Some("My::User"));
        assert_eq!(fact.candidate_packages, vec!["My::User", "My::Admin"]);
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn shadowed_environment_fact_wins_by_current_variable_identity() -> Result<(), String> {
        let mut parent = TypeEnvironment::new();
        parent.set_variable_fact(
            "groups".to_string(),
            hash_of_array_shape_fact("staff", 0, "Outer::User"),
        );
        let mut env = TypeEnvironment::with_parent(parent);
        env.set_variable_fact(
            "groups".to_string(),
            hash_of_array_shape_fact("staff", 0, "Inner::User"),
        );

        let fact = receiver_fact_for("$groups{staff}[0]->render();", "render", &env)?;

        assert_eq!(fact.package.as_deref(), Some("Inner::User"));
        assert_eq!(fact.candidate_packages, vec!["Inner::User"]);
        assert!(fact.evidence.iter().any(|evidence| {
            matches!(evidence, TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff")
        }));
        Ok(())
    }

    #[test]
    fn traversed_medium_confidence_caps_indexed_element_fact() -> Result<(), String> {
        let mut container = hash_of_array_shape_fact("staff", 0, "My::User");
        container.confidence = Confidence::Medium;
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("groups".to_string(), container);

        let fact = receiver_fact_for("$groups{staff}[0]->render();", "render", &env)?;

        assert_eq!(fact.package.as_deref(), Some("My::User"));
        assert_eq!(fact.confidence, Confidence::Medium);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn receiver_fact_range_is_the_outer_receiver_expression() -> Result<(), String> {
        let code = "$groups{staff}[0]->render();";
        let env_fact = hash_of_array_shape_fact("staff", 0, "My::User");
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("groups".to_string(), env_fact);
        let fact = receiver_fact_for(code, "render", &env)?;
        let start = code.find("$groups{staff}[0]").ok_or("receiver start missing")?;
        let end = start + "$groups{staff}[0]".len();

        assert_eq!(fact.source_range, Some((start, end)));
        Ok(())
    }

    #[test]
    fn malformed_receiver_does_not_fabricate_a_fact() -> Result<(), String> {
        let code = "$groups{staff}[0]->render(";
        let mut parser = Parser::new(code);
        let parsed = parser.parse();
        if let Ok(ast) = parsed
            && let Some(call) = method_call_named(&ast, "render")
        {
            let env = TypeEnvironment::new();
            let fact = receiver_fact_for_method_call(
                call,
                ReceiverFactContext::new(Some(&env)).with_source(code),
            );
            if fact.package.is_some() || fact.fallback_state == ReceiverFallbackState::Exact {
                return Err(format!("malformed receiver fabricated exact fact: {fact:?}"));
            }
        }
        Ok(())
    }

    #[test]
    fn source_derived_receiver_fact_for_reports_missing_method_call() -> Result<(), String> {
        // Exact error-variant observation: an AST without the requested
        // method call fails the source-derived helper with the exact
        // "expected method call" variant.
        let err = source_derived_receiver_fact_for("my $x = 1;", "render")
            .err()
            .ok_or("expected the missing-method-call error variant")?;
        assert_eq!(err, "expected method call");
        Ok(())
    }

    #[test]
    fn dynamic_array_index_stays_low_confidence() -> Result<(), String> {
        let mut env = TypeEnvironment::new();
        env.set_variable_fact("items".to_string(), array_shape_fact(0, "My::Item"));

        let fact = receiver_fact_for("$items[$i]->render();", "render", &env)?;

        assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
        assert_eq!(fact.package, None);
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::UnknownReceiver));
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }

    #[test]
    fn unknown_receiver_stays_low_confidence() -> Result<(), String> {
        let env = TypeEnvironment::new();

        let fact = receiver_fact_for("$unknown->run();", "run", &env)?;

        assert_eq!(fact.kind, ReceiverKind::Unknown);
        assert_eq!(fact.confidence, Confidence::Low);
        assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::UnknownReceiver));
        assert_eq!(fact.package, None);
        assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
        Ok(())
    }
}
