//! Registry-activated Dancer2 route fact minting (#8918).
//!
//! This module turns source-extracted Dancer2 route declarations into the
//! canonical [`RouteFact`] family. It is the facts side of the #8918 route
//! contract and builds directly on the #8914 activation seam:
//!
//! - route facts mint **only through the registry-activated adapter**: a
//!   detected framework and an exact activation are both required; without
//!   them this function returns no facts (never name-only route synthesis);
//! - a declaration whose route keyword the activating import excluded via
//!   `!keyword` is not a route of this activation — the keyword was never
//!   imported, so no fact is minted;
//! - every fact is generation-owned: the envelope carries the detection
//!   receipt's project generation and invalidation dependencies over the
//!   owning source file and the activating `Dancer2` module;
//! - facts are shadow receipts: the adapter disposition remains `Shadow`, so
//!   no provider surface can publish them yet (#6822 owns publication).
//!
//! Method semantics follow the reviewed Dancer2 1.x profile (the workspace
//! `dancer2_skeleton` fixture carries `1.1.1`): `get` routes also serve `HEAD`
//! requests, `del` normalizes to `DELETE` (including inside `any` method
//! arrays), bare `any` matches the reviewed default method vocabulary, and
//! bare `delete` is not a Dancer2 DSL keyword.

use crate::framework::AdapterDetectionResult;
use crate::framework_adapters::dancer2::{
    DANCER2_ADAPTER_ID, DANCER2_FRAMEWORK_NAME, Dancer2ActivationFacts, Dancer2KeywordState,
};
use crate::route::{
    RouteDeclaration, RouteFact, RouteHandler, RouteMethodSet, RouteOptions, RoutePatternKind,
    route_envelope, route_fact_identity,
};
use crate::{
    AnchorId, BoundaryKind, FileId, InvalidationDependency, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};

/// Reviewed default method vocabulary for a bare `any` route.
///
/// Dancer2 1.x registers a bare `any` route without a method restriction, so
/// the normalized method set is the full reviewed HTTP vocabulary the dispatcher
/// admits.
pub const DANCER2_ANY_DEFAULT_METHODS: &[&str] =
    &["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT"];

/// Route keywords of the reviewed Dancer2 DSL contract.
pub const DANCER2_ROUTE_KEYWORDS: &[&str] =
    &["get", "post", "put", "del", "options", "patch", "any"];

/// One source-extracted Dancer2 route declaration awaiting minting.
///
/// Produced by the AST extractor in
/// `perl_semantic_analyzer::analysis::dancer2_routes`; this carrier adds the
/// package/file/declaration identity around the canonical payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2RouteDeclaration {
    /// Package the declaration appears in (activation scope).
    pub package: Option<String>,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Full declaration range (keyword start to last operand end).
    pub declaration_start_byte: u32,
    pub declaration_end_byte: u32,
    /// Canonical route payload (name/methods/pattern/options/handler).
    pub route: RouteDeclaration,
}

/// Normalize one method name from a route method list.
///
/// `del` normalizes to `DELETE` as Dancer2 does; other names are uppercased
/// after quote stripping. Empty names normalize to an empty string, which the
/// caller rejects.
#[must_use]
pub fn normalize_dancer2_method(raw: &str) -> String {
    let stripped = raw.trim().trim_matches('\'').trim_matches('"').trim();
    let lowered = stripped.to_ascii_lowercase();
    if lowered == "del" { "DELETE".to_string() } else { lowered.to_ascii_uppercase() }
}

/// Normalized method set for one route keyword without an explicit method list.
///
/// Returns `None` for keywords that are not route keywords of the reviewed
/// contract (including bare `delete`, which Dancer2 does not export).
#[must_use]
pub fn dancer2_keyword_methods(keyword: &str) -> Option<RouteMethodSet> {
    let methods: &[&str] = match keyword {
        "get" => &["GET", "HEAD"],
        "post" => &["POST"],
        "put" => &["PUT"],
        "del" => &["DELETE"],
        "options" => &["OPTIONS"],
        "patch" => &["PATCH"],
        "any" => DANCER2_ANY_DEFAULT_METHODS,
        _ => return None,
    };
    Some(RouteMethodSet::Exact(methods.iter().map(|method| method.to_string()).collect()))
}

