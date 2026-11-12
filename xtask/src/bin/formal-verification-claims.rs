//! Validate the formal claim inventory, not the truth of a native proof result.
//! All source bindings are read from one immutable Git tree; no target code runs.
//! Maintainer runbook: docs/how-to/FORMAL_VERIFICATION.md.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    env,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "../formal_verification_claims_registry.rs"]
mod claim_registry;

const SCHEMA_VERSION: &str = "formal_verification_claims.v1";
const REGISTRY_PATH: &str = "xtask/src/formal_verification_claims_registry.rs";
const VALIDATOR_PATH: &str = "xtask/src/bin/formal-verification-claims.rs";
const COMPILED_REGISTRY: &str = include_str!("../formal_verification_claims_registry.rs");
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    claims: Vec<Claim>,
}

#[derive(Serialize)]
struct Report<'a> {
    report_kind: &'static str,
    native_proof_result: &'static str,
    schema_version: &'a str,
    source_commit: &'a str,
    source_tree: &'a str,
    inventory_inputs: BTreeMap<String, String>,
    claims: &'a [Claim],
}

struct Snapshot {
    root: PathBuf,
    commit: String,
    tree: String,
    inputs: RefCell<BTreeMap<String, String>>,
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).context("Git returned non-UTF-8 evidence")
}

fn clean(root: &Path) -> Result<()> {
    let status = git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    ensure!(status.is_empty(), "dirty working tree; no commit-bound report emitted");
    Ok(())
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with("archive/")
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
}

impl Snapshot {
    fn open(root: &Path) -> Result<Self> {
        clean(root)?;
        let commit = git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?.trim().to_owned();
        ensure!(
            matches!(commit.len(), 40 | 64) && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid commit identity"
        );
        let tree = git(root, &["rev-parse", &format!("{commit}^{{tree}}")])?.trim().to_owned();
        Ok(Self { root: root.to_owned(), commit, tree, inputs: RefCell::new(BTreeMap::new()) })
    }

    fn read(&self, path: &str) -> Result<String> {
        ensure!(valid_path(path), "noncanonical repository path: {path:?}");
        let listing =
            git(&self.root, &["--literal-pathspecs", "ls-tree", "-z", &self.tree, "--", path])?;
        let entries: Vec<_> = listing.split('\0').filter(|line| !line.is_empty()).collect();
        ensure!(entries.len() == 1, "missing or ambiguous committed input: {path}");
        let (metadata, actual_path) =
            entries[0].split_once('\t').context("malformed Git tree entry")?;
        let fields: Vec<_> = metadata.split_whitespace().collect();
        ensure!(
            actual_path == path
                && fields.len() == 3
                && matches!(fields[0], "100644" | "100755")
                && fields[1] == "blob",
            "input is not a regular committed file: {path}"
        );
        let content = git(&self.root, &["cat-file", "blob", fields[2]])?;
        self.inputs.borrow_mut().insert(path.to_owned(), fields[2].to_owned());
        Ok(content)
    }

    fn source(&self, source: &SourceRef) -> Result<String> {
        ensure!(!source.marker.trim().is_empty(), "empty source marker: {}", source.path);
        let content = self.read(&source.path)?;
        ensure!(content.contains(&source.marker), "stale source marker: {}", source.path);
        Ok(content)
    }

    fn bind_binary(&self) -> Result<()> {
        for (path, compiled) in
            [(REGISTRY_PATH, COMPILED_REGISTRY), (VALIDATOR_PATH, COMPILED_VALIDATOR)]
        {
            ensure!(self.read(path)? == compiled, "compiled source differs from snapshot: {path}");
        }
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        clean(&self.root)?;
        let head = git(&self.root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        ensure!(head.trim() == self.commit, "HEAD changed during inventory evaluation");
        Ok(())
    }
}

fn valid_claim_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("FV-") else {
        return false;
    };
    let Some((category, suffix)) = rest.rsplit_once('-') else {
        return false;
    };
    suffix.len() == 3
        && suffix.bytes().all(|b| b.is_ascii_digit())
        && category.split('-').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
}

