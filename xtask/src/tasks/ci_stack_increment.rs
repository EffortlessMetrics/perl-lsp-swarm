//! CLI surface for the exact parent-to-child incremental proof (#11229 S1):
//! `cargo xtask ci-stack subject|plan|validate|explain`.
//!
//! The commands are thin, fail-closed adapters over
//! [`crate::stack_increment`]: JSON files drive every tested path, and one
//! live `--pr` assembly path binds GitHub PR facts plus read-only Git probes
//! into the same typed compile. Nothing here mutates branches, PRs, checks,
//! or protected state.

use clap::Subcommand;
use color_eyre::eyre::{Result, bail, eyre};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use xtask::git_ancestry::{AncestryDisposition, classify_ancestry};
use xtask::stack_increment::{
    STACK_INCREMENT_RESULT_SCHEMA, STACK_INCREMENT_SUBJECT_SCHEMA, StackEndpoint,
    StackIncrementResultV1, StackIncrementSubjectV1, StackPlanRequest, StackSubjectInput,
    TrustContext, compile_stack_plan, compile_subject, render_explanation, resolve_endpoint_live,
    stack_plan_digest, subject_digest, validate_result, validate_subject,
};

/// One `ci-stack` subcommand.
#[derive(Subcommand)]
pub enum StackIncrementCommand {
    /// Assemble and validate an exact stack-increment subject.
    Subject {
        /// Input JSON file projecting a full subject input; mutually exclusive
        /// with `--pr`.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Assemble the child subject from a live PR in this repository.
        #[arg(long)]
        pr: Option<u64>,
        /// Repository used for Git-backed endpoint resolution.
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        /// Write the compiled subject JSON here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Select sufficient stack-local proof through the shared route planner.
    Plan {
        /// Input JSON file carrying the validated subject plus its authority
        /// projections, gate scopes, and execution identities.
        #[arg(long)]
        input: PathBuf,
        /// Output path for the compiled plan receipt.
        #[arg(long, default_value = "target/receipts/ci-stack-plan.json")]
        out: PathBuf,
    },
    /// Validate a subject or result artifact file against its contract.
    Validate {
        /// Artifact file whose `schema` field selects the validator.
        input: PathBuf,
    },
    /// Render the stable advisory explanation for a compiled result.
    Explain {
        /// Compiled result JSON to explain.
        #[arg(long)]
        result: PathBuf,
    },
}

/// JSON envelope for `ci-stack plan --input <file>`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanFileEnvelope {
    /// Fully validated subject artifact.
    subject: StackIncrementSubjectV1,
    /// Pure plan request over that exact subject.
    request: StackPlanRequest,
}

/// Live PR projection reduced to the facts admission consumes.
#[derive(Deserialize)]
struct PullRequestFacts {
    number: u64,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    #[serde(rename = "baseRefOid")]
    base_ref_oid: String,
    body: Option<String>,
    /// GitHub-derived cross-repository fact; trust is never asserted here.
    #[serde(rename = "isCrossRepository", default)]
    is_cross_repository: bool,
    /// Owner login of the repository holding the PR head.
    #[serde(rename = "headRepositoryOwner")]
    head_repository_owner: GhOwnerLogin,
    /// `owner/name` of the repository the PR targets.
    #[serde(rename = "baseRepository")]
    base_repository: GhBaseRepository,
}

/// `gh pr view` projection of a head repository owner.
#[derive(Deserialize)]
struct GhOwnerLogin {
    login: String,
}

/// `gh pr view` projection of the base repository identity.
#[derive(Deserialize)]
struct GhBaseRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

/// Dispatch entry point wired into the root CLI.
pub fn run(command: StackIncrementCommand) -> Result<()> {
    match command {
        StackIncrementCommand::Subject { input, pr, repository, out } => {
            run_subject(input.as_deref(), pr, &repository, out.as_deref())
        }
        StackIncrementCommand::Plan { input, out } => run_plan(&input, &out),
        StackIncrementCommand::Validate { input } => run_validate(&input),
        StackIncrementCommand::Explain { result } => run_explain(&result),
    }
}

