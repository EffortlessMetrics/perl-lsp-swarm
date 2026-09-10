//! Issue #9403: observations must not turn unknown input into conclusive controls.
use super::*;
use serde_json::{Value, json};

fn fixture_mut<'a>(
    value: &'a mut Value,
    pointer: &str,
) -> Result<&'a mut Value, Box<dyn std::error::Error>> {
    value
        .pointer_mut(pointer)
        .ok_or_else(|| format!("fixture is missing JSON pointer {pointer}").into())
}

fn fixture_object_mut<'a>(
    value: &'a mut Value,
    pointer: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, Box<dyn std::error::Error>> {
    fixture_mut(value, pointer)?
        .as_object_mut()
        .ok_or_else(|| format!("fixture JSON pointer {pointer} is not an object").into())
}

fn require(condition: bool, message: &str) -> TestResult {
    if condition { Ok(()) } else { Err(message.to_string().into()) }
}

#[test]
fn subject_branch_override_preserves_valid_names_and_rejects_empty_input() -> TestResult {
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join("policy"))?;
    std::fs::write(
        root.path().join("policy/product-identity.toml"),
        "[product]\ndevelopment_repository = 'owner/development'\npublic_repository = 'owner/public'\n",
    )?;
    for branch in ["main", "release/1.0"] {
        let subjects = xtask::release_live_controls::subjects_from_product_identity(
            root.path(),
            Some(branch),
        )?;
        require(subjects.len() == 2, "both governed repositories must be retained")?;
        require(
            subjects.iter().all(|subject| subject.branch == branch),
            "valid override must be preserved for both repositories",
        )?;
    }
    require(
        xtask::release_live_controls::subjects_from_product_identity(root.path(), Some(""))
            .is_err(),
        "empty branch must be rejected before it can enter a receipt",
    )
}

#[test]
fn empty_explicit_branch_preserves_existing_receipt() -> TestResult {
    let root = tempfile::tempdir()?;
    let output = root.path().join("receipt.json");
    std::fs::write(&output, "existing receipt")?;
    let result = xtask::release_live_controls::run(xtask::release_live_controls::ObserveOptions {
        repo_root: root.path().to_path_buf(),
        repositories: vec!["owner/repository".into()],
        branch: Some(String::new()),
        out: Some(output.clone()),
        json: false,
    });
    let error = result.err().ok_or("empty explicit branch must fail admission")?;
    require(error.to_string() == "branch must not be empty", "error must identify branch input")?;
    require(
        std::fs::read_to_string(output)? == "existing receipt",
        "rejected input must not replace an existing receipt",
    )
}

#[test]
fn bracket_patterns_select_the_branch() -> TestResult {
    let conditions = json!({"ref_name":{"include":["refs/heads/release/[0-9]*"],"exclude":[]}});
    require(
        ruleset_applies_to_branch(Some(&conditions), "release/1.0", Some("main")).value()
            == Some(&true),
        "GitHub bracket range must not silently exclude the release branch",
    )
}

#[test]
fn inactive_unknown_applicability_does_not_poison_required_union() -> TestResult {
    for enforcement in ["disabled", "evaluate"] {
        let ruleset = Ruleset {
            id: 1,
            name: "inactive".into(),
            target: "branch".into(),
            enforcement: enforcement.into(),
            applies_to_branch: Observed::not_proven("unreadable"),
            bypass_actors: Observed::not_proven("unreadable"),
            rules: Observed::not_proven("unreadable"),
        };
        let union =
            required_contexts_union(&conclusive_classic(), &Observed::observed(vec![ruleset]));
        require(
            union.state == ObservationState::Observed,
            "known inactive ruleset must not block active context union",
        )?;
    }
    Ok(())
}

