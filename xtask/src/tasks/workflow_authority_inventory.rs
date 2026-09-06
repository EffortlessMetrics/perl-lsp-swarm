//! Credential derivation inventory for GitHub workflow YAML.
//!
//! First slice of #14867 and the credential column of #9448. This surface is
//! diagnostic: it classifies credential shapes already present in
//! `.github/workflows/*.yml` and does not change workflow behavior.
//!
//! Schema-aware v2: new derivation kinds are added alongside v1 kinds. An
//! env-injected `${{ github.token }}` keeps `GithubToken` and also records
//! `EnvInjectedToken`. OIDC `id-token: write` is a new kind, not a re-label of
//! `secrets.*`.
//!
//! This slice recognizes explicit mapping `id-token: write` at workflow or job
//! scope. GitHub's `permissions: write-all` shorthand is residual, not an OIDC
//! derivation here.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::Serialize;
use serde_yaml_ng::{Mapping, Value};

use crate::utils::project_root;

/// Schema id for this credential-column slice. Generation 2 adds kinds;
/// generation 1 kinds remain independently recorded.
pub const SCHEMA: &str = "workflow_authority_credential.v2";

const WORKFLOWS_DIR: &str = ".github/workflows";

#[derive(Serialize)]
struct CredentialInventoryReport<'a> {
    schema: &'static str,
    workflow_count: usize,
    rows: &'a [WorkflowCredentialRow],
}

/// How a credential fact was derived from workflow YAML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialDerivationKind {
    /// v1: `${{ secrets.* }}` expressions.
    SecretExpression,
    /// v1: `${{ github.token }}` (not `secrets.GITHUB_TOKEN`).
    GithubToken,
    /// v1: selected token-backed action inputs (`token`, `github-token`, `github_token`).
    TokenBackedMutation,
    /// v2: `env:` bindings whose values are credential expressions.
    EnvInjectedToken,
    /// v2: `permissions: id-token: write` (OIDC token minting).
    OidcIdTokenWrite,
}

impl CredentialDerivationKind {
    /// Schema generation that introduced this kind.
    pub const fn generation(self) -> u8 {
        match self {
            Self::SecretExpression | Self::GithubToken | Self::TokenBackedMutation => 1,
            Self::EnvInjectedToken | Self::OidcIdTokenWrite => 2,
        }
    }
}

/// One recognized credential derivation at a YAML path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct CredentialDerivation {
    pub kind: CredentialDerivationKind,
    pub generation: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    pub path: String,
    pub evidence: String,
}

/// Per-workflow credential column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkflowCredentialRow {
    pub workflow: String,
    pub derivations: Vec<CredentialDerivation>,
}

#[derive(Clone, Copy)]
struct ScanCtx<'a> {
    job: Option<&'a str>,
    env_key: Option<&'a str>,
    with_key: Option<&'a str>,
}

impl<'a> ScanCtx<'a> {
    fn root() -> Self {
        Self { job: None, env_key: None, with_key: None }
    }

    fn with_job(self, job: &'a str) -> Self {
        Self { job: Some(job), ..self }
    }

    fn with_env_key(self, env_key: &'a str) -> Self {
        Self { env_key: Some(env_key), ..self }
    }

    fn with_with_key(self, with_key: &'a str) -> Self {
        Self { with_key: Some(with_key), ..self }
    }
}

/// Emit the credential column for `.github/workflows/*.yml`.
///
/// Advisory. This does not lint, fail closed, or modify workflow files.
pub fn run(receipt: Option<PathBuf>) -> Result<()> {
    let rows = inventory_repo_workflows()?;
    let report =
        CredentialInventoryReport { schema: SCHEMA, workflow_count: rows.len(), rows: &rows };
    let json = serde_json::to_string_pretty(&report)
        .with_context(|| "serialize workflow credential inventory")?;
    match receipt {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
            }
            fs::write(&path, &json).with_context(|| format!("writing {}", path.display()))?;
            println!(
                "workflow authority credential inventory: {} workflows -> {}",
                report.workflow_count,
                path.display()
            );
        }
        None => {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(json.as_bytes())
                .and_then(|()| stdout.write_all(b"\n"))
                .with_context(|| "write workflow credential inventory to stdout")?;
        }
    }
    Ok(())
}