fn gh_pr_view(repository: &Path, pr: u64) -> Result<PullRequestFacts> {
    let output = std::process::Command::new("gh")
        .args(["pr", "view", &pr.to_string(), "--json"])
        .arg("number,headRefName,headRefOid,baseRefOid,body,isCrossRepository,headRepositoryOwner,baseRepository")
        .current_dir(repository)
        .output()
        .map_err(|error| eyre!("failed to spawn gh: {error}"))?;
    if !output.status.success() {
        bail!("gh pr view failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| eyre!("gh pr view parse: {error}"))
}

fn ancestry_relation(
    repository: &Path,
    parent_head: &str,
    child_head: &str,
) -> Result<xtask::stack_increment::RelatedHistory> {
    use xtask::stack_increment::RelatedHistory;
    let receipt = classify_ancestry(repository, parent_head, child_head);
    Ok(match receipt.disposition {
        AncestryDisposition::Ancestor => RelatedHistory::Ancestor,
        AncestryDisposition::Diverged => RelatedHistory::Diverged,
        AncestryDisposition::Unrelated => RelatedHistory::Unrelated,
        AncestryDisposition::NotProvenShallow => RelatedHistory::NotProvenShallow,
        AncestryDisposition::NotProvenPartialClone => RelatedHistory::NotProvenPartialClone,
        AncestryDisposition::NotProvenMissingObject => RelatedHistory::NotProvenMissingObject,
        other => bail!(
            "ancestry classification could not decide ({:?}); fetch both heads and retry",
            other.as_str()
        ),
    })
}

/// Assemble a subject input directly from a live PR. Fails closed unless the
/// child PR carries the exact machine-readable declaration pinned to its
/// actual base head.
fn assemble_live_subject_input(repository: &Path, pr: u64) -> Result<StackSubjectInput> {
    let facts = gh_pr_view(repository, pr)?;
    let body = facts.body.clone().unwrap_or_default();
    let edge = xtask::stack_increment::parse_stack_edge_declaration(&body)
        .map_err(|error| eyre!("PR #{} cannot admit a stack edge: {error}", facts.number))?;
    let declared_parent_head = edge.declared_parent_head_sha.clone().ok_or_else(|| {
        eyre!(
            "declaration must pin `parent-head=<40hex>` so admission can bind child base \
                 {} to the exact declared parent head",
            facts.base_ref_oid
        )
    })?;
    if facts.base_ref_oid != declared_parent_head {
        bail!(
            "child base ref resolves to {} but the declaration pins \
             {declared_parent_head}; moved or wrong-parent stacks are refused",
            facts.base_ref_oid
        );
    }
    // Bind the declared parent PR by query (#13360 root cause 7): the
    // declared number must resolve to a real PR whose observed head equals
    // the pinned parent head, and that PR supplies the parent endpoint
    // identity. A fabricated number or a parent pointing elsewhere refuses
    // here instead of compiling a false-looking exact subject.
    let parent_facts = gh_pr_view(repository, edge.parent_pr_number).map_err(|error| {
        eyre!("declared parent PR #{} cannot be resolved: {error}", edge.parent_pr_number)
    })?;
    if parent_facts.number != edge.parent_pr_number {
        bail!(
            "declared parent PR {} resolved to PR {}; the declared parent identity does not bind",
            edge.parent_pr_number,
            parent_facts.number
        );
    }
    if parent_facts.head_ref_oid != declared_parent_head {
        bail!(
            "declared parent PR #{} observes head {} but the declaration pins \
             {declared_parent_head}; the parent moved or the declaration is false, and both are \
             refused",
            parent_facts.number,
            parent_facts.head_ref_oid
        );
    }
    // Derive same-repository trust from GitHub facts instead of asserting it
    // (#13360 root cause 6): a fork PR is refused even when its head object
    // happens to exist in the local clone.
    let local_identity = repository_identity(repository)?;
    let expected_owner = local_identity.split('/').next().unwrap_or_default().to_string();
    let same_repository = !facts.is_cross_repository
        && facts.base_repository.name_with_owner == local_identity
        && facts.head_repository_owner.login == expected_owner
        && parent_facts.base_repository.name_with_owner == local_identity
        && parent_facts.head_repository_owner.login == expected_owner;
    if !same_repository {
        bail!(
            "live --pr assembly admits only same-repository stacks: child PR #{} derives base \
             {:?} with head owner {:?}, parent PR #{} derives base {:?} with head owner {:?}, \
             while the local repository is {local_identity:?}",
            facts.number,
            facts.base_repository.name_with_owner,
            facts.head_repository_owner.login,
            parent_facts.number,
            parent_facts.base_repository.name_with_owner,
            parent_facts.head_repository_owner.login
        );
    }
    let (parent_sha, parent_tree) =
        resolve_endpoint_live(repository, &parent_facts.head_ref_oid)
            .map_err(|error| eyre!("parent head unresolvable locally: {error}"))?;
    let (child_sha, child_tree) = resolve_endpoint_live(repository, facts.head_ref_oid.as_str())
        .map_err(|error| eyre!("child head unresolvable locally: {error}"))?;
    let history = ancestry_relation(repository, &parent_sha, &child_sha)?;
    let delta =
        xtask::stack_increment::compute_delta_from_trees(repository, &parent_tree, &child_tree)
            .map_err(|error| eyre!("{error}"))?;
    Ok(StackSubjectInput {
        repository: local_identity,
        event_id: None,
        parent: StackEndpoint {
            pr_number: parent_facts.number,
            issue_node_id: String::new(),
            branch: parent_facts.head_ref_name,
            head_sha: parent_sha.clone(),
            head_tree: parent_tree,
        },
        child: StackEndpoint {
            pr_number: facts.number,
            issue_node_id: String::new(),
            branch: facts.head_ref_name,
            head_sha: child_sha.clone(),
            head_tree: child_tree,
        },
        edge: Some(edge),
        child_base_expected_head_sha: facts.base_ref_oid,
        observed_parent_head_sha: parent_sha,
        observed_child_head_sha: child_sha,
        trust: TrustContext {
            same_repository_declared: same_repository,
            external_context_admitted: false,
        },
        history,
        delta,
    })
}

/// Reduce an origin URL to `owner/name` without trusting the URL scheme.
fn repository_identity(repository: &Path) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["remote", "get-url", "origin"])
        .output()
        .map_err(|error| eyre!("failed to spawn git: {error}"))?;
    if !output.status.success() {
        bail!("origin remote unavailable: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let tail = url.trim_end_matches(".git");
    let mut parts: Vec<&str> = tail.rsplit(['/', ':']).take(2).collect();
    parts.reverse();
    let identity = parts.join("/");
    if !identity.contains('/') || identity.contains(char::is_whitespace) {
        bail!("cannot reduce origin URL {url:?} to an owner/name repository identity");
    }
    Ok(identity)
}

fn load_subject_input(
    input: Option<&Path>,
    pr: Option<u64>,
    repository: &Path,
) -> Result<StackSubjectInput> {
    match (input, pr) {
        (Some(_), Some(_)) => bail!("pass either --input <file> or --pr <number>, not both"),
        (Some(path), None) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|error| eyre!("failed to read {}: {error}", path.display()))?;
            serde_json::from_str(&raw)
                .map_err(|error| eyre!("subject input JSON is invalid: {error}"))
        }
        (None, Some(pr)) => assemble_live_subject_input(repository, pr),
        (None, None) => bail!("pass either --input <file> or --pr <number>"),
    }
}