#[test]
fn immutability_is_read_from_dedicated_endpoint() -> TestResult {
    let path = format!("repos/{OWNER}/{NAME}/immutable-releases");
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET")
        .on(&path, r#"{"enabled":false,"enforced_by_owner":false}"#);
    let receipt = observe(&commands, &[subject(OWNER, NAME)], "test".into());
    let repository = receipt.repositories.first().ok_or("missing repository")?;
    require(
        repository.release_posture.immutable_releases.value() == Some(&false),
        "ordinary repository payload is not immutable-release authority",
    )?;
    require(
        commands.calls.borrow().iter().filter(|call| *call == &path).count() == 1,
        "dedicated immutable endpoint must be read once",
    )
}

#[test]
fn omitted_classic_control_is_not_corroborated_absence() -> TestResult {
    let path = format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection");
    for field in ["required_status_checks", "required_pull_request_reviews", "restrictions"] {
        let mut commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET");
        let body = commands
            .responses
            .get(&path)
            .ok_or("missing fixture")?
            .as_ref()
            .map_err(|_| "unreadable fixture")?;
        let mut value: Value = serde_json::from_str(body)?;
        value.as_object_mut().ok_or("fixture not object")?.remove(field);
        commands = commands.on(&path, &value.to_string());
        let protection = collect_classic_protection(&commands, OWNER, NAME, BRANCH);
        let protection = protection.value().ok_or("expected per-control observation")?;
        let state = match field {
            "required_status_checks" => protection.required_status_checks.state,
            "required_pull_request_reviews" => protection.required_pull_request_reviews.state,
            _ => protection.restrictions_present.state,
        };
        require(state == ObservationState::NotProven, &format!("omitted {field} is unknown"))?;
    }
    Ok(())
}

#[test]
fn malformed_environment_rule_is_not_observed() -> TestResult {
    require(
        environment_with(environment_fixture(), None, None)?.protection_rules.is_observed(),
        "healthy environment rules must be observed",
    )?;
    for row in [
        json!(null),
        json!({}),
        json!({"type":"new_unsupported_kind"}),
        json!({"type":"required_reviewers","reviewers":[{}],"prevent_self_review":true}),
        json!({"type":"wait_timer","wait_timer":"ten"}),
    ] {
        let mut detail = environment_fixture();
        *fixture_mut(&mut detail, "/protection_rules")? = json!([row]);
        let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET")
            .on(&format!("repos/{OWNER}/{NAME}/environments/release"), &detail.to_string());
        let environments = collect_environments(&commands, OWNER, NAME);
        let environment =
            environments.value().and_then(|rows| rows.first()).ok_or("missing environment")?;
        require(
            environment.protection_rules.state == ObservationState::NotProven,
            "malformed environment rule must not produce a conclusive row",
        )?;
    }
    Ok(())
}

fn environment_fixture() -> Value {
    json!({"id":1,"node_id":"E_release","name":"release","protection_rules":[],"deployment_branch_policy":null})
}

fn environment_with(
    detail: Value,
    custom: Option<Value>,
    named: Option<Value>,
) -> Result<Environment, Box<dyn std::error::Error>> {
    let base = format!("repos/{OWNER}/{NAME}/environments/release");
    let mut commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SENTINEL_SECRET")
        .on(&base, &detail.to_string());
    if let Some(value) = custom {
        commands = commands.on(&format!("{base}/deployment_protection_rules"), &value.to_string());
    }
    if let Some(value) = named {
        commands = commands.on(
            &format!("{base}/deployment-branch-policies?per_page=100&page=1"),
            &value.to_string(),
        );
    }
    collect_environments(&commands, OWNER, NAME)
        .value()
        .and_then(|rows| rows.first())
        .cloned()
        .ok_or_else(|| "missing environment".into())
}

#[test]
fn null_deployment_policy_round_trips_as_unrestricted_absence() -> TestResult {
    let row = environment_with(environment_fixture(), None, None)?;
    require(
        row.deployment_branch_policy.state == ObservationState::Absent,
        "API null means explicitly unrestricted",
    )?;
    require(
        row.deployment_branch_policies.state == ObservationState::Absent,
        "unrestricted environment has no named policies",
    )?;
    let mut repo = admissible_repository();
    repo.environments = Observed::observed(vec![row]);
    let receipt = LiveControlsReceipt {
        schema_version: RELEASE_LIVE_CONTROLS_SCHEMA_VERSION.into(),
        observed_at: "2026-09-09T00:00:00Z".into(),
        currency: Currency::Live,
        instrument: Instrument {
            state: ObservationState::Observed,
            gh_version: Some("gh version test".into()),
            detail: None,
        },
        repositories: vec![repo],
        verdict: Verdict::Observed,
        limitations: vec![],
    };
    let encoded = serde_json::to_value(&receipt)?;
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/release_live_controls.v1.schema.json"
    ))?;
    let validator = jsonschema::validator_for(&schema)?;
    require(validator.is_valid(&encoded), "API null-origin receipt must satisfy schema")?;
    let decoded: LiveControlsReceipt = serde_json::from_value(encoded)?;
    require(
        decoded.structural_problem().is_none(),
        "null-origin receipt must remain structurally sound after serde replay",
    )
}