/// Classify every `.github/workflows/*.{yml,yaml}` file under the repo root.
pub fn inventory_repo_workflows() -> Result<Vec<WorkflowCredentialRow>> {
    let root = project_root()?;
    inventory_workflows_dir(&root.join(WORKFLOWS_DIR))
}

/// Classify workflow files in `dir` (used by tests with fixtures).
pub fn inventory_workflows_dir(dir: &Path) -> Result<Vec<WorkflowCredentialRow>> {
    if !dir.is_dir() {
        bail!("workflow directory {} does not exist", dir.display());
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry.with_context(|| format!("reading entry in {}", dir.display()))?.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        paths.push(path);
    }
    paths.sort();
    let mut rows = Vec::with_capacity(paths.len());
    for path in paths {
        rows.push(classify_workflow_file(&path)?);
    }
    Ok(rows)
}

/// Classify one workflow file.
pub fn classify_workflow_file(path: &Path) -> Result<WorkflowCredentialRow> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading workflow {}", path.display()))?;
    let workflow: Value = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing workflow YAML {}", path.display()))?;
    let workflow_name =
        path.file_name().and_then(|name| name.to_str()).unwrap_or("unknown.yml").to_string();
    Ok(WorkflowCredentialRow { workflow: workflow_name, derivations: classify_workflow(&workflow) })
}

/// Classify an already-parsed workflow document.
pub fn classify_workflow(workflow: &Value) -> Vec<CredentialDerivation> {
    let mut found = BTreeSet::new();
    scan_document(workflow, &mut found);
    found.into_iter().collect()
}

fn scan_document(workflow: &Value, out: &mut BTreeSet<CredentialDerivation>) {
    let Some(root) = workflow.as_mapping() else {
        return;
    };
    if let Some(permissions) = mapping_get(root, "permissions") {
        scan_permissions(permissions, ScanCtx::root(), "permissions", out);
    }
    if let Some(env) = mapping_get(root, "env") {
        scan_env(env, ScanCtx::root(), "env", out);
    }
    let Some(jobs) = mapping_get(root, "jobs").and_then(Value::as_mapping) else {
        return;
    };
    for (job_key, job_value) in jobs {
        let Some(job_name) = yaml_key(job_key) else {
            continue;
        };
        let Some(job) = job_value.as_mapping() else {
            continue;
        };
        let ctx = ScanCtx::root().with_job(job_name);
        let job_path = format!("jobs.{job_name}");
        if let Some(permissions) = mapping_get(job, "permissions") {
            scan_permissions(permissions, ctx, &format!("{job_path}.permissions"), out);
        }
        if let Some(env) = mapping_get(job, "env") {
            scan_env(env, ctx, &format!("{job_path}.env"), out);
        }
        if let Some(steps) = mapping_get(job, "steps").and_then(Value::as_sequence) {
            for (index, step) in steps.iter().enumerate() {
                scan_step(step, ctx, &format!("{job_path}.steps[{index}]"), out);
            }
        }
        // Remaining job fields (run scripts, names, `if:`) still carry v1 expressions.
        scan_value(job_value, ctx, &job_path, out);
    }
}

fn scan_step(step: &Value, ctx: ScanCtx<'_>, path: &str, out: &mut BTreeSet<CredentialDerivation>) {
    let Some(step_map) = step.as_mapping() else {
        scan_value(step, ctx, path, out);
        return;
    };
    if let Some(env) = mapping_get(step_map, "env") {
        scan_env(env, ctx, &format!("{path}.env"), out);
    }
    if let Some(with) = mapping_get(step_map, "with") {
        scan_with(with, ctx, &format!("{path}.with"), out);
    }
    scan_value(step, ctx, path, out);
}

