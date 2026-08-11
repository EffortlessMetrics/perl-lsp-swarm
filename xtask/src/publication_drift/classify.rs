use super::authority::{
    default_required_invariants, validate_authority, validate_comparison_version, validate_subject,
};
use super::model::{
    APPROVED_EXCLUSION, AuthoritySource, ClassificationState, ClassifiedDifference,
    ClassifiedInvariant, EXPECTED_TRANSLATION, ManifestRule, NOT_PROVEN_CLASS, Observation,
    PRODUCT_DRIFT, RELEASE_METADATA, Receipt, SUPPORTED_SCHEMA_VERSION, ValidatedAuthority,
    Verdict,
};
use super::path::valid_repository_path;
use std::collections::BTreeSet;

pub(crate) fn classify(observation: Observation, authority_source: AuthoritySource) -> Receipt {
    let Observation { schema_version, swarm, public, manifest, differences, invariants } =
        observation;
    let mut state = ClassificationState::default();

    if schema_version != SUPPORTED_SCHEMA_VERSION {
        state.mark_not_proven(
            "unsupported_schema_version",
            format!(
                "unsupported schema version {schema_version}; expected {SUPPORTED_SCHEMA_VERSION}"
            ),
            "release-engineering",
        );
    }

    validate_subject("swarm", &swarm, &mut state);
    validate_subject("public", &public, &mut state);
    if swarm.repository == public.repository {
        state.mark_not_proven(
            "repository_identity_collision",
            format!(
                "swarm and public subjects identify the same repository {:?}",
                swarm.repository
            ),
            "release-engineering",
        );
    }
    let comparison_version = validate_comparison_version(&swarm, &public, &mut state);

    let (authority, manifest_verification) = validate_authority(
        manifest.as_ref(),
        &authority_source,
        &swarm,
        &public,
        comparison_version.as_deref(),
        &mut state,
    );

    let observed_differences = match differences {
        Some(differences) => differences,
        None => {
            state.mark_not_proven(
                "differences_collection_missing",
                "observation omitted required differences collection",
                "release-engineering",
            );
            Vec::new()
        }
    };
    let observed_invariants = match invariants {
        Some(invariants) => invariants,
        None => {
            state.mark_not_proven(
                "invariants_collection_missing",
                "observation omitted required invariants collection",
                "release-engineering",
            );
            Vec::new()
        }
    };

    let classified_differences =
        classify_differences(observed_differences, authority.as_ref(), &mut state);
    let classified_invariants =
        classify_invariants(observed_invariants, authority.as_ref(), &mut state);

    if state.drift {
        if let Some(version) = comparison_version.as_deref() {
            state.push_blocker(
                "same_version_divergent_product",
                format!("version {version} has behavior or invariant drift"),
                "release-engineering",
            );
        }
    }

    state.blockers.sort();
    state.blockers.dedup();
    let authority_valid = authority.is_some() && comparison_version.is_some() && !state.not_proven;
    let verdict = if state.not_proven {
        Verdict::NotProven
    } else if state.drift {
        Verdict::Drift
    } else {
        Verdict::Clean
    };

    Receipt {
        schema_version: SUPPORTED_SCHEMA_VERSION,
        comparison_version,
        swarm,
        public,
        manifest,
        manifest_verification,
        differences: classified_differences,
        invariants: classified_invariants,
        authority_valid,
        blockers: state.blockers,
        verdict,
    }
}

