//! Falsifiers for `release_live_controls.v1` (#9403).
//!
//! Nothing here reads the network. The mocked API surface fails any path it
//! was not explicitly told to answer, so a test cannot accidentally pass by
//! reading something it never stubbed.
//!
//! The workspace denies `clippy::unwrap_used`/`expect_used`/`panic` even in
//! tests, so every test that can fail for a reason other than the law under
//! test returns [`TestResult`] and uses `?` at the assertion boundary rather
//! than `.unwrap()`/`.expect()`.

use std::collections::HashMap;

use xtask::release_live_controls::{
    ApiError, BypassActor, ClassicProtection, Currency, DeploymentBranchPolicy, Environment,
    EnvironmentProtectionRule, IdentityMatch, Instrument, LiveControlsReceipt, ObservationState,
    Observed, PullRequestReviewRule, RELEASE_LIVE_CONTROLS_SCHEMA_VERSION, ReadOnlyCommands,
    ReleasePosture, RepositoryControls, RepositoryIdentity, RepositorySubject, RequiredContextRow,
    RequiredContextsUnion, RequiredStatusChecks, Ruleset, RulesetRule, UnionContext, Verdict,
    collect_classic_protection, collect_rulesets, identity_match, limitations, observe,
    parse_http_status, required_contexts_union, verdict,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Mocked command surface
// ---------------------------------------------------------------------------

/// Canned `gh api` responses keyed by exact path. Any path not stubbed fails,
/// so a test cannot pass for the trivial reason that collection silently
/// tolerated an unexpected read.
#[derive(Default)]
struct FakeCommands {
    responses: HashMap<String, Result<String, ApiError>>,
    gh_version: Option<Result<String, ApiError>>,
}

impl FakeCommands {
    fn on(mut self, path: &str, body: &str) -> Self {
        self.responses.insert(path.to_string(), Ok(body.to_string()));
        self
    }

    fn failing(mut self, path: &str, status: Option<u16>, detail: &str) -> Self {
        self.responses
            .insert(path.to_string(), Err(ApiError { status, detail: detail.to_string() }));
        self
    }
}

impl ReadOnlyCommands for FakeCommands {
    fn api(&self, path: &str) -> Result<String, ApiError> {
        self.responses.get(path).cloned().unwrap_or_else(|| {
            Err(ApiError { status: None, detail: format!("unstubbed path: {path}") })
        })
    }

    fn gh_version(&self) -> Result<String, ApiError> {
        self.gh_version.clone().unwrap_or_else(|| Ok("gh version 2.60.0 (2026-01-01)".to_string()))
    }
}

const OWNER: &str = "EffortlessMetrics";
const NAME: &str = "perl-lsp-swarm";
const PUBLIC_NAME: &str = "perl-lsp";
const BRANCH: &str = "main";
const SHARED_CONTEXT: &str = "ci/required-check";

fn subject(owner: &str, name: &str) -> RepositorySubject {
    RepositorySubject {
        owner: owner.to_string(),
        name: name.to_string(),
        branch: BRANCH.to_string(),
    }
}

/// A fully healthy, conclusively-observable fixture for one repository. Every
/// endpoint the observer reads is stubbed with a shape that resolves to
/// `OBSERVED` everywhere. Individual tests override one stub to falsify one
/// law.
fn healthy_repository(
    commands: FakeCommands,
    owner: &str,
    name: &str,
    secret_name: &str,
) -> FakeCommands {
    let repo_body = format!(
        r#"{{"full_name":"{owner}/{name}","node_id":"R_kg{name}","id":42,"default_branch":"{BRANCH}","immutable_releases":true}}"#
    );
    let branch_body = r#"{"protected":true}"#.to_string();
    let protection_body = format!(
        r#"{{
            "required_status_checks": {{
                "strict": true,
                "checks": [{{"context": "{SHARED_CONTEXT}", "app_id": 15368}}]
            }},
            "enforce_admins": {{"enabled": true}},
            "required_pull_request_reviews": {{
                "required_approving_review_count": 2,
                "dismiss_stale_reviews": true,
                "require_code_owner_reviews": true,
                "require_last_push_approval": false
            }},
            "required_conversation_resolution": {{"enabled": true}},
            "restrictions": null
        }}"#
    );
    let rulesets_list = r#"[
        {"id": 1, "name": "main-guard", "target": "branch", "enforcement": "active"},
        {"id": 2, "name": "tag-guard", "target": "tag", "enforcement": "active"}
    ]"#
    .to_string();
    let branch_ruleset_detail = format!(
        r#"{{
            "bypass_actors": [],
            "rules": [
                {{"type": "required_status_checks", "parameters": {{"required_status_checks": [
                    {{"context": "{SHARED_CONTEXT}"}},
                    {{"context": "ruleset-only-check"}}
                ]}}}}
            ]
        }}"#
    );
    let tag_ruleset_detail = r#"{"bypass_actors": [], "rules": []}"#.to_string();
    let environments_list =
        r#"{"total_count": 1, "environments": [{"name": "release"}]}"#.to_string();
    let environment_detail = r#"{
        "protection_rules": [{"type": "required_reviewers", "reviewers": [{"type":"User","id":1}], "wait_timer": 0, "prevent_self_review": true}],
        "deployment_branch_policy": {"protected_branches": true, "custom_branch_policies": false}
    }"#
    .to_string();
    let secrets_body = format!(
        r#"{{"total_count": 1, "secrets": [{{"name": "{secret_name}", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"}}]}}"#
    );

    commands
        .on(&format!("repos/{owner}/{name}"), &repo_body)
        .on(&format!("repos/{owner}/{name}/branches/{BRANCH}"), &branch_body)
        .on(&format!("repos/{owner}/{name}/branches/{BRANCH}/protection"), &protection_body)
        .on(&format!("repos/{owner}/{name}/rulesets?includes_parents=true"), &rulesets_list)
        .on(&format!("repos/{owner}/{name}/rulesets/1"), &branch_ruleset_detail)
        .on(&format!("repos/{owner}/{name}/rulesets/2"), &tag_ruleset_detail)
        .on(&format!("repos/{owner}/{name}/environments"), &environments_list)
        .on(&format!("repos/{owner}/{name}/environments/release"), &environment_detail)
        .on(&format!("repos/{owner}/{name}/environments/release/secrets"), &secrets_body)
}

