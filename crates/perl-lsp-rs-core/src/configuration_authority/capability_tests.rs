//! Synthetic CA02A model fixtures, not production field classifications or effect proof.
//! Existing execution-plan/timeout IDs exercise laws only. No canonical test-runner
//! rows exist yet; positive test-runner binding/population belongs to #10801.
use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn require(value: bool, message: &str) -> TestResult {
    if value { Ok(()) } else { Err(message.into()) }
}
fn row(id: &str, role: CapabilityRole, effect: ExternalEffect) -> CapabilityDeclaration {
    CapabilityDeclaration {
        field_id: id.to_owned(),
        roles: vec![role],
        effects: vec![effect],
        composition: vec![],
        proof: Some(ProofRequirement::FirstEffect {
            owner: "fixture-owner".into(),
            negative_control_id: "fixture-negative".into(),
        }),
        reduction_exception: None,
    }
}
fn accepted(rows: &[CapabilityDeclaration]) -> TestResult {
    validate_capabilities(rows)
        .map(|_| ())
        .map_err(|error| format!("unexpected rejection: {error:?}").into())
}
fn rejected(rows: &[CapabilityDeclaration], kind: CapabilityViolationKind) -> TestResult {
    match validate_capabilities(rows) {
        Err(error) => {
            require(error.kind == kind, &format!("wrong rejection: {error:?}, expected {kind:?}"))?;
            if rows.len() <= MAX_DECLARATIONS {
                let expected_id = rows
                    .iter()
                    .map(|row| row.field_id.as_str())
                    .min()
                    .ok_or("empty rejection fixture")?;
                require(error.field_id == expected_id, "wrong diagnostic field identity")?;
            }
            Ok(())
        }
        Ok(_) => Err(format!("accepted invalid fixture: {kind:?}").into()),
    }
}
fn fixtures() -> Vec<CapabilityDeclaration> {
    use CapabilityRole as R;
    use ExternalEffect as E;
    let mut formatter = row("formatting.engine", R::Select, E::Process);
    formatter
        .composition
        .push(CompositionRule::ComposesWith { field: "formatting.format_on_save".into() });
    let mut save = row("formatting.format_on_save", R::Trigger, E::Process);
    save.composition = vec![
        CompositionRule::RequiresAuthorityFrom { field: "formatting.engine".into() },
        CompositionRule::AutomaticLifecycleTrigger,
    ];
    let mut enable = row("ai.user_enabled", R::Arm, E::Network);
    enable.composition.push(CompositionRule::RequiresAuthorityFrom { field: "ai.endpoint".into() });
    let mut reduction = row("ai.project_opt_out", R::Reduce, E::Network);
    reduction.composition = vec![
        CompositionRule::MayOnlyReduce,
        CompositionRule::RequiresAuthorityFrom { field: "ai.user_enabled".into() },
    ];
    let mut budget = row("limits.file_size_bytes", R::Reduce, E::ResourceBudget);
    budget
        .composition
        .push(CompositionRule::HardProductEnvelope { owner: "fixture-product-limit".into() });
    let mut timeout = row("workspace.resolution_timeout_ms", R::Reduce, E::ResourceBudget);
    timeout
        .composition
        .push(CompositionRule::HardProductEnvelope { owner: "fixture-resolver-limit".into() });
    let mut retired = row("workspace.perl_args", R::Select, E::Process);
    retired.proof = Some(ProofRequirement::Removed {
        owner: "fixture-tool-policy".into(),
        reason: "rejected execution-plan configuration".into(),
    });
    vec![
        formatter,
        save,
        row("ai.endpoint", R::Select, E::Network),
        row("ai.api_key_env", R::Select, E::CredentialUse),
        enable,
        reduction,
        budget,
        timeout,
        retired,
        row("workspace.external_include_paths", R::Select, E::ExternalFilesystemRead),
    ]
}
#[test]
fn staged_fixtures_resolve_canonical_rows_without_rewriting() -> TestResult {
    let rows = fixtures();
    let before = serde_json::to_string(&rows)?;
    let checked = validate_capabilities(&rows).map_err(|error| format!("{error:?}"))?;
    for binding in checked {
        let canonical =
            authority_by_id(&binding.declaration.field_id).ok_or("missing canonical row")?;
        require(std::ptr::eq(canonical, binding.field), "copied canonical field")?;
        require(
            rows.iter().any(|input| std::ptr::eq(input, binding.declaration)),
            "copied or repaired declaration",
        )?;
    }
    require(before == serde_json::to_string(&rows)?, "validation changed declaration")?;
    let decoded: Vec<CapabilityDeclaration> = serde_json::from_str(&before)?;
    require(rows == decoded, "serialization lost declaration")?;
    require(before == serde_json::to_string(&decoded)?, "serialization unstable")?;
    let mut reversed = rows.clone();
    reversed.reverse();
    let ids = |set: &[CapabilityDeclaration]| -> Result<Vec<String>, String> {
        validate_capabilities(set)
            .map(|checked| checked.iter().map(|item| item.field.id.to_owned()).collect())
            .map_err(|error| format!("{error:?}"))
    };
    require(ids(&rows)? == ids(&reversed)?, "checked ordering depends on input order")
}
#[test]
fn independent_effect_proof_and_envelope_rejections() -> TestResult {
    for effect in [
        ExternalEffect::Process,
        ExternalEffect::Network,
        ExternalEffect::ExternalFilesystemRead,
        ExternalEffect::WorkspaceWrite,
        ExternalEffect::CredentialUse,
        ExternalEffect::PresentationOnly,
    ] {
        let mut sample = row("inlay.enabled", CapabilityRole::Project, effect);
        sample.proof = None;
        rejected(&[sample], CapabilityViolationKind::MissingProof)?;
    }
    for (owner, control) in [("", "control"), ("owner", "")] {
        let mut sample = row("ai.endpoint", CapabilityRole::Select, ExternalEffect::Network);
        sample.proof = Some(ProofRequirement::FirstEffect {
            owner: owner.into(),
            negative_control_id: control.into(),
        });
        rejected(&[sample], CapabilityViolationKind::InvalidProof)?;
    }
    let sample =
        row("limits.file_size_bytes", CapabilityRole::Reduce, ExternalEffect::ResourceBudget);
    rejected(&[sample], CapabilityViolationKind::MissingEnvelope)
}
#[test]
fn reduction_cannot_strengthen_without_reviewed_exception() -> TestResult {
    for role in [
        CapabilityRole::Select,
        CapabilityRole::Arm,
        CapabilityRole::Trigger,
        CapabilityRole::Widen,
    ] {
        let mut sample = row("ai.project_opt_out", role, ExternalEffect::Network);
        sample.composition.push(CompositionRule::MayOnlyReduce);
        rejected(&[sample.clone()], CapabilityViolationKind::ReductionStrengthens)?;
        sample.reduction_exception = Some(ReviewedException {
            owner: "fixture".into(),
            review_ref: "issue:fixture".into(),
            reason: "explicit fixture exception".into(),
        });
        accepted(&[sample.clone()])?;
        if let Some(exception) = &mut sample.reduction_exception {
            exception.review_ref.clear();
        }
        rejected(&[sample], CapabilityViolationKind::InvalidException)?;
    }
    Ok(())
}
#[test]
fn composition_checks_identity_and_only_authority_cycles() -> TestResult {
    let mut left = row("ai.endpoint", CapabilityRole::Select, ExternalEffect::Network);
    let mut right = row("ai.user_enabled", CapabilityRole::Arm, ExternalEffect::Network);
    left.composition.push(CompositionRule::ComposesWith { field: right.field_id.clone() });
    right.composition.push(CompositionRule::ComposesWith { field: left.field_id.clone() });
    accepted(&[left.clone(), right.clone()])?;
    left.composition =
        vec![CompositionRule::RequiresAuthorityFrom { field: right.field_id.clone() }];
    right.composition =
        vec![CompositionRule::RequiresAuthorityFrom { field: left.field_id.clone() }];
    rejected(&[left.clone(), right], CapabilityViolationKind::AuthorityCycle)?;
    rejected(&[left.clone()], CapabilityViolationKind::UnclassifiedAuthorityTarget)?;
    left.composition = vec![CompositionRule::ComposesWith { field: "missing.field".into() }];
    rejected(&[left], CapabilityViolationKind::UnknownTarget)
}
#[test]
fn unsupported_removed_and_no_effect_are_explicit() -> TestResult {
    for proof in [
        ProofRequirement::Unsupported {
            owner: "owner".into(),
            reason: "unsupported fixture".into(),
        },
        ProofRequirement::Removed { owner: "owner".into(), reason: "removed fixture".into() },
    ] {
        let mut sample =
            row("workspace.perl_args", CapabilityRole::Select, ExternalEffect::Process);
        sample.proof = Some(proof);
        accepted(&[sample])?;
    }
    accepted(&[row("inlay.enabled", CapabilityRole::Project, ExternalEffect::PresentationOnly)])?;
    let mut sample = row("inlay.enabled", CapabilityRole::Project, ExternalEffect::None);
    sample.proof = None;
    accepted(&[sample.clone()])?;
    sample.effects.push(ExternalEffect::Process);
    rejected(&[sample], CapabilityViolationKind::InvalidEffects)
}
#[test]
fn malformed_and_unknown_active_classification_is_rejected() -> TestResult {
    let base = row("ai.endpoint", CapabilityRole::Select, ExternalEffect::Network);
    let mut sample = base.clone();
    sample.roles.clear();
    rejected(&[sample], CapabilityViolationKind::InvalidRoles)?;
    let mut sample = base.clone();
    sample.effects.clear();
    rejected(&[sample], CapabilityViolationKind::InvalidEffects)?;
    let mut sample = base.clone();
    sample.roles.push(CapabilityRole::Select);
    rejected(&[sample], CapabilityViolationKind::InvalidRoles)?;
    let mut sample = base.clone();
    sample.effects = vec![ExternalEffect::PresentationOnly];
    rejected(&[sample], CapabilityViolationKind::PlannerEffectMismatch)?;
    rejected(&[base.clone(), base.clone()], CapabilityViolationKind::DuplicateDeclaration)?;
    let encoded = serde_json::to_string(&base)?;
    require(
        serde_json::from_str::<CapabilityDeclaration>(
            &encoded.replace("\"select\"", "\"unknown\""),
        )
        .is_err(),
        "unknown role accepted",
    )?;
    require(
        serde_json::from_str::<CapabilityDeclaration>(
            &encoded.replace("\"network\"", "\"unknown\""),
        )
        .is_err(),
        "unknown effect accepted",
    )?;
    let unknown = row("testing.plan", CapabilityRole::Select, ExternalEffect::Process);
    rejected(&[unknown], CapabilityViolationKind::UnknownField)
}

