//! Live collection for `release_live_controls.v1` (#9403).
//!
//! [`evaluate`](super::evaluate) decides over an already-collected snapshot.
//! This module *builds* that snapshot from the real GitHub API, and nothing
//! else: every read issued here is `gh api <path>` (a GET) or `gh --version`,
//! run as argv through [`std::process::Command`] — never through a shell, and
//! never a write. No branch protection, ruleset, environment, or release
//! setting is ever created, edited, or deleted from this module.
//!
//! Every collector is fail-closed. GitHub's protection endpoint returns
//! `404` both when a branch genuinely has no protection *and* when the
//! token cannot read it — [`collect_classic_protection`] is the central
//! discriminator that tells those apart using the branch's own `protected`
//! flag, and refuses to guess when even that is unavailable. A ruleset
//! detail payload that omits `bypass_actors` is `NOT_PROVEN`, never an empty
//! list: an omitted bypass roster must never read as "no bypass".

use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use super::evaluate;
use super::model::{
    BypassActor, ClassicProtection, Currency, DeploymentBranchPolicy, Environment,
    EnvironmentProtectionRule, Instrument, LiveControlsReceipt, ObservationState, Observed,
    PullRequestReviewRule, RELEASE_LIVE_CONTROLS_SCHEMA_VERSION, ReleasePosture,
    RepositoryControls, RepositoryIdentity, RepositorySubject, RequiredContextRow,
    RequiredStatusChecks, Ruleset, RulesetRule,
};

/// One failed `gh` invocation: an HTTP status when it could be recovered from
/// `gh`'s stderr, and the raw detail text either way.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: Option<u16>,
    pub detail: String,
}

/// A bounded, read-only command surface, so collection can be proven without
/// a network. The real implementation shells out; tests supply canned
/// responses.
pub trait ReadOnlyCommands {
    /// `gh api <path>` — a GET. `path` may carry a query string.
    fn api(&self, path: &str) -> Result<String, ApiError>;
    /// `gh --version`, used only to record whether the instrument itself is
    /// usable.
    fn gh_version(&self) -> Result<String, ApiError>;
}

/// Shells out for real. Used by the CLI; never by tests.
pub struct SystemCommands;

impl ReadOnlyCommands for SystemCommands {
    fn api(&self, path: &str) -> Result<String, ApiError> {
        run_gh(&["api", path])
    }

    fn gh_version(&self) -> Result<String, ApiError> {
        run_gh(&["--version"])
    }
}

fn run_gh(args: &[&str]) -> Result<String, ApiError> {
    let output = Command::new("gh").args(args).output().map_err(|error| ApiError {
        status: None,
        detail: format!("running gh {}: {error}", args.join(" ")),
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError {
            status: parse_http_status(&stderr),
            detail: stderr
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.chars().take(200).collect())
                .unwrap_or_else(|| "gh returned a non-success status".to_string()),
        });
    }
    String::from_utf8(output.stdout).map_err(|error| ApiError {
        status: None,
        detail: format!("gh produced non-UTF-8 output: {error}"),
    })
}

/// Recover an HTTP status code from `gh`'s stderr text.
///
/// `gh api` reports a failed request as text containing `(HTTP 404)` or
/// `HTTP 403`, with no other structured signal. Returns `None` when no
/// three-digit code follows an `HTTP` token — a transport failure, an
/// unparseable response, or `gh` itself not being runnable all look like
/// this, and none of them may be read as any particular status.
pub fn parse_http_status(stderr: &str) -> Option<u16> {
    for (index, _) in stderr.match_indices("HTTP") {
        let rest = stderr[index + 4..].trim_start();
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.len() == 3
            && let Ok(code) = digits.parse::<u16>()
        {
            return Some(code);
        }
    }
    None
}

fn describe_error(error: &ApiError) -> String {
    match error.status {
        Some(status) => format!("HTTP {status}: {}", error.detail),
        None => error.detail.clone(),
    }
}