fn classify_differences(
    observed: Vec<super::model::ObservedDifference>,
    authority: Option<&ValidatedAuthority>,
    state: &mut ClassificationState,
) -> Vec<ClassifiedDifference> {
    let mut seen_paths = BTreeSet::new();
    let mut classified = Vec::new();

    for difference in observed {
        validate_owner("difference", &difference.path, &difference.owner, state);
        if !valid_repository_path(&difference.path) {
            state.mark_not_proven(
                "invalid_difference_path",
                format!(
                    "difference path must use canonical repository-relative syntax: {:?}",
                    difference.path
                ),
                owner_or_default(&difference.owner),
            );
        }
        if !seen_paths.insert(difference.path.clone()) {
            state.mark_not_proven(
                "duplicate_difference_path",
                format!("difference path appears more than once: {:?}", difference.path),
                owner_or_default(&difference.owner),
            );
        }
        validate_evidence(
            "difference",
            &difference.path,
            &difference.evidence,
            &difference.owner,
            state,
        );

        let mut effective = difference.classification.clone();
        if !allowed_classification(&difference.classification) {
            effective = NOT_PROVEN_CLASS.to_string();
            state.mark_not_proven(
                "unknown_difference_classification",
                format!(
                    "difference {:?} has unknown classification {:?}",
                    difference.path, difference.classification
                ),
                owner_or_default(&difference.owner),
            );
        }

        let rule_authorized = validate_manifest_rule(&difference, authority, state);
        if requires_manifest_rule(&difference.classification) && !rule_authorized {
            effective = NOT_PROVEN_CLASS.to_string();
        }

        if difference.behavior_changed && effective != PRODUCT_DRIFT {
            effective = PRODUCT_DRIFT.to_string();
            state.mark_drift(
                "behavioral_translation_is_product_drift",
                format!(
                    "difference {:?} changes behavior and cannot be accepted as {:?}",
                    difference.path, difference.classification
                ),
                owner_or_default(&difference.owner),
            );
        }

        match effective.as_str() {
            PRODUCT_DRIFT => state.mark_drift(
                "product_drift",
                format!("difference {:?} is classified as product drift", difference.path),
                owner_or_default(&difference.owner),
            ),
            NOT_PROVEN_CLASS => state.mark_not_proven(
                "difference_not_proven",
                format!("difference {:?} is not proven", difference.path),
                owner_or_default(&difference.owner),
            ),
            EXPECTED_TRANSLATION | APPROVED_EXCLUSION | RELEASE_METADATA => {}
            other => state.mark_not_proven(
                "unclassifiable_difference",
                format!(
                    "difference {:?} resolved to unrecognized effective classification {other:?}",
                    difference.path
                ),
                owner_or_default(&difference.owner),
            ),
        }

        classified.push(ClassifiedDifference {
            path: difference.path,
            declared_classification: difference.classification,
            effective_classification: effective,
            behavior_changed: difference.behavior_changed,
            manifest_rule: difference.manifest_rule,
            owner: difference.owner,
            evidence: difference.evidence,
        });
    }

    classified.sort_by(|left, right| left.path.cmp(&right.path));
    classified
}

fn validate_manifest_rule(
    difference: &super::model::ObservedDifference,
    authority: Option<&ValidatedAuthority>,
    state: &mut ClassificationState,
) -> bool {
    if requires_manifest_rule(&difference.classification) {
        let Some(rule_id) =
            difference.manifest_rule.as_deref().filter(|rule| !rule.trim().is_empty())
        else {
            state.mark_not_proven(
                "manifest_rule_missing",
                format!(
                    "difference {:?} is declared {:?} without a manifest rule",
                    difference.path, difference.classification
                ),
                owner_or_default(&difference.owner),
            );
            return false;
        };

        let Some(authority) = authority else {
            state.mark_not_proven(
                "manifest_rule_not_resolved",
                format!(
                    "difference {:?} references manifest rule {:?} without verified authority",
                    difference.path, rule_id
                ),
                owner_or_default(&difference.owner),
            );
            return false;
        };
        let Some(rule) = authority.rules.get(rule_id) else {
            state.mark_not_proven(
                "manifest_rule_unknown",
                format!(
                    "difference {:?} references unknown manifest rule {:?}",
                    difference.path, rule_id
                ),
                owner_or_default(&difference.owner),
            );
            return false;
        };
        return validate_rule_match(difference, rule, state);
    }

    if difference.manifest_rule.as_deref().is_some_and(|rule| !rule.trim().is_empty()) {
        state.mark_not_proven(
            "manifest_rule_not_applicable",
            format!(
                "difference {:?} with classification {:?} must not claim an acceptance rule",
                difference.path, difference.classification
            ),
            owner_or_default(&difference.owner),
        );
        return false;
    }
    true
}

fn validate_rule_match(
    difference: &super::model::ObservedDifference,
    rule: &ManifestRule,
    state: &mut ClassificationState,
) -> bool {
    let mut authorized = true;
    if rule.path != difference.path {
        authorized = false;
        state.mark_not_proven(
            "manifest_rule_path_mismatch",
            format!(
                "manifest rule {:?} authorizes path {:?}, not {:?}",
                rule.id, rule.path, difference.path
            ),
            owner_or_default(&difference.owner),
        );
    }
    if rule.classification != difference.classification {
        authorized = false;
        state.mark_not_proven(
            "manifest_rule_classification_mismatch",
            format!(
                "manifest rule {:?} authorizes classification {:?}, not {:?}",
                rule.id, rule.classification, difference.classification
            ),
            owner_or_default(&difference.owner),
        );
    }
    if rule.owner != difference.owner {
        authorized = false;
        state.mark_not_proven(
            "manifest_rule_owner_mismatch",
            format!(
                "manifest rule {:?} is owned by {:?}, not {:?}",
                rule.id, rule.owner, difference.owner
            ),
            owner_or_default(&difference.owner),
        );
    }
    authorized
}

