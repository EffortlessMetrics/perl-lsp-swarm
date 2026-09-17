use chrono::{NaiveDate, Utc};
use color_eyre::eyre::{Context, Result, bail};
use serde::Serialize;
use serde_yaml_ng::{Mapping, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::project_root;

/// Registry of runner isolation profiles for self-hosted capacity that can
/// execute pull-request-controlled code (#15070, under #7414).
const SELF_HOSTED_ISOLATION_POLICY: &str = "policy/self-hosted-runner-isolation.toml";

/// Runner substrate lifecycles a profile may declare.
///
/// `persistent` is a statement of fact, not a safety claim: it records that
/// cross-run state survives, which is precisely why #7414 still owns the
/// runtime decontamination proof.
const ISOLATION_LIFECYCLES: &[&str] = &["disposable", "reimaged", "persistent"];

/// Triggers from which a pull-request candidate can cause a run to start.
///
/// `merge_group` is deliberately excluded: it runs approved content after
/// review, which is a different trust subject from an open candidate.
///
/// `workflow_call` is included because a reusable workflow's reachability is
/// defined entirely by its callers, which this per-file walk cannot see. A
/// callee that declares `on: workflow_call:` and routes to self-hosted capacity
/// must therefore carry a profile regardless of who calls it today.
const PR_CONTROLLED_TRIGGERS: &[&str] = &[
    "pull_request",
    "pull_request_target",
    "pull_request_review",
    "pull_request_review_comment",
    "issue_comment",
    "workflow_call",
];

/// Label prefixes that identify GitHub-hosted capacity.
///
/// A `runs-on:` value outside this set cannot be cleared: a self-hosted runner
/// can be targeted by a custom label alone, without the implicit `self-hosted`
/// label ever appearing in the workflow file.
const GITHUB_HOSTED_LABEL_PREFIXES: &[&str] = &["ubuntu-", "windows-", "macos-"];

/// Files every `cargo xtask <subcommand>` invocation routes through.
///
/// `xtask/src/main.rs` is the clap dispatch and `xtask/src/tasks/mod.rs` is the
/// module registration it reaches subcommands through. `mod tasks;` is declared
/// in `main.rs`, not `lib.rs`, so both compile into the default `xtask` binary
/// and into no other target. A paths-filtered gate that runs a subcommand but
/// omits them cannot observe a change to its own dispatch (#14293).
const XTASK_CLI_WIRING_FILES: &[&str] = &["xtask/src/main.rs", "xtask/src/tasks/mod.rs"];

const ALLOWLIST_PR_CONTENTS_WRITE: &[&str] = &["ci.yml", "ci-nightly.yml", "droid-review.yml"];
const POLICY_WARN_UNPINNED_ACTIONS: bool = true;
const ALLOWLIST_BLANKET_CANCEL_IN_PROGRESS: &[&str] = &["docs-deploy.yml", "post-merge-status.yml"];

/// Multi-job workflows whose jobs may inherit workflow-default write authority
/// without declaring their own `permissions:`. Add an entry only with a
/// documented reason and a tracking issue.
///
/// `docker-publish.yml` was removed from this list by #12888: the digest-split
/// topology gives every job an explicit grant (`packages: write` exists only on
/// the checkout-free `publish-ghcr` publication job) and the workflow default
/// carries no write scope, so the allowlist entry became dead allowance.
const ALLOWLIST_INHERITED_JOB_WRITE: &[&str] = &[];

/// Workflow files that intentionally have no `policy/ci-lane-whitelist.toml`
/// entry. Add an entry here only when there's a documented reason — e.g. a
/// release/publish workflow that's release-time-only and not part of
/// per-PR economics.
const ALLOWLIST_WORKFLOW_LANE_MISSING: &[&str] = &[
    // Release / publish workflows: out of scope for the per-PR economics map.
    "brew-bump.yml",
    "chocolatey-bump.yml",
    "docker-publish.yml",
    "docs-deploy.yml",
    "post-merge-corpus-ratchet.yml",
    "post-merge-status.yml",
    "post-publish-smoke.yml",
    "publish-crates.yml",
    "publish-extension.yml",
    "publish-dry-run.yml",
    "release-orchestration.yml",
    "release.yml",
    "scoop-bump.yml",
    "tokmd.yml",
    "version-bump.yml",
    "vscode-published-extension-smoke.yml",
    "winget-bump.yml",
    // Schedule/utility workflows tracked separately from the lane economics.
    "ci-gate-self-tests.yml",
    // Manual-only, read-only macOS measurement lane (#5432). It never runs on a
    // pull request or merge group, so it carries no per-PR cost and has no
    // place on the lane economics map; its contract is proven by
    // `xtask/tests/release_artifact_size_shadow_workflow.rs` instead.
    "release-artifact-size-shadow.yml",
    // Advisory pull_request sentinel (#6238): payload-only head-name
    // classification alongside pr-plan.yml, deliberately outside the CI
    // economics map and outside `ci-lane-whitelist.toml`.
    "pr-plan-head-name-guard.yml",
    "triage-issues.yml",
    "workflow-trigger-lint.yml",
];

#[derive(Debug, Clone)]
pub struct WorkflowPolicyLintConfig {
    pub root: Option<PathBuf>,
    pub receipt: Option<PathBuf>,
    pub fixture: Option<PathBuf>,
    /// Run the per-workflow lane-whitelist check against
    /// `policy/ci-lane-whitelist.toml`. Advisory (warning-level) until the
    /// whitelist has stabilized.
    pub check_lane_whitelist: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LintIssue {
    level: &'static str,
    code: &'static str,
    workflow: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct WorkflowPolicyReceipt {
    schema_version: &'static str,
    receipt_kind: &'static str,
    passed: bool,
    error_count: usize,
    warning_count: usize,
    issues: Vec<LintIssue>,
    subject: WorkflowPolicySubject,
}

#[derive(Debug, Clone, Serialize)]
struct WorkflowPolicySubject {
    mode: &'static str,
    selection: &'static str,
    path_identity_sha256: Option<String>,
    workflow_file_count: usize,
    scan_completed: bool,
    lane_whitelist_requested: bool,
    isolation_registry_requested: bool,
}

fn subject_path_identity(path: &Path) -> String {
    Sha256::digest(path.as_os_str().as_encoded_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn lint_selected_subject(
    config: &WorkflowPolicyLintConfig,
    default_root: impl FnOnce() -> Result<PathBuf>,
    subject: &mut WorkflowPolicySubject,
    issues: &mut Vec<LintIssue>,
) -> Result<()> {
    if let Some(fixture) = &config.fixture {
        if config.root.is_some() || config.check_lane_whitelist {
            bail!("fixture mode cannot select a repository root or lane-whitelist check");
        }
        let canonical_fixture =
            fixture.canonicalize().wrap_err("resolving workflow-policy fixture")?;
        subject.path_identity_sha256 = Some(subject_path_identity(&canonical_fixture));
        lint_workflow_file(fixture, true, issues)?;
        subject.workflow_file_count = 1;
    } else {
        let root = match &config.root {
            Some(root) => root.clone(),
            None => default_root()?,
        };
        let root = root.canonicalize().wrap_err("resolving selected workflow-policy root")?;
        if !root.is_dir() {
            bail!("selected workflow-policy root is not a directory");
        }
        subject.path_identity_sha256 = Some(subject_path_identity(&root));
        let workflows_dir = root.join(".github").join("workflows");
        let mut workflows = Vec::new();
        for entry in fs::read_dir(&workflows_dir)
            .wrap_err("reading .github/workflows under selected workflow-policy root")?
        {
            let path = entry.wrap_err("reading workflow inventory entry")?.path();
            if matches!(path.extension().and_then(|value| value.to_str()), Some("yml" | "yaml")) {
                workflows.push(path);
            }
        }
        if workflows.is_empty() {
            bail!("selected workflow-policy root has no .yml or .yaml workflows");
        }
        workflows.sort();
        // Load the repository `justfile` once per scan so `just <recipe>`
        // invocations inside workflow `run:` steps resolve to their recipes
        // before the xtask-CLI wiring check fires. A missing `justfile` is
        // not an error — it preserves the previous lint behavior for repos
        // that do not use `just`.
        let just_recipes = load_project_justfile(&root)?.unwrap_or_default();
        for path in workflows {
            lint_workflow_file_with_just(&path, false, issues, &just_recipes)?;
            subject.workflow_file_count += 1;
        }
        if config.check_lane_whitelist {
            check_lane_whitelist(&root, issues)?;
        }
        check_self_hosted_isolation(&root, Utc::now().date_naive(), issues)?;
    }
    subject.scan_completed = true;
    Ok(())
}

pub fn run(config: WorkflowPolicyLintConfig) -> Result<()> {
    run_with_default_root(config, project_root)
}

fn run_with_default_root(
    config: WorkflowPolicyLintConfig,
    default_root: impl FnOnce() -> Result<PathBuf>,
) -> Result<()> {
    let mut subject = WorkflowPolicySubject {
        mode: if config.fixture.is_some() { "fixture" } else { "repository" },
        selection: if config.fixture.is_some() {
            "fixture"
        } else if config.root.is_some() {
            "explicit_root"
        } else {
            "compiled_root"
        },
        path_identity_sha256: None,
        workflow_file_count: 0,
        scan_completed: false,
        lane_whitelist_requested: config.check_lane_whitelist,
        isolation_registry_requested: config.fixture.is_none(),
    };
    let mut issues = Vec::new();
    let scan_result = lint_selected_subject(&config, default_root, &mut subject, &mut issues);
    if scan_result.is_err() {
        issues.push(LintIssue {
            level: "error",
            code: "WORKFLOW_POLICY_INPUT_UNAVAILABLE",
            workflow: "<subject>".to_string(),
            message: "selected inputs could not be evaluated; see the command diagnostic"
                .to_string(),
        });
    }
    issues.sort_by(|left, right| {
        (&left.level, &left.workflow, &left.code, &left.message).cmp(&(
            &right.level,
            &right.workflow,
            &right.code,
            &right.message,
        ))
    });

    let error_count = issues.iter().filter(|issue| issue.level == "error").count();
    let warning_count = issues.iter().filter(|issue| issue.level == "warning").count();
    let passed = error_count == 0;

    for issue in &issues {
        let prefix = if issue.level == "error" { "error" } else { "warning" };
        eprintln!("::{prefix}::{} [{}] {}", issue.workflow, issue.code, issue.message);
    }

    if let Some(receipt_path) = config.receipt {
        let receipt = WorkflowPolicyReceipt {
            schema_version: "1.0.0",
            receipt_kind: "workflow_policy_lint",
            passed,
            error_count,
            warning_count,
            issues,
            subject,
        };
        if let Some(parent) = receipt_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating receipt directory {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&receipt).context("serializing receipt")?;
        fs::write(&receipt_path, format!("{json}\n"))
            .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
        println!("Workflow policy lint receipt written: {}", receipt_path.display());
    }

    scan_result.wrap_err("workflow policy lint instrument failure")?;
    if !passed {
        bail!(
            "workflow policy lint failed with {} error(s) and {} warning(s)",
            error_count,
            warning_count
        );
    }

    println!(
        "Workflow policy lint passed ({} error(s), {} warning(s))",
        error_count, warning_count
    );
    Ok(())
}

fn lint_workflow_file(path: &Path, is_fixture: bool, issues: &mut Vec<LintIssue>) -> Result<()> {
    lint_workflow_file_with_just(path, is_fixture, issues, &NoJustRecipes)
}

/// Like [`lint_workflow_file`], but lets the caller supply a parsed `justfile`
/// so that `just <recipe>` invocations inside `run:` steps can be expanded into
/// the recipe body before the xtask-CLI check runs. A fixture without an
/// associated justfile calls the simpler wrapper instead.
fn lint_workflow_file_with_just(
    path: &Path,
    is_fixture: bool,
    issues: &mut Vec<LintIssue>,
    recipes: &dyn JustRecipeLookup,
) -> Result<()> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading workflow file {}", path.display()))?;
    let workflow: Value = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing workflow YAML {}", path.display()))?;

    let workflow_name = if is_fixture {
        path.display().to_string()
    } else {
        path.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>").to_string()
    };

    let triggers = triggers(&workflow);

    if is_pull_request_target(&triggers) && checks_out_pr_head(&workflow) {
        issues.push(LintIssue {
            level: "error",
            code: "PR_TARGET_CHECKOUT_HEAD",
            workflow: workflow_name.clone(),
            message:
                "pull_request_target workflow checks out pull_request.head commit/ref (unsafe)"
                    .to_string(),
        });
    }

    if has_write_all_permissions(&workflow) {
        issues.push(LintIssue {
            level: "error",
            code: "WRITE_ALL_PERMISSIONS",
            workflow: workflow_name.clone(),
            message: "workflow declares permissions: write-all".to_string(),
        });
    }

    if is_pull_request(&triggers)
        && has_contents_write_permission_on_pull_request_job(&workflow)
        && !is_contents_write_allowlisted(&workflow_name)
    {
        issues.push(LintIssue {
            level: "error",
            code: "PR_CONTENTS_WRITE",
            workflow: workflow_name.clone(),
            message: "pull_request workflow requests contents: write and is not in the allowlist"
                .to_string(),
        });
    }

    if is_untrusted_pr_secret_exposure(&triggers, &workflow) {
        issues.push(LintIssue {
            level: "error",
            code: "UNTRUSTED_PR_SECRETS",
            workflow: workflow_name.clone(),
            message: "untrusted PR code path appears to consume secrets.*".to_string(),
        });
    }

    if is_required_style(&workflow) {
        if !triggers.iter().any(|trigger| trigger == "merge_group") {
            issues.push(LintIssue {
                level: "error",
                code: "REQUIRED_STYLE_MISSING_MERGE_GROUP",
                workflow: workflow_name.clone(),
                message: "required-style workflow must include merge_group trigger".to_string(),
            });
        }

        if pull_request_has_paths_filter(&workflow) {
            issues.push(LintIssue {
                level: "error",
                code: "REQUIRED_STYLE_SELF_FILTERED",
                workflow: workflow_name.clone(),
                message: "required-style workflow must not path-filter itself".to_string(),
            });
        }
    }

    if blanket_cancel_in_progress(&workflow)
        && !ALLOWLIST_BLANKET_CANCEL_IN_PROGRESS.iter().any(|value| *value == workflow_name)
    {
        issues.push(LintIssue {
            level: "error",
            code: "BLANKET_CANCEL_IN_PROGRESS",
            workflow: workflow_name.clone(),
            message:
                "concurrency.cancel-in-progress must be false (or expression-gated) for master/merge_group truth runs"
                    .to_string(),
        });
    }

    if pull_request_has_label_triggers(&workflow)
        && cancel_in_progress_cancels_all_pr_events(&workflow)
    {
        issues.push(LintIssue {
            level: "error",
            code: "LABEL_EVENT_CANCELS_PR_RUN",
            workflow: workflow_name.clone(),
            message: "pull_request labeled/unlabeled workflows must not cancel in-progress PR runs; use github.event.action == 'synchronize' or remove label triggers".to_string(),
        });
    }

    if !is_inherited_write_allowlisted(&workflow_name) {
        for (job, scopes) in jobs_inheriting_write_permissions(&workflow) {
            issues.push(LintIssue {
                level: "error",
                code: "INHERITED_JOB_WRITE",
                workflow: workflow_name.clone(),
                message: format!(
                    "job '{job}' declares no permissions and silently inherits workflow-default write authority: {}",
                    scopes.join(", ")
                ),
            });
        }
    }

    if POLICY_WARN_UNPINNED_ACTIONS {
        for action in collect_unpinned_actions(&workflow) {
            issues.push(LintIssue {
                level: "warning",
                code: "UNPINNED_ACTION",
                workflow: workflow_name.clone(),
                message: format!("third-party action is not pinned to a commit SHA: {action}"),
            });
        }
    }

    match workflow_invokes_xtask_cli(&workflow, recipes) {
        Ok(true) => {
            for (trigger, paths) in triggers_with_paths_filters(&workflow) {
                let missing = XTASK_CLI_WIRING_FILES
                    .iter()
                    .filter(|wiring| !paths_filter_covers(&paths, wiring))
                    .copied()
                    .collect::<Vec<_>>();
                if missing.is_empty() {
                    continue;
                }
                issues.push(LintIssue {
                    level: "error",
                    code: "XTASK_CLI_WIRING_PATHS",
                    workflow: workflow_name.clone(),
                    message: format!(
                        "trigger '{trigger}' runs an xtask CLI subcommand but its paths filter omits {}; a change to the CLI wiring would skip this gate",
                        missing.join(", ")
                    ),
                });
            }
        }
        Ok(false) => {}
        Err(JustResolutionError::MissingRecipe(recipe)) => {
            issues.push(LintIssue {
                level: "error",
                code: "JUST_RECIPE_UNRESOLVED",
                workflow: workflow_name.clone(),
                message: format!(
                    "run: invokes `just {recipe}` but no recipe of that name is defined in justfile; \
                     a renamed or removed recipe would silently lose xtask-CLI wiring coverage"
                ),
            });
        }
        Err(JustResolutionError::RecursionDepthExceeded { recipe, depth }) => {
            issues.push(LintIssue {
                level: "error",
                code: "JUST_RECIPE_CYCLIC",
                workflow: workflow_name.clone(),
                message: format!(
                    "`just {recipe}` exceeds the {depth}-level recipe indirection bound; \
                     a cyclic recipe would otherwise lock the workflow policy lint"
                ),
            });
        }
    }

    Ok(())
}

/// Words that may precede `cargo` while still executing it.
const COMMAND_WRAPPERS: &[&str] = &["sudo", "env", "time", "exec", "nice", "command"];

/// Whether a `run:` script invokes the default `xtask` binary's CLI.
///
/// Detection is on the command in command position, not on substrings, so
/// prose that merely names the command — a `#` comment line inside a block
/// scalar, or `echo 'run cargo xtask foo locally'` — is not an invocation. The
/// finding this feeds is error-level, so a false positive would block an
/// otherwise valid workflow change.
///
/// What counts as the CLI is decided by what links `main.rs`:
///
/// - `cargo xtask <sub>` and `cargo run {-p,--package} xtask ... -- <sub>` do;
/// - so does `--bin xtask`, because `xtask/src/main.rs` *is* the `xtask` bin —
///   only another `--bin <name>` or an `--example` selects a different target;
/// - `cargo test -p xtask` compiles test targets rather than the dispatch.
///
/// Known limitation: an invocation reached indirectly, through a `just` recipe
/// or another script, is not visible here. See #14293 for the residual claim.
/// (Partially addressed for `just` recipes by [`command_invokes_xtask_cli_with_just`];
/// scripts other than `just` remain un-tracked.)
fn command_invokes_xtask_cli(script: &str) -> bool {
    // Without a recipe table there is nothing to resolve through. Indirection
    // is silently untracked for that callers — same residual as before #15509.
    command_invokes_xtask_cli_with_just(script, &NoJustRecipes).unwrap_or(false)
}

/// Maximum `just` recipe expansion depth, including the initial recipe call.
///
/// `just` recipes routinely chain two or three levels (`ci-full` → `_timed`
/// → underlying lint/test recipe). Eight is well above anything the repository
/// actually uses, and a hard cap stops a cyclic recipe from locking the lint.
const JUST_RESOLUTION_MAX_DEPTH: usize = 8;

/// Map of `recipe name → body lines`, with each body line pre-stripped of the
/// leading `@` echo-suppression marker and surrounding whitespace.
type JustRecipes = BTreeMap<String, Vec<String>>;

/// A parser-time marker that says "no `just` indirection can be resolved",
/// without forcing every caller to thread an `Option` they will never use.
struct NoJustRecipes;

impl JustRecipeLookup for NoJustRecipes {
    fn recipes(&self) -> &JustRecipes {
        static EMPTY: JustRecipes = BTreeMap::new();
        &EMPTY
    }
}

/// Recipe lookup the resolver can read from.
///
/// `NoJustRecipes` and the real `justfile`-derived `JustRecipes` both implement
/// this so the lint and the legacy single-argument wrapper share one path.
trait JustRecipeLookup {
    fn recipes(&self) -> &JustRecipes;
}

impl JustRecipeLookup for JustRecipes {
    fn recipes(&self) -> &JustRecipes {
        self
    }
}

/// Outcome of failing to expand a `just <recipe>` call inside a `run:` script.
#[derive(Debug, Clone, PartialEq, Eq)]
enum JustResolutionError {
    /// The recipe is named but does not exist in the parsed `justfile`. A
    /// silent pass here would let a typo'd recipe hide an xtask invocation;
    /// the lint surfaces it as an unresolved-resolution finding instead.
    MissingRecipe(String),
    /// Recursion hit `JUST_RESOLUTION_MAX_DEPTH`. A cyclic recipe would
    /// otherwise loop the lint; bound the depth so the lint terminates.
    RecursionDepthExceeded { recipe: String, depth: usize },
}

impl std::fmt::Display for JustResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRecipe(recipe) => {
                write!(formatter, "just recipe `{recipe}` is not defined in justfile")
            }
            Self::RecursionDepthExceeded { recipe, depth } => write!(
                formatter,
                "just recipe `{recipe}` exceeds the {depth}-level indirection bound"
            ),
        }
    }
}

impl std::error::Error for JustResolutionError {}

/// Whether a `run:` script invokes the default `xtask` binary's CLI, looking
/// through `just <recipe>` indirection when a recipe table is supplied.
///
/// The recipe table is consulted for every segment whose first token is `just`
/// (or a wrapper around `just`, after the existing assignment/wrapper skip).
/// Resolution is bounded by [`JUST_RESOLUTION_MAX_DEPTH`] and returns a
/// [`JustResolutionError`] when the named recipe is absent — both to keep the
/// verdict falsifiable and so that a future recipe-table drift cannot quietly
/// re-introduce the false-negative the gate exists to prevent.
fn command_invokes_xtask_cli_with_just(
    script: &str,
    recipes: &dyn JustRecipeLookup,
) -> Result<bool, JustResolutionError> {
    for command in shell_commands(script) {
        let expanded = expand_just_in_command(&command, recipes, 0)?;
        for inner in expanded {
            if command_tokens_invoke_xtask_cli(&inner) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Expand every `just <recipe>` invocation reachable from `command` into the
/// recipe's body lines. Wrapper assignments (`RUST_LOG=…`) and command
/// wrappers (`env`, `sudo`, …) are not followed: they do not change which
/// command runs in command position. A `just` invocation that runs inside a
/// body line is itself recursively expanded up to the depth bound.
///
/// The recursion is shallow on purpose — `just` recipe bodies are flat lists
/// of shell commands. Recursion is only needed when one body line re-runs a
/// `just` recipe, which happens for `_timed` and other dispatcher recipes.
fn expand_just_in_command(
    command: &[String],
    recipes: &dyn JustRecipeLookup,
    depth: usize,
) -> Result<Vec<Vec<String>>, JustResolutionError> {
    if depth > JUST_RESOLUTION_MAX_DEPTH {
        if let Some(recipe) = just_recipe_invoked(command) {
            return Err(JustResolutionError::RecursionDepthExceeded {
                recipe,
                depth: JUST_RESOLUTION_MAX_DEPTH,
            });
        }
        return Ok(Vec::new());
    }
    let Some(recipe) = just_recipe_invoked(command) else {
        return Ok(vec![command.to_vec()]);
    };
    let body = recipes
        .recipes()
        .get(&recipe)
        .ok_or_else(|| JustResolutionError::MissingRecipe(recipe.clone()))?;
    let mut expanded = Vec::new();
    for body_line in body {
        let tokens: Vec<String> = body_line.split_whitespace().map(str::to_string).collect();
        if tokens.is_empty() {
            continue;
        }
        if just_recipe_invoked(&tokens).is_some() {
            for nested in expand_just_in_command(&tokens, recipes, depth + 1)? {
                expanded.push(nested);
            }
        } else {
            expanded.push(tokens);
        }
    }
    Ok(expanded)
}

/// Return the recipe name invoked by a `just <recipe>` token sequence, if any.
///
/// Accepts a leading assignment (`RUST_LOG=debug just ci-fast`) and a single
/// wrapper (`env`, `sudo`, `time`, `exec`, `nice`, `command`) because the
/// existing `command_tokens_invoke_xtask_cli` recognises those as not changing
/// what runs. The wrapper set is identical to [`COMMAND_WRAPPERS`] so the two
/// paths agree on what counts as "the same `just"".
fn just_recipe_invoked(tokens: &[String]) -> Option<String> {
    let mut rest = tokens;
    loop {
        let Some(first) = rest.first().map(String::as_str) else {
            return None;
        };
        if is_env_assignment(first) {
            rest = &rest[1..];
            continue;
        }
        if COMMAND_WRAPPERS.contains(&first) {
            rest = &rest[1..];
            while let Some(next) = rest.first().map(String::as_str) {
                if next.starts_with('-') || is_env_assignment(next) {
                    rest = &rest[1..];
                } else {
                    break;
                }
            }
            continue;
        }
        break;
    }
    let (command, args) = rest.split_first()?;
    if command != "just" {
        return None;
    }
    // `just` always takes one of:
    //   - a recipe name (`just ci-fast`)
    //   - one of its flags (`just --list`, `just --evaluate`)
    //   - `--justfile <path> <recipe>` to point at a different file
    // A flag-only invocation has no recipe and is not an xtask claim by itself.
    let recipe = args.first()?;
    if recipe.starts_with('-') {
        return None;
    }
    if recipe == "--justfile" {
        // recipe sits after the path, i.e. `args[2]`
        return args.get(2).cloned();
    }
    Some(recipe.clone())
}

/// Parse a `justfile` body into a recipe map.
///
/// Format (https://just.systems/man/en/), in summary:
///
/// - lines starting with `#` are comments;
/// - top-level `name := value` / `name = value` lines are settings or
///   exported variables, not recipes;
/// - a recipe header is `name [params]: [deps]` — params and deps are not
///   modeled here, only the recipe name;
/// - the body is the run of indented lines after the header; each body line
///   has its leading `@` echo-suppression marker removed, since `just`
///   strips that before exec.
///
/// The parser is intentionally narrow: it accepts the shape the repository's
/// own `justfile` uses, and returns an empty map for any line it cannot
/// classify. The lint then runs `just <recipe>` against that map; a recipe
/// the parser cannot see cannot be expanded, and the call site gets a typed
/// `MissingRecipe` finding.
fn parse_justfile(content: &str) -> JustRecipes {
    let lines: Vec<&str> = content.lines().collect();
    let mut recipes: JustRecipes = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index];
        let leading_ws =
            raw.chars().take_while(|character| matches!(character, ' ' | '\t')).count();
        let stripped = raw[leading_ws..].trim_start();
        if leading_ws > 0 || stripped.is_empty() || stripped.starts_with('#') {
            index += 1;
            continue;
        }
        // A non-indented line is either an assignment (`cargo_safe := ...` /
        // `name = value`) or a recipe header (`name:` / `name params:` /
        // `name: dep1 dep2`). The discriminator is whether the next non-blank
        // line is indented: recipes always have a body, assignments never do.
        let Some(name) = recipe_header_name(stripped) else {
            index += 1;
            continue;
        };
        if !next_non_blank_is_indented(&lines, index) {
            // Treat as a setting or export. Skip.
            index += 1;
            continue;
        }
        index += 1;
        let mut body = Vec::new();
        while index < lines.len() {
            let inner = lines[index];
            let inner_ws =
                inner.chars().take_while(|character| matches!(character, ' ' | '\t')).count();
            if inner_ws == 0 {
                break;
            }
            let inner_stripped = inner[inner_ws..].trim_start();
            if inner_stripped.is_empty() || inner_stripped.starts_with('#') {
                index += 1;
                continue;
            }
            // `just` itself strips the leading `@` (and `@-`) before exec, so
            // removing it here keeps the existing token splitter honest.
            let body_line = inner_stripped.strip_prefix('@').unwrap_or(inner_stripped);
            body.push(body_line.to_string());
            index += 1;
        }
        // A recipe with an empty body cannot be reached by any `run:` step,
        // but record it so a misspelled recipe name produces a typed
        // `MissingRecipe` rather than a silent miss.
        recipes.insert(name.to_string(), body);
    }
    recipes
}

/// Extract the recipe name from a possible recipe header line.
///
/// `just` recipe headers are `name`, optionally followed by parameters and
/// `:` (with optional deps). The name itself never contains whitespace or
/// `:`. The trailing `:` is the strongest signal: assignments use `=` /
/// `:=`, recipes use a single `:`. We accept only headers that have a `:`;
/// assignments without `:` (e.g. `export RUST_LOG`) are not modeled.
fn recipe_header_name(line: &str) -> Option<&str> {
    let colon = line.find(':')?;
    let name = line[..colon].trim();
    if name.is_empty() {
        return None;
    }
    // A `[settings]` or `export` line would also contain `:`, but those
    // names start with a non-identifier character; gate on a valid recipe
    // identifier so we never confuse the two.
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        return None;
    }
    Some(name)
}

/// Whether the next non-blank line after `lines[index]` is indented.
fn next_non_blank_is_indented(lines: &[&str], index: usize) -> bool {
    lines
        .iter()
        .skip(index + 1)
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.starts_with(' ') || line.starts_with('\t'))
}

