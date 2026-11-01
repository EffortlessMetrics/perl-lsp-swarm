//! Credential derivation inventory for GitHub workflow YAML.
//!
//! Credential column of #14867 / #9448. Diagnostic only: it classifies shapes
//! already present in `.github/workflows/*.yml` and does not change workflow
//! behavior.
//!
//! Schema-aware v3: `id-token: write` is request authority (a permission fact),
//! not evidence that an OIDC JWT was minted. An OIDC credential derivation is
//! emitted only when that permission is joined to a concrete requester
//! (unconditional admitted action, `getIDToken(` call, or an HTTP client using
//! `$ACTIONS_ID_TOKEN_REQUEST_*`). Cloud login actions and incidental mentions
//! (echo, `--exclude-env`) are not requesters. Permission without a requester,
//! or a requester without permission, is unproven rather than credential-free.
//! v1/v2 kinds remain independently recorded.
//!
//! GitHub's `permissions: write-all` shorthand is residual, not an OIDC
//! permission or derivation here. Vault/cloud-SDK secret-manager shapes remain
//! residual on #14867.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::Serialize;
use serde_yaml_ng::{Mapping, Value};

use crate::utils::project_root;

/// Schema id for this credential-column slice. Generation 3 splits OIDC
/// request authority from joined requester derivation; generation 1/2 kinds
/// remain independently recorded.
pub const SCHEMA: &str = "workflow_authority_credential.v3";

const WORKFLOWS_DIR: &str = ".github/workflows";

/// YAML-admitted OIDC JWT requesters whose contract requests a token
/// unconditionally. Cloud login actions can take static keys, so their names
/// are not admitted here.
const KNOWN_OIDC_REQUESTER_ACTIONS: &[&str] =
    &["actions/deploy-pages", "actions/attest", "actions/attest-build-provenance"];

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
    /// v3: OIDC JWT request joined to a concrete requester. Not emitted from
    /// `id-token: write` alone.
    OidcJoinedRequester,
}

impl CredentialDerivationKind {
    /// Schema generation that introduced this kind.
    pub const fn generation(self) -> u8 {
        match self {
            Self::SecretExpression | Self::GithubToken | Self::TokenBackedMutation => 1,
            Self::EnvInjectedToken => 2,
            Self::OidcJoinedRequester => 3,
        }
    }
}

/// YAML permission / request-authority fact. Distinct from credential derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    /// Explicit mapping `id-token: write` at workflow or job scope.
    IdTokenWrite,
}

/// Source-backed OIDC facts that are not proven credential-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnprovenKind {
    /// Effective `id-token: write` with no admitted requester on that job.
    OidcPermissionWithoutRequester,
    /// Admitted requester (or request primitive) without effective `id-token: write`.
    OidcRequesterWithoutPermission,
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

/// One declared permission / request-authority fact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PermissionFact {
    pub kind: PermissionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    pub path: String,
    pub evidence: String,
}

/// One OIDC fact that is not proven credential-bearing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UnprovenFact {
    pub kind: UnprovenKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    pub path: String,
    pub evidence: String,
}

/// Per-workflow credential column: permissions, derivations, and unproven OIDC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkflowCredentialRow {
    pub workflow: String,
    pub permissions: Vec<PermissionFact>,
    pub derivations: Vec<CredentialDerivation>,
    pub unproven: Vec<UnprovenFact>,
}

/// Classified facts for one parsed workflow document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkflowClassification {
    pub permissions: Vec<PermissionFact>,
    pub derivations: Vec<CredentialDerivation>,
    pub unproven: Vec<UnprovenFact>,
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

struct ScanOut {
    permissions: BTreeSet<PermissionFact>,
    derivations: BTreeSet<CredentialDerivation>,
    unproven: BTreeSet<UnprovenFact>,
}

impl ScanOut {
    fn new() -> Self {
        Self {
            permissions: BTreeSet::new(),
            derivations: BTreeSet::new(),
            unproven: BTreeSet::new(),
        }
    }