// ---------------------------------------------------------------------------
// Pure `evaluate` laws
// ---------------------------------------------------------------------------

fn matched_identity(owner: &str, name: &str) -> Observed<RepositoryIdentity> {
    Observed::observed(RepositoryIdentity {
        full_name: format!("{owner}/{name}"),
        node_id: "R_kgOOAAA".to_string(),
        database_id: 42,
        default_branch: BRANCH.to_string(),
    })
}

fn conclusive_classic() -> Observed<ClassicProtection> {
    Observed::observed(ClassicProtection {
        required_status_checks: Observed::observed(RequiredStatusChecks {
            strict: true,
            contexts: vec![RequiredContextRow {
                context: SHARED_CONTEXT.to_string(),
                app_id: None,
            }],
        }),
        enforce_admins: Observed::observed(true),
        required_pull_request_reviews: Observed::absent("not required"),
        required_conversation_resolution: Observed::observed(true),
        restrictions_present: Observed::observed(false),
    })
}

fn conclusive_release_posture() -> ReleasePosture {
    ReleasePosture {
        immutable_releases: Observed::observed(true),
        tag_rulesets_present: Observed::observed(false),
    }
}

/// A repository whose every plane is conclusively observed — the only shape
/// that may ever reach `Verdict::Observed`. Each negative-control test
/// perturbs exactly one field.
fn admissible_repository() -> RepositoryControls {
    RepositoryControls {
        requested: subject(OWNER, NAME),
        identity: matched_identity(OWNER, NAME),
        identity_match: IdentityMatch::Matched,
        classic_branch_protection: conclusive_classic(),
        branch_rulesets: Observed::observed(Vec::new()),
        tag_rulesets: Observed::observed(Vec::new()),
        environments: Observed::observed(Vec::new()),
        release_posture: conclusive_release_posture(),
        required_contexts_union: RequiredContextsUnion {
            state: ObservationState::Observed,
            detail: None,
            contexts: vec![UnionContext {
                name: SHARED_CONTEXT.to_string(),
                sources: vec!["branch_protection".to_string()],
            }],
        },
    }
}

/// The control. Without it, every falsifier below could pass for the trivial
/// reason that nothing is ever observed.
#[test]
fn admissible_repository_reaches_observed() {
    let repositories = vec![admissible_repository()];
    assert_eq!(verdict(&repositories), Verdict::Observed);
    assert!(limitations(&repositories).is_empty());
}