/// Split a `run:` script into candidate commands.
///
/// Backslash continuations are joined so one logical command may span lines,
/// `&&`, `||`, `|` and `;` begin a new command, and comment lines are dropped.
fn shell_commands(script: &str) -> Vec<Vec<String>> {
    let mut joined = String::new();
    for line in script.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        match line.strip_suffix('\\') {
            Some(continued) => {
                joined.push_str(continued);
                joined.push(' ');
            }
            None => {
                joined.push_str(line);
                joined.push('\n');
            }
        }
    }
    let mut segments = Vec::new();
    let mut segment = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut characters = joined.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            segment.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            segment.push(character);
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            segment.push(character);
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        if character == '\'' || character == '"' {
            quote = Some(character);
            segment.push(character);
        } else if matches!(character, '\n' | ';' | '|')
            || (character == '&' && characters.peek() == Some(&'&'))
        {
            if character == '&' {
                characters.next();
            }
            segments.push(std::mem::take(&mut segment));
        } else {
            segment.push(character);
        }
    }
    segments.push(segment);
    segments
        .iter()
        .map(|segment| segment.split_whitespace().map(str::to_string).collect::<Vec<String>>())
        .filter(|tokens| !tokens.is_empty())
        .collect()
}

/// Whether a token is a shell variable assignment such as `RUST_LOG=debug`.
fn is_env_assignment(token: &str) -> bool {
    match token.split_once('=') {
        Some((name, _)) => {
            !name.is_empty() && name.chars().all(|byte| byte.is_ascii_alphanumeric() || byte == '_')
        }
        None => false,
    }
}

/// Whether these tokens run the `xtask` CLI, ignoring anything in front of the
/// command that does not change what runs.
///
/// A leading assignment (`RUST_LOG=debug cargo …`) and a wrapper together with
/// its own options and their arguments (`env A=1`, `sudo -E`, `nice -n 10`) are
/// consumed first. Skipping only the wrapper's name would leave a non-`cargo`
/// token in command position and silently miss the invocation.
fn command_tokens_invoke_xtask_cli(tokens: &[String]) -> bool {
    let mut rest = tokens;
    loop {
        let Some(first) = rest.first().map(String::as_str) else {
            return false;
        };
        if is_env_assignment(first) {
            rest = &rest[1..];
            continue;
        }
        if COMMAND_WRAPPERS.contains(&first) {
            rest = &rest[1..];
            // The wrapper's own options, their values, and any assignments it
            // carries sit between it and the real command.
            while let Some(next) = rest.first().map(String::as_str) {
                let is_option_or_value = next.starts_with('-')
                    || is_env_assignment(next)
                    || next.chars().all(|byte| byte.is_ascii_digit());
                if is_option_or_value {
                    rest = &rest[1..];
                } else {
                    break;
                }
            }
            continue;
        }
        break;
    }
    let Some((command, args)) = rest.split_first() else {
        return false;
    };
    if command != "cargo" {
        return false;
    }
    // `cargo +stable xtask …` pins a toolchain; it does not change what runs.
    let args = match args.split_first() {
        Some((first, tail)) if first.starts_with('+') => tail,
        _ => args,
    };
    // Cargo's own options may precede the subcommand without changing it.
    let args = skip_cargo_global_options(args);
    if selects_another_target(args) {
        return false;
    }
    match args.first().map(String::as_str) {
        // `cargo xtask [<sub>]` — the alias form.
        Some("xtask") => true,
        // `cargo run {-p,--package} xtask [flags] -- <sub>`, or the same
        // selected by manifest path.
        Some("run") => {
            let names_package = args
                .windows(2)
                .any(|pair| (pair[0] == "-p" || pair[0] == "--package") && pair[1] == "xtask")
                || args.iter().any(|arg| {
                    arg == "-pxtask"
                        || arg == "--package=xtask"
                        || arg.ends_with("xtask/Cargo.toml")
                });
            names_package && args.iter().any(|arg| arg == "--")
        }
        _ => false,
    }
}

/// Cargo options accepted before the subcommand that consume a separate value.
const CARGO_GLOBAL_OPTIONS_WITH_VALUE: &[&str] = &["--color", "--config", "--explain", "-Z", "-C"];

/// Skip cargo's own options, which sit between `cargo` and its subcommand
/// without changing which subcommand runs — `cargo --offline xtask …`,
/// `cargo -q run -p xtask -- …`, `cargo --color always xtask …`.
///
/// Stops at the first argument that is not an option, which is the subcommand.
/// A subcommand's own `--bin`/`--example` therefore stays visible to
/// [`selects_another_target`].
fn skip_cargo_global_options(mut args: &[String]) -> &[String] {
    while let Some(first) = args.first().map(String::as_str) {
        if !first.starts_with('-') {
            break;
        }
        let takes_value = CARGO_GLOBAL_OPTIONS_WITH_VALUE.contains(&first);
        args = &args[1..];
        if takes_value && !args.is_empty() {
            args = &args[1..];
        }
    }
    args
}

/// Whether the arguments select a build target other than the default `xtask`
/// binary. `--bin xtask` names `xtask/src/main.rs` itself, so it is not another
/// target.
fn selects_another_target(args: &[String]) -> bool {
    for (index, arg) in args.iter().enumerate() {
        if arg == "--example" || arg.starts_with("--example=") {
            return true;
        }
        if let Some(name) = arg.strip_prefix("--bin=") {
            return name != "xtask";
        }
        if arg == "--bin" {
            return args.get(index + 1).map(String::as_str) != Some("xtask");
        }
    }
    false
}

/// Whether any step in any job of this workflow runs the `xtask` CLI, looking
/// through `just <recipe>` indirection when a recipe table is supplied.
///
/// A missing `just` recipe name produces a [`JustResolutionError::MissingRecipe`]
/// rather than a silent false-negative. The caller is expected to surface that
/// as a lint issue, so a recipe rename in `justfile` cannot quietly make the
/// gate lose visibility of a step.
fn workflow_invokes_xtask_cli(
    workflow: &Value,
    recipes: &dyn JustRecipeLookup,
) -> Result<bool, JustResolutionError> {
    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return Ok(false);
    };
    for job in jobs.values() {
        let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
            continue;
        };
        for step in steps {
            if let Some(run) = step.get("run").and_then(Value::as_str) {
                if command_invokes_xtask_cli_with_just(run, recipes)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Load the `justfile` for a repository root, returning `Ok(None)` when the
/// file is absent. The lint treats a missing `justfile` as "no `just` recipes
/// to resolve through", which is the same verdict the gate had before #15509.
fn load_project_justfile(root: &Path) -> Result<Option<JustRecipes>> {
    let path = root.join("justfile");
    match fs::read_to_string(&path) {
        Ok(content) => Ok(Some(parse_justfile(&content))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading justfile {}", path.display())),
    }
}

/// Filter-pattern syntax GitHub defines differently from shell globbing.
///
/// GitHub reads `?` and `+` as quantifiers on the *preceding* character and
/// restricts `[...]` to simple ranges, whereas the `glob` crate reads `?` as
/// any single character, `+` as a literal, and `[...]` as a POSIX class. A
/// pattern using these cannot be evaluated faithfully here.
const GITHUB_SPECIFIC_PATTERN_SYNTAX: &[char] = &['?', '+', '['];

/// Whether a `paths:` allowlist selects `target`.
///
/// Entries are filter patterns, not literals: `xtask/**` already covers both
/// wiring files, so requiring them to be spelled out would be a false positive.
/// Later entries win, which is how a `!` exclusion takes a file back out of an
/// earlier glob.
///
/// Only the `*`/`**`/`!` forms shared with shell globbing are evaluated, which
/// is every form this repository's filters use. A pattern that does not parse,
/// or that uses the GitHub-specific syntax above, cannot be shown to cover
/// anything and so is not treated as coverage — the rule then asks for the
/// wiring file explicitly rather than returning a verdict it cannot justify.
fn paths_filter_covers(paths: &[String], target: &str) -> bool {
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let mut covered = false;
    for entry in paths {
        let (negated, raw) = match entry.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, entry.as_str()),
        };
        if raw.contains(GITHUB_SPECIFIC_PATTERN_SYNTAX) {
            covered &= !negated;
            continue;
        }
        let Ok(pattern) = glob::Pattern::new(raw) else {
            covered &= !negated;
            continue;
        };
        if pattern.matches_with(target, options) {
            covered = !negated;
        }
    }
    covered
}

/// Every trigger carrying a `paths:` allowlist, with its entries.
///
/// `paths-ignore:` is deliberately out of scope: it is a denylist, so omitting
/// a file from it cannot cause the gate to be skipped.
fn triggers_with_paths_filters(workflow: &Value) -> Vec<(String, Vec<String>)> {
    let Some(on) = workflow_on(workflow).and_then(Value::as_mapping) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (trigger, config) in on {
        let Some(name) = trigger.as_str() else {
            continue;
        };
        let Some(paths) = config.get("paths").and_then(Value::as_sequence) else {
            continue;
        };
        let entries =
            paths.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
        found.push((name.to_string(), entries));
    }
    found
}

fn is_contents_write_allowlisted(workflow_name: &str) -> bool {
    ALLOWLIST_PR_CONTENTS_WRITE.contains(&workflow_name)
}

fn is_inherited_write_allowlisted(workflow_name: &str) -> bool {
    ALLOWLIST_INHERITED_JOB_WRITE.contains(&workflow_name)
}

/// Jobs that declare no `permissions:` of their own in a multi-job workflow
/// whose top-level `permissions:` grants at least one `write` scope.
///
/// Such a job runs with every workflow-default write scope whether or not it
/// needs any of them — the shape that gave `release-orchestration.yml`'s
/// read-only validation job contents, Actions, OIDC and attestation authority
/// (#5989). Single-job workflows are exempt: the workflow default and the only
/// job's effective authority are the same declaration, so nothing is silently
/// widened.
fn jobs_inheriting_write_permissions(workflow: &Value) -> Vec<(String, Vec<String>)> {
    let write_scopes = default_write_scopes(workflow);
    if write_scopes.is_empty() {
        return Vec::new();
    }

    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return Vec::new();
    };
    if jobs.len() < 2 {
        return Vec::new();
    }

    jobs.iter()
        .filter_map(|(name, job)| {
            let job = job.as_mapping()?;
            if job.contains_key(Value::String("permissions".to_string())) {
                return None;
            }
            let name = name.as_str()?.to_string();
            Some((name, write_scopes.clone()))
        })
        .collect()
}