/// Mint canonical Dancer2 route facts for one activating package.
///
/// Returns an empty vector unless `detection` established the framework and
/// `activation` is exact (registry-activated adapter contract). Declarations of
/// other packages and declarations whose route keyword the activation excluded
/// are skipped. All minted facts carry the detection receipt's generation and
/// invalidation dependencies over the owning source file and the `Dancer2`
/// module.
#[must_use]
pub fn dancer2_route_facts(
    detection: &AdapterDetectionResult,
    activation: &Dancer2ActivationFacts,
    package: Option<&str>,
    declarations: &[Dancer2RouteDeclaration],
) -> Vec<RouteFact> {
    if !detection.is_detected() || !activation.is_exact() {
        return Vec::new();
    }
    let Dancer2ActivationFacts { state, keywords, .. } = activation;
    let crate::framework_adapters::dancer2::Dancer2ActivationState::Exact {
        application_name,
        framework_version,
        source_generation,
    } = state
    else {
        return Vec::new();
    };

    let mut facts = Vec::new();
    for declaration in declarations {
        if declaration.package.as_deref() != package {
            continue;
        }
        let keyword = declaration.route.keyword.as_str();
        let Some(keyword_fact) = keywords.iter().find(|fact| fact.keyword == keyword) else {
            continue;
        };
        if keyword_fact.state == Dancer2KeywordState::Excluded {
            // `!keyword` at the activating import: the route keyword was never
            // imported, so this declaration is not a route of this activation.
            continue;
        }
        facts.push(mint_route_fact(
            declaration,
            application_name,
            framework_version,
            source_generation,
        ));
    }
    facts
}

fn mint_route_fact(
    declaration: &Dancer2RouteDeclaration,
    application_name: &str,
    framework_version: &str,
    generation: &SourceGeneration,
) -> RouteFact {
    let (fact_id, entity_id) =
        route_fact_identity(declaration.file_id, declaration.route.declaration_index, generation);
    let declaration_anchor = SourceAnchor::new(
        Some(AnchorId(u64::from(declaration.declaration_start_byte))),
        declaration.file_id,
        declaration.declaration_start_byte,
        declaration.declaration_end_byte,
    );
    let exact = !declaration.route.has_boundary();
    let (boundary_kind, boundary_reason) = primary_boundary(&declaration.route);
    let envelope = route_envelope(
        fact_id,
        entity_id,
        declaration.package.as_deref(),
        declaration_anchor,
        generation,
        vec![
            InvalidationDependency::new(
                format!("source:{}", declaration.file_id.0),
                generation.clone(),
            ),
            InvalidationDependency::new("module:Dancer2", generation.clone()),
        ],
        boundary_kind,
        boundary_reason,
        exact,
    );
    RouteFact::new(
        envelope,
        DANCER2_FRAMEWORK_NAME,
        DANCER2_ADAPTER_ID,
        framework_version,
        application_name,
        declaration.route.clone(),
    )
}