fn explicit_bound(bound: &str) -> bool {
    // This is an inventory check, NOT an assertion that a solver used this bound.
    bound.split_whitespace().any(|word| word.parse::<u64>().is_ok_and(|value| value > 0))
}

fn required_gate(policy: &str, name: &str, runner: &str) -> Result<()> {
    let policy: serde_yaml_ng::Value = serde_yaml_ng::from_str(policy)?;
    let gates = policy["gates"].as_sequence().context("gate policy has no gates sequence")?;
    let matching: Vec<_> =
        gates.iter().filter(|gate| gate["name"].as_str() == Some(name)).collect();
    ensure!(matching.len() == 1, "required gate is missing or duplicated: {name}");
    let gate = matching[0];
    ensure!(gate["required"].as_bool() == Some(true), "gate is not required: {name}");
    ensure!(gate["tier"].as_str() == Some("merge_gate"), "gate has wrong tier: {name}");
    ensure!(gate["command"].as_str() == Some(runner), "gate command differs: {name}");
    Ok(())
}

fn required_gate_selected(workflow: &str, name: &str) -> Result<()> {
    let workflow: serde_yaml_ng::Value = serde_yaml_ng::from_str(workflow)?;
    let job = &workflow["jobs"]["merge-gate-shards"];
    ensure!(job.as_mapping().is_some(), "merge-gate-shards job missing");
    ensure!(job["continue-on-error"].is_null(), "shard job may ignore failure");
    // The surrounding draft/preflight predicate remains an explicit CI-contract dependency.
    ensure!(
        job["if"].as_str()
            == Some(
                "needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'"
            ),
        "shard job admission predicate changed; requalify the binding"
    );
    let matrix = job["strategy"]["matrix"].as_mapping().context("shard matrix missing")?;
    ensure!(matrix.len() == 1, "unsupported shard matrix expansion or exclusion");
    let rows =
        job["strategy"]["matrix"]["include"].as_sequence().context("shard matrix missing")?;
    let count = rows
        .iter()
        .filter(|row| {
            row["gates"]
                .as_str()
                .is_some_and(|value| value.split_whitespace().any(|gate| gate == name))
        })
        .count();
    ensure!(count == 1, "gate must be selected by exactly one shard: {name}");
    let steps = job["steps"].as_sequence().context("shard steps missing")?;
    let selected: Vec<_> = steps
        .iter()
        .filter(|step| step["name"].as_str() == Some("Run merge-gate shard with receipts"))
        .collect();
    ensure!(selected.len() == 1, "canonical shard execution step missing or duplicated");
    let step = selected[0];
    ensure!(
        step["if"].is_null() && step["continue-on-error"].is_null(),
        "shard execution is optional"
    );
    ensure!(
        step["env"]["SHARD_GATES"].as_str() == Some("${{ matrix.gates }}"),
        "shard gate input changed"
    );
    ensure!(step["shell"].as_str() == Some("bash"), "shard executor shell changed");
    let run = step["run"].as_str().context("shard has no executable body")?;
    for line in [
        "python3 scripts/ci/run_gate_shard.py \\",
        "--gate-policy .ci/gate-policy.yaml \\",
        "$SHARD_GATES",
        "exit \"$status\"",
    ] {
        ensure!(
            run.lines().any(|candidate| candidate.trim() == line),
            "shard command binding missing: {line}"
        );
    }
    // This does not interpret arbitrary Bash; native workflow results are still required.
    Ok(())
}

fn required_step(workflow: &str, job_name: &str, command: &str) -> Result<()> {
    let workflow: serde_yaml_ng::Value = serde_yaml_ng::from_str(workflow)?;
    let job = &workflow["jobs"][job_name];
    ensure!(job.as_mapping().is_some(), "required job missing: {job_name}");
    ensure!(job["continue-on-error"].is_null(), "required job may ignore failure");
    let steps = job["steps"].as_sequence().context("required job steps missing")?;
    let matching: Vec<_> =
        steps.iter().filter(|step| step["run"].as_str() == Some(command)).collect();
    ensure!(matching.len() == 1, "required execution step missing or duplicated: {command}");
    ensure!(
        matching[0]["if"].is_null() && matching[0]["continue-on-error"].is_null(),
        "required execution step is optional"
    );
    Ok(())
}

