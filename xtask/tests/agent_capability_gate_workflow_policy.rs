//! Contract tests for the M4b agent-capability workflow routing.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, ensure};
use serde_yaml_ng::Value;
use tempfile::tempdir;

#[test]
fn agent_capability_gate_preserves_trust_and_failure_boundaries() -> Result<()> {
    let (content, workflow) = workflow()?;
    let jobs = mapping_value(&workflow, "jobs")?;
    let router = mapping_value(jobs, "route-agent-capability-gate")?;
    let self_hosted = mapping_value(jobs, "agent-capability-gate-self-hosted")?;
    let hosted = mapping_value(jobs, "agent-capability-gate-hosted")?;
    let router_script = route_script(router)?;

    ensure!(
        scalar_string(mapping_value(router, "runs-on")?)? == "ubuntu-24.04",
        "router must stay on the fixed hosted runner"
    );
    ensure!(
        scalar_string(mapping_value(router, "if")?)?
            .contains("github.event_name == 'pull_request'"),
        "router must explicitly cover the pull-request trigger"
    );
    ensure!(
        !content.contains("ubuntu-latest"),
        "workflow must not use the floating ubuntu-latest runner"
    );
    ensure!(
        content.contains("IS_FORK_PR") && content.contains("PR_AUTHOR_TYPE"),
        "router must retain fork and bot isolation inputs"
    );

    let self_runs_on = mapping_value(self_hosted, "runs-on")?;
    let self_hosted_labels = if self_runs_on.is_sequence() {
        sequence_strings(self_runs_on)?
    } else {
        sequence_strings(mapping_value(self_runs_on, "labels")?)?
    };
    ensure!(
        self_hosted_labels
            == ["self-hosted", "linux", "x64", "em-ci", "trusted-pr", "workflow-nano"],
        "self-hosted job labels changed"
    );
    ensure!(
        self_hosted_labels.len() == 6,
        "self-hosted job labels must not have missing or extra entries"
    );
    ensure!(
        scalar_string(mapping_value(hosted, "runs-on")?)? == "ubuntu-24.04",
        "fallback must stay on the pinned hosted image"
    );
    ensure!(
        scalar_string(mapping_value(mapping_value(self_hosted, "runs-on")?, "group")?)?
            == "em-ci-nano",
        "self-hosted job must stay in the workflow-nano runner group"
    );
    for (job, target) in [(self_hosted, "self_hosted"), (hosted, "github")] {
        let condition = scalar_string(mapping_value(job, "if")?)?;
        let expected_condition = format!(
            "needs.route-agent-capability-gate.result == 'success' && needs.route-agent-capability-gate.outputs.target == '{target}'"
        );
        ensure!(
            condition == expected_condition,
            "execution job routing condition changed for {target}"
        );
        ensure!(
            has_need(mapping_value(job, "needs")?, "route-agent-capability-gate"),
            "execution job must depend on the router"
        );
    }

    for required in [
        "pull_request:",
        "'.claude/agents/**'",
        "merge_group:",
        "push:",
        "      - main",
        "      - master",
        "contents: read",
        "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
        "github.event.pull_request.head.repo.full_name != github.repository",
        "ORG: ${{ github.repository_owner }}",
        "bot_pr_github_hosted",
        "runner_token_missing",
        "runner_group_api_failed",
        "runner_group_missing",
        "runner_group_response_parse_failed",
        "runner_api_failed",
        "runner_response_parse_failed",
        "tempfile_creation_failed",
        "no_idle_runner",
        "workflow_nano_idle",
        "runner-groups?per_page=100",
        "em-ci-nano",
        "runner_group_id",
        "--paginate",
        "--slurp",
        "emit \"github\" \"fork_pr\" \"false\" \"true\"",
        "emit \"github\" \"bot_pr_github_hosted\" \"false\" \"true\"",
        "emit \"github\" \"runner_token_missing\" \"true\" \"true\"",
        "cargo xtask check-agent-capabilities",
        // The exact pin is enforced repo-wide by
        // `checkout_pins_share_one_full_commit_sha` below; naming the SHA here
        // broke this contract on every upstream pin bump (issue #11695).
        "uses: actions/checkout@",
        "uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772",
    ] {
        ensure!(content.contains(required), "workflow contract missing `{required}`");
    }
    for (guard, emit) in [
        (
            "if ! nano_group_id=",
            "emit \"github\" \"runner_group_response_parse_failed\" \"true\" \"true\"",
        ),
        (
            "if ! idle_runner_count_value=",
            "emit \"github\" \"runner_response_parse_failed\" \"true\" \"true\"",
        ),
        ("if ! group_response=", "emit \"github\" \"tempfile_creation_failed\" \"true\" \"true\""),
        ("if ! response=", "emit \"github\" \"tempfile_creation_failed\" \"true\" \"true\""),
    ] {
        ensure!(
            route_branch_contains_emit(router_script, guard, emit),
            "router guard `{guard}` must emit its hosted fallback before exiting"
        );
    }
    ensure!(
        content.matches("cargo xtask check-agent-capabilities").count() == 2,
        "both execution paths must run the capability checker"
    );
    ensure!(
        content.matches("fallback_allowed").count() >= 6,
        "router outputs and summaries must preserve fallback evidence"
    );

    Ok(())
}