fn scan_permissions(
    permissions: &Value,
    ctx: ScanCtx<'_>,
    path: &str,
    out: &mut BTreeSet<CredentialDerivation>,
) {
    let Some(map) = permissions.as_mapping() else {
        return;
    };
    for (key, value) in map {
        let Some(scope) = yaml_key(key) else {
            continue;
        };
        if scope == "id-token" && is_write_scope(value) {
            push(
                out,
                CredentialDerivationKind::OidcIdTokenWrite,
                ctx.job,
                format!("{path}.{scope}"),
                "id-token: write",
            );
        }
    }
}

fn scan_env(env: &Value, ctx: ScanCtx<'_>, path: &str, out: &mut BTreeSet<CredentialDerivation>) {
    let Some(map) = env.as_mapping() else {
        scan_value(env, ctx, path, out);
        return;
    };
    for (key, value) in map {
        let Some(env_key) = yaml_key(key) else {
            continue;
        };
        let entry_path = format!("{path}.{env_key}");
        scan_value(value, ctx.with_env_key(env_key), &entry_path, out);
    }
}

fn scan_with(with: &Value, ctx: ScanCtx<'_>, path: &str, out: &mut BTreeSet<CredentialDerivation>) {
    let Some(map) = with.as_mapping() else {
        scan_value(with, ctx, path, out);
        return;
    };
    for (key, value) in map {
        let Some(input_key) = yaml_key(key) else {
            continue;
        };
        let entry_path = format!("{path}.{input_key}");
        scan_value(value, ctx.with_with_key(input_key), &entry_path, out);
    }
}