/// Read `repos/{owner}/{name}`. Every field named here (`full_name`,
/// `node_id`, `id`, `default_branch`) must be present; any one missing
/// yields `NOT_PROVEN` rather than a partially populated identity.
pub fn collect_identity(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> Observed<RepositoryIdentity> {
    let path = format!("repos/{owner}/{name}");
    let body = match commands.api(&path) {
        Ok(body) => body,
        Err(error) => {
            return Observed::not_proven(format!(
                "reading repository {owner}/{name}: {}",
                describe_error(&error)
            ));
        }
    };
    let value: Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => {
            return Observed::not_proven(format!(
                "repository payload for {owner}/{name} did not parse: {error}"
            ));
        }
    };

    let full_name = value.get("full_name").and_then(Value::as_str);
    let node_id = value.get("node_id").and_then(Value::as_str);
    let database_id = value.get("id").and_then(Value::as_u64);
    let default_branch = value.get("default_branch").and_then(Value::as_str);

    match (full_name, node_id, database_id, default_branch) {
        (Some(full_name), Some(node_id), Some(database_id), Some(default_branch)) => {
            Observed::observed(RepositoryIdentity {
                full_name: full_name.to_string(),
                node_id: node_id.to_string(),
                database_id,
                default_branch: default_branch.to_string(),
            })
        }
        _ => Observed::not_proven(format!(
            "repository payload for {owner}/{name} was missing full_name, node_id, id, or default_branch"
        )),
    }
}

#[derive(Deserialize)]
struct RawBranch {
    protected: Option<bool>,
}

/// Read classic (non-ruleset) branch protection for `{owner}/{name}@{branch}`.
///
/// THE CENTRAL DISCRIMINATOR: GitHub returns HTTP 404 from the protection
/// endpoint both when protection is genuinely absent *and* when the token
/// lacks the read access to see it. The branch's own `protected` boolean —
/// read first, from a cheaper, more widely readable endpoint — is what tells
/// the two apart:
///
/// - protection 404 and `protected == false` → a corroborated absence.
/// - protection 404 and `protected == true`  → contradiction: the branch
///   claims protection but the detail is unreadable. `NOT_PROVEN`.
/// - protection 404 and `protected` unreadable → `NOT_PROVEN`.
/// - any other protection error (403, 401, transport) → `NOT_PROVEN`.
///
/// An inaccessible API must never become an empty list or a pass.
pub fn collect_classic_protection(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    branch: &str,
) -> Observed<ClassicProtection> {
    let branch_path = format!("repos/{owner}/{name}/branches/{branch}");
    let protected: Option<bool> = match commands.api(&branch_path) {
        Ok(body) => serde_json::from_str::<RawBranch>(&body).ok().and_then(|raw| raw.protected),
        Err(_) => None,
    };

    let protection_path = format!("{branch_path}/protection");
    match commands.api(&protection_path) {
        Ok(body) => match serde_json::from_str::<Value>(&body) {
            Ok(value) => Observed::observed(parse_classic_protection(&value)),
            Err(error) => Observed::not_proven(format!(
                "protection response for {owner}/{name}@{branch} did not parse: {error}"
            )),
        },
        Err(error) if error.status == Some(404) => match protected {
            Some(false) => Observed::absent(format!(
                "protection endpoint returned 404 and {owner}/{name}@{branch} reports protected: false"
            )),
            Some(true) => Observed::not_proven(
                "protection endpoint returned 404 while the branch reports protected: true — read access is insufficient to distinguish absent from inaccessible",
            ),
            None => Observed::not_proven(format!(
                "protection endpoint returned 404 for {owner}/{name}@{branch} and its protected flag could not be read"
            )),
        },
        Err(error) => Observed::not_proven(format!(
            "protection endpoint for {owner}/{name}@{branch}: {}",
            describe_error(&error)
        )),
    }
}