/// Write scopes granted by the workflow-level `permissions:` declaration.
///
/// `permissions: write-all` is reported separately by `WRITE_ALL_PERMISSIONS`
/// and is normalized here so a job inheriting it is still named.
fn default_write_scopes(workflow: &Value) -> Vec<String> {
    match workflow.get("permissions") {
        Some(Value::String(value)) if value == "write-all" => vec!["write-all".to_string()],
        Some(Value::Mapping(mapping)) => mapping
            .iter()
            .filter(|(_, value)| value.as_str() == Some("write"))
            .filter_map(|(scope, _)| scope.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn workflow_on(workflow: &Value) -> Option<&Value> {
    workflow.as_mapping()?.iter().find_map(|(key, value)| match key {
        Value::String(key) if key == "on" => Some(value),
        Value::Bool(true) => Some(value),
        _ => None,
    })
}

pub(crate) fn triggers(workflow: &Value) -> Vec<String> {
    let Some(on) = workflow_on(workflow) else {
        return Vec::new();
    };
    match on {
        Value::String(single) => vec![single.clone()],
        Value::Sequence(values) => {
            values.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect()
        }
        Value::Mapping(values) => {
            values.keys().filter_map(Value::as_str).map(ToOwned::to_owned).collect()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn is_pull_request(triggers: &[String]) -> bool {
    triggers.iter().any(|trigger| trigger == "pull_request")
}

pub(crate) fn is_pull_request_target(triggers: &[String]) -> bool {
    triggers.iter().any(|trigger| trigger == "pull_request_target")
}

fn is_required_style(workflow: &Value) -> bool {
    workflow
        .get("x-workflow-policy")
        .and_then(Value::as_mapping)
        .and_then(|mapping| mapping.get(Value::String("required-style".to_string())))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn pull_request_has_paths_filter(workflow: &Value) -> bool {
    workflow_on(workflow)
        .and_then(Value::as_mapping)
        .and_then(|mapping| mapping.get(Value::String("pull_request".to_string())))
        .and_then(Value::as_mapping)
        .is_some_and(|mapping| {
            mapping.contains_key(Value::String("paths".to_string()))
                || mapping.contains_key(Value::String("paths-ignore".to_string()))
        })
}

fn has_write_all_permissions(workflow: &Value) -> bool {
    workflow.get("permissions").and_then(Value::as_str).is_some_and(|value| value == "write-all")
}

fn has_contents_write_permission_on_pull_request_job(workflow: &Value) -> bool {
    if workflow
        .get("permissions")
        .and_then(Value::as_mapping)
        .and_then(|mapping| mapping.get(Value::String("contents".to_string())))
        .and_then(Value::as_str)
        .is_some_and(|value| value == "write")
    {
        return true;
    }

    workflow.get("jobs").and_then(Value::as_mapping).is_some_and(|jobs| {
        jobs.values().any(|job| {
            let Some(job) = job.as_mapping() else {
                return false;
            };
            job_has_contents_write_permission(job) && !job_is_statically_excluded_from_pr(job)
        })
    })
}

fn job_has_contents_write_permission(job: &Mapping) -> bool {
    job.get(Value::String("permissions".to_string()))
        .and_then(Value::as_mapping)
        .and_then(|mapping| mapping.get(Value::String("contents".to_string())))
        .and_then(Value::as_str)
        .is_some_and(|value| value == "write")
}

pub(crate) fn job_is_statically_excluded_from_pr(job: &Mapping) -> bool {
    let Some(condition) = job.get(Value::String("if".to_string())).and_then(Value::as_str) else {
        return false;
    };
    let condition = condition.trim();
    let condition = condition
        .strip_prefix("${{")
        .and_then(|inner| inner.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(condition);

    condition_excludes_pull_request(condition)
}

pub(crate) fn condition_excludes_pull_request(condition: &str) -> bool {
    let Some(condition) = strip_outer_parentheses(condition) else {
        return false;
    };
    let Some(branches) = split_top_level(condition, "||") else {
        return false;
    };

    !branches.is_empty() && branches.iter().all(|branch| branch_has_trusted_event_anchor(branch))
}

fn branch_has_trusted_event_anchor(branch: &str) -> bool {
    let Some(branch) = strip_outer_parentheses(branch) else {
        return false;
    };
    let Some(or_branches) = split_top_level(branch, "||") else {
        return false;
    };
    if or_branches.len() > 1 {
        return or_branches.iter().all(|branch| branch_has_trusted_event_anchor(branch));
    }
    let Some(terms) = split_top_level(branch, "&&") else {
        return false;
    };

    !terms.is_empty() && terms.iter().any(|term| term_has_trusted_event_anchor(term))
}

fn term_has_trusted_event_anchor(term: &str) -> bool {
    if term_is_trusted_event_equality(term) {
        return true;
    }
    let Some(stripped) = strip_outer_parentheses(term) else {
        return false;
    };
    stripped != term && condition_excludes_pull_request(stripped)
}

fn term_is_trusted_event_equality(term: &str) -> bool {
    let Some(term) = strip_outer_parentheses(term) else {
        return false;
    };
    let normalized: String = term.chars().filter(|ch| !ch.is_whitespace()).collect();
    matches!(
        normalized.as_str(),
        "github.event_name=='schedule'"
            | "github.event_name==\"schedule\""
            | "github.event_name=='workflow_dispatch'"
            | "github.event_name==\"workflow_dispatch\""
            | "github.event_name=='push'"
            | "github.event_name==\"push\""
            // `merge_group` runs content that has already passed review, so a
            // job anchored to it is as excluded from an open candidate as one
            // anchored to `push`. Omitting it made a merge-group-only job read
            // as pull-request-reachable.
            | "github.event_name=='merge_group'"
            | "github.event_name==\"merge_group\""
    )
}

pub(crate) fn split_top_level<'a>(condition: &'a str, operator: &str) -> Option<Vec<&'a str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut paren_depth = 0usize;
    let mut quote = None;

    while index < condition.len() {
        let ch = condition[index..].chars().next()?;
        let ch_len = ch.len_utf8();

        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            index += ch_len;
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.checked_sub(1)?;
            }
            _ => {}
        }

        if quote.is_none() && paren_depth == 0 && condition[index..].starts_with(operator) {
            let part = condition[start..index].trim();
            if part.is_empty() {
                return None;
            }
            parts.push(part);
            index += operator.len();
            start = index;
            continue;
        }

        index += ch_len;
    }

    if quote.is_some() || paren_depth != 0 {
        return None;
    }

    let part = condition[start..].trim();
    if part.is_empty() {
        return None;
    }
    parts.push(part);
    Some(parts)
}

pub(crate) fn strip_outer_parentheses(mut expression: &str) -> Option<&str> {
    loop {
        expression = expression.trim();
        if expression.is_empty() {
            return None;
        }
        if !expression.starts_with('(') {
            return Some(expression);
        }
        if !expression.ends_with(')') {
            return Some(expression);
        }
        if !outer_parentheses_wrap_expression(expression)? {
            return Some(expression);
        }
        expression = &expression[1..expression.len() - 1];
    }
}

fn outer_parentheses_wrap_expression(expression: &str) -> Option<bool> {
    let mut paren_depth = 0usize;
    let mut quote = None;

    for (index, ch) in expression.char_indices() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.checked_sub(1)?;
                if paren_depth == 0 {
                    return Some(index + ch.len_utf8() == expression.len());
                }
            }
            _ => {}
        }
    }

    if quote.is_some() {
        return None;
    }

    Some(false)
}

fn checks_out_pr_head(workflow: &Value) -> bool {
    workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .is_some_and(|jobs| jobs.values().any(job_checks_out_pr_head))
}

fn job_checks_out_pr_head(job: &Value) -> bool {
    let Some(steps) = job
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String("steps".to_string())))
        .and_then(Value::as_sequence)
    else {
        return false;
    };

    steps.iter().any(|step| {
        let Some(mapping) = step.as_mapping() else {
            return false;
        };
        let Some(uses) = mapping.get(Value::String("uses".to_string())).and_then(Value::as_str)
        else {
            return false;
        };
        if !uses.starts_with("actions/checkout") {
            return false;
        }
        let Some(with) = mapping.get(Value::String("with".to_string())).and_then(Value::as_mapping)
        else {
            return false;
        };

        with.values().filter_map(Value::as_str).any(|value| {
            value.contains("github.event.pull_request.head.sha")
                || value.contains("github.event.pull_request.head.ref")
        })
    })
}

fn is_untrusted_pr_secret_exposure(triggers: &[String], workflow: &Value) -> bool {
    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return false;
    };

    // We only block proven dangerous shapes:
    // pull_request_target + checkout of PR head + secrets usage in the same job.
    if is_pull_request_target(triggers) {
        return jobs.values().any(|job| {
            let Some(job_map) = job.as_mapping() else {
                return false;
            };
            job_runs_untrusted_code(job_map) && map_contains_secrets_in_mapping(job_map)
        });
    }

    false
}

fn job_runs_untrusted_code(job_map: &Mapping) -> bool {
    if job_map.contains_key(Value::String("run".to_string())) {
        return true;
    }

    let Some(steps) = job_map.get(Value::String("steps".to_string())).and_then(Value::as_sequence)
    else {
        return false;
    };

    steps.iter().any(|step| {
        let Some(step_map) = step.as_mapping() else {
            return false;
        };
        step_map.contains_key(Value::String("run".to_string())) || step_uses_checkout(step_map)
    })
}

fn step_uses_checkout(step_map: &Mapping) -> bool {
    step_map
        .get(Value::String("uses".to_string()))
        .and_then(Value::as_str)
        .is_some_and(|uses| uses.starts_with("actions/checkout"))
}

fn map_contains_secrets_in_mapping(map: &Mapping) -> bool {
    map.iter().any(|(key, nested)| map_contains_secrets(key) || map_contains_secrets(nested))
}

fn map_contains_secrets(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("secrets."),
        Value::Sequence(values) => values.iter().any(map_contains_secrets),
        Value::Mapping(map) => map
            .iter()
            .any(|(key, nested)| map_contains_secrets(key) || map_contains_secrets(nested)),
        _ => false,
    }
}

fn blanket_cancel_in_progress(workflow: &Value) -> bool {
    let trigger_names = triggers(workflow);
    let has_truth_runs = trigger_names.iter().any(|trigger| trigger == "merge_group")
        || workflow_on(workflow)
            .and_then(Value::as_mapping)
            .and_then(|mapping| mapping.get(Value::String("push".to_string())))
            .and_then(Value::as_mapping)
            .and_then(|push| push.get(Value::String("branches".to_string())))
            .and_then(Value::as_sequence)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|branch| branch == "main" || branch == "master")
            });

    if !has_truth_runs {
        return false;
    }

    let Some(concurrency) = workflow.get("concurrency") else {
        return false;
    };

    if let Some(boolean) = concurrency.as_bool() {
        return boolean;
    }

    let Some(map) = concurrency.as_mapping() else {
        return false;
    };

    map.get(Value::String("cancel-in-progress".to_string()))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn pull_request_has_label_triggers(workflow: &Value) -> bool {
    workflow_on(workflow)
        .and_then(Value::as_mapping)
        .and_then(|mapping| mapping.get(Value::String("pull_request".to_string())))
        .and_then(Value::as_mapping)
        .and_then(|pull_request| pull_request.get(Value::String("types".to_string())))
        .and_then(Value::as_sequence)
        .is_some_and(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .any(|event| event == "labeled" || event == "unlabeled")
        })
}

fn cancel_in_progress_cancels_all_pr_events(workflow: &Value) -> bool {
    let Some(concurrency) = workflow.get("concurrency") else {
        return false;
    };

    if let Some(enabled) = concurrency.as_bool() {
        return enabled;
    }

    let Some(map) = concurrency.as_mapping() else {
        return false;
    };

    let Some(cancel) = map.get(Value::String("cancel-in-progress".to_string())) else {
        return false;
    };

    if let Some(enabled) = cancel.as_bool() {
        return enabled;
    }

    cancel.as_str().is_some_and(|expr| expr.trim() == "${{ github.event_name == 'pull_request' }}")
}

fn collect_unpinned_actions(workflow: &Value) -> Vec<String> {
    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return Vec::new();
    };

    let mut actions = Vec::new();
    for job in jobs.values() {
        let Some(steps) = job
            .as_mapping()
            .and_then(|mapping| mapping.get(Value::String("steps".to_string())))
            .and_then(Value::as_sequence)
        else {
            continue;
        };

        for step in steps {
            let Some(uses) = step
                .as_mapping()
                .and_then(|mapping| mapping.get(Value::String("uses".to_string())))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if uses.starts_with("./") || uses.starts_with("docker://") {
                continue;
            }
            if uses.starts_with("actions/") || uses.starts_with("github/") {
                continue;
            }
            if !is_sha_pinned(uses) {
                actions.push(uses.to_string());
            }
        }
    }
    actions.sort();
    actions.dedup();
    actions
}

/// Is `uses:` pinned to a full 40-hex-char commit SHA? Shared with
/// `tasks::workflows` (the actionlint/zizmor contract layer, #3788) so the two
/// checkers agree on what "pinned" means rather than diverging definitions.
pub(crate) fn is_sha_pinned(uses: &str) -> bool {
    let Some((_, reference)) = uses.rsplit_once('@') else {
        return false;
    };
    reference.len() == 40 && reference.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Validate that every workflow under `.github/workflows/` is referenced by at
/// least one `[[lane]]` entry in `policy/ci-lane-whitelist.toml`, OR is in the
/// `ALLOWLIST_WORKFLOW_LANE_MISSING` allowlist (release/utility workflows).
///
/// Issues are emitted at warning level — advisory until the whitelist has
/// stabilized. PR 11 introduces this as advisory; promotion to error level
/// happens only after a calibration window.
fn check_lane_whitelist(root: &Path, issues: &mut Vec<LintIssue>) -> Result<()> {
    let whitelist_path = root.join("policy").join("ci-lane-whitelist.toml");
    let whitelist_text = fs::read_to_string(&whitelist_path)
        .with_context(|| format!("reading {}", whitelist_path.display()))?;
    let whitelist: toml::Value = toml::from_str(&whitelist_text)
        .with_context(|| format!("parsing {}", whitelist_path.display()))?;

    // Collect workflow paths referenced by whitelist lanes.
    let mut whitelisted_workflows: HashSet<String> = HashSet::new();
    if let Some(lanes) = whitelist.get("lane").and_then(|v| v.as_array()) {
        for lane in lanes {
            if let Some(workflow) = lane.get("workflow").and_then(|v| v.as_str()) {
                whitelisted_workflows.insert(workflow.to_string());
            }
        }
    }

    let workflows_dir = root.join(".github").join("workflows");
    if !workflows_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&workflows_dir)
        .with_context(|| format!("reading {}", workflows_dir.display()))?
    {
        let path = entry.context("reading workflow entry")?.path();
        let Some(ext) = path.extension().and_then(|v| v.to_str()) else {
            continue;
        };
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if ALLOWLIST_WORKFLOW_LANE_MISSING.contains(&file_name) {
            continue;
        }
        let workflow_ref = format!(".github/workflows/{file_name}");
        if !whitelisted_workflows.contains(&workflow_ref) {
            issues.push(LintIssue {
                level: "warning",
                code: "LANE_WHITELIST_MISSING",
                workflow: file_name.to_string(),
                message: "workflow has no `[[lane]]` entry in policy/ci-lane-whitelist.toml \
                     (and is not in ALLOWLIST_WORKFLOW_LANE_MISSING). Add an entry or \
                     allowlist with reason."
                    .to_string(),
            });
        }
    }

    // Check each lane entry for runner mismatch and stale job references.
    if let Some(lanes) = whitelist.get("lane").and_then(|v| v.as_array()) {
        for lane in lanes {
            check_runner_label_mismatch(&workflows_dir, lane, issues)?;
            check_stale_whitelist_job(&workflows_dir, lane, issues)?;
        }
    }

    Ok(())
}

/// Normalize a `runs-on:` value from the workflow YAML into the runner token
/// used in `policy/ci-lane-whitelist.toml`.
///
/// Returns `None` when the value is the `${{ matrix.os }}` expression or any
/// other expression that evaluates at runtime — those are "mixed" in the
/// whitelist and should never produce a mismatch warning.
fn normalize_runs_on(runs_on: &Value) -> Option<String> {
    match runs_on {
        Value::String(s) => {
            let s = s.trim();
            // Runtime matrix expression — whitelist uses "mixed"; skip check.
            if s.contains("matrix.") || s.starts_with("${{") {
                return None;
            }
            let normalized = match s {
                "ubuntu-latest" => "ubuntu_latest",
                "ubuntu-24.04" => "ubuntu_24_04",
                "ubuntu-22.04" => "ubuntu_22_04",
                "ubuntu-20.04" => "ubuntu_20_04",
                "windows-latest" => "windows_latest",
                "macos-latest" => "macos_latest",
                other => other,
            };
            Some(normalized.to_string())
        }
        Value::Mapping(map) => {
            // Object form: `runs-on: {group: em-ci-small, labels: [self-hosted, ..., cx53, ...]}`
            let labels = map.get(Value::String("labels".to_string())).and_then(Value::as_sequence);
            if let Some(label_seq) = labels {
                return normalize_self_hosted_labels(label_seq);
            }
            // Unknown object form — skip rather than false-positive.
            None
        }
        // Sequence form is used for self-hosted label lists. Unknown sequences
        // still skip rather than false-positive.
        Value::Sequence(seq) => normalize_self_hosted_labels(seq),
        _ => None,
    }
}

fn normalize_self_hosted_labels(labels: &[Value]) -> Option<String> {
    let label_strs: Vec<&str> = labels.iter().filter_map(Value::as_str).collect();
    if label_strs.contains(&"cx53") {
        return Some("self_hosted_cx53".to_string());
    }
    if label_strs.contains(&"cx43") {
        return Some("self_hosted_cx43".to_string());
    }
    if label_strs.contains(&"self-hosted") && label_strs.contains(&"droid-review") {
        return Some("self_hosted_droid_review".to_string());
    }
    if label_strs.contains(&"self-hosted") && label_strs.contains(&"droid") {
        return Some("self_hosted_droid".to_string());
    }
    if label_strs.contains(&"self-hosted") && label_strs.contains(&"workflow-nano") {
        return Some("self_hosted_workflow_nano".to_string());
    }
    None
}

/// Check 1 — `RUNNER_LABEL_MISMATCH`: for each `[[lane]]` entry that declares
/// both `workflow` and `job`, parse the workflow YAML and warn when the job's
/// `runs-on:` does not match the whitelist `runner` field.
///
/// "mixed" runner in the whitelist matches any actual runner (matrix jobs).
/// Object-form `runs-on:` values that don't contain a known self-hosted label
/// are skipped rather than false-positived.
fn check_runner_label_mismatch(
    workflows_dir: &Path,
    lane: &toml::Value,
    issues: &mut Vec<LintIssue>,
) -> Result<()> {
    let Some(workflow_ref) = lane.get("workflow").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let Some(job_id) = lane.get("job").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let Some(declared_runner) = lane.get("runner").and_then(|v| v.as_str()) else {
        return Ok(());
    };

    // "mixed" in the whitelist means the runner varies at runtime — skip check.
    if declared_runner == "mixed" {
        return Ok(());
    }

    // Derive the workflow file name from the ref path.
    let workflow_file = workflow_ref.trim_start_matches(".github/workflows/");
    let workflow_path = workflows_dir.join(workflow_file);
    if !workflow_path.exists() {
        // Workflow file absent — STALE_WHITELIST_JOB will catch this.
        return Ok(());
    }

    let raw = fs::read_to_string(&workflow_path)
        .with_context(|| format!("reading {}", workflow_path.display()))?;
    let workflow: Value = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing YAML {}", workflow_path.display()))?;

    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return Ok(());
    };

    // Find the job to inspect for runner info. If the exact job ID doesn't
    // exist (stale reference), fall back to the sole job when the workflow
    // has exactly one job. This surfaces the common pattern where a job was
    // renamed: the old name is stale (caught by STALE_WHITELIST_JOB) but the
    // runner mismatch on the surviving single job is still worth reporting.
    let job_opt = jobs.get(Value::String(job_id.to_string())).and_then(Value::as_mapping);
    let job = match job_opt {
        Some(j) => j,
        None if jobs.len() == 1 => {
            // Single-job workflow with a stale job reference: use the one
            // existing job for the runner check.
            match jobs.values().next().and_then(Value::as_mapping) {
                Some(j) => j,
                None => return Ok(()),
            }
        }
        None => {
            // Multi-job workflow with stale reference — skip runner check to
            // avoid false positives. STALE_WHITELIST_JOB will report this.
            return Ok(());
        }
    };

    let Some(runs_on) = job.get(Value::String("runs-on".to_string())) else {
        return Ok(());
    };

    let Some(actual_runner) = normalize_runs_on(runs_on) else {
        // Runtime expression or unrecognised object form — skip.
        return Ok(());
    };

    if actual_runner != declared_runner {
        issues.push(LintIssue {
            level: "warning",
            code: "RUNNER_LABEL_MISMATCH",
            workflow: workflow_file.to_string(),
            message: format!(
                "job `{job_id}` runs on `{actual_runner}` but whitelist declares `{declared_runner}` \
                 (workflow: {workflow_ref})"
            ),
        });
    }

    Ok(())
}