    fn into_classification(self) -> WorkflowClassification {
        WorkflowClassification {
            permissions: self.permissions.into_iter().collect(),
            derivations: self.derivations.into_iter().collect(),
            unproven: self.unproven.into_iter().collect(),
        }
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
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
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
    let classified = classify_workflow(&workflow);
    Ok(WorkflowCredentialRow {
        workflow: workflow_name,
        permissions: classified.permissions,
        derivations: classified.derivations,
        unproven: classified.unproven,
    })
}

/// Classify an already-parsed workflow document.
pub fn classify_workflow(workflow: &Value) -> WorkflowClassification {
    let mut out = ScanOut::new();
    scan_document(workflow, &mut out);
    out.into_classification()
}

fn scan_document(workflow: &Value, out: &mut ScanOut) {
    let Some(root) = workflow.as_mapping() else {
        return;
    };
    let workflow_id_token_write = match mapping_get(root, "permissions") {
        Some(permissions) => scan_permissions(permissions, ScanCtx::root(), "permissions", out),
        None => false,
    };
    if let Some(env) = mapping_get(root, "env") {
        scan_env(env, ScanCtx::root(), "env", &mut out.derivations);
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
        let declared_permissions = mapping_get(job, "permissions");
        let job_id_token_write = match declared_permissions {
            Some(permissions) => {
                scan_permissions(permissions, ctx, &format!("{job_path}.permissions"), out)
            }
            None => false,
        };
        if let Some(env) = mapping_get(job, "env") {
            scan_env(env, ctx, &format!("{job_path}.env"), &mut out.derivations);
        }
        let mut requesters = Vec::new();
        if let Some(steps) = mapping_get(job, "steps").and_then(Value::as_sequence) {
            for (index, step) in steps.iter().enumerate() {
                scan_step(
                    step,
                    ctx,
                    &format!("{job_path}.steps[{index}]"),
                    &mut requesters,
                    &mut out.derivations,
                );
            }
        }
        // Remaining job fields (run scripts, names, `if:`) still carry v1 expressions.
        scan_value(job_value, ctx, &job_path, &mut out.derivations);
        let effective_id_token_write = if declared_permissions.is_some() {
            job_id_token_write
        } else {
            workflow_id_token_write
        };
        join_job_oidc(job_name, &job_path, effective_id_token_write, &requesters, out);
    }
}

fn scan_step(
    step: &Value,
    ctx: ScanCtx<'_>,
    path: &str,
    requesters: &mut Vec<(String, String)>,
    derivations: &mut BTreeSet<CredentialDerivation>,
) {
    requesters.extend(oidc_requesters_in_step(step, path));
    let Some(step_map) = step.as_mapping() else {
        scan_value(step, ctx, path, derivations);
        return;
    };
    if let Some(env) = mapping_get(step_map, "env") {
        scan_env(env, ctx, &format!("{path}.env"), derivations);
    }
    if let Some(with) = mapping_get(step_map, "with") {
        scan_with(with, ctx, &format!("{path}.with"), derivations);
    }
    scan_value(step, ctx, path, derivations);
}

fn scan_permissions(permissions: &Value, ctx: ScanCtx<'_>, path: &str, out: &mut ScanOut) -> bool {
    let Some(map) = permissions.as_mapping() else {
        return false;
    };
    let mut id_token_write = false;
    for (key, value) in map {
        let Some(scope) = yaml_key(key) else {
            continue;
        };
        if scope == "id-token" && is_write_scope(value) {
            id_token_write = true;
            out.permissions.insert(PermissionFact {
                kind: PermissionKind::IdTokenWrite,
                job: ctx.job.map(str::to_string),
                path: format!("{path}.{scope}"),
                evidence: "id-token: write".to_string(),
            });
        }
    }
    id_token_write
}

fn join_job_oidc(
    job: &str,
    job_path: &str,
    effective_id_token_write: bool,
    requesters: &[(String, String)],
    out: &mut ScanOut,
) {
    if effective_id_token_write {
        if requesters.is_empty() {
            out.unproven.insert(UnprovenFact {
                kind: UnprovenKind::OidcPermissionWithoutRequester,
                job: Some(job.to_string()),
                path: job_path.to_string(),
                evidence: "id-token: write with no admitted requester".to_string(),
            });
        } else {
            for (path, evidence) in requesters {
                push(
                    &mut out.derivations,
                    CredentialDerivationKind::OidcJoinedRequester,
                    Some(job),
                    path.clone(),
                    evidence.clone(),
                );
            }
        }
        return;
    }
    for (path, evidence) in requesters {
        out.unproven.insert(UnprovenFact {
            kind: UnprovenKind::OidcRequesterWithoutPermission,
            job: Some(job.to_string()),
            path: path.clone(),
            evidence: format!("{evidence} without id-token: write"),
        });
    }
}

fn oidc_requesters_in_step(step: &Value, path: &str) -> Vec<(String, String)> {
    let Some(step_map) = step.as_mapping() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if let Some(uses) = mapping_get(step_map, "uses").and_then(Value::as_str)
        && let Some(action) = action_identity(uses)
        && is_known_oidc_requester(action)
    {
        found.push((format!("{path}.uses"), format!("uses: {action}")));
    }
    if let Some(run) = mapping_get(step_map, "run").and_then(Value::as_str)
        && let Some(primitive) = oidc_request_primitive(run)
    {
        found.push((format!("{path}.run"), primitive.to_string()));
    }
    if let Some(with) = mapping_get(step_map, "with").and_then(Value::as_mapping) {
        for (key, value) in with {
            let Some(name) = yaml_key(key) else {
                continue;
            };
            if let Some(text) = value.as_str()
                && let Some(primitive) = oidc_request_primitive(text)
            {
                found.push((format!("{path}.with.{name}"), primitive.to_string()));
            }
        }
    }
    found
}

fn action_identity(uses: &str) -> Option<&str> {
    let uses = uses.trim();
    if uses.is_empty() || uses.starts_with("./") || uses.starts_with("docker://") {
        return None;
    }
    uses.split('@').next().map(str::trim).filter(|action| !action.is_empty())
}

fn is_known_oidc_requester(action: &str) -> bool {
    KNOWN_OIDC_REQUESTER_ACTIONS.contains(&action)
}

fn oidc_request_primitive(text: &str) -> Option<&'static str> {
    if text.contains("getIDToken(") {
        return Some("getIDToken");
    }
    requests_oidc_http(text)
}