/// Classic branch protection alone — however completely it was read — must
/// never be reported as complete enforcement while branch rulesets (which
/// enforce additively) could not be read.
#[test]
fn classic_protection_alone_cannot_claim_complete_enforcement() -> TestResult {
    let classic = conclusive_classic();
    let branch_rulesets: Observed<Vec<Ruleset>> =
        Observed::not_proven("ruleset listing returned HTTP 403");

    let union = required_contexts_union(&classic, &branch_rulesets);
    assert_eq!(union.state, ObservationState::NotProven);
    let detail = union.detail.clone().ok_or("a NOT_PROVEN union must name what was missing")?;
    assert!(detail.contains("branch_rulesets"), "detail must name the missing half: {detail}");
    assert!(union.contexts.is_empty(), "a NOT_PROVEN union must not carry partial contexts");

    let mut repository = admissible_repository();
    repository.branch_rulesets = branch_rulesets;
    repository.required_contexts_union = union;
    assert_eq!(verdict(&[repository.clone()]), Verdict::NotProven);
    assert!(
        limitations(&[repository]).iter().any(|line| line.contains("required_contexts_union")),
        "limitations must name the union as the non-conclusive plane",
    );
    Ok(())
}

/// The mirror image: rulesets alone, with classic protection unreadable,
/// must also refuse to claim completeness.
#[test]
fn rulesets_alone_cannot_claim_complete_enforcement() -> TestResult {
    let classic: Observed<ClassicProtection> =
        Observed::not_proven("protection endpoint returned HTTP 403");
    let branch_rulesets = Observed::observed(vec![Ruleset {
        id: 9,
        name: "guard".to_string(),
        target: "branch".to_string(),
        enforcement: "active".to_string(),
        bypass_actors: Observed::observed(Vec::new()),
        rules: Observed::observed(vec![RulesetRule {
            rule_type: "required_status_checks".to_string(),
            required_contexts: vec![SHARED_CONTEXT.to_string()],
            required_approving_review_count: None,
            required_review_thread_resolution: None,
            dismiss_stale_reviews_on_push: None,
        }]),
    }]);

    let union = required_contexts_union(&classic, &branch_rulesets);
    assert_eq!(union.state, ObservationState::NotProven);
    let detail = union.detail.ok_or("detail required")?;
    assert!(detail.contains("classic_branch_protection"), "{detail}");
    Ok(())
}

/// A ruleset whose `rules` the API could not be read for must also block the
/// union, not merely a whole-list failure.
#[test]
fn a_single_contributing_ruleset_with_unproven_rules_blocks_the_union() -> TestResult {
    let classic = conclusive_classic();
    let branch_rulesets = Observed::observed(vec![Ruleset {
        id: 7,
        name: "guard".to_string(),
        target: "branch".to_string(),
        enforcement: "active".to_string(),
        bypass_actors: Observed::observed(Vec::new()),
        rules: Observed::not_proven("ruleset detail returned HTTP 403"),
    }]);

    let union = required_contexts_union(&classic, &branch_rulesets);
    assert_eq!(union.state, ObservationState::NotProven);
    let detail = union.detail.ok_or("detail required")?;
    assert!(detail.contains("branch_rulesets[7].rules"), "{detail}");
    Ok(())
}

/// `Matched` only when full_name matches case-insensitively AND a non-zero
/// database id AND a non-empty node id are all present.
#[test]
fn repository_identity_mismatch_is_rejected() -> TestResult {
    let requested = subject(OWNER, NAME);

    let matched = Observed::observed(RepositoryIdentity {
        full_name: format!("{OWNER}/{NAME}").to_ascii_uppercase(),
        node_id: "R_kgOOAAA".to_string(),
        database_id: 1,
        default_branch: BRANCH.to_string(),
    });
    assert_eq!(identity_match(&requested, &matched), IdentityMatch::Matched);

    let renamed = Observed::observed(RepositoryIdentity {
        full_name: "SomeoneElse/unrelated-fork".to_string(),
        node_id: "R_kgOOAAA".to_string(),
        database_id: 1,
        default_branch: BRANCH.to_string(),
    });
    let IdentityMatch::Mismatched { detail } = identity_match(&requested, &renamed) else {
        return Err("a renamed repository must be reported as Mismatched".into());
    };
    assert!(detail.contains("SomeoneElse/unrelated-fork"), "{detail}");

    let unobserved: Observed<RepositoryIdentity> =
        Observed::not_proven("repository payload did not parse");
    assert!(matches!(identity_match(&requested, &unobserved), IdentityMatch::NotProven { .. }));

    let mut repository = admissible_repository();
    repository.identity = renamed;
    repository.identity_match = IdentityMatch::Mismatched { detail: "mismatch".to_string() };
    assert_eq!(verdict(&[repository]), Verdict::NotProven);
    Ok(())
}