fn scan_value(
    value: &Value,
    ctx: ScanCtx<'_>,
    path: &str,
    out: &mut BTreeSet<CredentialDerivation>,
) {
    match value {
        Value::String(text) => record_expressions(text, ctx, path, out),
        Value::Mapping(map) => {
            for (key, nested) in map {
                let child = match yaml_key(key) {
                    Some(name) => format!("{path}.{name}"),
                    None => format!("{path}[key]"),
                };
                scan_value(nested, ctx, &child, out);
            }
        }
        Value::Sequence(items) => {
            for (index, nested) in items.iter().enumerate() {
                scan_value(nested, ctx, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

fn record_expressions(
    text: &str,
    ctx: ScanCtx<'_>,
    path: &str,
    out: &mut BTreeSet<CredentialDerivation>,
) {
    let mut saw_secret = false;
    for secret_name in secret_names(text) {
        saw_secret = true;
        push(
            out,
            CredentialDerivationKind::SecretExpression,
            ctx.job,
            path.to_string(),
            format!("secrets.{secret_name}"),
        );
    }
    let saw_github_token = contains_github_token(text);
    if saw_github_token {
        push(out, CredentialDerivationKind::GithubToken, ctx.job, path.to_string(), "github.token");
    }
    let credential_expr = saw_secret || saw_github_token;
    if credential_expr {
        if let Some(env_key) = ctx.env_key {
            push(
                out,
                CredentialDerivationKind::EnvInjectedToken,
                ctx.job,
                path.to_string(),
                env_key,
            );
        }
        if let Some(with_key) = ctx.with_key.filter(|key| is_token_backed_input(key)) {
            push(
                out,
                CredentialDerivationKind::TokenBackedMutation,
                ctx.job,
                path.to_string(),
                with_key,
            );
        }
    }
}

fn push(
    out: &mut BTreeSet<CredentialDerivation>,
    kind: CredentialDerivationKind,
    job: Option<&str>,
    path: String,
    evidence: impl Into<String>,
) {
    out.insert(CredentialDerivation {
        generation: kind.generation(),
        kind,
        job: job.map(str::to_string),
        path,
        evidence: evidence.into(),
    });
}

fn mapping_get<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

fn yaml_key(key: &Value) -> Option<&str> {
    key.as_str()
}

fn is_write_scope(value: &Value) -> bool {
    value.as_str().is_some_and(|scope| scope == "write")
}

fn is_token_backed_input(key: &str) -> bool {
    matches!(key.to_ascii_lowercase().as_str(), "token" | "github-token" | "github_token")
}

fn contains_github_token(text: &str) -> bool {
    // Direct default-token expression only. `secrets.GITHUB_TOKEN` stays a secret.
    let mut start = 0;
    while let Some(rel) = text[start..].find("${{") {
        let expr_at = start + rel;
        let Some(end_rel) = text[expr_at + 3..].find("}}") else {
            break;
        };
        let inner = text[expr_at + 3..expr_at + 3 + end_rel].trim();
        if inner == "github.token" {
            return true;
        }
        start = expr_at + 3 + end_rel + 2;
        if start >= text.len() {
            break;
        }
    }
    false
}

fn secret_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut start = 0;
    while let Some(rel) = text[start..].find("${{") {
        let expr_at = start + rel;
        let Some(end_rel) = text[expr_at + 3..].find("}}") else {
            break;
        };
        let inner = text[expr_at + 3..expr_at + 3 + end_rel].trim();
        if let Some(name) = inner.strip_prefix("secrets.") {
            let ident: String =
                name.chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect();
            if !ident.is_empty() {
                names.push(ident);
            }
        }
        start = expr_at + 3 + end_rel + 2;
        if start >= text.len() {
            break;
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_JOB: &str = r"
jobs:
  example:
    runs-on: ubuntu-latest
    steps:
      - run: echo ok
";

    fn with_job_body(body: &str) -> String {
        format!(
            r"
jobs:
  example:
    runs-on: ubuntu-latest
{body}
"
        )
    }

    #[test]
    fn schema_is_v2() {
        assert_eq!(SCHEMA, "workflow_authority_credential.v2");
        assert_eq!(CredentialDerivationKind::EnvInjectedToken.generation(), 2);
        assert_eq!(CredentialDerivationKind::OidcIdTokenWrite.generation(), 2);
        assert_eq!(CredentialDerivationKind::GithubToken.generation(), 1);
        assert_eq!(CredentialDerivationKind::EnvInjectedToken.generation(), 2);
    }

    #[test]
    fn env_injected_github_token_keeps_v1_kind_and_adds_v2() {
        let yaml = with_job_body(
            r"
    steps:
      - name: Call API
        env:
          GH_TOKEN: ${{ github.token }}
        run: gh api user
",
        );
        let kinds = kinds_of(&yaml);
        assert!(
            kinds.contains(&CredentialDerivationKind::GithubToken),
            "v2 must not silently reclassify github.token away: {kinds:?}"
        );
        assert!(
            kinds.contains(&CredentialDerivationKind::EnvInjectedToken),
            "env-injected github.token must add the v2 kind: {kinds:?}"
        );
        assert!(
            !kinds.contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "env injection is not OIDC: {kinds:?}"
        );
        let env = must_kind(&derivations_of(&yaml), CredentialDerivationKind::EnvInjectedToken);
        assert_eq!(env.generation, 2);
        assert_eq!(env.job.as_deref(), Some("example"));
        assert!(env.path.contains("env.GH_TOKEN"), "{}", env.path);
    }

    #[test]
    fn env_injected_secret_keeps_secret_expression_and_adds_v2() {
        let yaml = with_job_body(
            r"
    steps:
      - env:
          FOO_TOKEN: ${{ secrets.FOO_TOKEN }}
        run: echo using-token
",
        );
        let kinds = kinds_of(&yaml);
        assert!(kinds.contains(&CredentialDerivationKind::SecretExpression));
        assert!(kinds.contains(&CredentialDerivationKind::EnvInjectedToken));
        assert!(!kinds.contains(&CredentialDerivationKind::GithubToken));
        let secret = must_kind(&derivations_of(&yaml), CredentialDerivationKind::SecretExpression);
        assert_eq!(secret.generation, 1);
        assert_eq!(secret.evidence, "secrets.FOO_TOKEN");
    }

    #[test]
    fn workflow_level_env_secret_is_env_injected() {
        let yaml = r"
env:
  MINIMAX_API_KEY: ${{ secrets.MINIMAX_API_KEY }}
jobs:
  droid:
    runs-on: ubuntu-latest
    steps:
      - run: echo ${MINIMAX_API_KEY}
";
        let kinds = kinds_of(yaml);
        assert!(kinds.contains(&CredentialDerivationKind::SecretExpression));
        assert!(kinds.contains(&CredentialDerivationKind::EnvInjectedToken));
        let env = must_kind(&derivations_of(yaml), CredentialDerivationKind::EnvInjectedToken);
        assert!(env.job.is_none(), "workflow-level env has no job: {:?}", env.job);
        assert_eq!(env.path, "env.MINIMAX_API_KEY");
    }

    #[test]
    fn oidc_id_token_write_is_a_new_kind_not_a_secret() {
        let yaml = with_job_body(
            r"
    permissions:
      contents: read
      id-token: write
    steps:
      - run: echo federated
",
        );
        let kinds = kinds_of(&yaml);
        assert!(
            kinds.contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "id-token: write must be recognized: {kinds:?}"
        );
        assert!(
            !kinds.contains(&CredentialDerivationKind::SecretExpression),
            "OIDC write must not be reclassified as secrets.*: {kinds:?}"
        );
        assert!(
            !kinds.contains(&CredentialDerivationKind::GithubToken),
            "OIDC write is not github.token: {kinds:?}"
        );
        let oidc = must_kind(&derivations_of(&yaml), CredentialDerivationKind::OidcIdTokenWrite);
        assert_eq!(oidc.generation, 2);
        assert!(oidc.path.ends_with("permissions.id-token"), "{}", oidc.path);
    }

    #[test]
    fn workflow_level_oidc_id_token_write_is_recognized() {
        let yaml = r"
permissions:
  contents: read
  id-token: write
jobs:
  example:
    runs-on: ubuntu-latest
    steps:
      - run: echo federated
";
        let oidc = must_kind(&derivations_of(yaml), CredentialDerivationKind::OidcIdTokenWrite);
        assert_eq!(oidc.generation, 2);
        assert!(oidc.job.is_none(), "workflow-level OIDC has no job: {:?}", oidc.job);
        assert_eq!(oidc.path, "permissions.id-token");
        assert!(!kinds_of(yaml).contains(&CredentialDerivationKind::SecretExpression));
    }

    #[test]
    fn write_all_permissions_are_not_oidc_in_this_slice() {
        let yaml = r"
permissions: write-all
jobs:
  example:
    runs-on: ubuntu-latest
    steps:
      - run: echo residual
";
        assert!(
            !kinds_of(yaml).contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "permissions: write-all is residual, not a silent OIDC kind"
        );
    }

    #[test]
    fn oidc_id_token_read_is_not_write() {
        let yaml = with_job_body(
            r"
    permissions:
      id-token: read
    steps:
      - run: echo no mint
",
        );
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "id-token: read must not mint the v2 OIDC-write kind"
        );
    }

    #[test]
    fn contents_write_is_not_oidc() {
        let yaml = with_job_body(
            r"
    permissions:
      contents: write
    steps:
      - run: echo no oidc
",
        );
        assert!(kinds_of(&yaml).is_empty());
    }

    #[test]
    fn id_token_write_in_a_run_script_is_not_permissions() {
        let yaml = with_job_body(
            r#"
    steps:
      - run: 'echo "permissions: id-token: write"'
"#,
        );
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "string mention of id-token: write must not count as OIDC"
        );
    }

    #[test]
    fn action_token_input_is_v1_mutation_not_env_injection() {
        let yaml = with_job_body(
            r"
    steps:
      - uses: actions/download-artifact@v4
        with:
          github-token: ${{ github.token }}
",
        );
        let kinds = kinds_of(&yaml);
        assert!(kinds.contains(&CredentialDerivationKind::GithubToken));
        assert!(kinds.contains(&CredentialDerivationKind::TokenBackedMutation));
        assert!(
            !kinds.contains(&CredentialDerivationKind::EnvInjectedToken),
            "action with: token is not env injection: {kinds:?}"
        );
        let mutation =
            must_kind(&derivations_of(&yaml), CredentialDerivationKind::TokenBackedMutation);
        assert_eq!(mutation.generation, 1);
        assert!(mutation.path.contains("with.github-token"), "{}", mutation.path);
    }

    #[test]
    fn secret_in_run_script_is_v1_not_env_injected() {
        let yaml = with_job_body(
            r"
    steps:
      - run: echo ${{ secrets.FOO }}
",
        );
        let kinds = kinds_of(&yaml);
        assert!(kinds.contains(&CredentialDerivationKind::SecretExpression));
        assert!(!kinds.contains(&CredentialDerivationKind::EnvInjectedToken));
        assert!(!kinds.contains(&CredentialDerivationKind::TokenBackedMutation));
    }

    #[test]
    fn literal_token_shaped_env_without_expression_is_not_a_credential() {
        let yaml = with_job_body(
            r"
    steps:
      - env:
          FOO_TOKEN: dummy
        run: echo dummy
",
        );
        assert!(
            kinds_of(&yaml).is_empty(),
            "token-shaped env keys without a credential expression are residual"
        );
    }

    #[test]
    fn secrets_github_token_is_not_the_default_github_token_kind() {
        let yaml = with_job_body(
            r"
    steps:
      - env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: gh api user
",
        );
        let kinds = kinds_of(&yaml);
        assert!(kinds.contains(&CredentialDerivationKind::SecretExpression));
        assert!(kinds.contains(&CredentialDerivationKind::EnvInjectedToken));
        assert!(
            !kinds.contains(&CredentialDerivationKind::GithubToken),
            "secrets.GITHUB_TOKEN is a named secret, not github.token: {kinds:?}"
        );
    }

    #[test]
    fn credential_free_workflow_stays_empty() {
        assert!(kinds_of(MINIMAL_JOB).is_empty());
    }

    #[test]
    fn live_docs_deploy_records_oidc_write_and_env_injected_token() -> Result<()> {
        let row = live_row("docs-deploy.yml")?;
        let kinds = live_kinds(&row);
        assert!(
            kinds.contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "docs-deploy.yml deploy job grants id-token: write: {kinds:?}"
        );
        assert!(
            kinds.contains(&CredentialDerivationKind::EnvInjectedToken),
            "docs-deploy.yml injects GH_TOKEN: {kinds:?}"
        );
        assert!(
            kinds.contains(&CredentialDerivationKind::GithubToken),
            "docs-deploy.yml GH_TOKEN is github.token and must keep the v1 kind: {kinds:?}"
        );
        Ok(())
    }

    #[test]
    fn live_droid_records_oidc_write() -> Result<()> {
        let row = live_row("droid.yml")?;
        let kinds = live_kinds(&row);
        assert!(
            kinds.contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "droid.yml grants id-token: write: {kinds:?}"
        );
        assert!(
            kinds.contains(&CredentialDerivationKind::EnvInjectedToken),
            "droid.yml workflow env injects MINIMAX_API_KEY: {kinds:?}"
        );
        Ok(())
    }

    #[test]
    fn live_pr_candidate_set_records_env_injected_github_token() -> Result<()> {
        let row = live_row("pr-candidate-set.yml")?;
        let kinds = live_kinds(&row);
        assert!(kinds.contains(&CredentialDerivationKind::GithubToken));
        assert!(kinds.contains(&CredentialDerivationKind::EnvInjectedToken));
        assert!(
            !kinds.contains(&CredentialDerivationKind::OidcIdTokenWrite),
            "pr-candidate-set.yml has no OIDC write: {kinds:?}"
        );
        Ok(())
    }

    #[test]
    fn live_post_publish_smoke_records_token_backed_mutation_not_as_env() -> Result<()> {
        let row = live_row("post-publish-smoke.yml")?;
        let kinds = live_kinds(&row);
        assert!(kinds.contains(&CredentialDerivationKind::GithubToken));
        assert!(kinds.contains(&CredentialDerivationKind::TokenBackedMutation));
        let mutation_paths: Vec<&str> = row
            .derivations
            .iter()
            .filter(|row| row.kind == CredentialDerivationKind::TokenBackedMutation)
            .map(|row| row.path.as_str())
            .collect();
        assert!(
            mutation_paths.iter().any(|path| path.contains("github-token")),
            "expected github-token action input: {mutation_paths:?}"
        );
        Ok(())
    }

    #[test]
    fn live_workflow_trigger_lint_is_credential_free() -> Result<()> {
        let row = live_row("workflow-trigger-lint.yml")?;
        assert!(
            row.derivations.is_empty(),
            "workflow-trigger-lint.yml should stay a negative live control: {:?}",
            row.derivations
        );
        Ok(())
    }

    #[test]
    fn live_tree_inventory_parses_every_workflow() -> Result<()> {
        let rows = inventory_repo_workflows()?;
        assert!(rows.len() >= 80, "expected the repo workflow tree, found {}", rows.len());
        let with_oidc = rows
            .iter()
            .filter(|row| {
                row.derivations
                    .iter()
                    .any(|derivation| derivation.kind == CredentialDerivationKind::OidcIdTokenWrite)
            })
            .count();
        let with_env = rows
            .iter()
            .filter(|row| {
                row.derivations
                    .iter()
                    .any(|derivation| derivation.kind == CredentialDerivationKind::EnvInjectedToken)
            })
            .count();
        assert!(
            with_oidc >= 2,
            "origin/main has at least docs-deploy.yml and droid.yml OIDC write; found {with_oidc}"
        );
        assert!(
            with_env >= 10,
            "env-injected tokens are the high-frequency v2 kind; found {with_env} workflows"
        );
        Ok(())
    }

    fn live_row(name: &str) -> Result<WorkflowCredentialRow> {
        let path = project_root()?.join(WORKFLOWS_DIR).join(name);
        classify_workflow_file(&path)
    }

    fn live_kinds(row: &WorkflowCredentialRow) -> BTreeSet<CredentialDerivationKind> {
        row.derivations.iter().map(|row| row.kind).collect()
    }

    fn must_kind(
        rows: &[CredentialDerivation],
        kind: CredentialDerivationKind,
    ) -> CredentialDerivation {
        match rows.iter().find(|row| row.kind == kind) {
            Some(row) => row.clone(),
            None => panic!("missing derivation kind {kind:?} in {rows:?}"),
        }
    }

    fn parse_fixture(yaml: &str) -> Value {
        match serde_yaml_ng::from_str(yaml) {
            Ok(value) => value,
            Err(error) => panic!("fixture YAML must parse: {error}"),
        }
    }

    fn kinds_of(yaml: &str) -> BTreeSet<CredentialDerivationKind> {
        classify_workflow(&parse_fixture(yaml)).into_iter().map(|row| row.kind).collect()
    }

    fn derivations_of(yaml: &str) -> Vec<CredentialDerivation> {
        classify_workflow(&parse_fixture(yaml))
    }
}