/// Check 2 — `STALE_WHITELIST_JOB`: for each `[[lane]]` entry that declares a
/// `job` field, parse the workflow YAML and warn when that job name is **not**
/// present in the workflow's `jobs:` map.
fn check_stale_whitelist_job(
    workflows_dir: &Path,
    lane: &toml::Value,
    issues: &mut Vec<LintIssue>,
) -> Result<()> {
    let Some(workflow_ref) = lane.get("workflow").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let Some(job_id) = lane.get("job").and_then(|v| v.as_str()) else {
        return Ok(());
    };

    let workflow_file = workflow_ref.trim_start_matches(".github/workflows/");
    let workflow_path = workflows_dir.join(workflow_file);
    if !workflow_path.exists() {
        // Workflow file entirely missing — different check would cover that.
        return Ok(());
    }

    let raw = fs::read_to_string(&workflow_path)
        .with_context(|| format!("reading {}", workflow_path.display()))?;
    let workflow: Value = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing YAML {}", workflow_path.display()))?;

    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        // No jobs map — treat as stale.
        issues.push(LintIssue {
            level: "warning",
            code: "STALE_WHITELIST_JOB",
            workflow: workflow_file.to_string(),
            message: format!(
                "whitelist lane references job `{job_id}` but `{workflow_file}` has no `jobs:` map \
                 (workflow: {workflow_ref})"
            ),
        });
        return Ok(());
    };

    if !jobs.contains_key(Value::String(job_id.to_string())) {
        let actual_jobs: Vec<&str> = jobs.keys().filter_map(Value::as_str).collect();
        issues.push(LintIssue {
            level: "warning",
            code: "STALE_WHITELIST_JOB",
            workflow: workflow_file.to_string(),
            message: format!(
                "whitelist lane references job `{job_id}` but it does not exist in `{workflow_file}` \
                 (actual jobs: {})",
                actual_jobs.join(", ")
            ),
        });
    }

    Ok(())
}

/// One declared runner isolation profile, keyed by the workflow job it covers.
#[derive(Debug, Clone)]
struct IsolationProfile {
    workflow: String,
    job: String,
    runner: Option<String>,
    lifecycle: Option<String>,
    candidate_writable: Option<Vec<String>>,
    runtime_proof: Option<String>,
    review_after: Option<String>,
}

fn parse_isolation_profiles(text: &str) -> Result<Vec<IsolationProfile>> {
    let document: toml::Value =
        toml::from_str(text).context("parsing self-hosted runner isolation policy")?;
    let mut profiles = Vec::new();
    let Some(entries) = document.get("profile").and_then(|value| value.as_array()) else {
        return Ok(profiles);
    };
    for entry in entries {
        let string = |key: &str| entry.get(key).and_then(|v| v.as_str()).map(ToOwned::to_owned);
        profiles.push(IsolationProfile {
            workflow: string("workflow").unwrap_or_default(),
            job: string("job").unwrap_or_default(),
            runner: string("runner"),
            lifecycle: string("lifecycle"),
            candidate_writable: entry.get("candidate_writable").and_then(|v| v.as_array()).map(
                |items| items.iter().filter_map(|i| i.as_str()).map(ToOwned::to_owned).collect(),
            ),
            runtime_proof: string("runtime_proof"),
            review_after: string("review_after"),
        });
    }
    Ok(profiles)
}

/// How a job's `runs-on:` classifies for the trust boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum RunnerTarget {
    /// Routes to self-hosted capacity. `pool` is the lane-inventory token when
    /// the label set is one the inventory recognises; `descriptor` always
    /// describes the actual target for the operator reading the failure.
    SelfHosted { pool: Option<String>, descriptor: String },
    /// Cannot be classified statically, so it cannot be cleared statically.
    Unresolved(String),
    /// GitHub-hosted, or resolved at runtime from a matrix.
    Elsewhere,
}

/// Classify one `runs-on:` value.
///
/// Deliberately independent of [`normalize_runs_on`], which maps onto the lane
/// inventory's known pool tokens and answers `None` for any label set it does
/// not recognise. `None` is the right answer for a runner-economics comparison
/// and the wrong one for a trust boundary: `runs-on: self-hosted` and
/// `runs-on: [self-hosted, some-new-pool]` are both self-hosted capacity even
/// though the inventory has no token for them. Treating them as clean is the
/// bypass #7414's recurrence bullet names.
/// The single `${{ matrix.<key> }}` reference a `runs-on:` may be, if that is
/// all it is. A compound expression that merely mentions `matrix.` is not
/// resolvable here and must stay unresolved.
fn simple_matrix_reference(value: &str) -> Option<&str> {
    let inner = value.strip_prefix("${{")?.strip_suffix("}}")?.trim();
    let key = inner.strip_prefix("matrix.")?;
    if key.is_empty() || !key.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-') {
        return None;
    }
    Some(key)
}

/// Literal values a job's `strategy.matrix` gives one key, from both the direct
/// list and `include:` rows.
///
/// Any non-literal shape — a `fromJSON(...)` matrix, a value that is not a
/// plain string, a key that is not declared — answers `None`, which the caller
/// treats as unresolvable rather than clean.
fn matrix_values(job: &Mapping, key: &str) -> Option<Vec<String>> {
    let matrix = job
        .get(Value::String("strategy".to_string()))?
        .as_mapping()?
        .get(Value::String("matrix".to_string()))?
        .as_mapping()?;
    let mut values = Vec::new();

    if let Some(direct) = matrix.get(Value::String(key.to_string())) {
        for entry in direct.as_sequence()? {
            values.push(entry.as_str()?.to_string());
        }
    }

    // GitHub applies `exclude:` to the combinations generated from the axes, so
    // it can only subtract from those — never from `include:` rows, which are
    // processed afterwards and may add a combination back. Subtracting across
    // both would let `exclude` cancel an `include` that GitHub still schedules.
    //
    // Within the axis values: an exclude entry naming only this key removes the
    // value from every generated combination, so it is subtracted. One that
    // also constrains another axis removes only some combinations and leaves
    // the value reachable through the rest, so it is kept.
    if let Some(exclude) = matrix.get(Value::String("exclude".to_string())) {
        for row in exclude.as_sequence()? {
            let row = row.as_mapping()?;
            let Some(excluded) = row.get(Value::String(key.to_string())) else {
                continue;
            };
            if row.len() != 1 {
                continue;
            }
            let excluded = excluded.as_str()?;
            values.retain(|value| value != excluded);
        }
    }

    // Added after exclusion, and deliberately not subject to it.
    if let Some(include) = matrix.get(Value::String("include".to_string())) {
        for row in include.as_sequence()? {
            if let Some(entry) = row.as_mapping()?.get(Value::String(key.to_string())) {
                values.push(entry.as_str()?.to_string());
            }
        }
    }

    if values.is_empty() { None } else { Some(values) }
}

fn classify_runs_on(runs_on: &Value) -> RunnerTarget {
    // GitHub matches runner labels case-insensitively, so `Self-Hosted` routes
    // to self-hosted capacity exactly like `self-hosted`.
    fn is_self_hosted_label(label: &str) -> bool {
        label.trim().eq_ignore_ascii_case("self-hosted")
    }

    fn is_github_hosted_label(label: &str) -> bool {
        let lowered = label.trim().to_ascii_lowercase();
        GITHUB_HOSTED_LABEL_PREFIXES.iter().any(|prefix| lowered.starts_with(prefix))
    }

    let from_labels = |labels: &[Value], group: Option<&str>| -> RunnerTarget {
        let named: Vec<&str> = labels.iter().filter_map(Value::as_str).collect();
        if named.iter().copied().any(is_self_hosted_label) {
            return RunnerTarget::SelfHosted {
                pool: normalize_self_hosted_labels(labels),
                descriptor: named.join(", "),
            };
        }
        if named.iter().copied().any(is_github_hosted_label) {
            return RunnerTarget::Elsewhere;
        }
        // A runner group names an operator-defined pool, and a label set that
        // never says `self-hosted` does not prove the target is GitHub-hosted.
        // Adding one harmless label must not clear the ambiguity that the
        // labels-less case already fails closed on.
        let detail = match group {
            Some(group) => format!("runner group `{group}` with no recognised runner label"),
            None => format!("runner labels `{}` match no recognised runner", named.join(", ")),
        };
        RunnerTarget::Unresolved(detail)
    };

    match runs_on {
        Value::String(raw) => {
            let value = raw.trim();
            if value.contains("${{") {
                // A matrix reference is resolvable when the matrix lists literal
                // values: classify the values the job can actually select. It is
                // not enough that the expression mentions `matrix.` — a matrix
                // can select self-hosted capacity, so an unresolvable one stays
                // unresolved rather than clean.
                return RunnerTarget::Unresolved(
                    "runner target is computed from an expression".to_string(),
                );
            }
            if is_self_hosted_label(value) {
                RunnerTarget::SelfHosted { pool: None, descriptor: value.to_string() }
            } else if is_github_hosted_label(value) {
                RunnerTarget::Elsewhere
            } else {
                RunnerTarget::Unresolved(format!(
                    "`{value}` is not a recognised GitHub-hosted runner label"
                ))
            }
        }
        Value::Sequence(labels) => from_labels(labels, None),
        Value::Mapping(map) => {
            let group = map.get(Value::String("group".to_string())).and_then(Value::as_str);
            match map.get(Value::String("labels".to_string())) {
                Some(Value::Sequence(labels)) => from_labels(labels, group),
                // `labels:` present but not a literal list (an expression, say)
                // resolves at run time.
                Some(_) => {
                    RunnerTarget::Unresolved("runner labels are not a literal list".to_string())
                }
                // A bare `group:` may name self-hosted capacity or a GitHub
                // larger-runner group. Nothing in the file distinguishes them,
                // so it cannot be cleared without explicit labels.
                None if group.is_some() => {
                    RunnerTarget::Unresolved("runner group without explicit labels".to_string())
                }
                None => RunnerTarget::Elsewhere,
            }
        }
        _ => RunnerTarget::Elsewhere,
    }
}

/// Classify one job's `runs-on:`, resolving a `${{ matrix.<key> }}` reference
/// against that job's own literal matrix values where possible.
fn classify_job_runner(job: &Mapping, runs_on: &Value) -> RunnerTarget {
    if let Value::String(raw) = runs_on
        && let Some(key) = simple_matrix_reference(raw.trim())
    {
        let Some(values) = matrix_values(job, key) else {
            return RunnerTarget::Unresolved(format!(
                "`matrix.{key}` does not resolve to literal runner values"
            ));
        };
        // Every value the matrix can select is a runner target in its own
        // right. One self-hosted row makes the whole job self-hosted.
        let selected: Vec<Value> =
            values.iter().map(|value| Value::String(value.clone())).collect();
        let mut worst = RunnerTarget::Elsewhere;
        for value in &selected {
            match classify_runs_on(value) {
                RunnerTarget::SelfHosted { pool, descriptor } => {
                    return RunnerTarget::SelfHosted { pool, descriptor };
                }
                RunnerTarget::Unresolved(reason) => {
                    worst = RunnerTarget::Unresolved(format!("`matrix.{key}`: {reason}"));
                }
                RunnerTarget::Elsewhere => {}
            }
        }
        return worst;
    }
    classify_runs_on(runs_on)
}

/// Jobs in one workflow whose `runs-on:` is self-hosted or unclassifiable.
fn self_hosted_jobs(workflow: &Value) -> Vec<(String, RunnerTarget)> {
    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (job_key, job_value) in jobs {
        let (Some(job_id), Some(job_map)) = (job_key.as_str(), job_value.as_mapping()) else {
            continue;
        };
        let Some(runs_on) = job_map.get(Value::String("runs-on".to_string())) else {
            continue;
        };
        match classify_job_runner(job_map, runs_on) {
            RunnerTarget::Elsewhere => {}
            target => found.push((job_id.to_string(), target)),
        }
    }
    found.sort();
    found
}

/// `SELF_HOSTED_ISOLATION_*` — every job reachable from a pull-request-controlled
/// trigger that routes to self-hosted capacity must carry a current, complete
/// runner isolation profile.
///
/// This walk is **job-driven**, not profile-driven: a self-hosted job added with
/// no profile entry at all must fail, which is exactly the bypass #7414's
/// recurrence bullet names. A profile-driven walk would silently pass it.
fn evaluate_self_hosted_isolation(
    workflow_file: &str,
    workflow: &Value,
    profiles: &[IsolationProfile],
    today: NaiveDate,
    issues: &mut Vec<LintIssue>,
) {
    let workflow_triggers = triggers(workflow);
    if !workflow_triggers.iter().any(|trigger| PR_CONTROLLED_TRIGGERS.contains(&trigger.as_str())) {
        return;
    }
    let workflow_ref = format!(".github/workflows/{workflow_file}");
    let jobs = workflow.get("jobs").and_then(Value::as_mapping);

    for (job_id, target) in self_hosted_jobs(workflow) {
        // A mixed-trigger workflow can carry jobs that a candidate cannot
        // reach. Reuse the existing static-exclusion authority rather than
        // parsing conditions again: it proves exclusion only for `schedule`,
        // `workflow_dispatch`, and `push` anchors, and answers "not excluded"
        // for an absent or unclassifiable condition, so the fail-closed
        // default survives.
        if jobs
            .and_then(|jobs| jobs.get(Value::String(job_id.clone())))
            .and_then(Value::as_mapping)
            .is_some_and(job_is_statically_excluded_from_pr)
        {
            continue;
        }

        let (pool, descriptor) = match target {
            RunnerTarget::Unresolved(reason) => {
                issues.push(LintIssue {
                    level: "error",
                    code: "SELF_HOSTED_ISOLATION_UNRESOLVED",
                    workflow: workflow_file.to_string(),
                    message: format!(
                        "job `{job_id}` runs pull-request-controlled code on a runner target that \
                         cannot be classified statically ({reason}); state the runner labels \
                         explicitly so the trust boundary is decidable (#7414)"
                    ),
                });
                continue;
            }
            RunnerTarget::SelfHosted { pool, descriptor } => (pool, descriptor),
            // `self_hosted_jobs` never yields this variant.
            RunnerTarget::Elsewhere => continue,
        };
        let actual_runner = pool.clone().unwrap_or_else(|| descriptor.clone());

        let profile =
            profiles.iter().find(|entry| entry.workflow == workflow_ref && entry.job == job_id);
        let Some(profile) = profile else {
            issues.push(LintIssue {
                level: "error",
                code: "SELF_HOSTED_ISOLATION_UNDECLARED",
                workflow: workflow_file.to_string(),
                message: format!(
                    "job `{job_id}` runs pull-request-controlled code on `{actual_runner}` but has \
                     no [[profile]] entry in {SELF_HOSTED_ISOLATION_POLICY}; declare the runner \
                     lifecycle and candidate-writable surfaces before routing PR work to it (#7414)"
                ),
            });
            continue;
        };

        let mut invalid = |detail: String| {
            issues.push(LintIssue {
                level: "error",
                code: "SELF_HOSTED_ISOLATION_INVALID",
                workflow: workflow_file.to_string(),
                message: format!(
                    "job `{job_id}` isolation profile in {SELF_HOSTED_ISOLATION_POLICY} {detail}"
                ),
            });
        };

        match profile.lifecycle.as_deref() {
            None => invalid("declares no `lifecycle`".to_string()),
            Some(lifecycle) if !ISOLATION_LIFECYCLES.contains(&lifecycle) => invalid(format!(
                "declares unknown `lifecycle` `{lifecycle}` (expected one of {})",
                ISOLATION_LIFECYCLES.join(", ")
            )),
            Some(_) => {}
        }

        if profile.candidate_writable.is_none() {
            invalid(
                "declares no `candidate_writable` surfaces; use an empty list only when the \
                 substrate genuinely retains nothing"
                    .to_string(),
            );
        }

        if profile.runtime_proof.as_deref().unwrap_or_default().trim().is_empty() {
            invalid(
                "declares no `runtime_proof` owner; a static declaration does not establish \
                 cross-run decontamination and must name the issue that does"
                    .to_string(),
            );
        }

        // Runner drift: a profile written for one substrate must not silently
        // cover a job that has since been re-routed to different capacity.
        match (profile.runner.as_deref(), pool.as_deref()) {
            (Some(declared), Some(live_pool)) if declared != live_pool => invalid(format!(
                "declares runner `{declared}` but job `{job_id}` runs on `{live_pool}`"
            )),
            // A declared pool that no longer maps onto any known token cannot be
            // confirmed. Accepting it would let a profile written for one
            // persistent substrate keep covering a job re-routed to another.
            (Some(declared), None) => invalid(format!(
                "declares runner `{declared}` but job `{job_id}` now routes to an unrecognised \
                 self-hosted target (`{descriptor}`); the declaration can no longer be confirmed"
            )),
            _ => {}
        }

        match profile.review_after.as_deref() {
            None => invalid("declares no `review_after` date".to_string()),
            Some(raw) => match NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
                Err(_) => invalid(format!("declares malformed `review_after` `{raw}`")),
                Ok(review_after) if review_after < today => issues.push(LintIssue {
                    level: "error",
                    code: "SELF_HOSTED_ISOLATION_STALE",
                    workflow: workflow_file.to_string(),
                    message: format!(
                        "job `{job_id}` isolation profile lapsed on {raw}; re-establish the \
                         runner evidence and advance `review_after` in \
                         {SELF_HOSTED_ISOLATION_POLICY}"
                    ),
                }),
                Ok(_) => {}
            },
        }
    }
}

/// Report profiles that no longer describe a pull-request-controlled
/// self-hosted job, so the registry cannot accumulate dead allowances.
fn check_orphaned_isolation_profiles(
    workflows_dir: &Path,
    profiles: &[IsolationProfile],
    issues: &mut Vec<LintIssue>,
) -> Result<()> {
    for profile in profiles {
        let workflow_file = profile.workflow.trim_start_matches(".github/workflows/");
        let workflow_path = workflows_dir.join(workflow_file);
        let still_self_hosted = if workflow_path.exists() {
            let raw = fs::read_to_string(&workflow_path)
                .with_context(|| format!("reading {}", workflow_path.display()))?;
            let workflow: Value = serde_yaml_ng::from_str(&raw)
                .with_context(|| format!("parsing YAML {}", workflow_path.display()))?;
            self_hosted_jobs(&workflow).iter().any(|(job, _)| job == &profile.job)
        } else {
            false
        };
        if !still_self_hosted {
            issues.push(LintIssue {
                level: "warning",
                code: "SELF_HOSTED_ISOLATION_ORPHANED",
                workflow: workflow_file.to_string(),
                message: format!(
                    "{SELF_HOSTED_ISOLATION_POLICY} declares an isolation profile for job `{}` \
                     but that job no longer routes to self-hosted capacity; remove the stale \
                     profile",
                    profile.job
                ),
            });
        }
    }
    Ok(())
}