/// A row claiming `OBSERVED` while carrying no value is exactly the shape a
/// hand-written or replayed document could smuggle in; it must be rejected
/// rather than trusted.
#[test]
fn an_observed_row_with_a_null_value_is_structurally_rejected() -> TestResult {
    let malformed: Observed<bool> = serde_json::from_str(r#"{"state":"OBSERVED","value":null}"#)?;
    let problem = malformed
        .structural_problem("enforce_admins")
        .ok_or("an OBSERVED row with no value must be flagged")?;
    assert!(problem.contains("enforce_admins"));

    // The opposite direction: ABSENT/NOT_PROVEN carrying a value.
    let malformed_absent: Observed<bool> =
        serde_json::from_str(r#"{"state":"ABSENT","value":true}"#)?;
    assert!(malformed_absent.structural_problem("restrictions_present").is_some());

    let mut receipt = minimal_receipt(vec![admissible_repository()]);
    let protection =
        receipt.repositories[0].classic_branch_protection.value.as_mut().ok_or(
            "admissible_repository must carry an observed classic_branch_protection value",
        )?;
    protection.enforce_admins = malformed;
    assert!(receipt.structural_problem().is_some());
    Ok(())
}

fn minimal_receipt(repositories: Vec<RepositoryControls>) -> LiveControlsReceipt {
    let observed_verdict = verdict(&repositories);
    let observed_limitations = limitations(&repositories);
    LiveControlsReceipt {
        schema_version: RELEASE_LIVE_CONTROLS_SCHEMA_VERSION.to_string(),
        observed_at: "2026-09-03T00:00:00Z".to_string(),
        currency: Currency::Live,
        instrument: Instrument {
            state: ObservationState::Observed,
            gh_version: Some("gh 2.60.0".to_string()),
            detail: None,
        },
        repositories,
        verdict: observed_verdict,
        limitations: observed_limitations,
    }
}

/// A receipt file whose JSON claims `currency: "LIVE"` must load as a
/// `Snapshot` regardless: a replayed observation can never represent itself
/// as current.
#[test]
fn a_snapshot_cannot_represent_itself_as_current() -> TestResult {
    let receipt = minimal_receipt(vec![admissible_repository()]);
    let mut json = serde_json::to_value(&receipt)?;
    json["currency"] = serde_json::Value::String("LIVE".to_string());
    assert_eq!(json["currency"], "LIVE", "the fixture must actually claim LIVE before loading");

    let dir =
        std::env::temp_dir().join(format!("release-live-controls-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("snapshot.json");
    std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;

    let loaded = xtask::release_live_controls::load_snapshot(&path)?;
    assert_eq!(loaded.currency, Currency::Snapshot, "currency must be forced to Snapshot on load");

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `parse_http_status` must recover the code from `gh`'s free-text stderr
/// shapes, and return `None` when nothing three-digit follows an `HTTP`
/// token.
#[test]
fn parse_http_status_reads_gh_error_text() {
    assert_eq!(parse_http_status("gh: Not Found (HTTP 404)"), Some(404));
    assert_eq!(parse_http_status("HTTP 403: Forbidden"), Some(403));
    assert_eq!(parse_http_status("gh: some other error entirely"), None);
    assert_eq!(parse_http_status(""), None);
    assert_eq!(parse_http_status("HTTP"), None);
    assert_eq!(parse_http_status("connection reset: HTTPS handshake failed"), None);
    assert_eq!(
        parse_http_status("failed to connect; retry gave (HTTP 500) as well"),
        Some(500),
        "the first three-digit code following HTTP wins",
    );
}

// ---------------------------------------------------------------------------
// Live collection (mocked `gh api`)
// ---------------------------------------------------------------------------

/// The positive control for live collection. Without it, every fail-closed
/// case below could pass for the trivial reason that live collection never
/// reaches `OBSERVED`.
#[test]
fn a_fully_observed_pair_of_repositories_reaches_observed() -> TestResult {
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "SWARM_DEPLOY_TOKEN");
    let commands = healthy_repository(commands, OWNER, PUBLIC_NAME, "PUBLIC_DEPLOY_TOKEN");

    let subjects = vec![subject(OWNER, NAME), subject(OWNER, PUBLIC_NAME)];
    let receipt = observe(&commands, &subjects, "2026-09-03T00:00:00Z".to_string());

    assert_eq!(receipt.verdict, Verdict::Observed, "limitations: {:?}", receipt.limitations);
    assert!(receipt.limitations.is_empty());
    assert_eq!(receipt.currency, Currency::Live);
    assert!(receipt.structural_problem().is_none());

    let union = &receipt.repositories[0].required_contexts_union;
    assert_eq!(union.state, ObservationState::Observed);
    let shared = union
        .contexts
        .iter()
        .find(|context| context.name == SHARED_CONTEXT)
        .ok_or("the shared context must appear in the union")?;
    let mut sources = shared.sources.clone();
    sources.sort();
    assert_eq!(
        sources,
        vec!["branch_protection".to_string(), "ruleset:1".to_string()],
        "the union must merge classic protection and ruleset sources for the same context name",
    );
    let ruleset_only = union
        .contexts
        .iter()
        .find(|context| context.name == "ruleset-only-check")
        .ok_or("a ruleset-only context must still appear")?;
    assert_eq!(ruleset_only.sources, vec!["ruleset:1".to_string()]);
    Ok(())
}

/// GitHub's protection endpoint returning a plain permissions error (not a
/// 404) must never be read as an empty, passing configuration.
#[test]
fn inaccessible_protection_api_does_not_become_an_empty_pass() {
    let commands = FakeCommands::default()
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}"), r#"{"protected":true}"#)
        .failing(
            &format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"),
            Some(403),
            "HTTP 403: Forbidden",
        );

    let classic = collect_classic_protection(&commands, OWNER, NAME, BRANCH);
    assert_eq!(classic.state, ObservationState::NotProven, "403 must never become Absent");
    assert!(classic.value().is_none(), "no value — never an empty context list");

    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "TOKEN").failing(
        &format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"),
        Some(403),
        "HTTP 403: Forbidden",
    );
    let receipt = observe(&commands, &[subject(OWNER, NAME)], "2026-09-03T00:00:00Z".to_string());
    assert_eq!(receipt.verdict, Verdict::NotProven);
}

/// THE CENTRAL DISCRIMINATOR, first half: a 404 from the protection endpoint
/// while the branch itself claims `protected: true` is a contradiction this
/// build cannot resolve — never a pass, never a corroborated absence.
#[test]
fn protection_404_with_branch_reporting_protected_is_not_proven() {
    let commands = FakeCommands::default()
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}"), r#"{"protected":true}"#)
        .failing(
            &format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"),
            Some(404),
            "HTTP 404: Not Found",
        );

    let classic = collect_classic_protection(&commands, OWNER, NAME, BRANCH);
    assert_eq!(classic.state, ObservationState::NotProven);
    assert!(classic.value().is_none());
}

/// THE CENTRAL DISCRIMINATOR, second half: a 404 from the protection
/// endpoint while the branch itself reports `protected: false` is a
/// corroborated absence, and the union may still reach `OBSERVED` around it.
#[test]
fn protection_404_with_unprotected_branch_is_a_corroborated_absence() {
    let commands = FakeCommands::default()
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}"), r#"{"protected":false}"#)
        .failing(
            &format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"),
            Some(404),
            "HTTP 404: Not Found",
        );

    let classic = collect_classic_protection(&commands, OWNER, NAME, BRANCH);
    assert_eq!(classic.state, ObservationState::Absent);
    assert!(classic.value().is_none(), "ABSENT must not carry a value");

    let branch_rulesets: Observed<Vec<Ruleset>> = Observed::observed(Vec::new());
    let union = required_contexts_union(&classic, &branch_rulesets);
    assert_eq!(
        union.state,
        ObservationState::Observed,
        "an absent classic half plus an observed ruleset half is fully proven"
    );
}