/// HTTP request using the Actions OIDC request API. Printing or excluding the
/// variable names is not a requester.
fn requests_oidc_http(text: &str) -> Option<&'static str> {
    let uses_url = uses_id_token_request_var(text, "ACTIONS_ID_TOKEN_REQUEST_URL");
    let uses_token = uses_id_token_request_var(text, "ACTIONS_ID_TOKEN_REQUEST_TOKEN");
    if !uses_url && !uses_token {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let http_client = lower.contains("curl")
        || lower.contains("wget")
        || lower.contains("fetch(")
        || lower.contains("http.get")
        || lower.contains("http.request");
    if !http_client {
        return None;
    }
    if uses_url {
        Some("ACTIONS_ID_TOKEN_REQUEST_URL")
    } else {
        Some("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
    }
}

/// Request-variable *use*, not a bare name. `--exclude-env ACTIONS_ID_TOKEN_REQUEST_URL`
/// names the API in order to deny it and is not a requester.
fn uses_id_token_request_var(text: &str, name: &str) -> bool {
    let dollar = format!("${name}");
    let braced = format!("${{{name}}}");
    let process_env = format!("process.env.{name}");
    text.contains(&dollar) || text.contains(&braced) || text.contains(&process_env)
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
    fn schema_is_v3() {
        assert_eq!(SCHEMA, "workflow_authority_credential.v3");
        assert_eq!(CredentialDerivationKind::EnvInjectedToken.generation(), 2);
        assert_eq!(CredentialDerivationKind::OidcJoinedRequester.generation(), 3);
        assert_eq!(CredentialDerivationKind::GithubToken.generation(), 1);
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
            !kinds.contains(&CredentialDerivationKind::OidcJoinedRequester),
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
    fn id_token_write_without_requester_is_permission_not_minted_credential() {
        let yaml = with_job_body(
            r"
    permissions:
      contents: read
      id-token: write
    steps:
      - run: echo federated
",
        );
        let classified = classify_yaml(&yaml);
        assert_eq!(
            permission_kinds(&classified),
            [PermissionKind::IdTokenWrite].into_iter().collect(),
            "id-token: write is request authority: {:?}",
            classified.permissions
        );
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester),
            "permission alone must not mint an OIDC credential: {:?}",
            classified.derivations
        );
        assert_eq!(
            unproven_kinds(&classified),
            [UnprovenKind::OidcPermissionWithoutRequester].into_iter().collect(),
            "permission without requester is unproven, not credential-free: {:?}",
            classified.unproven
        );
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::SecretExpression),
            "OIDC permission must not be reclassified as secrets.*: {:?}",
            classified.derivations
        );
        let permission = must_permission(&classified, PermissionKind::IdTokenWrite);
        assert!(permission.path.ends_with("permissions.id-token"), "{}", permission.path);
        assert_eq!(permission.job.as_deref(), Some("example"));
    }

    #[test]
    fn id_token_write_plus_known_requester_joins_oidc_derivation() {
        let yaml = with_job_body(
            r"
    permissions:
      id-token: write
      pages: write
    steps:
      - uses: actions/deploy-pages@368f82528645a54fb793d4d04e342629a3f51346
",
        );
        let classified = classify_yaml(&yaml);
        assert!(
            permission_kinds(&classified).contains(&PermissionKind::IdTokenWrite),
            "permission axis remains: {:?}",
            classified.permissions
        );
        let oidc =
            must_kind(&classified.derivations, CredentialDerivationKind::OidcJoinedRequester);
        assert_eq!(oidc.generation, 3);
        assert_eq!(oidc.job.as_deref(), Some("example"));
        assert_eq!(oidc.evidence, "uses: actions/deploy-pages");
        assert!(oidc.path.ends_with(".uses"), "{}", oidc.path);
        assert!(
            classified.unproven.is_empty(),
            "joined requester is not unproven: {:?}",
            classified.unproven
        );
    }

    #[test]
    fn known_requester_without_permission_is_unproven_cannot_mint() {
        let yaml = with_job_body(
            r"
    permissions:
      contents: read
    steps:
      - uses: actions/deploy-pages@v5
",
        );
        let classified = classify_yaml(&yaml);
        assert!(
            classified.permissions.is_empty(),
            "no id-token write was declared: {:?}",
            classified.permissions
        );
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester),
            "requester without permission cannot mint: {:?}",
            classified.derivations
        );
        assert_eq!(
            unproven_kinds(&classified),
            [UnprovenKind::OidcRequesterWithoutPermission].into_iter().collect(),
            "requester without permission is unproven: {:?}",
            classified.unproven
        );
        let unproven = &classified.unproven[0];
        assert!(unproven.evidence.contains("uses: actions/deploy-pages"), "{}", unproven.evidence);
        assert!(unproven.evidence.contains("without id-token: write"), "{}", unproven.evidence);
    }

    #[test]
    fn get_id_token_primitive_joins_when_permission_is_present() {
        let yaml = with_job_body(
            r#"
    permissions:
      id-token: write
    steps:
      - run: node -e "require('@actions/core').getIDToken('aud')"
"#,
        );
        let oidc = must_kind(&derivations_of(&yaml), CredentialDerivationKind::OidcJoinedRequester);
        assert_eq!(oidc.evidence, "getIDToken");
        assert!(oidc.path.ends_with(".run"), "{}", oidc.path);
    }

    #[test]
    fn id_token_request_url_primitive_joins_when_permission_is_present() {
        let yaml = with_job_body(
            r#"
    permissions:
      id-token: write
    steps:
      - run: 'curl "$ACTIONS_ID_TOKEN_REQUEST_URL"'
"#,
        );
        let oidc = must_kind(&derivations_of(&yaml), CredentialDerivationKind::OidcJoinedRequester);
        assert_eq!(oidc.evidence, "ACTIONS_ID_TOKEN_REQUEST_URL");
    }

    #[test]
    fn exclude_env_id_token_request_name_is_not_a_requester() {
        let yaml = with_job_body(
            r"
    steps:
      - run: awf --exclude-env ACTIONS_ID_TOKEN_REQUEST_TOKEN --exclude-env ACTIONS_ID_TOKEN_REQUEST_URL
",
        );
        let classified = classify_yaml(&yaml);
        assert!(
            classified.unproven.is_empty(),
            "excluding the request env vars is not an OIDC requester: {:?}",
            classified.unproven
        );
        assert!(!kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester));
    }

    #[test]
    fn echo_of_id_token_request_url_is_not_a_requester() {
        let yaml = with_job_body(
            r#"
    permissions:
      id-token: write
    steps:
      - run: 'echo "$ACTIONS_ID_TOKEN_REQUEST_URL"'
"#,
        );
        let classified = classify_yaml(&yaml);
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester),
            "printing the request URL does not mint: {:?}",
            classified.derivations
        );
        assert_eq!(
            unproven_kinds(&classified),
            [UnprovenKind::OidcPermissionWithoutRequester].into_iter().collect()
        );
    }

    #[test]
    fn github_script_get_id_token_in_with_script_joins() {
        let yaml = with_job_body(
            r#"
    permissions:
      id-token: write
    steps:
      - uses: actions/github-script@v7
        with:
          script: await core.getIDToken("aud")
"#,
        );
        let classified = classify_yaml(&yaml);
        let oidc =
            must_kind(&classified.derivations, CredentialDerivationKind::OidcJoinedRequester);
        assert_eq!(oidc.evidence, "getIDToken");
        assert!(oidc.path.ends_with(".with.script"), "{}", oidc.path);
        assert!(classified.unproven.is_empty(), "{:?}", classified.unproven);
    }

    #[test]
    fn static_key_cloud_login_is_not_an_admitted_requester() {
        let yaml = with_job_body(
            r"
    permissions:
      id-token: write
    steps:
      - uses: aws-actions/configure-aws-credentials@v4
        with:
          aws-access-key-id: ${{ secrets.AWS_ACCESS_KEY_ID }}
          aws-secret-access-key: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
",
        );
        let classified = classify_yaml(&yaml);
        assert!(permission_kinds(&classified).contains(&PermissionKind::IdTokenWrite));
        assert!(
            !kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester),
            "static-key cloud login is not an OIDC requester: {:?}",
            classified.derivations
        );
        assert_eq!(
            unproven_kinds(&classified),
            [UnprovenKind::OidcPermissionWithoutRequester].into_iter().collect()
        );
    }

    #[test]
    fn workflow_level_id_token_write_is_inherited_by_jobs_without_permissions() {
        let yaml = r"
permissions:
  contents: read
  id-token: write
jobs:
  inherit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/deploy-pages@v5
  idle:
    runs-on: ubuntu-latest
    steps:
      - run: echo no requester
  override:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/deploy-pages@v5
";
        let classified = classify_yaml(yaml);
        let permission = must_permission(&classified, PermissionKind::IdTokenWrite);
        assert!(
            permission.job.is_none(),
            "workflow-level permission has no job: {:?}",
            permission.job
        );
        assert_eq!(permission.path, "permissions.id-token");

        let inherit = classified.derivations.iter().find(|row| {
            row.kind == CredentialDerivationKind::OidcJoinedRequester
                && row.job.as_deref() == Some("inherit")
        });
        let Some(inherit) = inherit else {
            panic!("inherited job with requester must join OIDC: {:?}", classified.derivations);
        };
        assert_eq!(inherit.evidence, "uses: actions/deploy-pages");

        assert!(
            classified.derivations.iter().all(|row| {
                row.kind != CredentialDerivationKind::OidcJoinedRequester
                    || row.job.as_deref() == Some("inherit")
            }),
            "override/idle must not mint OIDC: {:?}",
            classified.derivations
        );

        let idle = classified.unproven.iter().find(|row| {
            row.kind == UnprovenKind::OidcPermissionWithoutRequester
                && row.job.as_deref() == Some("idle")
        });
        assert!(
            idle.is_some(),
            "inherited permission without requester is unproven: {:?}",
            classified.unproven
        );

        let override_job = classified.unproven.iter().find(|row| {
            row.kind == UnprovenKind::OidcRequesterWithoutPermission
                && row.job.as_deref() == Some("override")
        });
        assert!(
            override_job.is_some(),
            "job-level permissions replace workflow-level; requester cannot mint: {:?}",
            classified.unproven
        );
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
        let classified = classify_yaml(yaml);
        assert!(
            classified.permissions.is_empty(),
            "permissions: write-all is residual, not a silent OIDC permission"
        );
        assert!(!kinds_of(yaml).contains(&CredentialDerivationKind::OidcJoinedRequester));
        assert!(classified.unproven.is_empty());
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
        let classified = classify_yaml(&yaml);
        assert!(classified.permissions.is_empty());
        assert!(!kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester));
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
        let classified = classify_yaml(&yaml);
        assert!(classified.permissions.is_empty());
        assert!(kinds_of(&yaml).is_empty());
        assert!(classified.unproven.is_empty());
    }

    #[test]
    fn id_token_write_in_a_run_script_is_not_permissions() {
        let yaml = with_job_body(
            r#"
    steps:
      - run: 'echo "permissions: id-token: write"'
"#,
        );
        let classified = classify_yaml(&yaml);
        assert!(
            classified.permissions.is_empty(),
            "string mention of id-token: write must not count as permission"
        );
        assert!(!kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester));
    }

    #[test]
    fn unknown_action_is_not_an_admitted_requester() {
        let yaml = with_job_body(
            r"
    permissions:
      id-token: write
    steps:
      - uses: some-org/not-an-oidc-action@v1
",
        );
        let classified = classify_yaml(&yaml);
        assert!(permission_kinds(&classified).contains(&PermissionKind::IdTokenWrite));
        assert!(!kinds_of(&yaml).contains(&CredentialDerivationKind::OidcJoinedRequester));
        assert_eq!(
            unproven_kinds(&classified),
            [UnprovenKind::OidcPermissionWithoutRequester].into_iter().collect()
        );
    }

    #[test]
    fn action_identity_strips_pin_and_rejects_local_or_docker() {
        assert_eq!(
            action_identity("actions/deploy-pages@368f82528645a54fb793d4d04e342629a3f51346"),
            Some("actions/deploy-pages")
        );
        assert_eq!(action_identity("./local-action"), None);
        assert_eq!(action_identity("docker://alpine:3"), None);
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
        let classified = classify_yaml(MINIMAL_JOB);
        assert!(kinds_of(MINIMAL_JOB).is_empty());
        assert!(classified.permissions.is_empty());
        assert!(classified.unproven.is_empty());
    }

    #[test]
    fn live_docs_deploy_joins_oidc_requester_and_keeps_env_injected_token() -> Result<()> {
        let row = live_row("docs-deploy.yml")?;
        let kinds = live_kinds(&row);
        assert!(
            row.permissions.iter().any(|fact| fact.kind == PermissionKind::IdTokenWrite),
            "docs-deploy.yml deploy job grants id-token: write: {:?}",
            row.permissions
        );
        assert!(
            kinds.contains(&CredentialDerivationKind::OidcJoinedRequester),
            "docs-deploy.yml deploy-pages is an admitted OIDC requester: {kinds:?}"
        );
        let oidc = row
            .derivations
            .iter()
            .find(|row| row.kind == CredentialDerivationKind::OidcJoinedRequester)
            .cloned();
        let Some(oidc) = oidc else {
            panic!("missing joined OIDC derivation: {:?}", row.derivations);
        };
        assert_eq!(oidc.evidence, "uses: actions/deploy-pages");
        assert_eq!(oidc.job.as_deref(), Some("deploy"));
        assert!(
            !row.unproven.iter().any(|fact| fact.job.as_deref() == Some("deploy")),
            "deploy job is joined, not unproven: {:?}",
            row.unproven
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
    fn live_release_candidate_joins_attest_requester() -> Result<()> {
        let row = live_row("release.yml")?;
        assert!(
            row.permissions.iter().any(|fact| {
                fact.kind == PermissionKind::IdTokenWrite
                    && fact.job.as_deref() == Some("candidate")
            }),
            "release.yml candidate job grants id-token: write: {:?}",
            row.permissions
        );
        let oidc = row.derivations.iter().find(|row| {
            row.kind == CredentialDerivationKind::OidcJoinedRequester
                && row.job.as_deref() == Some("candidate")
        });
        let Some(oidc) = oidc else {
            panic!("release.yml candidate must join actions/attest: {:?}", row.derivations);
        };
        assert_eq!(oidc.evidence, "uses: actions/attest");
        Ok(())
    }

    #[test]
    fn live_minimax_lock_exclude_env_is_not_an_oidc_requester() -> Result<()> {
        let row = live_row("minimax-coding-agent.lock.yml")?;
        assert!(
            row.unproven.is_empty(),
            "minimax-coding-agent.lock.yml excludes ACTIONS_ID_TOKEN_REQUEST_* from the sandbox; that is not a requester: {:?}",
            row.unproven
        );
        assert!(
            !row.derivations
                .iter()
                .any(|row| row.kind == CredentialDerivationKind::OidcJoinedRequester),
            "minimax-coding-agent.lock.yml must not join OIDC from exclude-env: {:?}",
            row.derivations
        );
        Ok(())
    }

    #[test]
    fn live_droid_records_no_oidc_write() -> Result<()> {
        let row = live_row("droid.yml")?;
        let kinds = live_kinds(&row);
        assert!(
            !row.permissions.iter().any(|fact| fact.kind == PermissionKind::IdTokenWrite),
            "droid.yml no longer grants id-token: write: {:?}",
            row.permissions
        );
        assert!(
            !kinds.contains(&CredentialDerivationKind::OidcJoinedRequester),
            "droid.yml has no joined OIDC requester: {kinds:?}"
        );
        assert!(
            kinds.contains(&CredentialDerivationKind::EnvInjectedToken),
            "droid.yml injects GH_TOKEN: {kinds:?}"
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
            !kinds.contains(&CredentialDerivationKind::OidcJoinedRequester),
            "pr-candidate-set.yml has no joined OIDC requester: {kinds:?}"
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
            row.derivations.is_empty() && row.permissions.is_empty() && row.unproven.is_empty(),
            "workflow-trigger-lint.yml should stay a negative live control: {:?}",
            row
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
                row.derivations.iter().any(|derivation| {
                    derivation.kind == CredentialDerivationKind::OidcJoinedRequester
                })
            })
            .count();
        let with_permission = rows
            .iter()
            .filter(|row| {
                row.permissions.iter().any(|fact| fact.kind == PermissionKind::IdTokenWrite)
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
            with_permission >= 2,
            "origin/main has at least docs-deploy.yml and release.yml id-token write; found {with_permission}"
        );
        assert!(
            with_oidc >= 2,
            "docs-deploy.yml (deploy-pages) and release.yml (attest) join OIDC requesters; found {with_oidc}"
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

    fn classify_yaml(yaml: &str) -> WorkflowClassification {
        classify_workflow(&parse_fixture(yaml))
    }

    fn permission_kinds(classified: &WorkflowClassification) -> BTreeSet<PermissionKind> {
        classified.permissions.iter().map(|row| row.kind).collect()
    }

    fn unproven_kinds(classified: &WorkflowClassification) -> BTreeSet<UnprovenKind> {
        classified.unproven.iter().map(|row| row.kind).collect()
    }

    fn must_permission(
        classified: &WorkflowClassification,
        kind: PermissionKind,
    ) -> PermissionFact {
        match classified.permissions.iter().find(|row| row.kind == kind) {
            Some(row) => row.clone(),
            None => panic!("missing permission kind {kind:?} in {:?}", classified.permissions),
        }
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
        classify_yaml(yaml).derivations.into_iter().map(|row| row.kind).collect()
    }

    fn derivations_of(yaml: &str) -> Vec<CredentialDerivation> {
        classify_yaml(yaml).derivations
    }
}
