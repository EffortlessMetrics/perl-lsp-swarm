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

use super::api_value as field;
pub use super::environment::collect_environments;
use super::evaluate;
use super::model::{
    BypassActor, ClassicProtection, Currency, Instrument, LiveControlsReceipt, ObservationState,
    Observed, RELEASE_LIVE_CONTROLS_SCHEMA_VERSION, ReleasePosture, RepositoryControls,
    RepositoryIdentity, RepositorySubject, Ruleset, RulesetRule,
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
        run_gh(&["api", "--method", "GET", path])
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
        Some(status) => format!("GitHub read failed (HTTP {status})"),
        None => "GitHub read failed without a recoverable HTTP status".into(),
    }
}

pub(super) fn read_json(commands: &dyn ReadOnlyCommands, path: &str) -> Result<Value, String> {
    let body = commands.api(path).map_err(|error| describe_error(&error))?;
    serde_json::from_str(&body).map_err(|_| "GitHub response was not valid JSON".into())
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

pub(super) fn repo_path(owner: &str, name: &str) -> String {
    format!("repos/{}/{}", encode_path_segment(owner), encode_path_segment(name))
}

/// Read repository identity once; immutable-release settings use their own endpoint.
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
    _owner: &str,
    _name: &str,
) -> Observed<RepositoryIdentity> {
    let value = match payload {
        Ok(value) => value,
        Err(detail) => return Observed::not_proven(detail.clone()),
    };

    field::observation((|| {
        Ok(RepositoryIdentity {
            full_name: field::string(value, "full_name")?,
            node_id: field::string(value, "node_id")?,
            database_id: field::id(value, "id")?,
            default_branch: field::string(value, "default_branch")?,
        })
    })())
}

#[derive(Deserialize)]
struct RawBranch {
    name: String,
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
    let observed_branch = commands
        .api(&branch_path)
        .ok()
        .and_then(|body| serde_json::from_str::<RawBranch>(&body).ok());
    let protected = match observed_branch {
        Some(observed) if observed.name == branch => observed.protected,
        _ => {
            return Observed::not_proven(
                "branch response identity did not match the requested branch",
            );
        }
    };

    let protection_path = format!("{branch_path}/protection");
    match commands.api(&protection_path) {
        Ok(body) => match serde_json::from_str::<Value>(&body) {
            Ok(value) => Observed::observed(super::classic::parse(&value)),
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

#[derive(Deserialize)]
struct RawRulesetListItem {
    id: u64,
    name: String,
    target: String,
    enforcement: String,
}

#[derive(Deserialize)]
struct RawRulesetDetail {
    id: u64,
    name: String,
    target: String,
    enforcement: String,
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
            .map_err(|_| format!("{label} page {page} did not match the expected array shape"))?;
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
/// pattern over the full ref (`*` stays in a segment; a whole `**/` component
/// can cross directories). An
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
            None | Some(Value::Null) => {
                Err(format!("conditions.ref_name.{key} was omitted or null"))
            }
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
    let matches = |pattern: &str| -> Result<bool, String> {
        match pattern {
            "~ALL" => Ok(true),
            "~DEFAULT_BRANCH" => default_branch
                .map(|default| default == branch)
                .ok_or_else(|| "default branch was not observed".into()),
            other if other.starts_with('~') => Err("unsupported special ref selector".into()),
            other => super::ref_pattern::matches(other, &full_ref),
        }
    };
    let any_match = |patterns: &[String]| {
        let mut unknown = None;
        for pattern in patterns {
            match matches(pattern) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(reason) => unknown = Some(reason),
            }
        }
        unknown.map_or(Ok(false), Err)
    };
    match (any_match(&include), any_match(&exclude)) {
        (_, Ok(true)) | (Ok(false), _) => Observed::observed(false),
        (Ok(true), Ok(false)) => Observed::observed(true),
        (Err(reason), _) | (_, Err(reason)) => Observed::not_proven(reason),
    }
}

#[derive(Deserialize)]
struct RawBypassActor {
    actor_id: Option<u64>,
    #[serde(default)]
    actor_type: Option<String>,
    #[serde(default)]
    bypass_mode: Option<String>,
}

/// The enforcement modes GitHub documents. Anything else is an unknown this
/// build cannot classify, and must not read as "not enforced".
const KNOWN_ENFORCEMENT: [&str; 3] = ["active", "evaluate", "disabled"];

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
            Err(_) => {
                let detail = "ruleset listing did not match the expected payload shape".to_string();
                return (Observed::not_proven(detail.clone()), Observed::not_proven(detail));
            }
        };