fn parse_classic_protection(value: &Value) -> ClassicProtection {
    let required_status_checks = match value.get("required_status_checks") {
        None | Some(Value::Null) => Observed::absent("required_status_checks not present"),
        Some(raw) => {
            let Some(object) = raw.as_object() else {
                return ClassicProtection {
                    required_status_checks: Observed::not_proven(
                        "required_status_checks was not an object",
                    ),
                    enforce_admins: Observed::not_proven("protection response was malformed"),
                    required_pull_request_reviews: Observed::not_proven(
                        "protection response was malformed",
                    ),
                    required_conversation_resolution: Observed::not_proven(
                        "protection response was malformed",
                    ),
                    restrictions_present: Observed::not_proven("protection response was malformed"),
                };
            };
            let Some(strict) = object.get("strict").and_then(Value::as_bool) else {
                return ClassicProtection {
                    required_status_checks: Observed::not_proven(
                        "required_status_checks.strict was missing or not boolean",
                    ),
                    enforce_admins: Observed::not_proven("protection response was incomplete"),
                    required_pull_request_reviews: Observed::not_proven(
                        "protection response was incomplete",
                    ),
                    required_conversation_resolution: Observed::not_proven(
                        "protection response was incomplete",
                    ),
                    restrictions_present: Observed::not_proven(
                        "protection response was incomplete",
                    ),
                };
            };
            let contexts = match object.get("checks") {
                None => match object.get("contexts") {
                    None => Ok(Vec::new()),
                    Some(Value::Array(rows)) => rows
                        .iter()
                        .enumerate()
                        .map(|(index, row)| {
                            row.as_str()
                                .map(|context| RequiredContextRow {
                                    context: context.to_string(),
                                    app_id: None,
                                })
                                .ok_or(index)
                        })
                        .collect(),
                    Some(_) => Err(usize::MAX),
                },
                Some(Value::Array(rows)) => rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| {
                        row.get("context")
                            .and_then(Value::as_str)
                            .map(|context| RequiredContextRow {
                                context: context.to_string(),
                                app_id: row.get("app_id").and_then(Value::as_u64),
                            })
                            .ok_or(index)
                    })
                    .collect(),
                Some(_) => Err(usize::MAX),
            };
            // An unreadable row is NOT "not a required check". Dropping it
            // would shrink the required set and let a malformed or newer
            // payload read as weaker enforcement than is actually in force —
            // the same permissive read this module refuses for an
            // unclassifiable ruleset target.
            match contexts {
                Ok(contexts) => Observed::observed(RequiredStatusChecks { strict, contexts }),
                Err(index) => Observed::not_proven(format!(
                    "required_status_checks entry {index} carries no readable context name; \
                     the required set cannot be established without dropping a row"
                )),
            }
        }
    };

    let enforce_admins = match value
        .get("enforce_admins")
        .and_then(|raw| raw.get("enabled"))
        .and_then(Value::as_bool)
    {
        Some(enabled) => Observed::observed(enabled),
        None => Observed::not_proven("enforce_admins missing or malformed in protection response"),
    };

    let required_pull_request_reviews = match value.get("required_pull_request_reviews") {
        None | Some(Value::Null) => Observed::absent("required_pull_request_reviews not present"),
        Some(Value::Object(raw)) => {
            let Some(required_approving_review_count) =
                optional_u32_field(raw, "required_approving_review_count")
            else {
                return malformed_classic_protection(
                    "required_pull_request_reviews.required_approving_review_count was malformed",
                );
            };
            let Some(dismiss_stale_reviews) = optional_bool_field(raw, "dismiss_stale_reviews")
            else {
                return malformed_classic_protection(
                    "required_pull_request_reviews.dismiss_stale_reviews was malformed",
                );
            };
            let Some(require_code_owner_reviews) =
                optional_bool_field(raw, "require_code_owner_reviews")
            else {
                return malformed_classic_protection(
                    "required_pull_request_reviews.require_code_owner_reviews was malformed",
                );
            };
            let Some(require_last_push_approval) =
                optional_bool_field(raw, "require_last_push_approval")
            else {
                return malformed_classic_protection(
                    "required_pull_request_reviews.require_last_push_approval was malformed",
                );
            };
            Observed::observed(PullRequestReviewRule {
                required_approving_review_count,
                dismiss_stale_reviews,
                require_code_owner_reviews,
                require_last_push_approval,
            })
        }
        Some(_) => {
            return malformed_classic_protection("required_pull_request_reviews was not an object");
        }
    };

    let required_conversation_resolution = match value
        .get("required_conversation_resolution")
        .and_then(|raw| raw.get("enabled"))
        .and_then(Value::as_bool)
    {
        Some(enabled) => Observed::observed(enabled),
        None => Observed::not_proven(
            "required_conversation_resolution missing or malformed in protection response",
        ),
    };

    let restrictions_present = match value.get("restrictions") {
        None | Some(Value::Null) => Observed::observed(false),
        Some(Value::Object(_)) => Observed::observed(true),
        Some(_) => {
            return malformed_classic_protection("restrictions was not an object or null");
        }
    };

    ClassicProtection {
        required_status_checks,
        enforce_admins,
        required_pull_request_reviews,
        required_conversation_resolution,
        restrictions_present,
    }
}