/// Primary envelope boundary for a bounded payload, in a fixed review order.
fn primary_boundary(route: &RouteDeclaration) -> (Option<BoundaryKind>, SemanticReasonCode) {
    if route.pattern.kind == RoutePatternKind::Dynamic {
        return (Some(BoundaryKind::DynamicValue), SemanticReasonCode::DynamicValue);
    }
    if matches!(route.methods, RouteMethodSet::Dynamic { .. }) {
        return (Some(BoundaryKind::DynamicValue), SemanticReasonCode::DynamicValue);
    }
    if let RouteHandler::Bounded { boundary, .. } = &route.handler {
        return (
            Some(match boundary {
                crate::route::RouteHandlerBoundary::String => BoundaryKind::Compatibility,
                crate::route::RouteHandlerBoundary::StaticCoderef
                | crate::route::RouteHandlerBoundary::Computed => BoundaryKind::DynamicValue,
            }),
            SemanticReasonCode::DynamicValue,
        );
    }
    if matches!(route.options, RouteOptions::Dynamic { .. })
        || matches!(&route.options, RouteOptions::Map(entries)
            if entries.iter().any(|entry| matches!(entry.value, crate::route::RouteOptionValue::Dynamic { .. })))
        || matches!(route.route_name, crate::route::RouteNameSelection::Dynamic { .. })
    {
        return (Some(BoundaryKind::DynamicValue), SemanticReasonCode::DynamicValue);
    }
    (None, SemanticReasonCode::ExactSource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{
        AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass,
        ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
        ModuleSelectorOutcome, ModuleVersionEvidence,
    };
    use crate::framework_adapters::dancer2::{
        Dancer2ActivationState, DslSelection, dancer2_activation_facts, dancer2_descriptor,
        detect_dancer2, parse_dancer2_import_args,
    };
    use crate::route::{
        RouteHandlerBoundary, RouteName, RouteNameSelection, RouteOption, RouteOptionValue,
        RoutePattern,
    };

    fn detected_input(generation: &str) -> AdapterDetectionInput {
        let activation = ModuleActivationIdentity::new(
            "Dancer2",
            Some(FileId(7)),
            SourceGeneration::known(generation),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            "1.1.1",
            SourceGeneration::known(generation),
        ));
        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known(generation),
            "sha256:fixture-input",
            vec![ModuleSelectorEvaluation::new(
                "Dancer2",
                ModuleSelectorOutcome::Matched {
                    activation,
                    evidence_class: DetectionEvidenceClass::ResolvedModule,
                },
            )],
        );
        AdapterDetectionInput::new(
            dancer2_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        )
    }

    fn exact_activation(generation: &str) -> Dancer2ActivationFacts {
        let detection = detect_dancer2(&detected_input(generation));
        dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]))
    }

    fn literal_get_declaration(index: u32) -> Dancer2RouteDeclaration {
        Dancer2RouteDeclaration {
            package: Some("App".to_string()),
            file_id: FileId(1),
            declaration_start_byte: 0,
            declaration_end_byte: 21,
            route: RouteDeclaration {
                declaration_index: index,
                keyword: "get".to_string(),
                keyword_anchor: SourceAnchor::new(Some(AnchorId(0)), FileId(1), 0, 3),
                route_name: RouteNameSelection::Absent,
                methods: RouteMethodSet::Exact(vec!["GET".to_string(), "HEAD".to_string()]),
                pattern: RoutePattern {
                    kind: RoutePatternKind::Literal,
                    value: Some("/x".to_string()),
                    anchor: SourceAnchor::new(Some(AnchorId(4)), FileId(1), 4, 8),
                },
                options: RouteOptions::Map(Vec::new()),
                handler: RouteHandler::InlineSub {
                    anchor: SourceAnchor::new(Some(AnchorId(12)), FileId(1), 12, 21),
                },
            },
        }
    }

    #[test]
    fn keyword_method_profile_matches_reviewed_contract() {
        assert_eq!(
            dancer2_keyword_methods("get"),
            Some(RouteMethodSet::Exact(vec!["GET".to_string(), "HEAD".to_string()]))
        );
        assert_eq!(
            dancer2_keyword_methods("del"),
            Some(RouteMethodSet::Exact(vec!["DELETE".to_string()]))
        );
        assert_eq!(
            dancer2_keyword_methods("options"),
            Some(RouteMethodSet::Exact(vec!["OPTIONS".to_string()]))
        );
        let any_methods =
            DANCER2_ANY_DEFAULT_METHODS.iter().map(|method| method.to_string()).collect();
        assert_eq!(dancer2_keyword_methods("any"), Some(RouteMethodSet::Exact(any_methods)));
        // Bare `delete` is not a reviewed Dancer2 DSL keyword.
        assert_eq!(dancer2_keyword_methods("delete"), None);
        assert_eq!(dancer2_keyword_methods("hook"), None);
    }

    #[test]
    fn method_normalization_strips_quotes_and_maps_del() {
        assert_eq!(normalize_dancer2_method("'get'"), "GET");
        assert_eq!(normalize_dancer2_method("del"), "DELETE");
        assert_eq!(normalize_dancer2_method("DEL"), "DELETE");
        assert_eq!(normalize_dancer2_method("patch"), "PATCH");
    }

    #[test]
    fn exact_activation_mints_generation_owned_facts() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let facts = dancer2_route_facts(
            &detection,
            &activation,
            Some("App"),
            &[literal_get_declaration(0), literal_get_declaration(1)],
        );
        assert_eq!(facts.len(), 2, "duplicate-looking routes stay distinct by order");
        for (index, fact) in facts.iter().enumerate() {
            assert_eq!(fact.framework_name, DANCER2_FRAMEWORK_NAME);
            assert_eq!(fact.adapter_id, DANCER2_ADAPTER_ID);
            assert_eq!(fact.framework_version, "1.1.1");
            assert_eq!(fact.application_name, "App");
            assert_eq!(fact.envelope.package.as_deref(), Some("App"));
            assert_eq!(fact.envelope.source_generation, SourceGeneration::known("gen-1"));
            assert_eq!(fact.route.declaration_index, index as u32);
            assert_eq!(fact.status(), crate::SemanticFactStatus::Exact);
        }
        assert_ne!(facts[0].envelope.fact_id, facts[1].envelope.fact_id);
    }

    #[test]
    fn no_activation_mints_no_facts() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let mut activation = exact_activation("gen-1");
        activation.state = Dancer2ActivationState::NotActivated { reason: "test".to_string() };
        assert!(
            dancer2_route_facts(
                &detection,
                &activation,
                Some("App"),
                &[literal_get_declaration(0)]
            )
            .is_empty(),
            "a Dancer2-looking file in an unactivated context mints zero facts"
        );
    }

    #[test]
    fn undetected_framework_mints_no_facts() {
        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known("gen-1"),
            "sha256:fixture-input",
            vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        );
        let input = AdapterDetectionInput::new(
            dancer2_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        );
        let detection = detect_dancer2(&input);
        let activation =
            dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]));
        assert!(
            dancer2_route_facts(
                &detection,
                &activation,
                Some("App"),
                &[literal_get_declaration(0)]
            )
            .is_empty()
        );
    }

    #[test]
    fn excluded_keyword_mints_no_fact_for_that_verb() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let args: Vec<String> = ["qw(!get)"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        let activation = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(activation.is_exact(), "exclusion alone keeps activation exact");

        let get_route = literal_get_declaration(0);
        let mut post_route = literal_get_declaration(1);
        post_route.route.keyword = "post".to_string();
        post_route.route.methods = RouteMethodSet::Exact(vec!["POST".to_string()]);

        let facts =
            dancer2_route_facts(&detection, &activation, Some("App"), &[get_route, post_route]);
        assert_eq!(facts.len(), 1, "only the non-excluded verb mints a fact");
        assert_eq!(facts[0].route.keyword, "post");
    }

    #[test]
    fn other_package_declarations_do_not_mint() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut other = literal_get_declaration(0);
        other.package = Some("Other".to_string());
        assert!(dancer2_route_facts(&detection, &activation, Some("App"), &[other]).is_empty());
    }

    #[test]
    fn bounded_handler_keeps_route_fact_degraded() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut declaration = literal_get_declaration(0);
        declaration.route.handler = RouteHandler::Bounded {
            boundary: RouteHandlerBoundary::String,
            anchor: None,
            reason: "string handler".to_string(),
        };
        let facts = dancer2_route_facts(&detection, &activation, Some("App"), &[declaration]);
        assert_eq!(facts.len(), 1, "bounded handler retains the route fact");
        assert_eq!(facts[0].status(), crate::SemanticFactStatus::Degraded);
        assert!(facts[0].envelope.boundary.is_some());
    }

    #[test]
    fn named_route_payload_is_preserved() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut declaration = literal_get_declaration(0);
        declaration.route.route_name = RouteNameSelection::Literal(RouteName {
            value: "user_show".to_string(),
            anchor: SourceAnchor::new(Some(AnchorId(4)), FileId(1), 4, 15),
        });
        declaration.route.pattern.value = Some("/users/:id".to_string());
        let facts = dancer2_route_facts(&detection, &activation, Some("App"), &[declaration]);
        assert_eq!(facts.len(), 1);
        let fact = &facts[0];
        let name_value = match &fact.route.route_name {
            RouteNameSelection::Literal(name) => Some(name.value.clone()),
            RouteNameSelection::Dynamic { .. } | RouteNameSelection::Absent => None,
        };
        assert_eq!(name_value.as_deref(), Some("user_show"));
        assert_eq!(fact.route.pattern.value.as_deref(), Some("/users/:id"));
        assert_ne!(
            fact.route.route_name_literal_value().as_deref(),
            fact.route.pattern.value.as_deref(),
            "name and pattern stay distinct fields"
        );
    }

    #[test]
    fn dynamic_option_entry_produces_an_envelope_boundary_link() {
        // Regression: a Map with one dynamic entry degraded the fact but left
        // the envelope boundary link absent.
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut declaration = literal_get_declaration(0);
        declaration.route.options = RouteOptions::Map(vec![RouteOption {
            key: "agent".to_string(),
            key_anchor: SourceAnchor::new(Some(AnchorId(9)), FileId(1), 9, 15),
            value: RouteOptionValue::Dynamic { reason: "computed".to_string() },
            value_anchor: SourceAnchor::new(Some(AnchorId(16)), FileId(1), 16, 20),
        }]);
        let facts = dancer2_route_facts(&detection, &activation, Some("App"), &[declaration]);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].status(), crate::SemanticFactStatus::Degraded);
        assert!(
            facts[0].envelope.boundary.is_some(),
            "per-entry dynamic options carry an envelope boundary link"
        );
    }

    #[test]
    fn default_dsl_and_shadow_disposition_are_documented_by_construction() {
        // The mint path is only reachable for exact activation, which requires
        // the default DSL selection; a custom DSL never carries route keywords.
        let detection = detect_dancer2(&detected_input("gen-1"));
        let mut evidence = parse_dancer2_import_args(&[]);
        evidence.dsl = Some(DslSelection::CustomLiteral("My::DSL".to_string()));
        let activation = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(!activation.is_exact());
        assert!(
            dancer2_route_facts(
                &detection,
                &activation,
                Some("App"),
                &[literal_get_declaration(0)]
            )
            .is_empty()
        );
    }
}