    let mut branch_rulesets = Vec::new();
    let mut tag_rulesets = Vec::new();
    let mut unrecognized: Vec<String> = Vec::new();

    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        if item.id == 0 || item.name.trim().is_empty() || !seen.insert(item.id) {
            return (
                Observed::not_proven("ruleset listing has an invalid or repeated identity"),
                Observed::not_proven("ruleset listing has an invalid or repeated identity"),
            );
        }
        let detail = fetch_ruleset_detail(commands, owner, name, item.id).and_then(|detail| {
            if detail.id != item.id
                || detail.name != item.name
                || detail.target != item.target
                || detail.enforcement != item.enforcement
            {
                Err("ruleset detail identity or enforcement disagrees with its listing".into())
            } else {
                Ok(detail)
            }
        });
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
            _ if !KNOWN_ENFORCEMENT.contains(&item.enforcement.as_str()) => {
                Observed::not_proven(format!(
                    "ruleset {} carries an unrecognised enforcement {:?}",
                    item.id, item.enforcement
                ))
            }
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
        .map_err(|_| "ruleset detail did not match the expected payload shape".to_string())
}

/// `NOT_PROVEN` when the detail payload omits `bypass_actors` entirely (the
/// field key absent, or explicitly `null`); `Observed(vec![])` only when the
/// payload carries an explicit empty array. These must never be conflated.
fn observed_bypass_actors(detail: &RawRulesetDetail) -> Observed<Vec<BypassActor>> {
    let Some(actors) = &detail.bypass_actors else {
        return Observed::not_proven("ruleset payload omitted bypass_actors");
    };
    let mut rows = Vec::with_capacity(actors.len());
    for (index, actor) in actors.iter().enumerate() {
        // A bypass row missing its type or mode is a bypass we cannot
        // characterise, not an empty one.
        let (Some(actor_type), Some(bypass_mode)) = (&actor.actor_type, &actor.bypass_mode) else {
            return Observed::not_proven(format!(
                "bypass_actors[{index}] omitted actor_type or bypass_mode"
            ));
        };
        if !matches!(
            actor_type.as_str(),
            "Integration" | "OrganizationAdmin" | "RepositoryRole" | "Team" | "DeployKey"
        ) || !matches!(bypass_mode.as_str(), "always" | "pull_request" | "exempt")
            || actor.actor_id == Some(0)
            || (actor.actor_id.is_none()
                && !matches!(actor_type.as_str(), "OrganizationAdmin" | "DeployKey"))
        {
            return Observed::not_proven(
                "bypass actor carries an unsupported type, mode or identifier",
            );
        }
        rows.push(BypassActor {
            actor_id: actor.actor_id,
            actor_type: actor_type.clone(),
            bypass_mode: bypass_mode.clone(),
        });
    }
    Observed::observed(rows)
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
    super::rules::parse(&raw.rule_type, raw.parameters.as_ref())
}

/// Read repository immutable-release settings from the dedicated endpoint.
pub fn collect_release_posture(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    tag_rulesets: &Observed<Vec<Ruleset>>,
) -> ReleasePosture {
    let settings = read_json(commands, &format!("{}/immutable-releases", repo_path(owner, name)));
    let read = |key| match &settings {
        Ok(value) => field::observation(field::boolean(value, key)),
        Err(detail) => Observed::not_proven(detail.clone()),
    };
    let immutable_releases = read("enabled");
    let immutable_releases_enforced_by_owner = read("enforced_by_owner");
    let tag_rulesets_present = if tag_rulesets.is_conclusive() {
        Observed::observed(tag_rulesets.value().is_some_and(|rulesets| !rulesets.is_empty()))
    } else {
        Observed::not_proven("tag ruleset observation is not conclusive")
    };
    ReleasePosture {
        immutable_releases,
        immutable_releases_enforced_by_owner,
        tag_rulesets_present,
    }
}

/// Observe every requested repository and assemble the complete receipt.
///
/// Sets `currency: Currency::Live`, since this is the one path that reads
/// the real API. Nothing here mutates anything.
///
/// Callers supply valid repository subjects with nonempty owner, name, and
/// branch fields, and an RFC 3339 observation timestamp. This low-level collector
/// does not validate caller-constructed subjects or timestamps; [`super::run`]
/// admits repository/branch strings before invoking it.
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