fn check_self_hosted_isolation(
    root: &Path,
    today: NaiveDate,
    issues: &mut Vec<LintIssue>,
) -> Result<()> {
    let policy_path = root.join(SELF_HOSTED_ISOLATION_POLICY);
    let profiles = if policy_path.exists() {
        let text = fs::read_to_string(&policy_path)
            .with_context(|| format!("reading {}", policy_path.display()))?;
        parse_isolation_profiles(&text)?
    } else {
        // Absent registry is not "clean": every self-hosted PR-controlled job
        // below reports as undeclared.
        Vec::new()
    };

    // A profile that names no workflow or job can never match a job, and would
    // otherwise reach the orphan walk as an empty path. Report it and drop it
    // rather than letting a typo abort the whole gate with an I/O error.
    let mut profiles = profiles;
    profiles.retain(|profile| {
        let malformed = profile.workflow.trim().is_empty() || profile.job.trim().is_empty();
        if malformed {
            issues.push(LintIssue {
                level: "error",
                code: "SELF_HOSTED_ISOLATION_INVALID",
                workflow: SELF_HOSTED_ISOLATION_POLICY.to_string(),
                message: format!(
                    "a [[profile]] entry is missing `workflow` or `job` (workflow={:?}, job={:?}); \
                     it can never cover a job",
                    profile.workflow, profile.job
                ),
            });
        }
        !malformed
    });

    // Two profiles for one job make the verdict depend on array order: a lapsed
    // entry listed after a current one is silently masked. A security registry
    // must not accumulate contradictory history.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for profile in &profiles {
        let key = (profile.workflow.clone(), profile.job.clone());
        if !seen.insert(key) {
            issues.push(LintIssue {
                level: "error",
                code: "SELF_HOSTED_ISOLATION_INVALID",
                workflow: SELF_HOSTED_ISOLATION_POLICY.to_string(),
                message: format!(
                    "duplicate [[profile]] entries for job `{}` in `{}`; remove the stale one",
                    profile.job, profile.workflow
                ),
            });
        }
    }

    let workflows_dir = root.join(".github").join("workflows");
    if !workflows_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&workflows_dir)
        .with_context(|| format!("reading {}", workflows_dir.display()))?
    {
        let path = entry.context("reading workflow entry")?.path();
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let Some(workflow_file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading workflow file {}", path.display()))?;
        let workflow: Value = serde_yaml_ng::from_str(&raw)
            .with_context(|| format!("parsing workflow YAML {}", path.display()))?;
        evaluate_self_hosted_isolation(workflow_file, &workflow, &profiles, today, issues);
    }

    check_orphaned_isolation_profiles(&workflows_dir, &profiles, issues)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;

    const CLEAN_SUBJECT_WORKFLOW: &str = "on: push\npermissions: read-all\njobs:\n  check:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: echo checked\n";

    fn subject_config(root: Option<PathBuf>, receipt: &Path) -> WorkflowPolicyLintConfig {
        WorkflowPolicyLintConfig {
            root,
            receipt: Some(receipt.to_path_buf()),
            fixture: None,
            check_lane_whitelist: false,
        }
    }

    fn subject_receipt(path: &Path) -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn write_subject_workflow(root: &Path, content: impl AsRef<[u8]>) -> Result<()> {
        let workflows = root.join(".github/workflows");
        fs::create_dir_all(&workflows)?;
        fs::write(workflows.join("subject.yml"), content)?;
        Ok(())
    }

    #[test]
    fn workflow_policy_unavailable_subject_replaces_stale_success() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let parent = temporary.path();
        fs::write(parent.join("file-root"), "not a directory")?;
        fs::create_dir(parent.join("missing-workflows"))?;
        fs::create_dir_all(parent.join("empty/.github/workflows"))?;
        fs::write(parent.join("empty/.github/workflows/README.md"), "not a workflow")?;
        fs::create_dir_all(parent.join("directory-yaml/.github/workflows/subject.yml"))?;
        write_subject_workflow(&parent.join("unreadable-yaml"), [0xff])?;
        for name in [
            "missing",
            "file-root",
            "missing-workflows",
            "empty",
            "directory-yaml",
            "unreadable-yaml",
        ] {
            let receipt = parent.join("receipt.json");
            fs::write(&receipt, r#"{"passed":true}"#)?;
            let result =
                run_with_default_root(subject_config(Some(parent.join(name)), &receipt), || {
                    bail!("explicit root unexpectedly consulted the default")
                });
            ensure!(result.is_err(), "unavailable subject {name} passed");
            let evidence = subject_receipt(&receipt)?;
            ensure!(evidence["passed"] == false, "stale success survived for {name}");
            ensure!(
                evidence["subject"]["scan_completed"] == false,
                "incomplete scan claimed completion"
            );
            ensure!(
                evidence["subject"]["workflow_file_count"] == 0,
                "unreadable workflow counted as evaluated"
            );
            ensure!(evidence["schema_version"] == "1.0.0", "receipt version changed");
            ensure!(
                evidence["issues"].as_array().is_some_and(|issues| issues
                    .iter()
                    .any(|issue| issue["code"] == "WORKFLOW_POLICY_INPUT_UNAVAILABLE")),
                "instrument failure absent from receipt"
            );
        }
        Ok(())
    }

    #[test]
    fn workflow_policy_explicit_root_overrides_unavailable_default() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("selected");
        let receipt = temporary.path().join("receipt.json");
        write_subject_workflow(&root, CLEAN_SUBJECT_WORKFLOW)?;
        run_with_default_root(subject_config(Some(root.clone()), &receipt), || {
            bail!("explicit root unexpectedly consulted the default")
        })?;
        let evidence = subject_receipt(&receipt)?;
        ensure!(evidence["passed"] == true, "clean explicit root failed");
        ensure!(evidence["subject"]["selection"] == "explicit_root", "wrong root selection");
        ensure!(evidence["subject"]["workflow_file_count"] == 1, "workflow denominator missing");
        ensure!(
            evidence["subject"]["path_identity_sha256"]
                .as_str()
                .is_some_and(|value| value.len() == 64),
            "root identity missing"
        );
        let missing = temporary.path().join("removed-build-root");
        ensure!(
            run_with_default_root(subject_config(None, &receipt), || Ok(missing)).is_err(),
            "unavailable compiled root passed"
        );
        ensure!(
            subject_receipt(&receipt)?["passed"] == false,
            "default root failure retained success"
        );
        run_with_default_root(subject_config(None, &receipt), || Ok(root.clone()))?;
        write_subject_workflow(&root, CLEAN_SUBJECT_WORKFLOW.replace("read-all", "write-all"))?;
        ensure!(
            run_with_default_root(subject_config(Some(root), &receipt), || bail!(
                "default consulted"
            ))
            .is_err(),
            "selected workflow violation passed"
        );
        let evidence = subject_receipt(&receipt)?;
        ensure!(
            evidence["subject"]["scan_completed"] == true,
            "policy failure misclassified as instrument failure"
        );
        ensure!(
            evidence["issues"].as_array().is_some_and(|issues| issues
                .iter()
                .any(|issue| issue["code"] == "WRITE_ALL_PERMISSIONS")),
            "wrong root or missing policy finding"
        );
        Ok(())
    }

    #[test]
    fn workflow_policy_fixture_has_no_repository_authority() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let fixture = temporary.path().join("fixture.yml");
        let receipt = temporary.path().join("receipt.json");
        fs::write(&fixture, CLEAN_SUBJECT_WORKFLOW)?;
        let mut config = subject_config(None, &receipt);
        config.fixture = Some(fixture);
        run_with_default_root(config.clone(), || {
            bail!("fixture consulted unavailable default root")
        })?;
        let evidence = subject_receipt(&receipt)?;
        ensure!(evidence["passed"] == true, "clean fixture failed");
        ensure!(evidence["subject"]["mode"] == "fixture", "fixture claims repository authority");
        ensure!(evidence["subject"]["workflow_file_count"] == 1, "fixture count differs");
        ensure!(
            evidence["subject"]["isolation_registry_requested"] == false,
            "fixture claims isolation coverage"
        );
        config.root = Some(temporary.path().to_path_buf());
        ensure!(
            run_with_default_root(config.clone(), || bail!("default consulted")).is_err(),
            "ambiguous fixture root accepted"
        );
        config.root = None;
        config.check_lane_whitelist = true;
        ensure!(
            run_with_default_root(config, || bail!("default consulted")).is_err(),
            "fixture silently ignores requested policy"
        );
        Ok(())
    }

    #[test]
    fn workflow_policy_required_lane_input_preserves_advisory_and_isolation_rules() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("selected");
        let receipt = temporary.path().join("receipt.json");
        write_subject_workflow(&root, CLEAN_SUBJECT_WORKFLOW)?;
        let mut config = subject_config(Some(root.clone()), &receipt);
        config.check_lane_whitelist = true;
        ensure!(
            run_with_default_root(config.clone(), || bail!("default consulted")).is_err(),
            "missing requested policy passed"
        );
        fs::create_dir(root.join("policy"))?;
        fs::write(root.join("policy/ci-lane-whitelist.toml"), "[malformed")?;
        ensure!(
            run_with_default_root(config.clone(), || bail!("default consulted")).is_err(),
            "malformed requested policy passed"
        );
        fs::write(root.join("policy/ci-lane-whitelist.toml"), "lane = []\n")?;
        run_with_default_root(config.clone(), || bail!("default consulted"))?;
        let evidence = subject_receipt(&receipt)?;
        ensure!(evidence["passed"] == true, "advisory findings became errors");
        ensure!(
            evidence["issues"].as_array().is_some_and(|issues| issues
                .iter()
                .any(|issue| issue["code"] == "LANE_WHITELIST_MISSING")),
            "advisory coverage silently skipped"
        );
        write_subject_workflow(
            &root,
            CLEAN_SUBJECT_WORKFLOW
                .replace("on: push", "on: pull_request")
                .replace("ubuntu-24.04", "self-hosted"),
        )?;
        config.check_lane_whitelist = false;
        ensure!(
            run_with_default_root(config, || bail!("default consulted")).is_err(),
            "missing isolation registry cleared self-hosted PR job"
        );
        let evidence = subject_receipt(&receipt)?;
        ensure!(
            evidence["subject"]["scan_completed"] == true,
            "absent isolation registry became an input error"
        );
        ensure!(
            evidence["issues"].as_array().is_some_and(|issues| issues
                .iter()
                .any(|issue| issue["code"] == "SELF_HOSTED_ISOLATION_UNDECLARED")),
            "missing isolation profile was not denied"
        );
        Ok(())
    }

    fn fixture_path(name: &str) -> Result<PathBuf> {
        let root = project_root()?;
        Ok(root.join("xtask/tests/fixtures/workflow-policy").join(name))
    }

    fn wiring_issues(name: &str) -> Result<Vec<LintIssue>> {
        let path = fixture_path(name)?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        Ok(issues.into_iter().filter(|issue| issue.code == "XTASK_CLI_WIRING_PATHS").collect())
    }

    #[test]
    fn xtask_cli_paths_missing_wiring_is_reported() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_missing_wiring.yml")?;
        assert_eq!(issues.len(), 1, "one finding for the one paths-filtered trigger: {issues:?}");
        let message = &issues[0].message;
        assert_eq!(issues[0].level, "error");
        assert!(message.contains("pull_request"), "names the trigger: {message}");
        assert!(message.contains("xtask/src/main.rs"), "names the missing file: {message}");
        assert!(message.contains("xtask/src/tasks/mod.rs"), "names the missing file: {message}");
        Ok(())
    }

    #[test]
    fn xtask_cli_paths_complete_wiring_passes() -> Result<()> {
        assert!(
            wiring_issues("xtask_cli_paths_complete_wiring.yml")?.is_empty(),
            "an enumerated filter is accepted, including the `cargo run -p xtask --` spelling"
        );
        Ok(())
    }

    #[test]
    fn xtask_cli_partial_wiring_reports_only_the_missing_file() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_partial_wiring.yml")?;
        assert_eq!(issues.len(), 1, "{issues:?}");
        let message = &issues[0].message;
        assert!(message.contains("xtask/src/tasks/mod.rs"), "names what is missing: {message}");
        assert!(
            !message.contains("xtask/src/main.rs"),
            "does not name the file that is already listed: {message}"
        );
        Ok(())
    }

    /// `--bin` selects a standalone target. `mod tasks;` is declared in
    /// `xtask/src/main.rs`, so neither wiring file is compiled into that
    /// binary and requiring them would be a false positive.
    #[test]
    fn xtask_bin_invocation_is_not_a_cli_claim() -> Result<()> {
        assert!(wiring_issues("xtask_bin_paths_missing_wiring.yml")?.is_empty());
        Ok(())
    }

    #[test]
    fn xtask_test_invocation_is_not_a_cli_claim() -> Result<()> {
        assert!(wiring_issues("xtask_test_paths_missing_wiring.yml")?.is_empty());
        Ok(())
    }

    #[test]
    fn xtask_cli_without_paths_filter_is_not_reported() -> Result<()> {
        assert!(
            wiring_issues("xtask_cli_no_paths_filter.yml")?.is_empty(),
            "an unfiltered trigger always runs; there is no filter to omit from"
        );
        Ok(())
    }

    #[test]
    fn xtask_cli_wiring_obligation_is_per_trigger() -> Result<()> {
        let issues = wiring_issues("xtask_cli_push_paths_missing_wiring.yml")?;
        assert_eq!(issues.len(), 1, "only the incomplete trigger is reported: {issues:?}");
        let message = &issues[0].message;
        assert!(message.contains("push"), "names the incomplete trigger: {message}");
        assert!(
            !message.contains("pull_request"),
            "does not report the complete trigger: {message}"
        );
        Ok(())
    }

    /// Spelling must not decide the verdict. A missed invocation leaves a gate
    /// silently unenumerated, which is the failure this rule exists to prevent.
    #[test]
    fn xtask_cli_is_detected_through_tabs_and_bare_invocation() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_tab_and_bare.yml")?;
        assert_eq!(issues.len(), 1, "one finding for the one paths-filtered trigger: {issues:?}");
        Ok(())
    }

    #[test]
    fn commented_out_invocation_is_not_a_cli_claim() -> Result<()> {
        assert!(
            wiring_issues("xtask_cli_commented_invocation.yml")?.is_empty(),
            "a documented command in a comment is prose, not a dependency"
        );
        Ok(())
    }

    /// Assignments and wrapper options do not change what runs, so each of
    /// these is still a CLI claim.
    #[test]
    fn wrapped_invocations_are_cli_claims() -> Result<()> {
        let issues = wiring_issues("xtask_cli_wrapped_invocation.yml")?;
        assert_eq!(issues.len(), 1, "one finding for the one paths-filtered trigger: {issues:?}");
        Ok(())
    }

    /// The standing inventory of invocation spellings.
    ///
    /// Every defect found in this detector so far has been the same shape: a
    /// spelling nobody enumerated. The shipped-tree ratchet cannot catch that,
    /// because it asks the detector itself what counts as an invocation. This
    /// table is the independent half — it fixes what each spelling *means*
    /// against Cargo's documented behaviour, so extending the detector means
    /// adding a row here rather than rediscovering the class.
    ///
    /// Each case is asserted on its own. A fixture with several jobs passes on
    /// any one of them, which would hide a missed form.
    #[test]
    fn cli_invocation_spellings_are_classified_by_what_they_run() {
        // Reaches `xtask/src/main.rs`, so the wiring files are a dependency.
        let runs_the_cli = [
            "cargo xtask example-contract check",
            "cargo xtask",
            "cargo xtask\texample-contract check",
            "cargo run -p xtask --locked -- example-contract check",
            "cargo run --package xtask --locked -- example-contract check",
            "cargo run -p xtask --bin xtask -- example-contract check",
            "cargo run --manifest-path xtask/Cargo.toml -- example-contract check",
            "cargo +stable xtask example-contract check",
            "cargo +nightly run -p xtask -- example-contract check",
            "cargo --offline xtask example-contract check",
            "cargo -q run -p xtask -- example-contract check",
            "cargo --color always xtask example-contract check",
            "cargo +stable --locked xtask example-contract check",
            "RUST_LOG=debug cargo xtask example-contract check",
            "env RUST_LOG=debug cargo xtask example-contract check",
            "sudo -E cargo xtask example-contract check",
            "nice -n 10 cargo xtask example-contract check",
            "make build && cargo xtask example-contract check",
        ];
        // Reaches a different target, or is not a command at all.
        let does_not_run_the_cli = [
            "cargo run -p xtask --bin generated-status-contract -- --check",
            "cargo run -p xtask --example public_beta_experience -- --check",
            "cargo test -p xtask --locked --test example_contract",
            "cargo build -p xtask",
            "cargo +stable test -p xtask",
            "cargo --offline test -p xtask",
            "cargo -q build -p xtask",
            "cargo --color always run -p xtask --bin other -- check",
            "echo 'run cargo xtask example-contract check locally'",
            "echo -n 10 cargo xtask example-contract check",
            "# cargo xtask example-contract check",
            "just ci-metrics-ratchet",
        ];

        for script in runs_the_cli {
            assert!(command_invokes_xtask_cli(script), "should be a CLI claim: {script}");
        }
        for script in does_not_run_the_cli {
            assert!(!command_invokes_xtask_cli(script), "should not be a CLI claim: {script}");
        }
    }

    #[test]
    fn echoed_invocation_is_not_a_cli_claim() -> Result<()> {
        assert!(
            wiring_issues("xtask_cli_echoed_invocation.yml")?.is_empty(),
            "printing the command as advice is not running it"
        );
        Ok(())
    }

    #[test]
    fn quoted_separators_do_not_invent_cli_invocations() -> Result<()> {
        for separator in [";", "&&", "||", "|"] {
            for quote in ['\'', '"'] {
                let advice = format!(
                    "echo {quote}Run local checks{separator} cargo xtask workflows check before pushing{quote}"
                );
                if command_invokes_xtask_cli(&advice) {
                    bail!("quoted advice was classified as an invocation: {advice}");
                }
                let invocation =
                    format!("echo {quote}ready{quote}{separator} cargo xtask workflows check");
                if !command_invokes_xtask_cli(&invocation) {
                    bail!("an unquoted separator hid the invocation: {invocation}");
                }
            }
        }
        for advice in [
            r#"echo "Say \"ready; cargo xtask workflows check\" locally""#,
            r"echo ready\; cargo xtask workflows check",
        ] {
            if command_invokes_xtask_cli(advice) {
                bail!("escaped advice was classified as an invocation: {advice}");
            }
        }
        Ok(())
    }

    /// `xtask/src/main.rs` is the `xtask` bin, `--package` is the long `-p`,
    /// and a command may span backslash continuations. Each still reaches the
    /// dispatch this rule guards.
    #[test]
    fn default_bin_long_package_and_continuations_are_cli_claims() -> Result<()> {
        let issues = wiring_issues("xtask_cli_default_bin_and_long_package.yml")?;
        assert_eq!(issues.len(), 1, "one finding for the one paths-filtered trigger: {issues:?}");
        Ok(())
    }

    /// Kills the mutant that drops `require_literal_separator`: with it off,
    /// `xtask/*` would wrongly appear to reach `xtask/src/main.rs`.
    #[test]
    fn single_star_does_not_cross_a_path_separator() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_single_star.yml")?;
        assert_eq!(issues.len(), 1, "{issues:?}");
        let message = &issues[0].message;
        for wiring in ["xtask/src/main.rs", "xtask/src/tasks/mod.rs"] {
            assert!(message.contains(wiring), "`xtask/*` reaches neither file: {message}");
        }
        Ok(())
    }

    /// GitHub's `?`/`+` are quantifiers on the preceding character; the glob
    /// crate disagrees. An unevaluable pattern must not be read as coverage.
    #[test]
    fn github_specific_pattern_syntax_is_not_counted_as_coverage() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_github_syntax.yml")?;
        assert_eq!(issues.len(), 1, "{issues:?}");
        let message = &issues[0].message;
        assert!(message.contains("xtask/src/main.rs"), "the unevaluable pattern: {message}");
        assert!(
            !message.contains("xtask/src/tasks/mod.rs"),
            "the plain literal beside it still covers: {message}"
        );
        Ok(())
    }

    #[test]
    fn unsupported_exclusions_invalidate_coverage_in_order() -> Result<()> {
        for target in XTASK_CLI_WIRING_FILES {
            for exclusion in ["!xtask/**/[a-z]*.rs", "!xtask/**/m?*.rs", "!xtask/**/m+*.rs"] {
                let mut paths = vec!["xtask/**".to_string(), exclusion.to_string()];
                if paths_filter_covers(&paths, target) {
                    bail!("unsupported exclusion {exclusion} retained coverage for {target}");
                }
                paths.push(target.to_string());
                if !paths_filter_covers(&paths, target) {
                    bail!("explicit later inclusion did not restore coverage for {target}");
                }
                paths.push(exclusion.to_string());
                if paths_filter_covers(&paths, target) {
                    bail!("repeated exclusion {exclusion} retained coverage for {target}");
                }
            }
            let paths = vec![target.to_string(), "xtask/**/[a-z]*.rs".to_string()];
            if !paths_filter_covers(&paths, target) {
                bail!("unsupported positive erased proven coverage for {target}");
            }
        }
        Ok(())
    }

    #[test]
    fn unsupported_exclusion_is_reported_by_workflow_lint() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("unsupported-exclusion.yml");
        for restore_main in [false, true] {
            let mut paths = vec![
                "xtask/**".to_string(),
                "!xtask/src/[a-z]*.rs".to_string(),
                "xtask/src/tasks/mod.rs".to_string(),
            ];
            if restore_main {
                paths.push("xtask/src/main.rs".to_string());
            }
            let workflow = serde_json::json!({
                "name": "unsupported-exclusion",
                "on": {"pull_request": {"paths": paths}},
                "permissions": {"contents": "read"},
                "jobs": {"contract": {
                    "runs-on": "ubuntu-latest",
                    "steps": [{"run": "cargo xtask example-contract check"}]
                }}
            });
            fs::write(&path, serde_yaml_ng::to_string(&workflow)?)?;
            let mut issues = Vec::new();
            lint_workflow_file(&path, true, &mut issues)?;
            let wiring: Vec<_> =
                issues.iter().filter(|issue| issue.code == "XTASK_CLI_WIRING_PATHS").collect();
            if restore_main {
                if !wiring.is_empty() {
                    bail!("explicit re-inclusion still reported missing wiring: {wiring:?}");
                }
            } else {
                let finding = wiring.first().ok_or_else(|| {
                    color_eyre::eyre::eyre!("unsupported exclusion produced no wiring finding")
                })?;
                if wiring.len() != 1
                    || finding.level != "error"
                    || !finding.message.contains("xtask/src/main.rs")
                    || finding.message.contains("xtask/src/tasks/mod.rs")
                {
                    bail!("expected only the excluded main.rs wiring finding: {wiring:?}");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn xtask_cli_wiring_may_be_covered_by_a_glob() -> Result<()> {
        assert!(
            wiring_issues("xtask_cli_paths_glob_wiring.yml")?.is_empty(),
            "`xtask/**` already selects both wiring files"
        );
        Ok(())
    }

    #[test]
    fn xtask_cli_wiring_glob_must_actually_reach_the_file() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_narrow_glob.yml")?;
        assert_eq!(issues.len(), 1, "{issues:?}");
        let message = &issues[0].message;
        assert!(
            message.contains("xtask/src/main.rs"),
            "`xtask/src/tasks/**` does not reach main.rs: {message}"
        );
        assert!(
            !message.contains("xtask/src/tasks/mod.rs"),
            "but it does reach tasks/mod.rs: {message}"
        );
        Ok(())
    }

    #[test]
    fn xtask_cli_wiring_excluded_by_negation_is_reported() -> Result<()> {
        let issues = wiring_issues("xtask_cli_paths_negated_wiring.yml")?;
        assert_eq!(issues.len(), 1, "{issues:?}");
        let message = &issues[0].message;
        assert!(message.contains("xtask/src/main.rs"), "the excluded file: {message}");
        assert!(!message.contains("xtask/src/tasks/mod.rs"), "still covered: {message}");
        Ok(())
    }

    /// The claim #14293 actually makes, asserted against the shipped
    /// workflows rather than fixtures: no paths-filtered gate that routes
    /// through the xtask CLI may omit the wiring files it depends on.
    #[test]
    fn shipped_workflows_enumerate_xtask_cli_wiring() -> Result<()> {
        let workflows_dir = project_root()?.join(".github").join("workflows");
        let mut issues = Vec::new();
        for entry in fs::read_dir(&workflows_dir)? {
            let path = entry?.path();
            let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
                continue;
            };
            if ext != "yml" && ext != "yaml" {
                continue;
            }
            lint_workflow_file(&path, false, &mut issues)?;
        }
        let wiring: Vec<_> =
            issues.iter().filter(|issue| issue.code == "XTASK_CLI_WIRING_PATHS").collect();
        assert!(wiring.is_empty(), "workflows with an unenumerated CLI dependency: {wiring:#?}");
        Ok(())
    }

    #[test]
    fn fixture_pr_target_checkout_head_fails() -> Result<()> {
        let path = fixture_path("pull_request_target_checkout_head.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().any(|issue| issue.code == "PR_TARGET_CHECKOUT_HEAD"));
        Ok(())
    }

    #[test]
    fn fixture_inherited_job_write_fails() -> Result<()> {
        let path = fixture_path("inherited_job_write.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        let inherited: Vec<_> =
            issues.iter().filter(|issue| issue.code == "INHERITED_JOB_WRITE").collect();
        assert_eq!(inherited.len(), 1, "only the job without its own permissions is reported");
        let message = &inherited[0].message;
        assert!(message.contains("validate"), "names the inheriting job: {message}");
        for scope in ["contents", "actions", "id-token", "attestations"] {
            assert!(message.contains(scope), "names inherited scope {scope}: {message}");
        }
        Ok(())
    }

    #[test]
    fn fixture_inherited_job_write_scoped_passes() -> Result<()> {
        let path = fixture_path("inherited_job_write_scoped.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(
            issues.iter().all(|issue| issue.level != "error"),
            "per-job least privilege is accepted: {issues:?}"
        );
        Ok(())
    }

    #[test]
    fn fixture_inherited_job_write_single_job_passes() -> Result<()> {
        let path = fixture_path("inherited_job_write_single_job.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "INHERITED_JOB_WRITE"));
        Ok(())
    }

    /// The claim #5989 actually makes, asserted against the shipped workflow
    /// rather than a fixture: validation and dispatch must not be able to
    /// write repository contents, and no job may inherit write authority.
    #[test]
    fn release_orchestration_grants_least_privilege() -> Result<()> {
        let path = project_root()?.join(".github/workflows/release-orchestration.yml");
        let raw = fs::read_to_string(&path)?;
        let workflow: Value = serde_yaml_ng::from_str(&raw)?;

        assert!(
            jobs_inheriting_write_permissions(&workflow).is_empty(),
            "no release-orchestration job may inherit workflow-default write authority"
        );
        assert!(
            default_write_scopes(&workflow).is_empty(),
            "release orchestration must default to read-only"
        );

        let jobs = workflow.get("jobs").and_then(Value::as_mapping).expect("jobs mapping");
        let scopes = |job: &str| -> Vec<(String, String)> {
            jobs.get(Value::String(job.to_string()))
                .and_then(Value::as_mapping)
                .and_then(|job| job.get(Value::String("permissions".to_string())))
                .and_then(Value::as_mapping)
                .map(|mapping| {
                    mapping
                        .iter()
                        .filter_map(|(key, value)| {
                            Some((key.as_str()?.to_string(), value.as_str()?.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        assert!(
            scopes("validate").iter().all(|(_, level)| level == "read"),
            "validation must be read-only: {:?}",
            scopes("validate")
        );
        // Tag creation must remain inside the validated release transaction:
        // no orchestration job may declare `contents: write` under any name.
        // Enumerate offenders instead of probing for a specific job name so a
        // rename cannot make this assertion vacuous.
        let mut contents_write_jobs: Vec<String> = Vec::new();
        for (name, job) in jobs.iter() {
            let writes_contents = job
                .as_mapping()
                .and_then(|job| job.get(Value::String("permissions".to_string())))
                .and_then(Value::as_mapping)
                .and_then(|permissions| permissions.get(Value::String("contents".to_string())))
                .and_then(Value::as_str)
                .is_some_and(|level| level == "write");
            if writes_contents {
                let job_name =
                    name.as_str().map(str::to_string).unwrap_or_else(|| "<unknown>".to_string());
                contents_write_jobs.push(job_name);
            }
        }
        assert!(
            contents_write_jobs.is_empty(),
            "tag creation must remain inside the validated release transaction; \
             jobs declaring contents: write: {contents_write_jobs:?}"
        );
        assert_eq!(
            scopes("trigger-release"),
            vec![("actions".to_string(), "write".to_string())],
            "only dispatch writes Actions state"
        );

        for job in jobs.values() {
            let Some(job) = job.as_mapping() else { continue };
            let declared = job
                .get(Value::String("permissions".to_string()))
                .and_then(Value::as_mapping)
                .map(|mapping| mapping.iter().filter_map(|(key, _)| key.as_str()).collect())
                .unwrap_or_else(Vec::new);
            for unused in ["id-token", "attestations"] {
                assert!(
                    !declared.contains(&unused),
                    "{unused} authority is unused by release orchestration"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn fixture_pull_request_read_only_passes() -> Result<()> {
        let path = fixture_path("pull_request_read_only.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.level != "error"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_job_write_fails() -> Result<()> {
        let path = fixture_path("pull_request_job_write.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().any(|issue| issue.code == "PR_CONTENTS_WRITE"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_with_scheduled_write_job_passes() -> Result<()> {
        let path = fixture_path("pull_request_with_scheduled_write_job.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "PR_CONTENTS_WRITE"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_write_job_refined_workflow_dispatch_passes() -> Result<()> {
        let path = fixture_path("pull_request_write_job_refined_workflow_dispatch.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "PR_CONTENTS_WRITE"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_write_job_refined_push_passes() -> Result<()> {
        let path = fixture_path("pull_request_write_job_refined_push.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "PR_CONTENTS_WRITE"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_write_job_parenthesized_trusted_or_passes() -> Result<()> {
        let path = fixture_path("pull_request_write_job_parenthesized_trusted_or.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "PR_CONTENTS_WRITE"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_write_job_parenthesized_trusted_or_refined_passes() -> Result<()> {
        let path = fixture_path("pull_request_write_job_parenthesized_trusted_or_refined.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "PR_CONTENTS_WRITE"));
        Ok(())
    }

    #[test]
    fn fixture_pull_request_write_job_event_not_schedule_fails() -> Result<()> {
        assert_fixture_fails_pr_contents_write("pull_request_write_job_event_not_schedule.yml")
    }

    #[test]
    fn fixture_pull_request_write_job_event_or_always_fails() -> Result<()> {
        assert_fixture_fails_pr_contents_write("pull_request_write_job_event_or_always.yml")
    }

    #[test]
    fn fixture_pull_request_write_job_unrelated_or_fails() -> Result<()> {
        assert_fixture_fails_pr_contents_write("pull_request_write_job_unrelated_or.yml")
    }

    #[test]
    fn fixture_pull_request_write_job_interpolated_event_fails() -> Result<()> {
        assert_fixture_fails_pr_contents_write("pull_request_write_job_interpolated_event.yml")
    }

    fn assert_fixture_fails_pr_contents_write(name: &str) -> Result<()> {
        let path = fixture_path(name)?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(
            issues.iter().any(|issue| issue.code == "PR_CONTENTS_WRITE"),
            "expected PR_CONTENTS_WRITE for {name}, got: {issues:?}"
        );
        Ok(())
    }

    #[test]
    fn event_name_exclusion_parser_accepts_trusted_event_refinements() -> Result<()> {
        for condition in [
            "github.event_name == 'schedule'",
            "github.event_name == \"schedule\"",
            "github.event_name == 'workflow_dispatch'",
            "github.event_name == \"workflow_dispatch\"",
            "github.event_name == 'push'",
            "github.event_name == \"push\"",
            "github.event_name == 'workflow_dispatch' && github.event.inputs.mode == 'full'",
            "github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.event.inputs.mode == 'full')",
            "(github.event_name == 'push' && github.repository == 'EffortlessMetrics/perl-lsp-swarm')",
            "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')",
            "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && github.repository == 'EffortlessMetrics/perl-lsp-swarm'",
        ] {
            assert!(
                condition_excludes_pull_request(condition),
                "expected trusted condition to pass: {condition}"
            );
        }

        for condition in [
            "github.event_name != 'schedule'",
            "github.event_name == 'pull_request'",
            "github.event_name == format('{0}', 'schedule')",
            "always()",
            "github.repository == 'EffortlessMetrics/perl-lsp-swarm'",
            "github.event_name == 'schedule' || always()",
            "github.event_name == 'schedule' || github.event_name == 'pull_request'",
            "(github.event_name == 'schedule' || always()) && github.repository == 'EffortlessMetrics/perl-lsp-swarm'",
            "(github.event_name == 'schedule' || github.event_name == 'pull_request') && github.repository == 'EffortlessMetrics/perl-lsp-swarm'",
            "github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.repository == 'EffortlessMetrics/perl-lsp-swarm' || github.event_name == 'pull_request')",
            "github.event_name == 'schedule' ||",
            "(github.event_name == 'schedule'",
        ] {
            assert!(
                !condition_excludes_pull_request(condition),
                "expected unsafe condition to fail: {condition}"
            );
        }
        Ok(())
    }

    #[test]
    fn fixture_label_event_cancel_expression_fails() -> Result<()> {
        let path = fixture_path("label_event_cancel_expression.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().any(|issue| issue.code == "LABEL_EVENT_CANCELS_PR_RUN"));
        Ok(())
    }

    #[test]
    fn fixture_label_event_synchronize_cancel_passes() -> Result<()> {
        let path = fixture_path("label_event_synchronize_cancel.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().all(|issue| issue.code != "LABEL_EVENT_CANCELS_PR_RUN"));
        Ok(())
    }

    #[test]
    fn fixture_write_all_fails() -> Result<()> {
        let path = fixture_path("write_all_permissions.yml")?;
        let mut issues = Vec::new();
        lint_workflow_file(&path, true, &mut issues)?;
        assert!(issues.iter().any(|issue| issue.code == "WRITE_ALL_PERMISSIONS"));
        Ok(())
    }

    // ── Fixture tests: RUNNER_LABEL_MISMATCH ──────────────────────────────────

    /// Fixture A: whitelist declares ubuntu_24_04 but job runs on ubuntu-latest → mismatch fires.
    #[test]
    fn runner_mismatch_fires_when_runner_differs() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        // Build a synthetic lane entry pointing at runner_mismatch.yml / job "lint"
        // with declared runner "ubuntu_24_04".
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/runner_mismatch.yml"
            job = "lint"
            runner = "ubuntu_24_04"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().any(|i| i.code == "RUNNER_LABEL_MISMATCH"),
            "expected RUNNER_LABEL_MISMATCH, got: {issues:?}"
        );
        Ok(())
    }

    /// Fixture B: whitelist declares ubuntu_24_04 and job runs on ubuntu-24.04 → no mismatch.
    #[test]
    fn runner_match_is_silent() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/runner_match.yml"
            job = "lint"
            runner = "ubuntu_24_04"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "RUNNER_LABEL_MISMATCH"),
            "unexpected RUNNER_LABEL_MISMATCH: {issues:?}"
        );
        Ok(())
    }

    /// Object-form runs-on with cx53 label → normalized to self_hosted_cx53, no false positive
    /// when whitelist also declares self_hosted_cx53.
    #[test]
    fn runner_cx53_object_form_matches_self_hosted_cx53() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/cx53_object_runs_on.yml"
            job = "rust-small-cx53"
            runner = "self_hosted_cx53"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "RUNNER_LABEL_MISMATCH"),
            "unexpected RUNNER_LABEL_MISMATCH for cx53 object form: {issues:?}"
        );
        Ok(())
    }

    /// Droid's paused hosted runner matches the automatic lane policy, while a
    /// stale self-hosted declaration remains detectable.
    #[test]
    fn runner_droid_paused_hosted_runner_matches_policy() -> Result<()> {
        let real_workflows_dir = {
            let root = project_root()?;
            root.join(".github").join("workflows")
        };
        if !real_workflows_dir.join("droid-review.yml").exists() {
            return Ok(());
        }
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/droid-review.yml"
            job = "droid-review"
            runner = "ubuntu_24_04"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&real_workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "RUNNER_LABEL_MISMATCH"),
            "unexpected RUNNER_LABEL_MISMATCH for paused Droid hosted runner: {issues:?}"
        );

        let stale_lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/droid-review.yml"
            job = "droid-review"
            runner = "self_hosted_droid_review"
            "#,
        )?;
        issues.clear();
        check_runner_label_mismatch(&real_workflows_dir, &stale_lane, &mut issues)?;
        assert!(
            issues.iter().any(|i| i.code == "RUNNER_LABEL_MISMATCH"),
            "expected stale Droid self-hosted policy to mismatch: {issues:?}"
        );
        Ok(())
    }

    /// "mixed" runner in whitelist skips the check entirely, even if the actual
    /// runner differs.
    #[test]
    fn runner_mixed_skips_check() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/runner_mismatch.yml"
            job = "lint"
            runner = "mixed"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "RUNNER_LABEL_MISMATCH"),
            "unexpected RUNNER_LABEL_MISMATCH for mixed runner: {issues:?}"
        );
        Ok(())
    }

    // ── Fixture tests: STALE_WHITELIST_JOB ───────────────────────────────────

    /// Fixture C: whitelist references job "old-job" but workflow only has "actual-job" → stale fires.
    #[test]
    fn stale_job_fires_when_job_missing() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/stale_job.yml"
            job = "old-job"
            "#,
        )?;
        let mut issues = Vec::new();
        check_stale_whitelist_job(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().any(|i| i.code == "STALE_WHITELIST_JOB"),
            "expected STALE_WHITELIST_JOB, got: {issues:?}"
        );
        Ok(())
    }

    /// Fixture D: whitelist references job "lint" and workflow has "lint" → no stale warning.
    #[test]
    fn valid_job_is_silent() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/valid_job.yml"
            job = "lint"
            "#,
        )?;
        let mut issues = Vec::new();
        check_stale_whitelist_job(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "STALE_WHITELIST_JOB"),
            "unexpected STALE_WHITELIST_JOB: {issues:?}"
        );
        Ok(())
    }

    /// Single-job fallback: stale job reference to a single-job workflow —
    /// the runner mismatch should still fire (the sole surviving job has a
    /// different runner than declared in the whitelist).
    #[test]
    fn runner_mismatch_fires_via_single_job_fallback() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        // stale_job.yml has exactly one job: "actual-job" (runs-on: ubuntu-latest).
        // We reference a non-existent job "old-job" with declared runner
        // "ubuntu_24_04" — the fallback picks up "actual-job" and detects mismatch.
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/stale_job.yml"
            job = "old-job"
            runner = "ubuntu_24_04"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().any(|i| i.code == "RUNNER_LABEL_MISMATCH"),
            "expected RUNNER_LABEL_MISMATCH via single-job fallback, got: {issues:?}"
        );
        Ok(())
    }

    /// Multi-job stale reference: stale job in a workflow with multiple jobs —
    /// runner check is SKIPPED to avoid false positives. Only STALE_WHITELIST_JOB fires.
    #[test]
    fn runner_mismatch_skipped_for_stale_ref_in_multi_job_workflow() -> Result<()> {
        // Use the real ci.yml (many jobs) with a nonexistent job reference.
        let real_workflows_dir = {
            let root = project_root()?;
            root.join(".github").join("workflows")
        };
        if !real_workflows_dir.join("ci.yml").exists() {
            // Skip if not in the full project tree.
            return Ok(());
        }
        let lane: toml::Value = toml::from_str(
            r#"
            workflow = ".github/workflows/ci.yml"
            job = "nonexistent-job-xyz"
            runner = "ubuntu_24_04"
            "#,
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&real_workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "RUNNER_LABEL_MISMATCH"),
            "unexpected RUNNER_LABEL_MISMATCH for stale ref in multi-job workflow: {issues:?}"
        );
        Ok(())
    }

    // ── normalize_runs_on unit tests ──────────────────────────────────────────

    #[test]
    fn normalize_ubuntu_latest() {
        let v = Value::String("ubuntu-latest".to_string());
        assert_eq!(normalize_runs_on(&v), Some("ubuntu_latest".to_string()));
    }

    #[test]
    fn normalize_ubuntu_24_04() {
        let v = Value::String("ubuntu-24.04".to_string());
        assert_eq!(normalize_runs_on(&v), Some("ubuntu_24_04".to_string()));
    }

    #[test]
    fn normalize_windows_latest() {
        let v = Value::String("windows-latest".to_string());
        assert_eq!(normalize_runs_on(&v), Some("windows_latest".to_string()));
    }

    #[test]
    fn normalize_matrix_expression_returns_none() {
        let v = Value::String("${{ matrix.os }}".to_string());
        assert_eq!(normalize_runs_on(&v), None);
    }

    #[test]
    fn normalize_cx53_object_form() -> Result<()> {
        let yaml = r#"
group: em-ci-small
labels: [self-hosted, linux, x64, em-ci, cx53, rust-small]
"#;
        let v: Value = serde_yaml_ng::from_str(yaml)?;
        assert_eq!(normalize_runs_on(&v), Some("self_hosted_cx53".to_string()));
        Ok(())
    }

    #[test]
    fn normalize_cx43_object_form() -> Result<()> {
        let yaml = r#"
group: em-ci-small
labels: [self-hosted, linux, x64, em-ci, cx43, rust-small]
"#;
        let v: Value = serde_yaml_ng::from_str(yaml)?;
        assert_eq!(normalize_runs_on(&v), Some("self_hosted_cx43".to_string()));
        Ok(())
    }

    #[test]
    fn normalize_workflow_nano_sequence_form() -> Result<()> {
        let v: Value =
            serde_yaml_ng::from_str("[self-hosted, linux, x64, em-ci, trusted-pr, workflow-nano]")?;
        assert_eq!(normalize_runs_on(&v), Some("self_hosted_workflow_nano".to_string()));
        Ok(())
    }

    #[test]
    fn normalize_droid_review_sequence_form() -> Result<()> {
        let v: Value = serde_yaml_ng::from_str(
            "[self-hosted, linux, x64, em-ci, trusted-pr, review-nano, droid-review]",
        )?;
        assert_eq!(normalize_runs_on(&v), Some("self_hosted_droid_review".to_string()));
        Ok(())
    }

    // ── Coverage: defensive early-return branches ─────────────────────────────

    /// normalize: an unknown plain string passes through unchanged (so it can be
    /// compared against — and mismatch — a named whitelist token).
    #[test]
    fn normalize_unknown_string_passthrough() {
        let v = Value::String("some-custom-runner".to_string());
        assert_eq!(normalize_runs_on(&v), Some("some-custom-runner".to_string()));
    }

    /// normalize: object form without cx53/cx43 labels is unrecognized → None (skip).
    #[test]
    fn normalize_object_without_known_labels_returns_none() -> Result<()> {
        let yaml = "group: generic\nlabels: [self-hosted, linux]\n";
        let v: Value = serde_yaml_ng::from_str(yaml)?;
        assert_eq!(normalize_runs_on(&v), None);
        Ok(())
    }

    /// normalize: an unrecognized sequence is skipped.
    #[test]
    fn normalize_sequence_returns_none() -> Result<()> {
        let v: Value = serde_yaml_ng::from_str("[ubuntu-latest, windows-latest]")?;
        assert_eq!(normalize_runs_on(&v), None);
        Ok(())
    }

    /// runner check: lane missing the `runner` field → early return, no panic, no issue.
    #[test]
    fn runner_check_skips_lane_without_runner_field() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value =
            toml::from_str("workflow = \".github/workflows/runner_match.yml\"\njob = \"lint\"\n")?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
        Ok(())
    }

    /// runner check: lane missing the `job` field → early return.
    #[test]
    fn runner_check_skips_lane_without_job_field() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            "workflow = \".github/workflows/runner_match.yml\"\nrunner = \"ubuntu_24_04\"\n",
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
        Ok(())
    }

    /// runner check: `runner = "mixed"` short-circuits even against a differing actual runner.
    #[test]
    fn runner_check_mixed_short_circuits() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            "workflow = \".github/workflows/runner_mismatch.yml\"\njob = \"lint\"\nrunner = \"mixed\"\n",
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().all(|i| i.code != "RUNNER_LABEL_MISMATCH"),
            "mixed runner must not fire mismatch: {issues:?}"
        );
        Ok(())
    }

    /// runner check: workflow file absent → early return (STALE check covers it).
    #[test]
    fn runner_check_skips_missing_workflow_file() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            "workflow = \".github/workflows/does_not_exist_xyz.yml\"\njob = \"lint\"\nrunner = \"ubuntu_24_04\"\n",
        )?;
        let mut issues = Vec::new();
        check_runner_label_mismatch(&workflows_dir, &lane, &mut issues)?;
        assert!(issues.is_empty(), "expected no issues for missing file, got: {issues:?}");
        Ok(())
    }

    /// stale check: lane missing `job` field → early return, no issue.
    #[test]
    fn stale_check_skips_lane_without_job_field() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str("workflow = \".github/workflows/valid_job.yml\"\n")?;
        let mut issues = Vec::new();
        check_stale_whitelist_job(&workflows_dir, &lane, &mut issues)?;
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
        Ok(())
    }

    /// stale check: workflow file absent → early return (deferred to LANE_WHITELIST_MISSING).
    #[test]
    fn stale_check_skips_missing_workflow_file() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            "workflow = \".github/workflows/does_not_exist_xyz.yml\"\njob = \"whatever\"\n",
        )?;
        let mut issues = Vec::new();
        check_stale_whitelist_job(&workflows_dir, &lane, &mut issues)?;
        assert!(issues.is_empty(), "expected no issues for missing file, got: {issues:?}");
        Ok(())
    }

    /// stale check: workflow with no `jobs:` map → STALE_WHITELIST_JOB fires.
    #[test]
    fn stale_check_fires_on_workflow_without_jobs_map() -> Result<()> {
        let workflows_dir = fixture_path("")?;
        let lane: toml::Value = toml::from_str(
            "workflow = \".github/workflows/no_jobs_map.yml\"\njob = \"anything\"\n",
        )?;
        let mut issues = Vec::new();
        check_stale_whitelist_job(&workflows_dir, &lane, &mut issues)?;
        assert!(
            issues.iter().any(|i| i.code == "STALE_WHITELIST_JOB"),
            "expected STALE_WHITELIST_JOB for jobless workflow, got: {issues:?}"
        );
        Ok(())
    }

    // ---------------------------------------------------------------------
    // #15070 (under #7414): PR-controlled self-hosted capacity must declare a
    // current runner isolation profile.
    // ---------------------------------------------------------------------

    /// Fixed evaluation date so the freshness boundary is deterministic rather
    /// than dependent on when the suite happens to run.
    fn today() -> Result<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, 9, 7)
            .ok_or_else(|| color_eyre::eyre::eyre!("test date is a valid calendar date"))
    }

    /// A complete, current profile for the exact job under test.
    fn valid_profiles() -> Result<Vec<IsolationProfile>> {
        parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runner = "self_hosted_cx53"
lifecycle = "persistent"
candidate_writable = ["/mnt/ci-cache/cargo-home"]
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )
    }

    /// Build a workflow whose single job routes to CX53 under the given triggers.
    fn self_hosted_workflow(on_block: &str) -> Result<Value> {
        let yaml = format!(
            "name: candidate\n\
             {on_block}\
             jobs:\n  \
             build:\n    \
             runs-on:\n      \
             group: em-ci-small\n      \
             labels: [self-hosted, linux, x64, em-ci, cx53, rust-small, trusted-pr]\n    \
             steps:\n      \
             - run: cargo test\n"
        );
        serde_yaml_ng::from_str(&yaml).with_context(|| format!("parsing fixture workflow:\n{yaml}"))
    }

    fn evaluate(workflow: &Value, profiles: &[IsolationProfile]) -> Result<Vec<LintIssue>> {
        let mut issues = Vec::new();
        evaluate_self_hosted_isolation("candidate.yml", workflow, profiles, today()?, &mut issues);
        Ok(issues)
    }

    fn codes(issues: &[LintIssue]) -> Vec<&str> {
        issues.iter().map(|issue| issue.code).collect()
    }

    #[test]
    fn declared_pr_controlled_self_hosted_job_is_clean() -> Result<()> {
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &valid_profiles()?)?;
        assert!(issues.is_empty(), "complete current profile is accepted: {issues:?}");
        Ok(())
    }

    /// The load-bearing anti-bypass property: omitting the profile entirely must
    /// fail, not silently pass. A profile-driven walk would miss this.
    #[test]
    fn undeclared_pr_controlled_self_hosted_job_is_an_error() -> Result<()> {
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &[])?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_UNDECLARED"]);
        assert_eq!(issues[0].level, "error");
        assert!(
            issues[0].message.contains("self_hosted_cx53"),
            "names the resolved runner: {}",
            issues[0].message
        );
        Ok(())
    }

    #[test]
    fn review_and_comment_triggers_are_pr_controlled() -> Result<()> {
        for on_block in [
            "on:\n  pull_request_target:\n",
            "on:\n  pull_request_review:\n",
            "on:\n  pull_request_review_comment:\n",
            "on:\n  issue_comment:\n",
            "on: [issue_comment]\n",
        ] {
            let workflow = self_hosted_workflow(on_block)?;
            let issues = evaluate(&workflow, &[])?;
            assert_eq!(
                codes(&issues),
                vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
                "trigger block should be treated as PR-controlled: {on_block}"
            );
        }
        Ok(())
    }

    /// `merge_group` runs reviewed content, and `push`/`schedule` are not
    /// candidate-initiated. Neither may drag a job into this obligation.
    #[test]
    fn non_pr_controlled_triggers_are_not_covered() -> Result<()> {
        for on_block in [
            "on:\n  merge_group:\n",
            "on:\n  push:\n    branches: [main]\n",
            "on:\n  schedule:\n    - cron: '0 0 * * *'\n",
            "on:\n  workflow_dispatch: {}\n",
        ] {
            let workflow = self_hosted_workflow(on_block)?;
            let issues = evaluate(&workflow, &[])?;
            assert!(issues.is_empty(), "{on_block} must not require a profile: {issues:?}");
        }
        Ok(())
    }

    #[test]
    fn github_hosted_pr_job_is_not_covered() -> Result<()> {
        let workflow: Value = serde_yaml_ng::from_str(
            "name: candidate\non:\n  pull_request:\njobs:\n  build:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: cargo test\n",
        )
        .context("parsing GitHub-hosted fixture")?;
        let issues = evaluate(&workflow, &[])?;
        assert!(issues.is_empty(), "GitHub-hosted capacity is out of scope: {issues:?}");
        Ok(())
    }

    /// A `matrix.` reference with no matrix to read is unresolvable, not clean.
    /// A matrix whose values *are* readable is classified by those values —
    /// see `matrix_of_github_hosted_labels_is_not_covered` and
    /// `matrix_selecting_self_hosted_requires_a_profile`.
    #[test]
    fn matrix_reference_without_a_declared_matrix_is_unresolved() -> Result<()> {
        let workflow: Value = serde_yaml_ng::from_str(
            "name: candidate\non:\n  pull_request:\njobs:\n  build:\n    runs-on: ${{ matrix.os }}\n    steps:\n      - run: cargo test\n",
        )
        .context("parsing matrix-expression fixture")?;
        assert_eq!(codes(&evaluate(&workflow, &[])?), vec!["SELF_HOSTED_ISOLATION_UNRESOLVED"]);
        Ok(())
    }

    #[test]
    fn missing_lifecycle_and_surfaces_are_errors() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &profiles)?;
        assert_eq!(issues.len(), 2, "lifecycle and candidate_writable both report: {issues:?}");
        assert!(issues.iter().all(|issue| issue.code == "SELF_HOSTED_ISOLATION_INVALID"));
        assert!(issues.iter().any(|issue| issue.message.contains("lifecycle")));
        assert!(issues.iter().any(|issue| issue.message.contains("candidate_writable")));
        Ok(())
    }

    #[test]
    fn unknown_lifecycle_is_an_error() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