/// A ruleset detail payload that omits `bypass_actors` entirely must be
/// `NOT_PROVEN`, never an empty list — an omitted bypass roster is not
/// evidence of "no bypass".
#[test]
fn ruleset_without_a_bypass_actors_field_is_not_proven() -> TestResult {
    let commands = FakeCommands::default()
        .on(
            &format!("repos/{OWNER}/{NAME}/rulesets?includes_parents=true"),
            r#"[{"id": 1, "name": "guard", "target": "branch", "enforcement": "active"}]"#,
        )
        .on(&format!("repos/{OWNER}/{NAME}/rulesets/1"), r#"{"rules": []}"#);

    let (branch_rulesets, _tag_rulesets) = collect_rulesets(&commands, OWNER, NAME);
    let rulesets = branch_rulesets.value().ok_or("the ruleset row itself must still appear")?;
    assert_eq!(rulesets.len(), 1, "the row must not vanish because one field was unproven");
    assert_eq!(rulesets[0].bypass_actors.state, ObservationState::NotProven);
    assert!(rulesets[0].bypass_actors.value().is_none());

    let mut repository = admissible_repository();
    repository.branch_rulesets = branch_rulesets;
    assert_eq!(
        verdict(&[repository]),
        Verdict::NotProven,
        "an unproven bypass roster must block the verdict"
    );
    Ok(())
}

/// The opposite control: an explicit empty `bypass_actors` array is a real,
/// conclusive observation of "no bypass actors configured".
#[test]
fn an_explicit_empty_bypass_actors_array_is_observed() -> TestResult {
    let commands = FakeCommands::default()
        .on(
            &format!("repos/{OWNER}/{NAME}/rulesets?includes_parents=true"),
            r#"[{"id": 1, "name": "guard", "target": "branch", "enforcement": "active"}]"#,
        )
        .on(&format!("repos/{OWNER}/{NAME}/rulesets/1"), r#"{"bypass_actors": [], "rules": []}"#);

    let (branch_rulesets, _tag_rulesets) = collect_rulesets(&commands, OWNER, NAME);
    let rulesets = branch_rulesets.value().ok_or("the ruleset list must be observed")?;
    assert_eq!(rulesets[0].bypass_actors, Observed::observed(Vec::<BypassActor>::new()));

    let mut repository = admissible_repository();
    repository.classic_branch_protection = Observed::absent("no classic protection");
    repository.branch_rulesets = branch_rulesets;
    repository.required_contexts_union =
        required_contexts_union(&repository.classic_branch_protection, &repository.branch_rulesets);
    assert_eq!(verdict(&[repository]), Verdict::Observed);
    Ok(())
}

/// An unrecognised ruleset `target` must not silently drop the row from
/// either bucket: both collections come back `NOT_PROVEN`.
#[test]
fn an_unrecognised_ruleset_target_makes_both_buckets_not_proven() {
    let commands = FakeCommands::default()
        .on(
            &format!("repos/{OWNER}/{NAME}/rulesets?includes_parents=true"),
            r#"[{"id": 1, "name": "mystery", "target": "push", "enforcement": "active"}]"#,
        )
        .on(&format!("repos/{OWNER}/{NAME}/rulesets/1"), r#"{"bypass_actors": [], "rules": []}"#);

    let (branch_rulesets, tag_rulesets) = collect_rulesets(&commands, OWNER, NAME);
    assert_eq!(branch_rulesets.state, ObservationState::NotProven);
    assert_eq!(tag_rulesets.state, ObservationState::NotProven);
}

/// Redaction law: environment secret NAMES must never appear in the
/// serialized receipt, only their count.
#[test]
fn environment_secret_names_are_never_recorded() -> TestResult {
    const PLANTED_SECRET_NAME: &str = "SUPER_SECRET_DEPLOY_TOKEN_MARKER";
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, PLANTED_SECRET_NAME);

    let receipt = observe(&commands, &[subject(OWNER, NAME)], "2026-09-03T00:00:00Z".to_string());
    let environments =
        receipt.repositories[0].environments.value().ok_or("environments must be observed")?;
    let environment = environments.first().ok_or("at least one environment must be observed")?;
    assert_eq!(environment.secret_count, Observed::observed(1usize));

    let serialized = serde_json::to_string_pretty(&receipt)?;
    assert!(
        !serialized.contains(PLANTED_SECRET_NAME),
        "the serialized receipt must never contain a secret name, only its count",
    );
    assert!(serialized.contains("\"secret_count\""));
    Ok(())
}

/// A repository whose payload names a different repository than the one
/// requested must be refused end to end, through the live path.
#[test]
fn repository_identity_mismatch_is_rejected_through_live_collection() {
    let commands = healthy_repository(FakeCommands::default(), OWNER, NAME, "TOKEN").on(
        &format!("repos/{OWNER}/{NAME}"),
        r#"{"full_name":"SomeoneElse/impostor","node_id":"R_kgIMPOSTOR","id":1,"default_branch":"main"}"#,
    );

    let receipt = observe(&commands, &[subject(OWNER, NAME)], "2026-09-03T00:00:00Z".to_string());
    assert!(matches!(receipt.repositories[0].identity_match, IdentityMatch::Mismatched { .. }));
    assert_eq!(receipt.verdict, Verdict::NotProven);
}

/// Every model type round-trips through JSON without losing the
/// `OBSERVED`/`ABSENT`/`NOT_PROVEN` distinction — the invariant the whole
/// module exists to keep.
#[test]
fn observed_absent_and_not_proven_round_trip_distinctly() -> TestResult {
    let observed: Observed<bool> = Observed::observed(true);
    let absent: Observed<bool> = Observed::absent("the API said no");
    let not_proven: Observed<bool> = Observed::not_proven("could not tell");

    for value in [&observed, &absent, &not_proven] {
        let json = serde_json::to_string(value)?;
        let round_tripped: Observed<bool> = serde_json::from_str(&json)?;
        assert_eq!(&round_tripped, value);
        assert!(round_tripped.structural_problem("field").is_none());
    }

    assert_ne!(
        absent.state, not_proven.state,
        "ABSENT and NOT_PROVEN must never collapse into one state"
    );
    Ok(())
}

/// `Environment`, `DeploymentBranchPolicy`, and related structs must exist
/// and be constructible with the documented fields — a compile-time
/// falsifier for the redaction-law surface.
#[test]
fn environment_model_carries_only_counts_types_and_names() -> TestResult {
    let environment = Environment {
        name: "production".to_string(),
        protection_rules: Observed::observed(vec![EnvironmentProtectionRule {
            rule_type: "required_reviewers".to_string(),
            wait_timer: Some(0),
            reviewer_count: Some(2),
            prevent_self_review: Some(true),
        }]),
        deployment_branch_policy: Observed::observed(Some(DeploymentBranchPolicy {
            protected_branches: true,
            custom_branch_policies: false,
        })),
        secret_count: Observed::observed(3usize),
    };
    let json = serde_json::to_string(&environment)?;
    assert!(json.contains("\"secret_count\""));
    assert!(json.contains("\"reviewer_count\":2"));
    Ok(())
}

/// A malformed `PullRequestReviewRule` field being absent must read as
/// "the API did not say", not `false`.
#[test]
fn absent_review_rule_fields_are_none_not_false() {
    let rule = PullRequestReviewRule {
        required_approving_review_count: None,
        dismiss_stale_reviews: None,
        require_code_owner_reviews: None,
        require_last_push_approval: None,
    };
    assert_eq!(rule.dismiss_stale_reviews, None);
    assert_ne!(rule.dismiss_stale_reviews, Some(false));
}

/// The schema file must parse as JSON, and its `schema_version.const` must
/// match the Rust constant — the two must never drift apart.
#[test]
fn schema_file_parses_and_matches_the_schema_version_constant() -> TestResult {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("schemas")
        .join("release_live_controls.v1.schema.json");
    let raw = std::fs::read_to_string(&path)?;
    let schema: serde_json::Value = serde_json::from_str(&raw)?;
    let const_value = schema
        .get("properties")
        .and_then(|properties| properties.get("schema_version"))
        .and_then(|schema_version| schema_version.get("const"))
        .and_then(serde_json::Value::as_str)
        .ok_or("schema_version.const must be a string")?;
    assert_eq!(const_value, RELEASE_LIVE_CONTROLS_SCHEMA_VERSION);
    assert_eq!(
        schema.get("$schema").and_then(serde_json::Value::as_str),
        Some("https://json-schema.org/draft/2020-12/schema")
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// An unreadable required-context row must poison its observation, not vanish
// ---------------------------------------------------------------------------

/// A classic-protection `checks` entry whose `context` is not a readable
/// string must take `required_status_checks` to `NOT_PROVEN`.
///
/// Dropping the row instead would report a *smaller* required set with full
/// confidence — a live gate the reader never learns about. That is the same
/// permissive read the module already refuses for an unclassifiable ruleset
/// target, and the union must not be able to launder it.
#[test]
fn an_unreadable_classic_context_row_is_not_proven_rather_than_dropped() -> TestResult {
    let protection_body = format!(
        r#"{{
            "required_status_checks": {{
                "strict": true,
                "checks": [
                    {{"context": "{SHARED_CONTEXT}", "app_id": 15368}},
                    {{"app_id": 99}}
                ]
            }},
            "enforce_admins": {{"enabled": true}},
            "required_pull_request_reviews": null,
            "required_conversation_resolution": {{"enabled": true}},
            "restrictions": null
        }}"#
    );
    let commands = FakeCommands::default()
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}"), r#"{"protected":true}"#)
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"), &protection_body);

    let classic = collect_classic_protection(&commands, OWNER, NAME, BRANCH);
    let protection = classic.value().ok_or("protection payload should still be observed")?;
    assert_eq!(
        protection.required_status_checks.state,
        ObservationState::NotProven,
        "an entry with no readable context name must not be silently dropped"
    );

    // And the union must inherit that, rather than reporting the one context
    // it could read as the complete required set.
    let union = required_contexts_union(&classic, &Observed::observed(Vec::new()));
    assert_eq!(union.state, ObservationState::NotProven);
    assert!(union.contexts.is_empty(), "a not-proven union must not publish a partial set");
    Ok(())
}

/// The opposite-direction control: a well-formed `contexts` array (the older
/// non-`checks` shape) still reaches `OBSERVED`, so the guard above rejects
/// malformed rows rather than every row.
#[test]
fn well_formed_legacy_contexts_array_still_reaches_observed() -> TestResult {
    let protection_body = format!(
        r#"{{
            "required_status_checks": {{"strict": false, "contexts": ["{SHARED_CONTEXT}"]}},
            "enforce_admins": {{"enabled": true}},
            "required_pull_request_reviews": null,
            "required_conversation_resolution": {{"enabled": true}},
            "restrictions": null
        }}"#
    );
    let commands = FakeCommands::default()
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}"), r#"{"protected":true}"#)
        .on(&format!("repos/{OWNER}/{NAME}/branches/{BRANCH}/protection"), &protection_body);

    let classic = collect_classic_protection(&commands, OWNER, NAME, BRANCH);
    let protection = classic.value().ok_or("protection payload should be observed")?;
    assert_eq!(protection.required_status_checks.state, ObservationState::Observed);
    let checks = protection
        .required_status_checks
        .value()
        .ok_or("observed required_status_checks must carry a value")?;
    assert_eq!(checks.contexts.len(), 1);
    assert_eq!(checks.contexts[0].context, SHARED_CONTEXT);
    Ok(())
}

