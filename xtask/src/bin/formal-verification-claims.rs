//! Truthful claim registry for the repository's formal-verification program.
//!
//! The strict data file is the reviewable denominator. This binary validates
//! it against the checked-out production sources and emits JSON or Markdown.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

const SCHEMA_VERSION: &str = "formal_verification_claims.v1";
#[path = "../formal_verification_claims_registry.rs"]
mod claim_registry;

const REGISTRY_SOURCE_PATH: &str = "xtask/src/formal_verification_claims_registry.rs";
const VALIDATOR_PATH: &str = "xtask/src/bin/formal-verification-claims.rs";
const COMPILED_REGISTRY: &str = claim_registry::REGISTRY_JSON;
const COMPILED_REGISTRY_SOURCE: &str = include_str!("../formal_verification_claims_registry.rs");
const COMPILED_VALIDATOR: &str = include_str!("formal-verification-claims.rs");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Enforced,
    ImplementedUnwired,
    Planned,
}

impl Status {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Enforced => "enforced",
            Self::ImplementedUnwired => "implemented_unwired",
            Self::Planned => "planned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    BuildCheck,
    StructuralInvariant,
    PropertyTest,
    BoundedModel,
    MemorySafetyCheck,
}

impl Evidence {
    const fn as_str(self) -> &'static str {
        match self {
            Self::BuildCheck => "build_check",
            Self::StructuralInvariant => "structural_invariant",
            Self::PropertyTest => "property_test",
            Self::BoundedModel => "bounded_model",
            Self::MemorySafetyCheck => "memory_safety_check",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum AuthorityKind {
    SourceMarker,
    RequiredGate,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceRef {
    path: String,
    marker: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Authority {
    kind: AuthorityKind,
    source: Option<SourceRef>,
    policy: Option<SourceRef>,
    workflow: Option<SourceRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    id: String,
    status: Status,
    evidence: Evidence,
    subject: String,
    assertion: String,
    target: SourceRef,
    runner: Option<String>,
    authority: Option<Authority>,
    bound: String,
    assumptions: String,
    limitations: String,
    counterexample: String,
    issue: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    schema_version: String,
    claims: Vec<Claim>,
}

#[derive(Serialize)]
struct RegistryOutput<'a> {
    schema_version: &'a str,
    source_commit: &'a str,
    source_tree: &'a str,
    claims: &'a [Claim],
}

struct SourceIdentity {
    commit: String,
    tree: String,
}

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live directly beneath the repository root")
}

fn registry() -> Result<RegistryFile> {
    serde_json::from_str(COMPILED_REGISTRY).context("parse formal claim registry")
}

fn valid_repo_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && !value.starts_with("archive/")
        && path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args.iter().copied())
        .current_dir(root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).context("git output was not UTF-8")
}

fn check_source(root: &Path, source: &SourceRef) -> Result<()> {
    ensure!(valid_repo_path(&source.path), "unsafe repository path {}", source.path);
    git_output(root, &["ls-files", "--error-unmatch", "--", &source.path])
        .with_context(|| format!("{} is not tracked", source.path))?;
    let path = root.join(&source.path);
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    ensure!(
        content.contains(&source.marker),
        "marker {:?} missing from {}",
        source.marker,
        source.path
    );
    Ok(())
}

fn check_authority(root: &Path, authority: &Authority) -> Result<()> {
    match authority.kind {
        AuthorityKind::SourceMarker => {
            ensure!(
                authority.policy.is_none() && authority.workflow.is_none(),
                "source_marker authority carries gate-only fields"
            );
            check_source(
                root,
                authority.source.as_ref().context("source_marker authority has no source")?,
            )
        }
        AuthorityKind::RequiredGate => {
            ensure!(
                authority.source.is_none(),
                "required_gate authority carries source-only fields"
            );
            check_source(
                root,
                authority
                    .policy
                    .as_ref()
                    .context("required_gate authority has no policy binding")?,
            )
            .context("required gate policy binding")?;
            check_source(
                root,
                authority
                    .workflow
                    .as_ref()
                    .context("required_gate authority has no workflow binding")?,
            )
            .context("required gate workflow binding")
        }
    }
}