fn authority(snapshot: &Snapshot, claim: &Claim, authority: &Authority) -> Result<()> {
    match authority.kind {
        AuthorityKind::SourceMarker => {
            ensure!(
                authority.policy.is_none() && authority.workflow.is_none(),
                "unexpected gate fields"
            );
            let binding = authority.source.as_ref().context("missing authority source")?;
            let body = snapshot.source(binding)?;
            let runner = claim.runner.as_deref().context("missing authority runner")?;
            match binding.path.as_str() {
                ".github/workflows/ci.yml" => required_step(&body, "check-all-targets", runner)?,
                "scripts/ci/required-merge-surface.sh" => {
                    ensure!(
                        body.lines().any(|line| line == runner),
                        "required producer command missing"
                    );
                    let workflow = snapshot.read(".github/workflows/ci.yml")?;
                    required_step(
                        &workflow,
                        "check-all-targets",
                        "bash scripts/ci/required-merge-surface.sh",
                    )?;
                }
                _ => bail!("unsupported source authority: {}", binding.path),
            }
        }
        AuthorityKind::RequiredGate => {
            ensure!(authority.source.is_none(), "unexpected source-only field");
            let policy = authority.policy.as_ref().context("missing gate policy")?;
            let expected: Vec<serde_yaml_ng::Value> = serde_yaml_ng::from_str(&policy.marker)?;
            ensure!(expected.len() == 1, "gate declaration must name exactly one policy row");
            let name = expected[0]["name"].as_str().context("gate declaration has no name")?;
            let runner = claim.runner.as_deref().context("gate runner missing")?;
            required_gate(&snapshot.read(&policy.path)?, name, runner)?;
            let workflow = authority.workflow.as_ref().context("missing workflow binding")?;
            required_gate_selected(&snapshot.read(&workflow.path)?, name)?;
        }
    }
    Ok(())
}

fn registry() -> Result<Registry> {
    serde_json::from_str(claim_registry::REGISTRY_JSON).context("parse formal claim registry")
}

fn validate(snapshot: &Snapshot, registry: &Registry) -> Result<()> {
    ensure!(registry.schema_version == SCHEMA_VERSION, "unsupported schema version");
    ensure!(!registry.claims.is_empty(), "claim inventory is empty");
    let mut ids = BTreeSet::new();
    for claim in &registry.claims {
        ensure!(valid_claim_id(&claim.id), "invalid claim ID: {}", claim.id);
        ensure!(ids.insert(&claim.id), "duplicate claim ID: {}", claim.id);
        ensure!(
            claim.issue.strip_prefix('#').is_some_and(|n| {
                !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && n.parse::<u64>().is_ok_and(|n| n > 0)
            }),
            "invalid owner issue: {}",
            claim.id
        );
        snapshot.source(&claim.target).with_context(|| format!("{}: target", claim.id))?;
        for (field, text) in [
            ("subject", &claim.subject),
            ("assertion", &claim.assertion),
            ("bound", &claim.bound),
            ("assumptions", &claim.assumptions),
            ("limitations", &claim.limitations),
            ("counterexample", &claim.counterexample),
        ] {
            ensure!(
                !text.trim().is_empty() && !text.contains(['\n', '\r']),
                "{}: invalid {field}",
                claim.id
            );
        }
        if claim.evidence == Evidence::BoundedModel {
            ensure!(
                explicit_bound(&claim.bound),
                "{}: no explicit positive numeric bound",
                claim.id
            );
        }
        if let Some(runner) = &claim.runner {
            ensure!(
                !runner.trim().is_empty() && !runner.contains(['\n', '\r']),
                "{}: empty/multiline runner",
                claim.id
            );
        }
        for forbidden in ["all possible", "fully proven", "unconditionally correct"] {
            ensure!(
                !claim.assertion.to_ascii_lowercase().contains(forbidden),
                "{}: universal wording",
                claim.id
            );
        }
        match claim.status {
            Status::Enforced => {
                ensure!(
                    matches!(claim.evidence, Evidence::BuildCheck | Evidence::StructuralInvariant),
                    "{}: native proof admission is not implemented",
                    claim.id
                );
                ensure!(claim.runner.is_some(), "{}: missing runner", claim.id);
                authority(snapshot, claim, claim.authority.as_ref().context("missing authority")?)?;
            }
            Status::ImplementedUnwired => {
                ensure!(
                    claim.runner.is_some() && claim.authority.is_none(),
                    "{}: invalid unwired metadata",
                    claim.id
                );
            }
            Status::Planned => {
                ensure!(
                    claim.runner.is_none() && claim.authority.is_none(),
                    "{}: planned claim advertises evidence",
                    claim.id
                );
            }
        }
    }
    Ok(())
}