#[test]
fn named_and_installed_deployment_controls_have_typed_metadata() -> TestResult {
    let mut detail = environment_fixture();
    *fixture_mut(&mut detail, "/deployment_branch_policy")? =
        json!({"protected_branches":false,"custom_branch_policies":true});
    *fixture_mut(&mut detail, "/protection_rules")? = json!([{"id":11,"node_id":"PR_11","type":"required_reviewers","prevent_self_review":true,
        "reviewers":[{"type":"Team","reviewer":{"id":42,"node_id":"T_42"}}]}]);
    let named = json!({"total_count":2,"branch_policies":[
        {"id":21,"node_id":"BP_21","name":"release/*","type":"branch"},
        {"id":22,"node_id":"BP_22","name":"v*","type":"tag"}]});
    let custom = json!({"total_count":1,"custom_deployment_protection_rules":[{"id":31,"node_id":"DPR_31","enabled":true,
        "app":{"id":99,"node_id":"APP_99","slug":"release-review","integration_url":"https://github.com/apps/release-review"}}]});
    let row = environment_with(detail.clone(), Some(custom.clone()), Some(named.clone()))?;
    require(
        row.deployment_branch_policies.value().is_some_and(|v| v.len() == 2),
        "both branch and tag policies must be retained",
    )?;
    require(
        row.custom_deployment_protection_rules.value().is_some_and(|v| v.len() == 1),
        "installed rule must be retained",
    )?;
    let serialized = serde_json::to_string(&row)?;
    require(
        serialized.contains("T_42") && serialized.contains("APP_99"),
        "typed reviewer and app identities must survive",
    )?;
    require(
        !serialized.contains("integration_url") && !serialized.contains("SENTINEL_SECRET"),
        "receipt excludes URL and secret metadata",
    )?;
    for bad in [
        json!({}),
        json!({"total_count":2,"custom_deployment_protection_rules":[]}),
        json!({"total_count":1,"custom_deployment_protection_rules":[{}]}),
    ] {
        let row = environment_with(detail.clone(), Some(bad), Some(named.clone()))?;
        require(
            row.custom_deployment_protection_rules.state == ObservationState::NotProven,
            "malformed installed rule listing stays unknown",
        )?;
    }
    for bad in [
        json!({"total_count":0,"branch_policies":[{}]}),
        json!({"total_count":1,"branch_policies":[{"id":21,"node_id":"BP_21","name":"v*","type":"unknown"}]}),
    ] {
        let row = environment_with(detail.clone(), Some(custom.clone()), Some(bad))?;
        require(
            row.deployment_branch_policies.state == ObservationState::NotProven,
            "malformed named policy listing stays unknown",
        )?;
    }
    Ok(())
}

#[test]
fn ref_matching_uses_pathname_semantics_and_rejects_unknown_syntax() -> TestResult {
    for (pattern, branch, expected) in [
        ("refs/heads/release/**", "release/a/b", false),
        ("refs/heads/release/**/*", "release/a/b", true),
        ("refs/heads/**/main", "main", true),
        ("refs/heads/a**/main", "a/b/main", false),
        ("refs/heads/release/[!a-c]?", "release/zé", true),
        ("refs/heads/release/?", "release/é", true),
    ] {
        let conditions = json!({"ref_name":{"include":[pattern],"exclude":[]}});
        require(
            ruleset_applies_to_branch(Some(&conditions), branch, Some("main")).value()
                == Some(&expected),
            &format!("pattern {pattern} must match Ruby pathname semantics"),
        )?;
    }
    for pattern in [
        "refs/heads/[abc",
        "refs/heads/[^a]",
        "refs/heads/a\\*",
        "refs/heads/{a,b}",
        "refs/heads/[!]a]",
        "refs/heads/[!]]",
        "~FUTURE",
    ] {
        let conditions = json!({"ref_name":{"include":[pattern],"exclude":[]}});
        require(
            ruleset_applies_to_branch(Some(&conditions), "main", Some("main")).state
                == ObservationState::NotProven,
            "unsupported pattern must not silently exclude",
        )?;
    }
    for conditions in [
        json!({"ref_name":{"include":["~ALL"]}}),
        json!({"ref_name":{"include":null,"exclude":[]}}),
    ] {
        require(
            ruleset_applies_to_branch(Some(&conditions), "main", Some("main")).state
                == ObservationState::NotProven,
            "missing condition arrays are unknown",
        )?;
    }
    Ok(())
}

#[test]
fn arbitrary_api_errors_are_redacted() -> TestResult {
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET").failing(
        &format!("repos/{OWNER}/{NAME}/environments/release/secrets"),
        Some(403),
        "SECRET_NAME_SENTINEL https://token:password@example.com",
    );
    let receipt = observe(&commands, &[subject(OWNER, NAME)], "test".into());
    let json = serde_json::to_string(&receipt)?;
    require(
        !json.contains("SECRET_NAME_SENTINEL") && !json.contains("password@example"),
        "raw upstream diagnostic must not leak into receipt",
    )?;
    require(receipt.verdict == Verdict::NotProven, "redaction preserves inconclusive result")
}