/// Every `actions/checkout` use under `.github` must share one pinned ref and
/// that ref must be a full 40-hex commit SHA.
///
/// A hardcoded SHA here broke on every upstream pin bump while partial sweeps
/// (some files bumped, some not) stayed invisible to every required check
/// (issue #11695). Asserting the invariant instead of the literal keeps the
/// contract true across bumps: a bump updates all uses together, and any
/// mutable tag pin or missed file fails loudly.
#[test]
fn checkout_pins_share_one_full_commit_sha() -> Result<()> {
    let root = repo_root()?;
    let refs = collect_checkout_refs(&root.join(".github"))?;
    validate_checkout_refs(&refs)
}

fn collect_checkout_refs(dir: &Path) -> Result<BTreeSet<String>> {
    let mut refs = BTreeSet::new();
    for relative_dir in ["workflows", "actions"] {
        let scan_dir = dir.join(relative_dir);
        ensure!(
            scan_dir.is_dir(),
            "checkout pin authority directory is missing: {}",
            scan_dir.display()
        );
        collect_checkout_refs_from_dir(&scan_dir, &mut refs)?;
    }
    Ok(refs)
}

fn collect_checkout_refs_from_dir(dir: &Path, refs: &mut BTreeSet<String>) -> Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("reading workflow directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_checkout_refs_from_dir(&path, refs)?;
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "yml" && ext != "yaml") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("reading workflow file {}", path.display()))?;
        refs.extend(checkout_refs_in_yaml(&content, &path)?);
    }
    Ok(())
}

fn checkout_refs_in_yaml(content: &str, path: &Path) -> Result<BTreeSet<String>> {
    let document: Value = serde_yaml_ng::from_str(content)
        .with_context(|| format!("parsing workflow file {}", path.display()))?;
    let mut refs = BTreeSet::new();
    collect_checkout_refs_from_value(&document, path, &mut refs)?;
    Ok(refs)
}