fn report<'a>(snapshot: &'a Snapshot, registry: &'a Registry) -> Report<'a> {
    Report {
        report_kind: "inventory_validation",
        native_proof_result: "not_evaluated",
        schema_version: &registry.schema_version,
        source_commit: &snapshot.commit,
        source_tree: &snapshot.tree,
        inventory_inputs: snapshot.inputs.borrow().clone(),
        claims: &registry.claims,
    }
}

fn markdown(report: &Report<'_>) -> Result<String> {
    let mut text = format!(
        "# Formal verification inventory\n\nCommit: `{}`\n\nTree: `{}`\n\n\
         **Inventory validation only. Native proof result: `not_evaluated`.**\n\n\
         Rollout status is not a current proof result. `planned` is not evidence; \
         `implemented_unwired` has no required execution route. `enforced` records an \
         authority declaration, not observation of a successful run. Source markers \
         are structural bindings, not semantic equivalence or live-consumer proof.\n\n",
        report.source_commit, report.source_tree
    );
    for claim in report.claims {
        text.push_str(&format!(
            "## {} — {}\n\nStatus: `{}`. Evidence class: `{}`. Owner: {}.\n\n\
             **Claim:** {}\n\n**Scope/bound:** {}\n\n**Assumptions:** {}\n\n\
             **Limitations:** {}\n\n**Counterexample/replay:** {}\n\n\
             Target: `{}`\n\nRunner: `{}`\n\n",
            claim.id,
            claim.subject,
            claim.status.as_str(),
            claim.evidence.as_str(),
            claim.issue,
            claim.assertion,
            claim.bound,
            claim.assumptions,
            claim.limitations,
            claim.counterexample,
            claim.target.path,
            claim.runner.as_deref().unwrap_or("not implemented")
        ));
    }
    text.push_str("## Exact inventory inputs\n\n```json\n");
    text.push_str(&serde_json::to_string_pretty(&report.inventory_inputs)?);
    text.push_str(
        "\n```\n\nThese blobs are inventory inputs, not a complete native proof closure.\n",
    );
    Ok(text)
}

#[derive(Clone, Copy)]
enum Mode {
    Check,
    Json,
    Markdown,
    Help,
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [] => Mode::Check,
        [arg] if arg == "--check" => Mode::Check,
        [arg] if arg == "--json" => Mode::Json,
        [arg] if arg == "--markdown" => Mode::Markdown,
        [arg] if arg == "--help" || arg == "-h" => Mode::Help,
        _ => bail!("usage: formal-verification-claims [--check|--json|--markdown|--help]"),
    };
    if matches!(mode, Mode::Help) {
        println!("usage: formal-verification-claims [--check|--json|--markdown|--help]");
        return Ok(());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().context("missing repository root")?;
    let snapshot = Snapshot::open(root)?;
    snapshot.bind_binary()?;
    let registry = registry()?;
    validate(&snapshot, &registry)?;
    let report = report(&snapshot, &registry);
    let output = match mode {
        Mode::Check => format!(
            "inventory_validated; native_proof=not_evaluated; claims={}; head={}; tree={}\n",
            registry.claims.len(),
            snapshot.commit,
            snapshot.tree
        ),
        Mode::Json => format!("{}\n", serde_json::to_string_pretty(&report)?),
        Mode::Markdown => markdown(&report)?,
        Mode::Help => String::new(),
    };
    snapshot.finish()?;
    print!("{output}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("formal-verification-claims NOT_PROVEN: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "../formal_verification_claims_tests.rs"]
mod tests;
