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

/// Percent-encode one REST path segment, keeping only RFC 3986 unreserved
/// bytes. A branch named `release/1.0` must address
/// `branches/release%2F1.0`, not a different route.
pub fn encode_path_segment(value: &str) -> String {
    fn hex_digit(value: u8) -> char {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        HEX[usize::from(value & 0x0f)] as char
    }
    let mut encoded = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn repo_path(owner: &str, name: &str) -> String {
    format!("repos/{}/{}", encode_path_segment(owner), encode_path_segment(name))
}

/// Read `repos/{owner}/{name}` exactly once. Both the identity and the
/// release posture derive from this single payload, so the two can never
/// disagree with each other.
fn fetch_repository_payload(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> Result<Value, String> {
    let body = commands.api(&repo_path(owner, name)).map_err(|error| {
        format!("reading repository {owner}/{name}: {}", describe_error(&error))
    })?;
    serde_json::from_str(&body)
        .map_err(|error| format!("repository payload for {owner}/{name} did not parse: {error}"))
}

/// Read `repos/{owner}/{name}` and derive the identity from it.
pub fn collect_identity(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> Observed<RepositoryIdentity> {
    identity_from_payload(&fetch_repository_payload(commands, owner, name), owner, name)
}

/// Every field named here (`full_name`, `node_id`, `id`, `default_branch`)
/// must be present; any one missing yields `NOT_PROVEN` rather than a
/// partially populated identity.
fn identity_from_payload(
    payload: &Result<Value, String>,
    owner: &str,
    name: &str,
) -> Observed<RepositoryIdentity> {
    let value = match payload {
        Ok(value) => value,
        Err(detail) => return Observed::not_proven(detail.clone()),
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
    let branch_path =
        format!("{}/branches/{}", repo_path(owner, name), encode_path_segment(branch));
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
    #[serde(default)]
    conditions: Option<Value>,
}

/// Largest number of pages any list endpoint is followed for. A listing that
/// is still full at this bound is `NOT_PROVEN`, never truncated silently.
const MAX_PAGES: u32 = 20;
const PAGE_SIZE: usize = 100;

/// Follow `page=1..` on an endpoint that returns a bare JSON array until a
/// short page arrives. A page that fails, does not parse, or is still full at
/// [`MAX_PAGES`] fails the whole listing: page 1 alone must never stand in
/// for the collection.
fn paginate_array(
    commands: &dyn ReadOnlyCommands,
    path_with_query: &str,
    label: &str,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    for page in 1..=MAX_PAGES {
        let path = format!("{path_with_query}&per_page={PAGE_SIZE}&page={page}");
        let body = commands
            .api(&path)
            .map_err(|error| format!("reading {label} page {page}: {}", describe_error(&error)))?;
        let page_items: Vec<Value> = serde_json::from_str(&body)
            .map_err(|error| format!("{label} page {page} did not parse: {error}"))?;
        let short = page_items.len() < PAGE_SIZE;
        items.extend(page_items);
        if short {
            return Ok(items);
        }
    }
    Err(format!("{label} was still full after {MAX_PAGES} pages; refusing to truncate"))
}

/// Whether a ruleset's `conditions.ref_name` selects `refs/heads/{branch}`.
///
/// Include patterns are GitHub's: `~ALL`, `~DEFAULT_BRANCH`, or an fnmatch
/// pattern over the full ref (`*` does not cross `/`, `**` does). An
/// exclude match wins over any include. `~DEFAULT_BRANCH` needs the
/// repository's observed default branch; without it the answer is
/// `NOT_PROVEN`, and so is a payload whose `conditions.ref_name` cannot be
/// read at all: an unreadable condition set is a gap, not an exclusion.
pub fn ruleset_applies_to_branch(
    conditions: Option<&Value>,
    branch: &str,
    default_branch: Option<&str>,
) -> Observed<bool> {
    let Some(ref_name) = conditions.and_then(|conditions| conditions.get("ref_name")) else {
        return Observed::not_proven("ruleset detail carried no conditions.ref_name");
    };
    let patterns = |key: &str| -> Result<Vec<String>, String> {
        match ref_name.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("conditions.ref_name.{key} carried a non-string"))
                })
                .collect(),
            Some(_) => Err(format!("conditions.ref_name.{key} is not an array")),
        }
    };
    let (include, exclude) = match (patterns("include"), patterns("exclude")) {
        (Ok(include), Ok(exclude)) => (include, exclude),
        (Err(detail), _) | (_, Err(detail)) => return Observed::not_proven(detail),
    };

    let full_ref = format!("refs/heads/{branch}");
    let matches = |pattern: &str| -> Option<bool> {
        match pattern {
            "~ALL" => Some(true),
            "~DEFAULT_BRANCH" => default_branch.map(|default| default == branch),
            other => Some(ref_glob_matches(other, &full_ref)),
        }
    };
    let mut included = false;
    for pattern in &include {
        match matches(pattern) {
            Some(true) => included = true,
            Some(false) => {}
            None => {
                return Observed::not_proven(
                    "ruleset includes ~DEFAULT_BRANCH but the default branch was not observed",
                );
            }
        }
    }
    if !included {
        return Observed::observed(false);
    }
    for pattern in &exclude {
        match matches(pattern) {
            Some(true) => return Observed::observed(false),
            Some(false) => {}
            None => {
                return Observed::not_proven(
                    "ruleset excludes ~DEFAULT_BRANCH but the default branch was not observed",
                );
            }
        }
    }
    Observed::observed(true)
}