#[test]
fn collections_are_bounded_unique_and_unknown_structure_is_rejected() -> TestResult {
    let base = row("ai.endpoint", CapabilityRole::Select, ExternalEffect::Network);
    rejected(&vec![base.clone(); MAX_DECLARATIONS + 1], CapabilityViolationKind::CollectionBounds)?;
    let mut duplicate_effect = base.clone();
    duplicate_effect.effects.push(ExternalEffect::Network);
    rejected(&[duplicate_effect], CapabilityViolationKind::InvalidEffects)?;
    let mut duplicate_rule = base.clone();
    duplicate_rule.composition = vec![CompositionRule::ExplicitUserActionRequired; 2];
    rejected(&[duplicate_rule], CapabilityViolationKind::DuplicateComposition)?;
    let mut oversized = base.clone();
    oversized.composition = vec![CompositionRule::ExplicitUserActionRequired; MAX_COMPOSITION + 1];
    rejected(&[oversized], CapabilityViolationKind::CollectionBounds)?;
    let mut invalid_envelope =
        row("limits.file_size_bytes", CapabilityRole::Reduce, ExternalEffect::ResourceBudget);
    invalid_envelope.composition.push(CompositionRule::HardProductEnvelope { owner: " ".into() });
    rejected(&[invalid_envelope], CapabilityViolationKind::MissingEnvelope)?;
    let mut mixed = base.clone();
    mixed.effects.push(ExternalEffect::PresentationOnly);
    rejected(&[mixed], CapabilityViolationKind::InvalidEffects)?;
    let mut encoded = serde_json::to_value(&base)?;
    encoded
        .as_object_mut()
        .ok_or("fixture is not an object")?
        .insert("invented".into(), true.into());
    require(
        serde_json::from_value::<CapabilityDeclaration>(encoded).is_err(),
        "unknown declaration property accepted",
    )?;
    // No canonical test-runner rows exist yet. CA02B owns their positive binding.
    // These exercise serialization, not a claim that test-runner authority is populated.
    for id in ["testing.plan", "testing.timeout"] {
        let declaration = row(id, CapabilityRole::Select, ExternalEffect::Process);
        let encoded = serde_json::to_string(&declaration)?;
        let decoded = serde_json::from_str::<CapabilityDeclaration>(&encoded)?;
        rejected(&[decoded], CapabilityViolationKind::UnknownField)?;
    }
    Ok(())
}