fn source_identity(root: &Path) -> Result<SourceIdentity> {
    for (path, compiled) in
        [(REGISTRY_SOURCE_PATH, COMPILED_REGISTRY_SOURCE), (VALIDATOR_PATH, COMPILED_VALIDATOR)]
    {
        let runtime =
            fs::read_to_string(root.join(path)).with_context(|| format!("read {path}"))?;
        ensure!(runtime == compiled, "binary was compiled from a different {path}");
    }

    let status = git_output(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    ensure!(
        status.trim().is_empty(),
        "formal-verification claims require a clean working tree; dirty entries:\n{status}"
    );

    let commit = git_output(root, &["rev-parse", "HEAD"])?.trim().to_owned();
    let tree = git_output(root, &["rev-parse", "HEAD^{tree}"])?.trim().to_owned();
    ensure!(!commit.is_empty() && !tree.is_empty(), "git identity is empty");
    Ok(SourceIdentity { commit, tree })
}

fn valid_claim_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("FV-") else {
        return false;
    };
    let Some((category, suffix)) = rest.rsplit_once('-') else {
        return false;
    };
    suffix.len() == 3
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
        && category.split('-').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        })
}

fn validate(root: &Path, registry: &RegistryFile) -> Result<()> {
    ensure!(
        registry.schema_version == SCHEMA_VERSION,
        "unsupported schema version {}",
        registry.schema_version
    );
    ensure!(!registry.claims.is_empty(), "formal claim denominator is empty");
    let mut ids = BTreeSet::new();

    for claim in &registry.claims {
        ensure!(valid_claim_id(&claim.id), "{}: invalid claim ID", claim.id);
        ensure!(ids.insert(&claim.id), "{}: duplicate claim ID", claim.id);
        ensure!(
            claim.issue.len() > 1
                && claim.issue.starts_with('#')
                && claim.issue[1..].bytes().all(|byte| byte.is_ascii_digit()),
            "{}: issue must be a #number",
            claim.id
        );
        check_source(root, &claim.target).with_context(|| format!("{}: stale target", claim.id))?;

        for (name, value) in [
            ("subject", claim.subject.as_str()),
            ("assertion", claim.assertion.as_str()),
            ("bound", claim.bound.as_str()),
            ("assumptions", claim.assumptions.as_str()),
            ("limitations", claim.limitations.as_str()),
            ("counterexample", claim.counterexample.as_str()),
        ] {
            ensure!(!value.trim().is_empty(), "{}: empty {name}", claim.id);
            ensure!(!value.contains('\n'), "{}: multiline {name}", claim.id);
        }

        if claim.evidence == Evidence::BoundedModel {
            ensure!(
                claim.bound.bytes().any(|byte| byte.is_ascii_digit()),
                "{}: bounded model has no numeric bound",
                claim.id
            );
        }

        let assertion = claim.assertion.to_ascii_lowercase();
        for forbidden in ["all possible", "fully proven", "unconditionally correct"] {
            ensure!(
                !assertion.contains(forbidden),
                "{}: unsupported universal wording {forbidden:?}",
                claim.id
            );
        }

        match claim.status {
            Status::Enforced => {
                ensure!(claim.runner.is_some(), "{}: enforced claim has no runner", claim.id);
                check_authority(
                    root,
                    claim.authority.as_ref().with_context(|| {
                        format!("{}: enforced claim has no authority", claim.id)
                    })?,
                )
                .with_context(|| format!("{}: stale authority", claim.id))?;
            }
            Status::ImplementedUnwired => {
                ensure!(
                    claim.runner.is_some() && claim.authority.is_none(),
                    "{}: implemented-unwired metadata is inconsistent",
                    claim.id
                );
            }
            Status::Planned => {
                ensure!(
                    claim.runner.is_none() && claim.authority.is_none(),
                    "{}: planned claim advertises unearned evidence",
                    claim.id
                );
            }
        }
    }

    Ok(())
}