/// A ruleset `required_status_checks` entry with no readable context name
/// must take that ruleset's whole `rules` observation to `NOT_PROVEN`.
///
/// A dropped entry would under-report what the ruleset enforces while the row
/// still looks confidently observed.
#[test]
fn an_unreadable_ruleset_context_row_is_not_proven_rather_than_dropped() -> TestResult {
    let detail = r#"{
        "bypass_actors": [],
        "rules": [
            {"type": "required_status_checks", "parameters": {"required_status_checks": [
                {"context": "readable-check"},
                {"integration_id": 7}
            ]}}
        ]
    }"#;
    let commands = FakeCommands::default()
        .on(
            &format!("repos/{OWNER}/{NAME}/rulesets?includes_parents=true"),
            r#"[{"id": 1, "name": "main-guard", "target": "branch", "enforcement": "active"}]"#,
        )
        .on(&format!("repos/{OWNER}/{NAME}/rulesets/1"), detail);

    let (branch_rulesets, _tag_rulesets) = collect_rulesets(&commands, OWNER, NAME);
    let rulesets = branch_rulesets.value().ok_or("the ruleset list should still be observed")?;
    let row = rulesets.first().ok_or("the ruleset row must survive")?;
    assert_eq!(
        row.rules.state,
        ObservationState::NotProven,
        "an unreadable required_status_checks entry must not be silently dropped"
    );

    // The union must refuse rather than publish the one context it could read.
    let union =
        required_contexts_union(&Observed::absent("no classic protection"), &branch_rulesets);
    assert_eq!(union.state, ObservationState::NotProven);
    assert!(union.contexts.is_empty());

    // And the verdict must not go green behind it.
    let controls = RepositoryControls {
        requested: subject(OWNER, NAME),
        identity: Observed::observed(RepositoryIdentity {
            full_name: format!("{OWNER}/{NAME}"),
            node_id: "R_kgtest".to_string(),
            database_id: 42,
            default_branch: BRANCH.to_string(),
        }),
        identity_match: IdentityMatch::Matched,
        classic_branch_protection: Observed::absent("no classic protection"),
        branch_rulesets,
        tag_rulesets: Observed::observed(Vec::new()),
        environments: Observed::observed(Vec::new()),
        release_posture: ReleasePosture {
            immutable_releases: Observed::observed(true),
            tag_rulesets_present: Observed::observed(false),
        },
        required_contexts_union: union,
    };
    assert_eq!(verdict(std::slice::from_ref(&controls)), Verdict::NotProven);
    Ok(())
}