fn classify_invariants(
    observed: Vec<super::model::ObservedInvariant>,
    authority: Option<&ValidatedAuthority>,
    state: &mut ClassificationState,
) -> Vec<ClassifiedInvariant> {
    let required = authority
        .map(|authority| authority.required_invariants.clone())
        .unwrap_or_else(default_required_invariants);
    let mut seen = BTreeSet::new();
    let mut classified = Vec::new();

    for invariant in observed {
        validate_owner("invariant", &invariant.id, &invariant.owner, state);
        if invariant.id.trim().is_empty() {
            state.mark_not_proven(
                "empty_invariant_id",
                "invariant id is empty",
                owner_or_default(&invariant.owner),
            );
        }
        if !seen.insert(invariant.id.clone()) {
            state.mark_not_proven(
                "duplicate_invariant",
                format!("invariant appears more than once: {:?}", invariant.id),
                owner_or_default(&invariant.owner),
            );
        }
        validate_evidence("invariant", &invariant.id, &invariant.evidence, &invariant.owner, state);

        match required.get(&invariant.id) {
            Some(expected_owner) if expected_owner != &invariant.owner => {
                state.mark_not_proven(
                    "invariant_owner_mismatch",
                    format!(
                        "invariant {:?} is owned by {:?}, expected {:?}",
                        invariant.id, invariant.owner, expected_owner
                    ),
                    owner_or_default(&invariant.owner),
                );
            }
            Some(_) => {}
            None => {
                state.mark_not_proven(
                    "unknown_invariant",
                    format!(
                        "invariant {:?} is not declared by the comparison authority",
                        invariant.id
                    ),
                    owner_or_default(&invariant.owner),
                );
            }
        }

        match invariant.status.as_str() {
            "pass" => {}
            "fail" => state.mark_drift(
                "invariant_failed",
                format!("invariant {:?} failed", invariant.id),
                owner_or_default(&invariant.owner),
            ),
            "not_proven" => state.mark_not_proven(
                "invariant_not_proven",
                format!("invariant {:?} is not proven", invariant.id),
                owner_or_default(&invariant.owner),
            ),
            other => state.mark_not_proven(
                "unknown_invariant_status",
                format!("invariant {:?} has unknown status {:?}", invariant.id, other),
                owner_or_default(&invariant.owner),
            ),
        }

        classified.push(ClassifiedInvariant {
            id: invariant.id,
            status: invariant.status,
            owner: invariant.owner,
            evidence: invariant.evidence,
        });
    }

    for (required_id, required_owner) in &required {
        if !seen.contains(required_id) {
            state.mark_not_proven(
                "required_invariant_missing",
                format!("required publication invariant {required_id:?} is absent"),
                required_owner.as_str(),
            );
        }
    }

    classified.sort_by(|left, right| left.id.cmp(&right.id));
    classified
}

fn validate_owner(kind: &str, identity: &str, owner: &str, state: &mut ClassificationState) {
    if owner.trim().is_empty() {
        state.mark_not_proven(
            "owner_missing",
            format!("{kind} {identity:?} has no owner"),
            "release-engineering",
        );
    }
}

fn validate_evidence(
    kind: &str,
    identity: &str,
    evidence: &[String],
    owner: &str,
    state: &mut ClassificationState,
) {
    if evidence.is_empty() || evidence.iter().any(|entry| entry.trim().is_empty()) {
        state.mark_not_proven(
            "evidence_missing",
            format!("{kind} {identity:?} has missing or empty evidence"),
            owner_or_default(owner),
        );
        return;
    }

    let unique = evidence.iter().map(|entry| entry.trim()).collect::<BTreeSet<_>>();
    if unique.len() != evidence.len() {
        state.mark_not_proven(
            "duplicate_evidence",
            format!("{kind} {identity:?} repeats evidence entries"),
            owner_or_default(owner),
        );
    }
}

fn owner_or_default(owner: &str) -> &str {
    if owner.trim().is_empty() { "release-engineering" } else { owner }
}

fn allowed_classification(classification: &str) -> bool {
    matches!(
        classification,
        EXPECTED_TRANSLATION
            | APPROVED_EXCLUSION
            | RELEASE_METADATA
            | PRODUCT_DRIFT
            | NOT_PROVEN_CLASS
    )
}

fn requires_manifest_rule(classification: &str) -> bool {
    matches!(classification, EXPECTED_TRANSLATION | APPROVED_EXCLUSION | RELEASE_METADATA)
}
