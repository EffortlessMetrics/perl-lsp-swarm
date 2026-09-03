//! `release_live_controls.v1` — a read-only observer of LIVE GitHub release
//! enforcement (#9403).
//!
//! This module answers one question honestly: *what does GitHub actually
//! enforce right now*, as distinct from what the repository's checked-in
//! source policy (workflow YAML, CODEOWNERS, documented process) merely
//! *describes*. A workflow file naming a required check is not evidence that
//! GitHub's branch protection requires it — only the live API is.
//!
//! Three invariants hold everywhere in this module and its submodules:
//!
//! 1. **Classic branch protection and rulesets are additive.** GitHub
//!    enforces both simultaneously, so a required-contexts view that reads
//!    only one of them understates enforcement. [`evaluate::required_contexts_union`]
//!    merges them, and is `NOT_PROVEN` — never guessed — when either half, or
//!    a contributing ruleset's rules, could not be read.
//! 2. **An inaccessible half is `NOT_PROVEN`, never inferred.** GitHub
//!    returns the same HTTP 404 for "no branch protection configured" and
//!    for "this token cannot see branch protection". Collapsing those into
//!    one reading — in either direction — is exactly the failure this module
//!    exists to refuse. See [`live::collect_classic_protection`] for the
//!    discriminator, and [`model::ObservationState`] for the three states
//!    every control is reported in.
//! 3. **Nothing here mutates a live setting.** Every request issued by
//!    [`live`] is `gh api <path>` (a GET) or `gh --version`. No branch
//!    protection, ruleset, environment, or release setting is ever created,
//!    edited, or deleted from this crate.

mod evaluate;
mod live;
mod model;

pub use evaluate::{identity_match, limitations, required_contexts_union, verdict};
pub use live::{
    ApiError, ReadOnlyCommands, SystemCommands, collect_classic_protection, collect_environments,
    collect_identity, collect_release_posture, collect_rulesets, observe, parse_http_status,
};
pub use model::{
    BypassActor, ClassicProtection, Currency, DeploymentBranchPolicy, Environment,
    EnvironmentProtectionRule, IdentityMatch, Instrument, LiveControlsReceipt, ObservationState,
    Observed, PullRequestReviewRule, RELEASE_LIVE_CONTROLS_SCHEMA_VERSION, ReleasePosture,
    RepositoryControls, RepositoryIdentity, RepositorySubject, RequiredContextRow,
    RequiredContextsUnion, RequiredStatusChecks, Ruleset, RulesetRule, UnionContext, Verdict,
};

use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Deserialize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Process exit code a caller should use when the verdict is `NOT_PROVEN`.
///
/// Distinct from 1 so a shell caller can tell "observed and enforcement is
/// incomplete or unverifiable" from "the observer itself failed to run".
pub const NOT_PROVEN_EXIT_CODE: i32 = 3;

const PRODUCT_IDENTITY_PATH: &str = "policy/product-identity.toml";
const DEFAULT_BRANCH: &str = "main";

#[derive(Debug, Deserialize)]
struct ProductIdentityToml {
    product: ProductIdentitySection,
}

#[derive(Debug, Deserialize)]
struct ProductIdentitySection {
    #[serde(default)]
    public_repository: Option<String>,
    #[serde(default)]
    development_repository: Option<String>,
}

/// Read the development and public repositories from `policy/product-identity.toml` —
/// the checked-in authority for which repositories to observe, so this module
/// never hardcodes a repository name.
///
/// Returns the development repository first, then the public repository.
/// Either key missing is an error naming exactly which one.
pub fn subjects_from_product_identity(
    repo_root: &Path,
    branch_override: Option<&str>,
) -> Result<Vec<RepositorySubject>> {
    let path = repo_root.join(PRODUCT_IDENTITY_PATH);
    let raw = std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading product identity contract {}", path.display()))?;
    let contract: ProductIdentityToml = toml::from_str(&raw)
        .wrap_err_with(|| format!("parsing product identity contract {}", path.display()))?;

    let development = contract
        .product
        .development_repository
        .ok_or_else(|| eyre!("{} is missing product.development_repository", path.display()))?;
    let public = contract
        .product
        .public_repository
        .ok_or_else(|| eyre!("{} is missing product.public_repository", path.display()))?;

    let (development_branch, public_branch) = match branch_override {
        Some(branch) => (branch.to_string(), branch.to_string()),
        None => {
            let topology = crate::contributor_topology::build_projection(repo_root, None)
                .map_err(|error| eyre!("loading contributor topology branch authority: {error}"))?;
            (
                topology.static_topology.development_default_branch,
                topology.static_topology.publication_branch,
            )
        }
    };

    Ok(vec![
        parse_subject(&development, &development_branch)?,
        parse_subject(&public, &public_branch)?,
    ])
}