#[test]
fn branch_and_repository_identity_are_required_for_conclusive_controls() -> TestResult {
    let healthy = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET");
    require(
        collect_classic_protection(&healthy, OWNER, NAME, BRANCH).is_observed(),
        "matching branch is the positive control",
    )?;
    for value in [json!({"name":"other","protected":false}), json!({"protected":false})] {
        for missing_protection in [false, true] {
            let mut commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET")
                .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}"), &value.to_string());
            if missing_protection {
                commands = commands.failing(
                    &format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"),
                    Some(404),
                    "missing",
                );
            }
            require(
                collect_classic_protection(&commands, OWNER, NAME, BRANCH).state
                    == ObservationState::NotProven,
                "wrong or missing branch identity cannot establish presence or absence",
            )?;
        }
    }
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET").on(
        &format!("repos/{OWNER}/{NAME}"),
        &json!({"full_name":format!("{OWNER}/{NAME}"),"node_id":"R_1","id":1,"default_branch":""})
            .to_string(),
    );
    let receipt = observe(&commands, &[subject(OWNER, NAME)], "test".into());
    require(
        receipt.repositories.first().ok_or("missing repository")?.identity.state
            == ObservationState::NotProven,
        "empty default branch is not an observed identity",
    )
}

#[test]
fn ruleset_bindings_and_review_fields_are_retained_or_unknown() -> TestResult {
    let path = format!("repos/{OWNER}/{NAME}/rulesets/1");
    let parse = |rule: Value| -> Result<Observed<Vec<RulesetRule>>, Box<dyn std::error::Error>> {
        let detail = json!({"id":1,"name":"main-guard","target":"branch","enforcement":"active", "conditions":{"ref_name":{"include":["~ALL"],"exclude":[]}},"bypass_actors":[],"rules":[rule]});
        let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET")
            .on(&path, &detail.to_string());
        let (rulesets, _) = collect_rulesets(&commands, OWNER, NAME, BRANCH, Some(BRANCH));
        Ok(rulesets.value().and_then(|rows| rows.first()).ok_or("missing ruleset")?.rules.clone())
    };
    let status = json!({"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"required_status_checks":[{"context":"bound","integration_id":42}]}});
    let observed = parse(status.clone())?;
    let rule = observed
        .value()
        .and_then(|rows| rows.first())
        .ok_or("positive status rule not observed")?;
    require(
        rule.strict_required_status_checks_policy == Some(true)
            && rule.required_contexts.first().is_some_and(|row| row.app_id == Some(42)),
        "strict and App binding must survive collection",
    )?;
    for binding in [Value::Null, json!("42"), json!(0), json!(-1)] {
        let mut bad = status.clone();
        *fixture_mut(&mut bad, "/parameters/required_status_checks/0/integration_id")? = binding;
        require(
            parse(bad)?.state == ObservationState::NotProven,
            "invalid integration identity is unknown",
        )?;
    }
    let mut omitted = status;
    fixture_object_mut(&mut omitted, "/parameters/required_status_checks/0")?
        .remove("integration_id");
    require(
        parse(omitted)?.state == ObservationState::NotProven,
        "omitted integration identity is unknown",
    )?;
    let review = json!({"type":"pull_request","parameters":{"required_approving_review_count":2,"required_review_thread_resolution":true,"dismiss_stale_reviews_on_push":false,"require_code_owner_review":true,"require_last_push_approval":false}});
    let observed = parse(review.clone())?;
    let rule = observed
        .value()
        .and_then(|rows| rows.first())
        .ok_or("positive review rule not observed")?;
    require(
        rule.require_code_owner_review == Some(true)
            && rule.require_last_push_approval == Some(false),
        "review booleans must survive collection",
    )?;
    for field in [
        "required_approving_review_count",
        "required_review_thread_resolution",
        "dismiss_stale_reviews_on_push",
        "require_code_owner_review",
        "require_last_push_approval",
    ] {
        let mut bad = review.clone();
        fixture_object_mut(&mut bad, "/parameters")?.remove(field);
        require(
            parse(bad)?.state == ObservationState::NotProven,
            "omitted mandatory review field is unknown",
        )?;
    }
    let mut extra = review;
    fixture_object_mut(&mut extra, "/parameters")?.insert("required_reviewers".into(), json!([]));
    require(
        parse(extra)?.state == ObservationState::NotProven,
        "unmodeled optional parameter is unknown",
    )
}

#[test]
fn matcher_joins_only_relevant_uncertainty_and_obeys_bounds() -> TestResult {
    for (include, exclude, expected) in [
        (json!(["~FUTURE"]), json!(["~ALL"]), false),
        (json!(["~FUTURE", "~ALL"]), json!([]), true),
        (json!([]), json!(["~FUTURE"]), false),
        (json!(["refs/heads/***/main"]), json!([]), false),
    ] {
        let conditions = json!({"ref_name":{"include":include,"exclude":exclude}});
        require(
            ruleset_applies_to_branch(Some(&conditions), "main", Some("main")).value()
                == Some(&expected),
            "decisive matches dominate unrelated unknowns",
        )?;
    }
    for (pattern, subject) in [
        ("x".repeat(1025), "main".to_string()),
        ("refs/heads/*".into(), "a".repeat(1025)),
        ("refs/heads/[]a]".into(), "main".into()),
    ] {
        let conditions = json!({"ref_name":{"include":[pattern],"exclude":[]}});
        require(
            ruleset_applies_to_branch(Some(&conditions), &subject, Some("main")).state
                == ObservationState::NotProven,
            "unsupported syntax or observation bound is unknown",
        )?;
    }
    Ok(())
}

#[test]
fn immutable_settings_are_independent_and_fail_closed() -> TestResult {
    for (value, enabled, owner) in [
        (
            json!({"enabled":true,"enforced_by_owner":false}),
            ObservationState::Observed,
            ObservationState::Observed,
        ),
        (json!({"enabled":false}), ObservationState::Observed, ObservationState::NotProven),
        (
            json!({"enabled":"false","enforced_by_owner":true}),
            ObservationState::NotProven,
            ObservationState::Observed,
        ),
        (json!(null), ObservationState::NotProven, ObservationState::NotProven),
    ] {
        let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET")
            .on(&format!("repos/{OWNER}/{NAME}/immutable-releases"), &value.to_string());
        let receipt = observe(&commands, &[subject(OWNER, NAME)], "test".into());
        let posture = &receipt.repositories.first().ok_or("missing repository")?.release_posture;
        require(
            posture.immutable_releases.state == enabled
                && posture.immutable_releases_enforced_by_owner.state == owner,
            "missing/malformed setting only affects its own observation",
        )?;
    }
    Ok(())
}

#[test]
fn environment_identity_and_listing_counts_must_agree() -> TestResult {
    let mut mismatch = environment_fixture();
    *fixture_mut(&mut mismatch, "/id")? = json!(2);
    let row = environment_with(mismatch, None, None)?;
    require(
        row.protection_rules.state == ObservationState::NotProven
            && row.deployment_branch_policy.state == ObservationState::NotProven,
        "wrong environment detail identity cannot prove controls",
    )?;
    for listing in [
        json!({"total_count":0,"environments":[{"id":1,"node_id":"E_release","name":"release"}]}),
        json!({"total_count":2,"environments":[{"id":1,"node_id":"E_release","name":"release"},{"id":1,"node_id":"E_release","name":"release"}]}),
    ] {
        let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET").on(
            &format!("repos/{OWNER}/{NAME}/environments?per_page=100&page=1"),
            &listing.to_string(),
        );
        require(
            collect_environments(&commands, OWNER, NAME).state == ObservationState::NotProven,
            "over-counted/duplicate environment listing is unknown",
        )?;
    }
    Ok(())
}

#[test]
fn schema_instrument_and_snapshot_contract_agree() -> TestResult {
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SECRET");
    let receipt = observe(&commands, &[subject(OWNER, NAME)], "test".into());
    let mut value = serde_json::to_value(&receipt)?;
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/release_live_controls.v1.schema.json"
    ))?;
    let validator = jsonschema::validator_for(&schema)?;
    require(validator.is_valid(&value), "complete live control must satisfy schema")?;
    fixture_object_mut(&mut value, "/instrument")?.remove("gh_version");
    require(!validator.is_valid(&value), "observed instrument requires version")?;
    *fixture_mut(&mut value, "/instrument/state")? = json!("NOT_PROVEN");
    fixture_object_mut(&mut value, "/instrument")?
        .insert("gh_version".into(), json!("gh version test"));
    require(!validator.is_valid(&value), "inconclusive instrument cannot carry version")?;
    let mut old = serde_json::to_value(&receipt)?;
    fixture_object_mut(&mut old, "/repositories/0/release_posture")?
        .remove("immutable_releases_enforced_by_owner");
    require(
        !validator.is_valid(&old) && serde_json::from_value::<LiveControlsReceipt>(old).is_err(),
        "older incomplete v1 must require regeneration",
    )
}