#[test]
fn contradictory_planner_claims_and_invalid_dispositions_are_rejected() -> TestResult {
    for effect in [ExternalEffect::None, ExternalEffect::PresentationOnly] {
        let sample = row("ai.endpoint", CapabilityRole::Project, effect);
        rejected(&[sample], CapabilityViolationKind::PlannerEffectMismatch)?;
    }
    for proof in [
        ProofRequirement::Unsupported { owner: "".into(), reason: "fixture".into() },
        ProofRequirement::Removed { owner: "owner".into(), reason: " ".into() },
    ] {
        let mut sample =
            row("workspace.perl_args", CapabilityRole::Select, ExternalEffect::Process);
        sample.proof = Some(proof);
        rejected(&[sample], CapabilityViolationKind::InvalidProof)?;
    }
    let mut self_cycle = row("ai.endpoint", CapabilityRole::Select, ExternalEffect::Network);
    self_cycle
        .composition
        .push(CompositionRule::RequiresAuthorityFrom { field: self_cycle.field_id.clone() });
    rejected(&[self_cycle], CapabilityViolationKind::AuthorityCycle)
}

#[test]
fn reduce_role_itself_requires_exception_even_without_marker() -> TestResult {
    for strength in [
        CapabilityRole::Select,
        CapabilityRole::Arm,
        CapabilityRole::Trigger,
        CapabilityRole::Widen,
    ] {
        let mut sample = row("ai.project_opt_out", CapabilityRole::Reduce, ExternalEffect::Network);
        sample.roles.push(strength);
        rejected(&[sample.clone()], CapabilityViolationKind::ReductionStrengthens)?;
        sample.reduction_exception = Some(ReviewedException {
            owner: "fixture".into(),
            review_ref: "fixture-reviewed-exception".into(),
            reason: "explicit multi-role fixture".into(),
        });
        accepted(&[sample])?;
    }
    Ok(())
}
#[test]
fn canonical_legacy_critic_planners_cannot_claim_presentation_or_none() -> TestResult {
    for id in ["critic.legacy_profile", "critic.legacy_theme"] {
        let field = authority_by_id(id).ok_or("missing canonical legacy critic row")?;
        require(
            field.consumers.contains(&ConfigConsumer::LegacyCritic),
            "fixture no longer targets legacy planner",
        )?;
        for effect in [ExternalEffect::None, ExternalEffect::PresentationOnly] {
            rejected(
                &[row(id, CapabilityRole::Project, effect)],
                CapabilityViolationKind::PlannerEffectMismatch,
            )?;
        }
    }
    Ok(())
}