fn optional_u32_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<Option<u32>> {
    match object.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(Value::Number(value)) => {
            value.as_u64().and_then(|value| u32::try_from(value).ok()).map(Some)
        }
        Some(_) => None,
    }
}

fn optional_bool_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<Option<bool>> {
    match object.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(Value::Bool(value)) => Some(Some(*value)),
        Some(_) => None,
    }
}

fn malformed_classic_protection(message: &str) -> ClassicProtection {
    ClassicProtection {
        required_status_checks: Observed::not_proven(message),
        enforce_admins: Observed::not_proven(message),
        required_pull_request_reviews: Observed::not_proven(message),
        required_conversation_resolution: Observed::not_proven(message),
        restrictions_present: Observed::not_proven(message),
    }
}

#[derive(Deserialize)]
struct RawRulesetListItem {
    id: u64,
    name: String,
    target: String,
    enforcement: String,
}

#[derive(Deserialize)]
struct RawRulesetDetail {
    #[serde(default)]
    bypass_actors: Option<Vec<RawBypassActor>>,
    #[serde(default)]
    rules: Option<Vec<RawRulesetRule>>,
}

#[derive(Deserialize)]
struct RawBypassActor {
    actor_id: Option<u64>,
    #[serde(default)]
    actor_type: String,
    #[serde(default)]
    bypass_mode: String,
}

#[derive(Deserialize)]
struct RawRulesetRule {
    #[serde(rename = "type")]
    rule_type: String,
    #[serde(default)]
    parameters: Option<Value>,
}