fn markdown(claims: &[Claim]) -> String {
    let mut output = String::from(
        "# Formal verification claims\n\n\
         `enforced` names a required authority; `implemented_unwired` has a checker but no merge authority; `planned` is not evidence.\n\n\
         | ID | Status | Evidence | Subject | Assertion | Bound | Issue |\n\
         | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for claim in claims {
        output.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} | {} | {} | {} |\n",
            claim.id,
            claim.status.as_str(),
            claim.evidence.as_str(),
            escape(&claim.subject),
            escape(&claim.assertion),
            escape(&claim.bound),
            claim.issue
        ));
    }
    output
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|")
}

fn usage() {
    eprintln!("usage: formal-verification-claims [--check | --json | --markdown]");
}

fn main() -> Result<()> {
    let root = repo_root()?;
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [] => "--check",
        [mode] if mode == "--check" => "--check",
        [mode] if mode == "--json" => "--json",
        [mode] if mode == "--markdown" => "--markdown",
        [mode] if mode == "--help" || mode == "-h" => {
            usage();
            return Ok(());
        }
        _ => {
            usage();
            bail!("expected exactly one supported mode");
        }
    };

    let registry = registry()?;
    validate(&root, &registry)?;
    let identity = source_identity(&root)?;
    match mode {
        "--check" => println!(
            "{SCHEMA_VERSION}: valid; claims={}; head={}; tree={}",
            registry.claims.len(),
            identity.commit,
            identity.tree
        ),
        "--json" => println!(
            "{}",
            serde_json::to_string_pretty(&RegistryOutput {
                schema_version: &registry.schema_version,
                source_commit: &identity.commit,
                source_tree: &identity.tree,
                claims: &registry.claims,
            })?
        ),
        "--markdown" => print!("{}", markdown(&registry.claims)),
        _ => unreachable!("validated mode"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid() -> Result<()> {
        let registry = registry()?;
        validate(&repo_root()?, &registry)
    }

    #[test]
    fn projections_cover_the_denominator_and_wire_values() -> Result<()> {
        let registry = registry()?;
        let rendered = markdown(&registry.claims);
        let json = serde_json::to_value(RegistryOutput {
            schema_version: &registry.schema_version,
            source_commit: "commit",
            source_tree: "tree",
            claims: &registry.claims,
        })?;

        assert_eq!(json["claims"].as_array().map(Vec::len), Some(registry.claims.len()));
        for claim in &registry.claims {
            assert!(rendered.contains(&claim.id), "Markdown omits {}", claim.id);
            assert!(rendered.contains(claim.status.as_str()));
            assert!(rendered.contains(claim.evidence.as_str()));
        }
        assert!(!rendered.contains("ImplementedUnwired"));
        assert!(!rendered.contains("BoundedModel"));
        Ok(())
    }

    #[test]
    fn only_enforced_claims_name_authority() -> Result<()> {
        for claim in registry()?.claims {
            assert_eq!(
                claim.status == Status::Enforced,
                claim.authority.is_some(),
                "{} has inconsistent authority metadata",
                claim.id
            );
        }
        Ok(())
    }

    #[test]
    fn claim_id_grammar_is_closed() -> Result<()> {
        for claim in registry()?.claims {
            assert!(valid_claim_id(&claim.id));
        }
        for invalid in
            ["FV-!1", "FV-A-1", "FV-a-001", "FV--001", "FV-A B-001", "FV-A-0001", "XX-A-001"]
        {
            assert!(!valid_claim_id(invalid), "accepted malformed ID {invalid}");
        }
        Ok(())
    }
}