fn parse_subject(full_name: &str, branch: &str) -> Result<RepositorySubject> {
    let (owner, name) = full_name
        .split_once('/')
        .ok_or_else(|| eyre!("{full_name:?} is not an owner/name repository identity"))?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        bail!("{full_name:?} is not an owner/name repository identity");
    }
    Ok(RepositorySubject {
        owner: owner.to_string(),
        name: name.to_string(),
        branch: branch.to_string(),
    })
}

/// Options for one `release-live-controls` observation run.
#[derive(Debug, Clone)]
pub struct ObserveOptions {
    pub repo_root: PathBuf,
    /// Explicit `owner/name` repositories, overriding the product-identity
    /// defaults when non-empty.
    pub repositories: Vec<String>,
    pub branch: Option<String>,
    /// Path the complete receipt (pretty JSON) is written to, if any.
    pub out: Option<PathBuf>,
    /// Emit the receipt as JSON to stdout instead of the human summary.
    pub json: bool,
}

/// Observe the requested repositories and report the typed verdict.
///
/// This function never calls `std::process::exit`: the caller maps
/// [`Verdict::NotProven`] to [`NOT_PROVEN_EXIT_CODE`].
pub fn run(options: ObserveOptions) -> Result<Verdict> {
    let subjects = if options.repositories.is_empty() {
        subjects_from_product_identity(&options.repo_root, options.branch.as_deref())?
    } else {
        let branch = options.branch.as_deref().unwrap_or(DEFAULT_BRANCH);
        options
            .repositories
            .iter()
            .map(|full_name| parse_subject(full_name, branch))
            .collect::<Result<Vec<_>>>()?
    };

    let commands = SystemCommands;
    let observed_at = chrono::Utc::now().to_rfc3339();
    let receipt = observe(&commands, &subjects, observed_at);

    if let Some(path) = &options.out {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("creating {}", parent.display()))?;
        }
        let pretty = serde_json::to_string_pretty(&receipt)
            .wrap_err("serializing the live-controls receipt")?;
        std::fs::write(path, format!("{pretty}\n"))
            .wrap_err_with(|| format!("writing {}", path.display()))?;
    }

    if options.json {
        let pretty = serde_json::to_string_pretty(&receipt)
            .wrap_err("serializing the live-controls receipt")?;
        write_out(&format!("{pretty}\n"))?;
    } else {
        for repository in &receipt.repositories {
            write_out(&format!("{}\n", render_repository_summary(repository)))?;
        }
        write_out(&format!(
            "verdict: {}\n",
            match receipt.verdict {
                Verdict::Observed => "OBSERVED",
                Verdict::NotProven => "NOT_PROVEN",
            }
        ))?;
    }

    Ok(receipt.verdict)
}

fn render_repository_summary(repository: &RepositoryControls) -> String {
    format!(
        "{}: identity={} identity_match={} classic_branch_protection={} branch_rulesets={} tag_rulesets={} environments={} release_posture={} required_contexts_union={}",
        repository.requested.render(),
        observation_label(repository.identity.state),
        identity_match_label(&repository.identity_match),
        observation_label(repository.classic_branch_protection.state),
        observation_label(repository.branch_rulesets.state),
        observation_label(repository.tag_rulesets.state),
        observation_label(repository.environments.state),
        if repository.release_posture.immutable_releases.is_conclusive()
            && repository.release_posture.tag_rulesets_present.is_conclusive()
        {
            "CONCLUSIVE"
        } else {
            "NOT_PROVEN"
        },
        observation_label(repository.required_contexts_union.state),
    )
}

fn observation_label(state: ObservationState) -> &'static str {
    match state {
        ObservationState::Observed => "OBSERVED",
        ObservationState::Absent => "ABSENT",
        ObservationState::NotProven => "NOT_PROVEN",
    }
}

fn identity_match_label(identity_match: &IdentityMatch) -> &'static str {
    match identity_match {
        IdentityMatch::Matched => "MATCHED",
        IdentityMatch::Mismatched { .. } => "MISMATCHED",
        IdentityMatch::NotProven { .. } => "NOT_PROVEN",
    }
}

/// Load a previously written receipt from disk.
///
/// Forces `currency = Currency::Snapshot` regardless of what the file
/// claims, so a replayed observation can never represent itself as current.
/// Also runs [`LiveControlsReceipt::structural_problem`], rejecting a
/// malformed row rather than trusting it.
pub fn load_snapshot(path: &Path) -> Result<LiveControlsReceipt> {
    let raw = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("reading live-controls snapshot {}", path.display()))?;
    let mut receipt: LiveControlsReceipt = serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing live-controls snapshot {}", path.display()))?;
    receipt.currency = Currency::Snapshot;
    if let Some(problem) = receipt.structural_problem() {
        bail!("live-controls snapshot {} is structurally invalid: {problem}", path.display());
    }
    Ok(receipt)
}

fn write_out(rendered: &str) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(rendered.as_bytes()).wrap_err("writing the live-controls output")?;
    handle.flush().wrap_err("flushing the live-controls output")?;
    Ok(())
}