/// Read branch and tag rulesets for `{owner}/{name}`, split by `target`.
///
/// An unrecognised `target` makes **both** returned collections `NOT_PROVEN`:
/// a row this build cannot classify must not silently vanish from either
/// bucket, which is exactly what dropping it would do. A per-ruleset detail
/// fetch that fails makes only that ruleset's `bypass_actors`/`rules`
/// `NOT_PROVEN` — the ruleset row itself still appears.
pub fn collect_rulesets(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> (Observed<Vec<Ruleset>>, Observed<Vec<Ruleset>>) {
    let list_path = format!("repos/{owner}/{name}/rulesets?includes_parents=true");
    let list_body = match commands.api(&list_path) {
        Ok(body) => body,
        Err(error) => {
            let detail = format!("reading rulesets for {owner}/{name}: {}", describe_error(&error));
            return (Observed::not_proven(detail.clone()), Observed::not_proven(detail));
        }
    };
    let items: Vec<RawRulesetListItem> = match serde_json::from_str(&list_body) {
        Ok(items) => items,
        Err(error) => {
            let detail = format!("ruleset listing for {owner}/{name} did not parse: {error}");
            return (Observed::not_proven(detail.clone()), Observed::not_proven(detail));
        }
    };

    let mut branch_rulesets = Vec::new();
    let mut tag_rulesets = Vec::new();
    let mut unrecognized: Vec<String> = Vec::new();

    for item in items {
        let (bypass_actors, rules) = fetch_ruleset_detail(commands, owner, name, item.id);
        let target = item.target.clone();
        let ruleset = Ruleset {
            id: item.id,
            name: item.name,
            target: item.target,
            enforcement: item.enforcement,
            bypass_actors,
            rules,
        };
        match target.as_str() {
            "branch" => branch_rulesets.push(ruleset),
            "tag" => tag_rulesets.push(ruleset),
            other => {
                unrecognized
                    .push(format!("ruleset {} has an unrecognised target {other:?}", ruleset.id));
            }
        }
    }

    if !unrecognized.is_empty() {
        let detail = unrecognized.join("; ");
        return (Observed::not_proven(detail.clone()), Observed::not_proven(detail));
    }

    (Observed::observed(branch_rulesets), Observed::observed(tag_rulesets))
}

fn fetch_ruleset_detail(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    id: u64,
) -> (Observed<Vec<BypassActor>>, Observed<Vec<RulesetRule>>) {
    let path = format!("repos/{owner}/{name}/rulesets/{id}");
    match commands.api(&path) {
        Ok(body) => match serde_json::from_str::<RawRulesetDetail>(&body) {
            Ok(detail) => (observed_bypass_actors(&detail), observed_rules(&detail)),
            Err(error) => {
                let message =
                    format!("ruleset {id} detail for {owner}/{name} did not parse: {error}");
                (Observed::not_proven(message.clone()), Observed::not_proven(message))
            }
        },
        Err(error) => {
            let message = format!(
                "reading ruleset {id} detail for {owner}/{name}: {}",
                describe_error(&error)
            );
            (Observed::not_proven(message.clone()), Observed::not_proven(message))
        }
    }
}

/// `NOT_PROVEN` when the detail payload omits `bypass_actors` entirely (the
/// field key absent, or explicitly `null`); `Observed(vec![])` only when the
/// payload carries an explicit empty array. These must never be conflated.
fn observed_bypass_actors(detail: &RawRulesetDetail) -> Observed<Vec<BypassActor>> {
    match &detail.bypass_actors {
        Some(actors) => Observed::observed(
            actors
                .iter()
                .map(|actor| BypassActor {
                    actor_id: actor.actor_id,
                    actor_type: actor.actor_type.clone(),
                    bypass_mode: actor.bypass_mode.clone(),
                })
                .collect(),
        ),
        None => Observed::not_proven("ruleset payload omitted bypass_actors"),
    }
}

fn observed_rules(detail: &RawRulesetDetail) -> Observed<Vec<RulesetRule>> {
    match &detail.rules {
        Some(rules) => match rules.iter().map(parse_ruleset_rule).collect() {
            Ok(parsed) => Observed::observed(parsed),
            Err(reason) => Observed::not_proven(reason),
        },
        None => Observed::not_proven("ruleset payload omitted rules"),
    }
}

/// Parse one ruleset rule, refusing to drop a required-status-check entry
/// whose context name cannot be read.
///
/// A silently dropped entry would under-report the contexts a ruleset
/// actually enforces, which is the precise misreading this observer exists to
/// prevent — so an unreadable entry makes the whole `rules` observation
/// `NOT_PROVEN` rather than yielding a smaller, confident-looking list.
fn parse_ruleset_rule(raw: &RawRulesetRule) -> Result<RulesetRule, String> {
    let mut required_contexts = Vec::new();
    let mut required_approving_review_count = None;
    let mut required_review_thread_resolution = None;
    let mut dismiss_stale_reviews_on_push = None;

    if raw.rule_type == "required_status_checks" {
        let parameters = raw
            .parameters
            .as_ref()
            .ok_or_else(|| "required_status_checks rule omitted parameters".to_string())?;
        let parameters = parameters.as_object().ok_or_else(|| {
            "required_status_checks rule parameters must be an object".to_string()
        })?;
        let checks = parameters
            .get("required_status_checks")
            .ok_or_else(|| {
                "required_status_checks rule omitted required_status_checks".to_string()
            })?
            .as_array()
            .ok_or_else(|| {
                "required_status_checks rule required_status_checks must be an array".to_string()
            })?;
        {
            for (index, check) in checks.iter().enumerate() {
                match check.get("context").and_then(Value::as_str) {
                    Some(context) => required_contexts.push(context.to_string()),
                    None => {
                        return Err(format!(
                            "rule {:?} required_status_checks entry {index} carries no readable \
                             context name; the enforced set cannot be established without \
                             dropping a row",
                            raw.rule_type
                        ));
                    }
                }
            }
        }
        required_approving_review_count = parameters
            .get("required_approving_review_count")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        required_review_thread_resolution =
            parameters.get("required_review_thread_resolution").and_then(Value::as_bool);
        dismiss_stale_reviews_on_push =
            parameters.get("dismiss_stale_reviews_on_push").and_then(Value::as_bool);
    } else if let Some(parameters) = &raw.parameters {
        required_approving_review_count = parameters
            .get("required_approving_review_count")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        required_review_thread_resolution =
            parameters.get("required_review_thread_resolution").and_then(Value::as_bool);
        dismiss_stale_reviews_on_push =
            parameters.get("dismiss_stale_reviews_on_push").and_then(Value::as_bool);
    }

    Ok(RulesetRule {
        rule_type: raw.rule_type.clone(),
        required_contexts,
        required_approving_review_count,
        required_review_thread_resolution,
        dismiss_stale_reviews_on_push,
    })
}

/// Read deployment environments for `{owner}/{name}`, then per-environment
/// protection rules, deployment-branch policy, and secret **count**.
///
/// REDACTION LAW: only counts, types, and names of environments/rules are
/// ever recorded here — never a secret value, and never a secret *name*.
pub fn collect_environments(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> Observed<Vec<Environment>> {
    let list_path = format!("repos/{owner}/{name}/environments");
    let list_body = match commands.api(&list_path) {
        Ok(body) => body,
        Err(error) => {
            return Observed::not_proven(format!(
                "reading environments for {owner}/{name}: {}",
                describe_error(&error)
            ));
        }
    };
    let list: Value = match serde_json::from_str(&list_body) {
        Ok(value) => value,
        Err(error) => {
            return Observed::not_proven(format!(
                "environment listing for {owner}/{name} did not parse: {error}"
            ));
        }
    };
    let Some(items) = list.get("environments").and_then(Value::as_array) else {
        return Observed::not_proven(format!(
            "environment listing for {owner}/{name} carried no environments array"
        ));
    };

    let mut environments = Vec::new();
    for item in items {
        let Some(env_name) = item.get("name").and_then(Value::as_str) else {
            return Observed::not_proven(format!(
                "an environment for {owner}/{name} carried no name"
            ));
        };
        environments.push(collect_one_environment(commands, owner, name, env_name));
    }
    Observed::observed(environments)
}

fn collect_one_environment(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    env_name: &str,
) -> Environment {
    let detail_path = format!("repos/{owner}/{name}/environments/{env_name}");
    let (protection_rules, deployment_branch_policy) = match commands.api(&detail_path) {
        Ok(body) => match serde_json::from_str::<Value>(&body) {
            Ok(detail) => (
                parse_environment_protection_rules(&detail),
                parse_deployment_branch_policy(&detail),
            ),
            Err(error) => {
                let message = format!(
                    "environment {env_name} detail for {owner}/{name} did not parse: {error}"
                );
                (Observed::not_proven(message.clone()), Observed::not_proven(message))
            }
        },
        Err(error) => {
            let message = format!(
                "reading environment {env_name} detail for {owner}/{name}: {}",
                describe_error(&error)
            );
            (Observed::not_proven(message.clone()), Observed::not_proven(message))
        }
    };

    let secrets_path = format!("repos/{owner}/{name}/environments/{env_name}/secrets");
    let secret_count = match commands.api(&secrets_path) {
        Ok(body) => match serde_json::from_str::<Value>(&body) {
            Ok(value) => match value.get("total_count").and_then(Value::as_u64) {
                Some(count) => Observed::observed(count as usize),
                None => Observed::not_proven(format!(
                    "environment {env_name} secrets listing for {owner}/{name} carried no total_count"
                )),
            },
            Err(error) => Observed::not_proven(format!(
                "environment {env_name} secrets listing for {owner}/{name} did not parse: {error}"
            )),
        },
        Err(error) => Observed::not_proven(format!(
            "reading environment {env_name} secrets for {owner}/{name}: {}",
            describe_error(&error)
        )),
    };

    Environment {
        name: env_name.to_string(),
        protection_rules,
        deployment_branch_policy,
        secret_count,
    }
}

fn parse_environment_protection_rules(detail: &Value) -> Observed<Vec<EnvironmentProtectionRule>> {
    match detail.get("protection_rules") {
        None => Observed::not_proven("environment payload omitted protection_rules"),
        Some(value) => match value.as_array() {
            None => Observed::not_proven("environment protection_rules was not an array"),
            Some(rows) => Observed::observed(
                rows.iter()
                    .map(|row| EnvironmentProtectionRule {
                        rule_type: row
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string(),
                        wait_timer: row.get("wait_timer").and_then(Value::as_u64),
                        reviewer_count: row
                            .get("reviewers")
                            .and_then(Value::as_array)
                            .map(Vec::len),
                        prevent_self_review: row
                            .get("prevent_self_review")
                            .and_then(Value::as_bool),
                    })
                    .collect(),
            ),
        },
    }
}

fn parse_deployment_branch_policy(detail: &Value) -> Observed<Option<DeploymentBranchPolicy>> {
    match detail.get("deployment_branch_policy") {
        None => Observed::not_proven("environment payload omitted deployment_branch_policy"),
        Some(Value::Null) => Observed::observed(None),
        Some(value) => {
            let protected_branches = value.get("protected_branches").and_then(Value::as_bool);
            let custom_branch_policies =
                value.get("custom_branch_policies").and_then(Value::as_bool);
            match (protected_branches, custom_branch_policies) {
                (Some(protected_branches), Some(custom_branch_policies)) => {
                    Observed::observed(Some(DeploymentBranchPolicy {
                        protected_branches,
                        custom_branch_policies,
                    }))
                }
                _ => Observed::not_proven(
                    "environment deployment_branch_policy object was missing protected_branches or custom_branch_policies",
                ),
            }
        }
    }
}

/// Repository release posture: `immutable_releases` straight from the
/// repository payload (`NOT_PROVEN` when the field is absent — never
/// `false`), and `tag_rulesets_present` derived from `tag_rulesets`, which is
/// conclusive only when that observation itself was.
pub fn collect_release_posture(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    tag_rulesets: &Observed<Vec<Ruleset>>,
) -> ReleasePosture {
    let immutable_releases = match commands.api(&format!("repos/{owner}/{name}")) {
        Ok(body) => match serde_json::from_str::<Value>(&body) {
            Ok(value) => match value.get("immutable_releases").and_then(Value::as_bool) {
                Some(flag) => Observed::observed(flag),
                None => Observed::not_proven(format!(
                    "repository payload for {owner}/{name} did not carry immutable_releases"
                )),
            },
            Err(error) => Observed::not_proven(format!(
                "repository payload for {owner}/{name} did not parse: {error}"
            )),
        },
        Err(error) => Observed::not_proven(format!(
            "reading repository {owner}/{name}: {}",
            describe_error(&error)
        )),
    };

    let tag_rulesets_present = if tag_rulesets.is_conclusive() {
        Observed::observed(tag_rulesets.value().is_some_and(|rulesets| !rulesets.is_empty()))
    } else {
        Observed::not_proven("tag ruleset observation is not conclusive")
    };

    ReleasePosture { immutable_releases, tag_rulesets_present }
}

/// Observe every requested repository and assemble the complete receipt.
///
/// Sets `currency: Currency::Live`, since this is the one path that reads
/// the real API. Nothing here mutates anything.
pub fn observe(
    commands: &dyn ReadOnlyCommands,
    subjects: &[RepositorySubject],
    observed_at: String,
) -> LiveControlsReceipt {
    let instrument = match commands.gh_version() {
        Ok(version) => Instrument {
            state: ObservationState::Observed,
            gh_version: Some(version.trim().to_string()),
            detail: None,
        },
        Err(error) => Instrument {
            state: ObservationState::NotProven,
            gh_version: None,
            detail: Some(describe_error(&error)),
        },
    };

    let repositories: Vec<RepositoryControls> =
        subjects.iter().map(|subject| observe_repository(commands, subject)).collect();

    let verdict = evaluate::verdict(&repositories);
    let limitations = evaluate::limitations(&repositories);

    LiveControlsReceipt {
        schema_version: RELEASE_LIVE_CONTROLS_SCHEMA_VERSION.to_string(),
        observed_at,
        currency: Currency::Live,
        instrument,
        repositories,
        verdict,
        limitations,
    }
}

fn observe_repository(
    commands: &dyn ReadOnlyCommands,
    subject: &RepositorySubject,
) -> RepositoryControls {
    let identity = collect_identity(commands, &subject.owner, &subject.name);
    let identity_match = evaluate::identity_match(subject, &identity);
    let classic_branch_protection =
        collect_classic_protection(commands, &subject.owner, &subject.name, &subject.branch);
    let (branch_rulesets, tag_rulesets) = collect_rulesets(commands, &subject.owner, &subject.name);
    let environments = collect_environments(commands, &subject.owner, &subject.name);
    let release_posture =
        collect_release_posture(commands, &subject.owner, &subject.name, &tag_rulesets);
    let required_contexts_union =
        evaluate::required_contexts_union(&classic_branch_protection, &branch_rulesets);

    RepositoryControls {
        requested: subject.clone(),
        identity,
        identity_match,
        classic_branch_protection,
        branch_rulesets,
        tag_rulesets,
        environments,
        release_posture,
        required_contexts_union,
    }
}