fn collect_checkout_refs_from_value(
    value: &Value,
    path: &Path,
    refs: &mut BTreeSet<String>,
) -> Result<()> {
    match value {
        Value::Mapping(mapping) => {
            for (key, value) in mapping {
                if key.as_str() == Some("uses") {
                    let uses = scalar_string(value).with_context(|| {
                        format!("`uses` in {} must be a YAML string scalar", path.display())
                    })?;
                    if let Some(reference) = uses.strip_prefix("actions/checkout@") {
                        ensure!(
                            !reference.is_empty(),
                            "`actions/checkout@` in {} has no pinned reference",
                            path.display()
                        );
                        refs.insert(reference.to_owned());
                    }
                }
                collect_checkout_refs_from_value(value, path, refs)?;
            }
        }
        Value::Sequence(sequence) => {
            for value in sequence {
                collect_checkout_refs_from_value(value, path, refs)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_checkout_refs(refs: &BTreeSet<String>) -> Result<()> {
    ensure!(
        !refs.is_empty(),
        "no `actions/checkout@<ref>` found under .github; the pin contract needs a reference"
    );
    ensure!(refs.len() == 1, "actions/checkout pins must share exactly one SHA, found {refs:?}");

    let pinned =
        refs.iter().next().ok_or_else(|| anyhow!("pin set was empty after uniqueness check"))?;
    ensure!(
        pinned.len() == 40 && pinned.chars().all(|c| c.is_ascii_hexdigit()),
        "actions/checkout pin `{pinned}` must be a full 40-character commit SHA, \
         not a mutable tag or branch"
    );

    Ok(())
}

#[test]
fn checkout_ref_scan_ignores_comments_but_rejects_stale_executable_use() -> Result<()> {
    const CURRENT: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";
    const STALE: &str = "9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0";
    let path = Path::new("checkout-fixture.yml");

    let comment_only = format!(
        "jobs:\n  build:\n    steps:\n      # stale prose: actions/checkout@{STALE}\n      - uses: actions/checkout@{CURRENT}\n"
    );
    let refs = checkout_refs_in_yaml(&comment_only, path)?;
    assert_eq!(refs, BTreeSet::from([CURRENT.to_owned()]));
    validate_checkout_refs(&refs)?;

    let executable_stale = format!(
        "jobs:\n  build:\n    steps:\n      - uses: actions/checkout@{CURRENT}\n      - uses: actions/checkout@{STALE}\n"
    );
    let refs = checkout_refs_in_yaml(&executable_stale, path)?;
    assert_eq!(refs, BTreeSet::from([CURRENT.to_owned(), STALE.to_owned()]));
    assert!(
        validate_checkout_refs(&refs).is_err(),
        "a stale executable checkout use must not be ignored"
    );

    assert!(
        validate_checkout_refs(&BTreeSet::new()).is_err(),
        "an absent executable checkout use must fail closed"
    );

    let non_string = "jobs:\n  build:\n    steps:\n      - uses:\n          - actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n";
    assert!(
        checkout_refs_in_yaml(non_string, path).is_err(),
        "a non-string executable checkout use must fail closed"
    );

    let malformed = "jobs:\n  build:\n    steps:\n      - uses: actions/checkout@\n";
    assert!(
        checkout_refs_in_yaml(malformed, path).is_err(),
        "an executable checkout with no ref must fail closed"
    );

    Ok(())
}

#[test]
fn checkout_ref_scan_excludes_unrelated_github_yaml() -> Result<()> {
    const PIN: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";
    const UNRELATED_PIN: &str = "9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0";
    let temp = tempdir()?;
    let github = temp.path().join(".github");
    fs::create_dir_all(github.join("workflows"))?;
    fs::create_dir_all(github.join("actions"))?;

    let workflow = format!("jobs:\n  build:\n    steps:\n      - uses: actions/checkout@{PIN}\n");
    fs::write(github.join("workflows/fixture.yml"), workflow)?;
    let action = format!(
        "name: fixture\nruns:\n  using: composite\n  steps:\n    - uses: actions/checkout@{PIN}\n"
    );
    fs::write(github.join("actions/fixture.yml"), action)?;
    let unrelated = format!("steps:\n  - uses: actions/checkout@{UNRELATED_PIN}\n");
    fs::write(github.join("unrelated.yml"), unrelated)?;

    let refs = collect_checkout_refs(&github)?;
    assert_eq!(refs, BTreeSet::from([PIN.to_owned()]));
    validate_checkout_refs(&refs)
}

fn workflow() -> Result<(String, Value)> {
    let path = repo_root()?.join(".github/workflows/agent-capability-gate.yml");
    let content = fs::read_to_string(path)?;
    let workflow = serde_yaml_ng::from_str(&content)?;
    Ok((content, workflow))
}

fn route_script(router: &Value) -> Result<&str> {
    mapping_value(router, "steps")?
        .as_sequence()
        .ok_or_else(|| anyhow!("router steps must be a YAML sequence"))?
        .iter()
        .find(|step| {
            mapping_value(step, "id").and_then(scalar_string).is_ok_and(|id| id == "route")
        })
        .ok_or_else(|| anyhow!("router route step is missing"))
        .and_then(|step| scalar_string(mapping_value(step, "run")?))
}

fn route_branch_contains_emit(script: &str, guard: &str, emit: &str) -> bool {
    let Some(start) = script.find(guard) else {
        return false;
    };
    let branch = &script[start..];
    let Some(exit) = branch.find("exit 0") else {
        return false;
    };
    branch[..exit].contains(emit)
}

fn repo_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("xtask must live under the repository root"))
}

fn mapping_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_mapping()
        .ok_or_else(|| anyhow!("expected YAML mapping while looking for `{key}`"))?
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("missing YAML key `{key}`"))
}

fn scalar_string(value: &Value) -> Result<&str> {
    value.as_str().ok_or_else(|| anyhow!("expected YAML string scalar"))
}

fn sequence_strings(value: &Value) -> Result<Vec<&str>> {
    value
        .as_sequence()
        .ok_or_else(|| anyhow!("expected YAML sequence"))?
        .iter()
        .map(scalar_string)
        .collect()
}

fn has_need(value: &Value, expected: &str) -> bool {
    value.as_str() == Some(expected)
        || value
            .as_sequence()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
}