lifecycle = "probably-fine"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &profiles)?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_INVALID"]);
        assert!(issues[0].message.contains("probably-fine"));
        Ok(())
    }

    /// A static declaration must name the owner of the runtime proof it does not
    /// itself supply, so `lifecycle = "persistent"` can never read as a clearance.
    #[test]
    fn missing_runtime_proof_owner_is_an_error() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
lifecycle = "persistent"
candidate_writable = ["/mnt/ci-cache/sccache"]
runtime_proof = "  "
review_after = "2099-01-01"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &profiles)?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_INVALID"]);
        assert!(issues[0].message.contains("runtime_proof"));
        Ok(())
    }

    /// A profile written for nano capacity must not silently cover a job that
    /// has been re-routed onto a bigger persistent host.
    #[test]
    fn runner_drift_between_profile_and_job_is_an_error() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runner = "self_hosted_workflow_nano"
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &profiles)?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_INVALID"]);
        assert!(issues[0].message.contains("self_hosted_workflow_nano"));
        assert!(issues[0].message.contains("self_hosted_cx53"));
        Ok(())
    }

    #[test]
    fn lapsed_and_malformed_review_dates_are_rejected() -> Result<()> {
        let lapsed = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runner = "self_hosted_cx53"
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2026-09-06"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        let issues = evaluate(&workflow, &lapsed)?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_STALE"]);
        assert_eq!(issues[0].level, "error");

        let malformed = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runner = "self_hosted_cx53"
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "March 2027"
"##,
        )?;
        let issues = evaluate(&workflow, &malformed)?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_INVALID"]);
        assert!(issues[0].message.contains("review_after"));
        Ok(())
    }

    /// The boundary is inclusive: a profile is current on its own review date.
    #[test]
    fn profile_is_current_on_its_review_date() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runner = "self_hosted_cx53"
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2026-09-07"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        assert!(evaluate(&workflow, &profiles)?.is_empty());
        Ok(())
    }

    /// A profile naming a different job must not satisfy this job's obligation.
    #[test]
    fn profile_for_another_job_does_not_cover_this_one() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "some-other-job"
