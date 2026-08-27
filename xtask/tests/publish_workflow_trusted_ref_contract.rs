//! Contract tests binding credential-bearing publish workflows to trusted refs.
//!
//! `publish-extension.yml` and `docker-publish.yml` expose real publish
//! authority (Marketplace/Open VSX PATs, GHCR and Docker Hub registry
//! credentials). Their `workflow_dispatch` trigger lets the operator choose any
//! branch or tag, and every job previously checked that selected `github.sha`
//! out verbatim — so whoever could dispatch controlled the source executed next
//! to those credentials (#9595, #9597).
//!
//! The repair contract enforced here:
//!
//! 1. each workflow carries a credentialess gate job (`resolve-trusted-anchor`)
//!    that resolves the trusted subjects server-side — the repository's
//!    default-branch head and the commit behind the dispatch version's
//!    `v<version>` tag (queried through its peeled `^{}` commit, because
//!    release orchestration cuts annotated tags) — via anonymous `git
//!    ls-remote`, with no checkout and no token mounted;
//! 2. a run is approved only when the dispatch-selected subject equals one of
//!    those anchors; anything else fails closed before any credential-bearing
//!    job is scheduled;
//! 3. every job that mounts `secrets.*` depends on the gate, checks out only
//!    the approved subject (`needs.resolve-trusted-anchor.outputs.approved_sha`,
//!    with `persist-credentials: false` retained), and reaches its first secret
//!    strictly after its checkout;
//! 4. negative fixtures prove each clause fails loudly when a mutation drops
//!    the pin, removes the gate, reorders credentials ahead of provenance, or
//!    turns the gate itself into a secrets consumer.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, ensure};
use serde_yaml_ng::Value;

const GATE_JOB: &str = "resolve-trusted-anchor";
const APPROVED_REF_EXPR: &str = "needs.resolve-trusted-anchor.outputs.approved_sha";
const TRUSTED_ANCHOR_ERROR: &str = "::error::Publishing requires a trusted anchor";
const DEFAULT_BRANCH_EXPR: &str = "${{ github.event.repository.default_branch }}";

#[test]
fn publish_extension_credentials_are_bound_to_trusted_anchors() -> Result<()> {
    let (content, workflow) = read_workflow("publish-extension.yml")?;
    validate_anchor_contract(&workflow)?;

    // The exact credential surface stays locked: new secret consumers widen
    // this contract knowingly instead of inheriting the binding silently.
    assert_consumed_secrets(
        &workflow,
        &["OVSX_PAT@publish-open-vsx", "VSCE_PAT@publish-vscode-marketplace"],
    )?;

    // The VSIX builder decides what gets published; its subject must be the
    // gate's approved anchor, never the dispatch-selected head.
    let prepare = mapping_value(mapping_value(&workflow, "jobs")?, "prepare-vsix")?;
    assert_job_needs_gate("prepare-vsix", prepare)?;
    let outputs = mapping_value(prepare, "outputs")?;
    ensure!(
        scalar_string(mapping_value(outputs, "build_sha")?)?.contains(APPROVED_REF_EXPR),
        "prepare-vsix.outputs.build_sha must bind to `{APPROVED_REF_EXPR}`"
    );

    // The #9597 release-identity binding must survive this change: release
    // tags stay resolved server-side and compared to the built subject before
    // any asset upload runs.
    ensure!(
        content.contains("Verify release tag resolves to the built subject"),
        "release-tag identity binding step went missing from publish-extension.yml"
    );
    Ok(())
}