/// fnmatch over a git ref: `**` matches across `/`, `*` within one segment,
/// `?` one non-`/` character; everything else is literal.
fn ref_glob_matches(pattern: &str, subject: &str) -> bool {
    fn go(pattern: &[u8], subject: &[u8]) -> bool {
        match pattern {
            [] => subject.is_empty(),
            [b'*', b'*', rest @ ..] => {
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=subject.len()).any(|split| go(rest, &subject[split..]))
            }
            [b'*', rest @ ..] => (0..=subject.len())
                .take_while(|&split| split == 0 || subject[split - 1] != b'/')
                .any(|split| go(rest, &subject[split..])),
            [b'?', rest @ ..] => match subject {
                [first, tail @ ..] if *first != b'/' => go(rest, tail),
                _ => false,
            },
            [literal, rest @ ..] => match subject {
                [first, tail @ ..] if first == literal => go(rest, tail),
                _ => false,
            },
        }
    }
    go(pattern.as_bytes(), subject.as_bytes())
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
    branch: &str,
    default_branch: Option<&str>,
) -> (Observed<Vec<Ruleset>>, Observed<Vec<Ruleset>>) {
    let list_path = format!("{}/rulesets?includes_parents=true", repo_path(owner, name));
    let raw_items =
        match paginate_array(commands, &list_path, &format!("rulesets for {owner}/{name}")) {
            Ok(items) => items,
            Err(detail) => {
                return (Observed::not_proven(detail.clone()), Observed::not_proven(detail));
            }
        };
    let items: Vec<RawRulesetListItem> =
        match raw_items.into_iter().map(serde_json::from_value).collect::<Result<Vec<_>, _>>() {
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
        let detail = fetch_ruleset_detail(commands, owner, name, item.id);
        let (bypass_actors, rules, conditions) = match &detail {
            Ok(detail) => (
                observed_bypass_actors(detail),
                observed_rules(detail),
                Some(detail.conditions.as_ref()),
            ),
            Err(message) => {
                (Observed::not_proven(message.clone()), Observed::not_proven(message.clone()), None)
            }
        };
        let applies_to_branch = match (item.target.as_str(), conditions) {
            ("branch", Some(conditions)) => {
                ruleset_applies_to_branch(conditions, branch, default_branch)
            }
            ("branch", None) => Observed::not_proven("ruleset detail was not readable"),
            _ => Observed::absent("only branch rulesets apply to a branch"),
        };
        let target = item.target.clone();
        let ruleset = Ruleset {
            id: item.id,
            name: item.name,
            target: item.target,
            enforcement: item.enforcement,
            applies_to_branch,
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
) -> Result<RawRulesetDetail, String> {
    let path = format!("{}/rulesets/{id}", repo_path(owner, name));
    let body = commands.api(&path).map_err(|error| {
        format!("reading ruleset {id} detail for {owner}/{name}: {}", describe_error(&error))
    })?;
    serde_json::from_str::<RawRulesetDetail>(&body)
        .map_err(|error| format!("ruleset {id} detail for {owner}/{name} did not parse: {error}"))
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
    let items = match paginate_environment_listing(commands, owner, name) {
        Ok(items) => items,
        Err(detail) => return Observed::not_proven(detail),
    };

    let mut environments = Vec::new();
    for item in &items {
        let Some(env_name) = item.get("name").and_then(Value::as_str) else {
            return Observed::not_proven(format!(
                "an environment for {owner}/{name} carried no name"
            ));
        };
        environments.push(collect_one_environment(commands, owner, name, env_name));
    }
    Observed::observed(environments)
}

/// The environments endpoint wraps its page in `{total_count, environments}`.
/// Pages are followed until `total_count` rows have been read; a short page
/// before that, or a listing still growing at [`MAX_PAGES`], is a failure.
fn paginate_environment_listing(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> Result<Vec<Value>, String> {
    let base = format!("{}/environments", repo_path(owner, name));
    let mut items: Vec<Value> = Vec::new();
    let mut expected: Option<usize> = None;
    for page in 1..=MAX_PAGES {
        let path = format!("{base}?per_page={PAGE_SIZE}&page={page}");
        let body = commands.api(&path).map_err(|error| {
            format!(
                "reading environments for {owner}/{name} page {page}: {}",
                describe_error(&error)
            )
        })?;
        let list: Value = serde_json::from_str(&body).map_err(|error| {
            format!("environment listing for {owner}/{name} page {page} did not parse: {error}")
        })?;
        let total = list.get("total_count").and_then(Value::as_u64).ok_or_else(|| {
            format!("environment listing for {owner}/{name} page {page} carried no total_count")
        })?;
        let total = usize::try_from(total).map_err(|_| "total_count overflowed".to_string())?;
        if let Some(previous) = expected
            && previous != total
        {
            return Err(format!(
                "environment listing for {owner}/{name} changed total_count between pages ({previous} then {total})"
            ));
        }
        expected = Some(total);
        let page_items = list.get("environments").and_then(Value::as_array).ok_or_else(|| {
            format!(
                "environment listing for {owner}/{name} page {page} carried no environments array"
            )
        })?;
        if page_items.is_empty() && items.len() < total {
            return Err(format!(
                "environment listing for {owner}/{name} ended at {} of {total} rows",
                items.len()
            ));
        }
        items.extend(page_items.iter().cloned());
        if items.len() >= total {
            return Ok(items);
        }
    }
    Err(format!(
        "environment listing for {owner}/{name} was still growing after {MAX_PAGES} pages; refusing to truncate"
    ))
}

fn collect_one_environment(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    env_name: &str,
) -> Environment {
    let detail_path =
        format!("{}/environments/{}", repo_path(owner, name), encode_path_segment(env_name));
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

    let secrets_path = format!("{detail_path}/secrets");
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
    repository_payload: &Result<Value, String>,
    owner: &str,
    name: &str,
    tag_rulesets: &Observed<Vec<Ruleset>>,
) -> ReleasePosture {
    let immutable_releases = match repository_payload {
        Ok(value) => match value.get("immutable_releases").and_then(Value::as_bool) {
            Some(flag) => Observed::observed(flag),
            None => Observed::not_proven(format!(
                "repository payload for {owner}/{name} did not carry immutable_releases"
            )),
        },
        Err(detail) => Observed::not_proven(detail.clone()),
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

    let verdict = evaluate::receipt_verdict(&instrument, &repositories);
    let limitations = evaluate::receipt_limitations(&instrument, &repositories);

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
    let repository_payload = fetch_repository_payload(commands, &subject.owner, &subject.name);
    let identity = identity_from_payload(&repository_payload, &subject.owner, &subject.name);
    let identity_match = evaluate::identity_match(subject, &identity);
    let default_branch = identity.value().map(|identity| identity.default_branch.as_str());
    let classic_branch_protection =
        collect_classic_protection(commands, &subject.owner, &subject.name, &subject.branch);
    let (branch_rulesets, tag_rulesets) =
        collect_rulesets(commands, &subject.owner, &subject.name, &subject.branch, default_branch);
    let environments = collect_environments(commands, &subject.owner, &subject.name);
    let release_posture =
        collect_release_posture(&repository_payload, &subject.owner, &subject.name, &tag_rulesets);
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