runner = "self_hosted_cx53"
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )?;
        let workflow = self_hosted_workflow("on:\n  pull_request:\n")?;
        assert_eq!(
            codes(&evaluate(&workflow, &profiles)?),
            vec!["SELF_HOSTED_ISOLATION_UNDECLARED"]
        );
        Ok(())
    }

    /// Build a PR-triggered workflow with an arbitrary `runs-on:` block.
    fn workflow_with_runs_on(runs_on_block: &str) -> Result<Value> {
        let yaml = format!(
            "name: candidate\n\
             on:\n  \
             pull_request:\n\
             jobs:\n  \
             build:\n    \
             runs-on: {runs_on_block}\n    \
             steps:\n      \
             - run: echo hi\n"
        );
        serde_yaml_ng::from_str(&yaml).with_context(|| format!("parsing fixture:\n{yaml}"))
    }

    /// `normalize_runs_on` answers `None` for these shapes because the lane
    /// inventory has no pool token for them. They are still self-hosted capacity
    /// executing candidate code, so the trust boundary must not clear them.
    #[test]
    fn self_hosted_shapes_without_a_known_pool_still_require_a_profile() -> Result<()> {
        for runs_on in [
            "self-hosted",
            "[self-hosted]",
            "[self-hosted, linux, brand-new-pool]",
            "{labels: [self-hosted, linux, brand-new-pool]}",
        ] {
            let workflow = workflow_with_runs_on(runs_on)?;
            let issues = evaluate(&workflow, &[])?;
            assert_eq!(
                codes(&issues),
                vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
                "`runs-on: {runs_on}` must still require a profile"
            );
        }
        Ok(())
    }

    /// A bare runner `group:` names either self-hosted capacity or a GitHub
    /// larger-runner group. Nothing in the file distinguishes them, so it cannot
    /// be cleared statically.
    #[test]
    fn runner_group_without_labels_is_unresolved() -> Result<()> {
        let workflow = workflow_with_runs_on("{group: em-ci-small}")?;
        let issues = evaluate(&workflow, &[])?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_UNRESOLVED"]);
        assert_eq!(issues[0].level, "error");
        Ok(())
    }

    /// Ordinary GitHub-hosted spellings must not be dragged in by the widened
    /// classifier.
    #[test]
    fn github_hosted_shapes_stay_out_of_scope() -> Result<()> {
        for runs_on in ["ubuntu-24.04", "ubuntu-latest", "windows-latest", "[ubuntu-24.04]"] {
            let workflow = workflow_with_runs_on(runs_on)?;
            let issues = evaluate(&workflow, &[])?;
            assert!(issues.is_empty(), "`runs-on: {runs_on}` must stay out of scope: {issues:?}");
        }
        Ok(())
    }

    /// GitHub matches runner labels case-insensitively, so a differently-cased
    /// `self-hosted` still routes to self-hosted capacity.
    #[test]
    fn self_hosted_label_match_is_case_insensitive() -> Result<()> {
        for runs_on in ["Self-Hosted", "[Self-Hosted, linux]", "{labels: [SELF-HOSTED, linux]}"] {
            let workflow = workflow_with_runs_on(runs_on)?;
            assert_eq!(
                codes(&evaluate(&workflow, &[])?),
                vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
                "`runs-on: {runs_on}` is self-hosted capacity"
            );
        }
        Ok(())
    }

    /// A runner target computed at run time cannot be cleared statically. The
    /// one exception is a `matrix.*` reference, which the lane inventory
    /// already records as `mixed`.
    #[test]
    fn non_matrix_runner_expressions_are_unresolved() -> Result<()> {
        for runs_on in [
            "${{ vars.RUNNER_LABEL }}",
            "${{ inputs.target == 'x' && 'ubuntu-24.04' || 'self-hosted' }}",
            "{labels: \"${{ inputs.runner_label }}\"}",
        ] {
            let workflow = workflow_with_runs_on(runs_on)?;
            assert_eq!(
                codes(&evaluate(&workflow, &[])?),
                vec!["SELF_HOSTED_ISOLATION_UNRESOLVED"],
                "`runs-on: {runs_on}` must not be cleared"
            );
        }
        Ok(())
    }

    /// Adding one harmless label must not clear the ambiguity that a bare
    /// `group:` already fails closed on.
    #[test]
    fn runner_group_with_unrecognised_labels_is_unresolved() -> Result<()> {
        for runs_on in [
            "{group: operator-pool, labels: [linux, x64]}",
            "[linux, x64]",
            "some-custom-runner-label",
        ] {
            let workflow = workflow_with_runs_on(runs_on)?;
            assert_eq!(
                codes(&evaluate(&workflow, &[])?),
                vec!["SELF_HOSTED_ISOLATION_UNRESOLVED"],
                "`runs-on: {runs_on}` names no recognised runner"
            );
        }
        // A recognised GitHub-hosted label in the set still clears it.
        let hosted = workflow_with_runs_on("{group: larger-runners, labels: [ubuntu-24.04]}")?;
        assert!(evaluate(&hosted, &[])?.is_empty());
        Ok(())
    }

    /// Build a PR-triggered workflow whose job selects its runner from a matrix.
    fn matrix_workflow(matrix_block: &str) -> Result<Value> {
        let yaml = format!(
            "name: candidate\n\
             on:\n  \
             pull_request:\n\
             jobs:\n  \
             build:\n    \
             strategy:\n      \
             matrix:\n        \
             {matrix_block}\n    \
             runs-on: ${{{{ matrix.os }}}}\n    \
             steps:\n      \
             - run: echo hi\n"
        );
        serde_yaml_ng::from_str(&yaml).with_context(|| format!("parsing fixture:\n{yaml}"))
    }

    fn evaluate_workflow(workflow: &Value) -> Result<Vec<LintIssue>> {
        let mut issues = Vec::new();
        evaluate_self_hosted_isolation("candidate.yml", workflow, &[], today()?, &mut issues);
        Ok(issues)
    }

    /// A matrix that can select self-hosted capacity carries the obligation. It
    /// is not enough that the expression mentions `matrix.`.
    #[test]
    fn matrix_selecting_self_hosted_requires_a_profile() -> Result<()> {
        for matrix in [
            "os: [ubuntu-24.04, self-hosted]",
            "os: [Self-Hosted]",
            "include:\n          - os: ubuntu-24.04\n          - os: self-hosted",
        ] {
            let workflow = matrix_workflow(matrix)?;
            assert_eq!(
                codes(&evaluate_workflow(&workflow)?),
                vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
                "matrix `{matrix}` can select self-hosted capacity"
            );
        }
        Ok(())
    }

    /// A matrix of only GitHub-hosted labels stays out of scope, so resolving
    /// the values does not create false obligations.
    #[test]
    fn matrix_of_github_hosted_labels_is_not_covered() -> Result<()> {
        for matrix in [
            "os: [ubuntu-latest, macos-latest, windows-latest]",
            "include:\n          - os: ubuntu-24.04\n          - os: macos-14",
        ] {
            let workflow = matrix_workflow(matrix)?;
            let issues = evaluate_workflow(&workflow)?;
            assert!(issues.is_empty(), "matrix `{matrix}` is GitHub-hosted: {issues:?}");
        }
        Ok(())
    }

    /// A value `exclude:` removes from every scheduled combination is not a
    /// runner the job can select, so it carries no obligation.
    #[test]
    fn fully_excluded_matrix_value_is_not_covered() -> Result<()> {
        let workflow = matrix_workflow(
            "os: [ubuntu-24.04, self-hosted]\n        \
             exclude:\n          - os: self-hosted",
        )?;
        let issues = evaluate_workflow(&workflow)?;
        assert!(issues.is_empty(), "the self-hosted row is never scheduled: {issues:?}");
        Ok(())
    }

    /// `include:` is processed after `exclude:` and can add a combination back.
    /// An exclusion must therefore never cancel an inclusion — GitHub still
    /// schedules the included row.
    #[test]
    fn included_row_survives_an_exclusion_of_the_same_value() -> Result<()> {
        let workflow = matrix_workflow(
            "os: [ubuntu-24.04, self-hosted]\n        \
             exclude:\n          - os: self-hosted\n        \
             include:\n          - os: self-hosted",
        )?;
        assert_eq!(
            codes(&evaluate_workflow(&workflow)?),
            vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
            "the included self-hosted row is still scheduled"
        );
        Ok(())
    }

    /// A multi-axis `exclude` removes only some combinations, so the value stays
    /// reachable through the others and the obligation stands. Subtracting it
    /// here would convert a real obligation into a silent pass.
    #[test]
    fn partially_excluded_matrix_value_still_requires_a_profile() -> Result<()> {
        let workflow = matrix_workflow(
            "os: [ubuntu-24.04, self-hosted]\n        \
             arch: [x64, arm64]\n        \
             exclude:\n          - os: self-hosted\n            arch: arm64",
        )?;
        assert_eq!(
            codes(&evaluate_workflow(&workflow)?),
            vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
            "self-hosted still runs on x64"
        );
        Ok(())
    }

    /// A matrix this walk cannot read is unresolvable, not clean.
    #[test]
    fn unresolvable_matrix_is_unresolved() -> Result<()> {
        for matrix in [
            "os: ${{ fromJSON(needs.plan.outputs.runners) }}",
            "arch: [x64]",
            "os: [[self-hosted, linux]]",
        ] {
            let workflow = matrix_workflow(matrix)?;
            assert_eq!(
                codes(&evaluate_workflow(&workflow)?),
                vec!["SELF_HOSTED_ISOLATION_UNRESOLVED"],
                "matrix `{matrix}` must not be cleared"
            );
        }
        Ok(())
    }

    /// `merge_group` runs reviewed content, so a job anchored to it is excluded
    /// from an open candidate exactly like a `push`-anchored one.
    #[test]
    fn merge_group_only_job_needs_no_profile() -> Result<()> {
        let workflow = mixed_trigger_workflow("github.event_name == 'merge_group'")?;
        let issues = evaluate(&workflow, &[])?;
        assert!(issues.is_empty(), "a merge-group-only job is not candidate-reachable: {issues:?}");
        Ok(())
    }

    /// A profile whose declared pool no longer maps onto the live target cannot
    /// be confirmed, so it must not keep covering the job.
    #[test]
    fn declared_pool_that_no_longer_resolves_is_an_error() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ".github/workflows/candidate.yml"