#[test]
fn docker_publish_credentials_are_bound_to_trusted_anchors() -> Result<()> {
    let (_content, workflow) = read_workflow("docker-publish.yml")?;
    validate_anchor_contract(&workflow)?;

    assert_consumed_secrets(
        &workflow,
        &[
            "DOCKER_PASSWORD@publish-dockerhub",
            "DOCKER_PASSWORD@publish-dockerhub-perl",
            "DOCKER_USERNAME@publish-dockerhub",
            "DOCKER_USERNAME@publish-dockerhub-perl",
            "GITHUB_TOKEN@build",
            "GITHUB_TOKEN@build-perl-runtime",
        ],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Structural contract shared by both workflows
// ---------------------------------------------------------------------------

/// Clauses 1–3: gate exists and stays credentialess; every `secrets.*`
/// consumer depends on the gate and touches secrets only after checking out
/// exactly the approved subject.
fn validate_anchor_contract(workflow: &Value) -> Result<()> {
    let jobs = mapping_value(workflow, "jobs")?;
    let gate = mapping_value(jobs, GATE_JOB)
        .map_err(|_| anyhow!("workflow must define the `{GATE_JOB}` gate job"))?;

    let permissions = mapping_value(gate, "permissions")?;
    ensure!(
        scalar_string(mapping_value(permissions, "contents")?)? == "none",
        "gate job must hold no repository grants (contents: none)"
    );
    let mut secret_hit = String::new();
    collect_matches(gate, "secrets.", &mut secret_hit);
    ensure!(
        secret_hit.is_empty(),
        "gate job must stay credentialess but references `{secret_hit}`"
    );

    let script = combined_run_script(gate)?;
    // `^{}` pins the annotated-tag peel: release orchestration cuts tags with
    // `git tag -a`, so an unpeeled `refs/tags/v<X>` comparison rejects every
    // normal orchestrated release instead of approving its commit.
    for required in [
        "git ls-remote",
        "refs/heads/",
        "refs/tags/v",
        "^{}",
        "SELECTED_SHA",
        "approved_sha=",
        TRUSTED_ANCHOR_ERROR,
    ] {
        ensure!(script.contains(required), "gate script missing `{required}`");
    }
    ensure!(
        !script.contains("${{"),
        "gate run block must not inline expressions; inputs arrive through env"
    );
    let gate_steps = steps_of(gate)?;
    let env = mapping_value(&gate_steps[0], "env")?;
    ensure!(
        scalar_string(mapping_value(env, "DEFAULT_BRANCH")?)? == DEFAULT_BRANCH_EXPR,
        "gate must derive the default branch from the event payload, not from github.ref"
    );

    for (name, job) in jobs_mapping_iter(workflow)? {
        let mut blob = String::new();
        // Match bare `secrets.` too: `if:` conditions consume secrets without
        // the `${{ ... }}` wrapper, so the wrapper-spelled needle would let a
        // credential consumer slip past this clause undetected.
        collect_matches(job, "secrets.", &mut blob);
        if blob.is_empty() || name == GATE_JOB {
            continue;
        }
        assert_job_needs_gate(&name, job)?;

        let steps = steps_of(job)?;
        let checkout_idx = steps.iter().position(step_is_checkout).ok_or_else(|| {
            anyhow!("credential-bearing job `{name}` has no actions/checkout step")
        })?;
        let checkout = mapping_value(&steps[checkout_idx], "with")
            .with_context(|| format!("job `{name}` checkout lacks a with block"))?;
        let with_ref = mapping_value(checkout, "ref").with_context(|| {
            format!(
                "job `{name}` checks out without pinning to `{APPROVED_REF_EXPR}`; \
                 an operator-selected ref would run beside live credentials (#9595)"
            )
        })?;
        ensure!(
            scalar_string(with_ref)?.contains(APPROVED_REF_EXPR),
            "job `{name}` must check out `{APPROVED_REF_EXPR}`, found `{with_ref:?}`"
        );
        ensure!(
            scalar_flag_text(mapping_value(checkout, "persist-credentials")?)? == "false",
            "job `{name}` must retain persist-credentials: false on its pinned checkout"
        );

        let secrets_idx = steps
            .iter()
            .position(|step| {
                let mut hit = String::new();
                collect_matches(step, "secrets.", &mut hit);
                !hit.is_empty()
            })
            .ok_or_else(|| anyhow!("job `{name}` references secrets outside any step"))?;
        ensure!(
            checkout_idx < secrets_idx,
            "job `{name}` must prove provenance by checking out the approved subject \
             before its first step touches a secret"
        );
    }
    Ok(())
}

fn assert_consumed_secrets(workflow: &Value, expected: &[&str]) -> Result<()> {
    let mut actual: Vec<String> = Vec::new();
    for (name, job) in jobs_mapping_iter(workflow)? {
        if name == GATE_JOB {
            continue;
        }
        let mut blob = String::new();
        // Same widened needle as the shared contract: bare `secrets.` in an
        // `if:` condition mounts credentials just like `${{ secrets.* }}`.
        collect_matches(job, "secrets.", &mut blob);
        for secret in ["VSCE_PAT", "OVSX_PAT", "GITHUB_TOKEN", "DOCKER_USERNAME", "DOCKER_PASSWORD"]
        {
            if blob.contains(secret) {
                actual.push(format!("{secret}@{name}"));
            }
        }
    }
    actual.sort();

    let mut expected_owned: Vec<String> = expected.iter().map(|entry| entry.to_string()).collect();
    expected_owned.sort();

    ensure!(
        actual == expected_owned,
        "credentialed surface changed: expected {expected_owned:?}, found {actual:?}"
    );
    Ok(())
}

fn assert_job_needs_gate(name: &str, job: &Value) -> Result<()> {
    let needs = needs_of(job)?;
    ensure!(
        needs.iter().any(|need| need == GATE_JOB),
        "credential-bearing job `{name}` must depend on the `{GATE_JOB}` gate so it cannot \
         be scheduled when the dispatch ref is not an approved anchor"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative falsifiers over synthetic documents
// ---------------------------------------------------------------------------

/// Happy-path document satisfying the whole shared contract.
const FIXTURE_YAML: &str = r##"
name: Fixture Publish
on:
  workflow_dispatch:
jobs:
  resolve-trusted-anchor:
    name: Resolve trusted publish anchor
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    permissions:
      contents: none
    outputs:
      approved_sha: ${{ steps.anchor.outputs.approved_sha }}
    steps:
      - name: Resolve trusted anchors server-side
        id: anchor
        env:
          REPO_URL: https://github.com/example/repo.git
          SELECTED_SHA: ${{ github.sha }}
          DEFAULT_BRANCH: ${{ github.event.repository.default_branch }}
          VERSION_INPUT_RAW: ${{ github.event.inputs.version }}
        run: |
          git ls-remote "$REPO_URL" "refs/heads/${DEFAULT_BRANCH}"
          git ls-remote "$REPO_URL" "refs/tags/v${VERSION_INPUT_RAW}" "refs/tags/v${VERSION_INPUT_RAW}^{}"
          echo '::error::Publishing requires a trusted anchor'
          printf 'approved_sha=%s\n' "$SELECTED_SHA"
  build-publish:
    name: Build and push
    runs-on: ubuntu-24.04
    needs: [resolve-trusted-anchor]
    steps:
      - name: Checkout
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
          ref: ${{ needs.resolve-trusted-anchor.outputs.approved_sha }}
      - name: Log in to Container Registry
        uses: docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0
        with:
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
"##;

fn parse_fixture(yaml: &str) -> Result<Value> {
    serde_yaml_ng::from_str(yaml).map_err(|error| anyhow!("fixture must stay valid YAML: {error}"))
}

/// Parse a mutated fixture and return the contract's rejection message; any
/// mutation that still validates is itself a failure of the falsifier.
fn rejection_of(yaml: &str) -> Result<String> {
    let document = parse_fixture(yaml)?;
    match validate_anchor_contract(&document) {
        Ok(()) => Err(anyhow!("mutated fixture must fail the anchor contract")),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn well_formed_fixture_satisfies_the_contract() -> Result<()> {
    validate_anchor_contract(&parse_fixture(FIXTURE_YAML)?)
}

#[test]
fn workflow_without_the_gate_job_fails_closed() -> Result<()> {
    // A credentialed job with no gate dependency at all.
    let gateless = r##"
name: Gateless Publish
on:
  workflow_dispatch:
jobs:
  build-publish:
    name: Build and push
    runs-on: ubuntu-24.04
    steps:
      - name: Checkout
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: false
      - name: Log in
        uses: docker/login-action@dbcb813823bdd20940b903addbd779551569679f
        with:
          password: ${{ secrets.GITHUB_TOKEN }}
"##;
    let message = rejection_of(gateless)?;
    ensure!(message.contains("`resolve-trusted-anchor`"), "unexpected rejection: {message}");
    Ok(())
}

#[test]
fn unpinned_credentialed_checkout_fails_closed() -> Result<()> {
    let pin_line = "          ref: ${{ needs.resolve-trusted-anchor.outputs.approved_sha }}\n";
    assert!(FIXTURE_YAML.contains(pin_line), "fixture drifted: pin line missing");
    let message = rejection_of(&FIXTURE_YAML.replace(pin_line, ""))?;
    ensure!(message.contains("checks out without pinning"), "unexpected rejection: {message}");
    Ok(())
}

#[test]
fn credential_step_ahead_of_pinned_checkout_fails_closed() -> Result<()> {
    // Swap the step order wholesale so the pin stays intact and only the
    // provenance-before-secret sequencing regresses.
    let checkout_at =
        FIXTURE_YAML.find("      - name: Checkout").ok_or_else(|| anyhow!("fixture drifted"))?;
    let login_at =
        FIXTURE_YAML.find("      - name: Log in").ok_or_else(|| anyhow!("fixture drifted"))?;
    assert!(checkout_at < login_at, "fixture drifted: step order changed");
    let (head, rest) = FIXTURE_YAML.split_at(checkout_at);
    let (checkout_step, secret_steps) = rest.split_at(login_at - checkout_at);
    let message = rejection_of(&format!("{head}{secret_steps}{checkout_step}"))?;
    ensure!(
        message.contains("before its first step touches a secret"),
        "unexpected rejection: {message}"
    );
    Ok(())
}

#[test]
fn wrapperless_secret_use_before_checkout_fails_closed() -> Result<()> {
    // `if:` conditions consume secrets without the `${{ }}` wrapper. Under the
    // historical wrapper-spelled needle this swap stayed invisible, so the
    // detector itself must match the bare spelling.
    let checkout_at =
        FIXTURE_YAML.find("      - name: Checkout").ok_or_else(|| anyhow!("fixture drifted"))?;
    let login_at =
        FIXTURE_YAML.find("      - name: Log in").ok_or_else(|| anyhow!("fixture drifted"))?;
    assert!(checkout_at < login_at, "fixture drifted: step order changed");
    let (head, rest) = FIXTURE_YAML.split_at(checkout_at);
    let (checkout_step, secret_steps) = rest.split_at(login_at - checkout_at);
    let swapped_without_wrapper = format!("{head}{secret_steps}{checkout_step}")
        .replace("${{ secrets.GITHUB_TOKEN }}", "secrets.GITHUB_TOKEN");
    ensure!(
        !swapped_without_wrapper.contains("${{ secrets."),
        "fixture drifted: wrapper strip failed"
    );
    let message = rejection_of(&swapped_without_wrapper)?;
    ensure!(
        message.contains("before its first step touches a secret"),
        "unexpected rejection: {message}"
    );
    Ok(())
}

#[test]
fn unpeeled_annotated_tag_comparison_fails_closed() -> Result<()> {
    // Dropping only the peeled `^{}` query reintroduces the regression where
    // every orchestrated release (cut with `git tag -a`) is rejected.
    let dual_query = "\"refs/tags/v${VERSION_INPUT_RAW}\" \"refs/tags/v${VERSION_INPUT_RAW}^{}\"\n";
    assert!(FIXTURE_YAML.contains(dual_query), "fixture drifted: dual tag query missing");
    let mutated = FIXTURE_YAML.replace(dual_query, "\"refs/tags/v${VERSION_INPUT_RAW}\"\n");
    let message = rejection_of(&mutated)?;
    ensure!(message.contains("gate script missing `^{}`"), "unexpected rejection: {message}");
    Ok(())
}

#[test]
fn gate_consuming_a_secret_stays_rejected() -> Result<()> {
    let mutated = FIXTURE_YAML.replacen(
        "          SELECTED_SHA: ${{ github.sha }}",
        "          SELECTED_SHA: ${{ secrets.GITHUB_TOKEN }}",
        1,
    );
    let message = rejection_of(&mutated)?;
    ensure!(message.contains("credentialess"), "unexpected rejection: {message}");
    Ok(())
}

#[test]
fn credentialed_job_without_gate_dependency_fails_closed() -> Result<()> {
    let mutated = FIXTURE_YAML.replace("    needs: [resolve-trusted-anchor]\n", "");
    let message = rejection_of(&mutated)?;
    ensure!(
        message.contains("must depend on the `resolve-trusted-anchor` gate"),
        "unexpected rejection: {message}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared YAML traversal helpers
// ---------------------------------------------------------------------------

fn read_workflow(name: &str) -> Result<(String, Value)> {
    let path = repo_root()?.join(".github/workflows").join(name);
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let workflow: Value =
        serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    Ok((content, workflow))
}

fn repo_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("xtask must live under the repository root"))
}

fn jobs_mapping_iter(workflow: &Value) -> Result<Vec<(String, &Value)>> {
    let jobs = mapping_value(workflow, "jobs")?;
    let map = jobs.as_mapping().ok_or_else(|| anyhow!("`jobs` must be a YAML mapping"))?;
    Ok(map
        .into_iter()
        .filter_map(|(key, value)| key.as_str().map(|key| (key.to_string(), value)))
        .collect())
}

fn steps_of(job: &Value) -> Result<&Vec<Value>> {
    mapping_value(job, "steps")?
        .as_sequence()
        .ok_or_else(|| anyhow!("job `steps` must be a YAML sequence"))
}

fn step_is_checkout(step: &Value) -> bool {
    mapping_value(step, "uses")
        .and_then(scalar_string)
        .is_ok_and(|uses| uses.starts_with("actions/checkout@"))
}

fn combined_run_script(job: &Value) -> Result<String> {
    let mut script = String::new();
    for step in steps_of(job)? {
        if let Ok(run) = mapping_value(step, "run").and_then(scalar_string) {
            // Comment-only lines are dropped: shape clauses describe executed
            // commands, and prose may legitimately spell the same tokens.
            script.extend(
                run.lines()
                    .filter(|line| !line.trim_start().starts_with('#'))
                    .map(|line| format!("{line}\n")),
            );
        }
    }
    Ok(script)
}

fn needs_of(job: &Value) -> Result<Vec<String>> {
    let Some(needs) = job.get("needs") else {
        // A credential-bearing job with no dependencies at all is exactly the
        // mutation the gate-dependency clause must reject.
        return Ok(Vec::new());
    };
    if let Ok(single) = scalar_string(needs) {
        return Ok(vec![single.to_string()]);
    }
    needs
        .as_sequence()
        .ok_or_else(|| anyhow!("`needs` must be a scalar or a sequence"))?
        .iter()
        .map(|value| scalar_string(value).map(str::to_string))
        .collect()
}

fn collect_matches(value: &Value, needle: &str, out: &mut String) {
    match value {
        Value::Mapping(mapping) => {
            for (key, child) in mapping {
                collect_matches(key, needle, out);
                collect_matches(child, needle, out);
            }
        }
        Value::Sequence(sequence) => {
            for child in sequence {
                collect_matches(child, needle, out);
            }
        }
        Value::String(text) if text.contains(needle) => {
            out.push_str(text);
            out.push('\n');
        }
        _ => {}
    }
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

/// YAML booleans (`persist-credentials: false`) parse as `Value::Bool`, so a
/// flag-style comparison must treat bools and their string spellings alike.
fn scalar_flag_text(value: &Value) -> Result<String> {
    match value {
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::String(text) => Ok(text.clone()),
        _ => Err(anyhow!("expected YAML boolean or string scalar")),
    }
}