fn write_pretty<T: serde::Serialize>(out: Option<&Path>, value: &T) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| eyre!("serialize: {error}"))?;
    if let Some(path) = out {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| eyre!("create {}: {error}", parent.display()))?;
        }
        std::fs::write(path, &bytes).map_err(|error| eyre!("write {}: {error}", path.display()))?;
    }
    Ok(bytes)
}

fn run_subject(
    input: Option<&Path>,
    pr: Option<u64>,
    repository: &Path,
    out: Option<&Path>,
) -> Result<()> {
    let subject = compile_subject(load_subject_input(input, pr, repository)?)
        .map_err(|error| eyre!("{error}"))?;
    let bytes = write_pretty(out, &subject)?;
    println!("{}", String::from_utf8_lossy(&bytes));
    println!("subject digest {}", subject_digest(&subject));
    Ok(())
}

fn run_plan(input: &Path, out: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(input)
        .map_err(|error| eyre!("failed to read {}: {error}", input.display()))?;
    let envelope: PlanFileEnvelope =
        serde_json::from_str(&raw).map_err(|error| eyre!("plan request invalid: {error}"))?;
    validate_subject(&envelope.subject).map_err(|error| eyre!("subject refuses: {error}"))?;
    let request = StackPlanRequest { subject: envelope.subject, ..envelope.request };
    let plan = compile_stack_plan(request).map_err(|error| eyre!("{error}"))?;
    let digest = stack_plan_digest(&plan).map_err(|error| eyre!("{error}"))?;
    write_pretty(Some(out), &plan)?;
    println!("{}", String::from_utf8_lossy(&serde_json::to_vec(&plan)?));
    println!("route plan digest {digest}");
    Ok(())
}

fn run_validate(input: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(input)
        .map_err(|error| eyre!("failed to read {}: {error}", input.display()))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| eyre!("invalid JSON: {error}"))?;
    let schema = parsed
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| eyre!("artifact carries no schema field"))?;
    match schema {
        STACK_INCREMENT_SUBJECT_SCHEMA => {
            let subject: StackIncrementSubjectV1 =
                serde_json::from_value(parsed).map_err(|error| eyre!("subject shape: {error}"))?;
            validate_subject(&subject).map_err(|error| eyre!("subject refuses: {error}"))?;
            println!("subject valid; digest {}", subject_digest(&subject));
        }
        STACK_INCREMENT_RESULT_SCHEMA => {
            let result: StackIncrementResultV1 =
                serde_json::from_value(parsed).map_err(|error| eyre!("result shape: {error}"))?;
            // Semantic validation, not identity spot-checks: rows, digests,
            // aggregates, and the published status must reconcile (#13360
            // root cause 5).
            validate_result(&result).map_err(|error| eyre!("result refuses: {error}"))?;
            println!("result valid; context status {:?}", result.context_status);
        }
        other => bail!("unsupported artifact schema {other:?}"),
    }
    Ok(())
}

fn run_explain(result_path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(result_path)
        .map_err(|error| eyre!("failed to read {}: {error}", result_path.display()))?;
    let result: StackIncrementResultV1 =
        serde_json::from_str(&raw).map_err(|error| eyre!("result shape: {error}"))?;
    // A forged or internally inconsistent artifact can never drive the
    // advisory exit code: semantic validation runs before any green decision
    // (#13360 root cause 5).
    validate_result(&result).map_err(|error| eyre!("result refuses: {error}"))?;
    print!("{}", render_explanation(&result));
    if matches!(
        result.context_status,
        xtask::stack_increment::ContextStatus::CurrentSuccess
            | xtask::stack_increment::ContextStatus::ScopedNoop
    ) {
        Ok(())
    } else {
        bail!("advisory context is not green: {:?}", result.context_status)
    }
}