job = "build"
runner = "self_hosted_cx53"
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )?;
        // A self-hosted label set the lane inventory has no token for.
        let workflow = workflow_with_runs_on("[self-hosted, linux, brand-new-pool]")?;
        let issues = evaluate(&workflow, &profiles)?;
        assert_eq!(codes(&issues), vec!["SELF_HOSTED_ISOLATION_INVALID"]);
        assert!(issues[0].message.contains("no longer be confirmed"));
        Ok(())
    }

    /// A reusable workflow's reachability is defined by its callers, which this
    /// per-file walk cannot see. `on: workflow_call:` must therefore carry the
    /// obligation rather than escaping it.
    #[test]
    fn reusable_workflow_self_hosted_job_requires_a_profile() -> Result<()> {
        let workflow = self_hosted_workflow("on:\n  workflow_call:\n")?;
        assert_eq!(
            codes(&evaluate(&workflow, &[])?),
            vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
            "a reusable workflow cannot prove it is unreachable from a candidate"
        );
        Ok(())
    }

    /// A malformed registry entry must report, not abort the whole gate.
    #[test]
    fn profile_missing_workflow_or_job_is_reported_not_fatal() -> Result<()> {
        let profiles = parse_isolation_profiles(
            r##"
[[profile]]
workflow = ""
job = ""
lifecycle = "persistent"
candidate_writable = []
runtime_proof = "#7414"
review_after = "2099-01-01"
"##,
        )?;
        assert_eq!(profiles.len(), 1, "the malformed entry parses");
        assert!(profiles[0].workflow.is_empty());
        Ok(())
    }

    /// Mixed-trigger workflow: `pull_request` plus a manual trigger, with the
    /// self-hosted job guarded by a job-level `if:`.
    fn mixed_trigger_workflow(job_condition: &str) -> Result<Value> {
        let yaml = format!(
            "name: candidate\n\
             on:\n  \
             pull_request:\n  \
             workflow_dispatch: {{}}\n\
             jobs:\n  \
             build:\n    \
             if: {job_condition}\n    \
             runs-on:\n      \
             group: em-ci-small\n      \
             labels: [self-hosted, linux, x64, em-ci, cx53, rust-small, trusted-pr]\n    \
             steps:\n      \
             - run: cargo test\n"
        );
        serde_yaml_ng::from_str(&yaml).with_context(|| format!("parsing fixture:\n{yaml}"))
    }

    /// A job a candidate cannot reach carries no candidate trust obligation.
    #[test]
    fn manual_only_job_in_a_mixed_trigger_workflow_needs_no_profile() -> Result<()> {
        for condition in [
            "github.event_name == 'workflow_dispatch'",
            "${{ github.event_name == 'workflow_dispatch' }}",
            "github.event_name == 'workflow_dispatch' || github.event_name == 'schedule'",
        ] {
            let workflow = mixed_trigger_workflow(condition)?;
            let issues = evaluate(&workflow, &[])?;
            assert!(
                issues.is_empty(),
                "`if: {condition}` keeps the job off every PR-controlled trigger: {issues:?}"
            );
        }
        Ok(())
    }

    /// The exclusion is only honoured when it is *proven*. A condition that does
    /// not statically exclude candidate events keeps the obligation, so the
    /// fail-closed default survives the narrowing above.
    #[test]
    fn unprovable_or_pr_reachable_conditions_still_require_a_profile() -> Result<()> {
        for condition in [
            // Reachable from a pull request.
            "github.event_name == 'pull_request'",
            // Not an event anchor at all — unclassifiable.
            "github.event.inputs.mode == 'manual'",
            "always()",
            // One branch is anchored, the other is not.
            "github.event_name == 'workflow_dispatch' || github.actor == 'someone'",
        ] {
            let workflow = mixed_trigger_workflow(condition)?;
            let issues = evaluate(&workflow, &[])?;
            assert_eq!(
                codes(&issues),
                vec!["SELF_HOSTED_ISOLATION_UNDECLARED"],
                "`if: {condition}` must not clear the obligation"
            );
        }
        Ok(())
    }

    /// A declared profile for a manual-only self-hosted job is not orphaned: the
    /// job still routes to self-hosted capacity, so keeping its declaration is
    /// useful rather than stale.
    #[test]
    fn manual_only_self_hosted_job_is_not_reported_orphaned() -> Result<()> {
        let workflow = mixed_trigger_workflow("github.event_name == 'workflow_dispatch'")?;
        assert!(
            self_hosted_jobs(&workflow).iter().any(|(job, _)| job == "build"),
            "the job is still self-hosted capacity for the orphan walk"
        );
        Ok(())
    }

    /// Every self-hosted job in the shipped tree is declared, and every shipped
    /// declaration still describes a real self-hosted job. This is the assertion
    /// that keeps the registry and `.github/workflows/**` from drifting apart.
    ///
    /// Evaluated at a fixed date on purpose. Drift and freshness are different
    /// propositions: this test owns drift, and pinning the date keeps it from
    /// turning red on the shipped profiles' review date for a reason that has
    /// nothing to do with drift. Freshness is enforced by the live gate, and
    /// owners are warned ahead of it through `policy/cadence-records.json`.
    #[test]
    fn shipped_tree_has_no_undeclared_or_orphaned_self_hosted_capacity() -> Result<()> {
        let root = project_root()?;
        let mut issues = Vec::new();
        check_self_hosted_isolation(&root, today()?, &mut issues)?;
        assert!(
            issues.is_empty(),
            "current tree must be clean under the isolation gate: {issues:?}"
        );
        Ok(())
    }

    /// The shipped profiles must still be within their review window when this
    /// lands, so the gate is not born stale.
    #[test]
    fn shipped_profiles_are_current_today() -> Result<()> {
        let root = project_root()?;
        let mut issues = Vec::new();
        check_self_hosted_isolation(&root, Utc::now().date_naive(), &mut issues)?;
        let stale: Vec<_> =
            issues.iter().filter(|issue| issue.code == "SELF_HOSTED_ISOLATION_STALE").collect();
        assert!(stale.is_empty(), "shipped isolation profiles have lapsed: {stale:?}");
        Ok(())
    }

    /// The minimal justfile shape the lint accepts: recipe headers at column
    /// zero, indented bodies, `@` echo-suppression stripped, comments skipped,
    /// settings (`name := value`) and `[private]` attributes ignored.
    #[test]
    fn parse_justfile_accepts_minimal_recipe_shape() {
        let justfile = "\
# Settings first; never treated as a recipe.
cargo_safe := \"./scripts/cargo-safe\"

[private]
_check-tools:
    @echo tools

# A recipe that runs xtask through the lint's detection rule.
ci-fast:
    cargo run -p xtask -- ci-fast-check

# A recipe that does not, and stays unclaimed.
devplane-init:
    ./scripts/devplane-init
";
        let recipes = parse_justfile(justfile);
        assert_eq!(
            recipes.get("ci-fast").map(Vec::as_slice),
            Some(["cargo run -p xtask -- ci-fast-check".to_string()].as_slice()),
            "recipe bodies must carry the post-strip shell line"
        );
        assert_eq!(
            recipes.get("devplane-init").map(Vec::as_slice),
            Some(["./scripts/devplane-init".to_string()].as_slice()),
            "plain-shell recipes must round-trip"
        );
        assert!(
            !recipes.contains_key("cargo_safe"),
            "settings (`name := value`) must not register as a recipe"
        );
        // `[private]` attributes must not steal the recipe body of the line
        // that follows them. `_check-tools` keeps its body.
        assert!(
            recipes.contains_key("_check-tools"),
            "[private] attributes must not bind a recipe body to the [private] line"
        );
        assert_eq!(
            recipes["_check-tools"],
            vec!["echo tools".to_string()],
            "@ echo-suppression prefix must be stripped from the body line"
        );
    }

    /// The acceptance ladder for #15509:
    ///
    /// 1. a `just <recipe>` whose body invokes the xtask CLI is a CLI claim;
    /// 2. a recipe that runs `cargo test -p xtask` (a test target, not the
    ///    dispatch) is not;
    /// 3. a recipe that runs another binary through `--bin <other>` is not;
    /// 4. a recipe name that does not exist in the justfile is a typed
    ///    `MissingRecipe` finding rather than a false negative.
    #[test]
    fn just_recipe_invocation_classification_matches_acceptance_ladder() -> Result<()> {
        let justfile = "\
ci-fast:
    cargo run -p xtask -- ci-fast-check

# Compiles the test target, not the dispatch. The existing
# `selects_another_target` rule already excludes this, so this is a regression
# guard rather than a new exclusion.
ci-test:
    cargo test -p xtask --locked --test ci_test

# Same exclusion path: a different `--bin` reaches a different binary.
ci-other-bin:
    cargo run -p xtask --bin other-bin -- check
";
        let recipes = parse_justfile(justfile);

        // (1) positive — a recipe that reaches the CLI.
        assert!(
            command_invokes_xtask_cli_with_just("just ci-fast", &recipes)?,
            "ci-fast reaches xtask/src/main.rs and must count as a CLI claim"
        );

        // (2) negative — test target, not dispatch.
        assert!(
            !command_invokes_xtask_cli_with_just("just ci-test", &recipes)?,
            "ci-test compiles a test target and must not count as a CLI claim"
        );

        // (3) negative — another --bin target.
        assert!(
            !command_invokes_xtask_cli_with_just("just ci-other-bin", &recipes)?,
            "ci-other-bin reaches a different --bin and must not count"
        );

        // (4) typed missing recipe.
        let result = command_invokes_xtask_cli_with_just("just nonexistent-recipe", &recipes);
        assert!(
            matches!(result, Err(JustResolutionError::MissingRecipe(ref name)) if name == "nonexistent-recipe"),
            "missing recipe must surface as typed MissingRecipe: {result:?}"
        );

        Ok(())
    }

    /// The depth bound holds. A two-recipe cycle (`a -> b -> a`) must surface
    /// as a typed `RecursionDepthExceeded` finding rather than lock the lint
    /// or silently miss the indirection.
    #[test]
    fn just_recipe_recursion_is_bounded_and_typed() {
        let justfile = "\
recipe-a:
    just recipe-b

recipe-b:
    just recipe-a
";
        let recipes = parse_justfile(justfile);
        let result = command_invokes_xtask_cli_with_just("just recipe-a", &recipes);
        assert!(
            matches!(result, Err(JustResolutionError::RecursionDepthExceeded { .. })),
            "a cyclic recipe must surface as RecursionDepthExceeded, not silently pass: {result:?}"
        );
    }

    /// `just` indirection survives the same wrapper-skip rules that already
    /// apply to `cargo`. `env`, `RUST_LOG=`, and a leading wrapper option
    /// must not make `just ci-fast` lose its xtask claim.
    #[test]
    fn just_invocation_survives_assignment_and_wrapper_skip() -> Result<()> {
        let recipes = parse_justfile("ci-fast:\n    cargo run -p xtask -- ci-fast-check\n");
        for invocation in [
            "just ci-fast",
            "RUST_LOG=debug just ci-fast",
            "env RUST_LOG=debug just ci-fast",
            "sudo -E just ci-fast",
        ] {
            assert!(
                command_invokes_xtask_cli_with_just(invocation, &recipes)?,
                "wrapping must not change the verdict: {invocation}"
            );
        }
        Ok(())
    }

    /// End-to-end: a workflow whose `run:` invokes `just <recipe>`, plus a
    /// parsed justfile, surfaces the same `XTASK_CLI_WIRING_PATHS` finding the
    /// detector would produce for a direct `cargo xtask …` line. Without the
    /// justfile, the same workflow would pass silently — which is exactly
    /// the #15509 false-negative.
    #[test]
    fn workflow_just_invocation_is_a_cli_claim_when_justfile_resolves_it() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let workflow_path = directory.path().join("ci-fast.yml");
        let workflow_yaml = "\
name: just-fixture
on:
  pull_request:
    paths:
      - 'xtask/src/tasks/example_contract.rs'
permissions:
  contents: read
jobs:
  contract:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - run: just ci-fast
";
        fs::write(&workflow_path, workflow_yaml)?;
        let justfile = parse_justfile("ci-fast:\n    cargo run -p xtask -- ci-fast-check\n");

        let mut issues = Vec::new();
        lint_workflow_file_with_just(&workflow_path, true, &mut issues, &justfile)?;
        let wiring: Vec<_> =
            issues.iter().filter(|issue| issue.code == "XTASK_CLI_WIRING_PATHS").collect();
        assert_eq!(
            wiring.len(),
            1,
            "a just-routed CLI claim must fire the wiring finding; got: {issues:?}"
        );

        // Same workflow, but the justfile does not resolve `ci-fast`. The
        // wiring finding must be absent — same residual the gate had before
        // #15509 — and no typed `JUST_RECIPE_UNRESOLVED` either, because the
        // expanded verdict was simply "no xtask here".
        let mut issues = Vec::new();
        lint_workflow_file_with_just(&workflow_path, true, &mut issues, &NoJustRecipes)?;
        let wiring: Vec<_> =
            issues.iter().filter(|issue| issue.code == "XTASK_CLI_WIRING_PATHS").collect();
        assert!(
            wiring.is_empty(),
            "without a recipe resolution, the old residual behavior holds: {issues:?}"
        );
        Ok(())
    }

    /// A `run: just <recipe>` invocation that does not appear in the justfile
    /// surfaces as `JUST_RECIPE_UNRESOLVED` rather than silently passing. This
    /// is the AC-3 typed-unresolved-state requirement.
    #[test]
    fn workflow_with_undefined_just_recipe_reports_typed_finding() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let workflow_path = directory.path().join("orphan.yml");
        let workflow_yaml = "\
name: just-orphan
on:
  pull_request:
    paths:
      - 'xtask/src/tasks/example_contract.rs'
permissions:
  contents: read
jobs:
  contract:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - run: just recipe-renamed-without-replacement
";
        fs::write(&workflow_path, workflow_yaml)?;
        let justfile = parse_justfile("ci-fast:\n    cargo run -p xtask -- ci-fast-check\n");

        let mut issues = Vec::new();
        lint_workflow_file_with_just(&workflow_path, true, &mut issues, &justfile)?;
        let unresolved: Vec<_> =
            issues.iter().filter(|issue| issue.code == "JUST_RECIPE_UNRESOLVED").collect();
        assert_eq!(
            unresolved.len(),
            1,
            "undefined recipe must surface as JUST_RECIPE_UNRESOLVED; got: {issues:?}"
        );
        Ok(())
    }

    /// A cyclic just recipe (`a -> b -> a`) surfaces as `JUST_RECIPE_CYCLIC`
    /// with a depth-bounded expansion. The lint does not lock.
    #[test]
    fn workflow_with_cyclic_just_recipe_reports_typed_finding() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let workflow_path = directory.path().join("cyclic.yml");
        let workflow_yaml = "\
name: just-cyclic
on:
  pull_request:
    paths:
      - 'xtask/src/tasks/example_contract.rs'
permissions:
  contents: read
jobs:
  contract:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - run: just recipe-a
";
        fs::write(&workflow_path, workflow_yaml)?;
        let justfile =
            parse_justfile("recipe-a:\n    just recipe-b\n\nrecipe-b:\n    just recipe-a\n");

        let mut issues = Vec::new();
        lint_workflow_file_with_just(&workflow_path, true, &mut issues, &justfile)?;
        let cyclic: Vec<_> =
            issues.iter().filter(|issue| issue.code == "JUST_RECIPE_CYCLIC").collect();
        assert_eq!(
            cyclic.len(),
            1,
            "a cyclic recipe must surface as JUST_RECIPE_CYCLIC; got: {issues:?}"
        );
        Ok(())
    }
}
