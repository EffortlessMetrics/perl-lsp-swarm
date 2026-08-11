//! Drift, path-safety, and claim-boundary checks for
//! `policy/lsp-client-support.toml`.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const EXECUTABLE_RECEIPT_SCHEMA: &str = "lsp-client-execution-receipt.v1";
const ACTUAL_CLIENT_RECEIPT_ROOT: &str = "docs/receipts/lsp-clients/actual";
const PACKAGED_PRODUCT_RECEIPT_ROOT: &str = "docs/receipts/lsp-clients/packaged";

#[derive(Debug, Deserialize)]
struct Registry {
    meta: Meta,
    client: Vec<Client>,
}

#[derive(Debug, Deserialize)]
struct Meta {
    schema: String,
    source_document: String,
    owner_issue: u64,
    allowed_tiers: Vec<String>,
    allowed_evidence_kinds: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Client {
    id: String,
    display_name: String,
    integration_mode: String,
    tier: String,
    owner_issue: u64,
    requires_actual_client_receipt: bool,
    synthetic_profile: bool,
    evidence: Vec<Evidence>,
    known_overrides: Vec<String>,
    external_dependency: String,
    claim_boundary: String,
}

#[derive(Debug, Deserialize)]
struct Evidence {
    path: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionReceipt {
    schema: String,
    client_id: String,
    evidence_kind: String,
    recorded_at_utc: String,
    source_revision: String,
    client_executable: String,
    client_version: String,
    server_command: Vec<String>,
    transport: String,
    exit_code: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    assertions: Vec<ReceiptAssertion>,
    artifact: Option<ReceiptArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptAssertion {
    name: String,
    passed: bool,
    evidence: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptArtifact {
    path: String,
    sha256: String,
    size_bytes: u64,
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("perl-lsp-rs must live under <workspace>/crates")
}

fn load_registry(root: &Path) -> Result<Registry> {
    let path = root.join("policy/lsp-client-support.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read client support registry at {}", path.display()))?;
    toml::from_str(&source).context("parse client support registry")
}

fn load_tracked_files(root: &Path) -> Result<BTreeSet<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .context("run git ls-files for client-support evidence validation")?;
    ensure!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect())
}

fn validate_relative_path(raw: &str) -> Result<PathBuf> {
    ensure!(!raw.trim().is_empty(), "repository path must not be empty");
    let path = Path::new(raw);
    ensure!(!path.is_absolute(), "repository path must be relative: {raw}");

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => bail!("repository path must not contain '.': {raw}"),
            Component::ParentDir => bail!("repository path must not contain '..': {raw}"),
            Component::RootDir | Component::Prefix(_) => {
                bail!("repository path must not be rooted: {raw}")
            }
        }
    }
    ensure!(
        !normalized.as_os_str().is_empty(),
        "repository path must name a file: {raw}"
    );
    Ok(normalized)
}

fn repository_path_string(path: &Path) -> String {
    path.iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn resolve_repository_file(root: &Path, raw: &str) -> Result<(PathBuf, PathBuf)> {
    let relative = validate_relative_path(raw)?;
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("canonicalize repository root {}", root.display()))?;

    let mut candidate = canonical_root.clone();
    for component in relative.components() {
        candidate.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&candidate).with_context(|| {
            format!("inspect repository evidence component {}", candidate.display())
        })?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "repository evidence path crosses a symlink: {}",
            candidate.display()
        );
    }

    let canonical_candidate = fs::canonicalize(&candidate)
        .with_context(|| format!("canonicalize repository evidence {}", candidate.display()))?;
    ensure!(
        canonical_candidate.starts_with(&canonical_root),
        "repository evidence escapes the repository root: {}",
        canonical_candidate.display()
    );
    let metadata = fs::metadata(&canonical_candidate).with_context(|| {
        format!("inspect repository evidence {}", canonical_candidate.display())
    })?;
    ensure!(
        metadata.is_file(),
        "repository evidence is not a regular file: {}",
        canonical_candidate.display()
    );
    Ok((relative, canonical_candidate))
}

fn validate_repository_file(
    root: &Path,
    raw: &str,
    tracked_files: &BTreeSet<String>,
) -> Result<PathBuf> {
    let (relative, canonical_candidate) = resolve_repository_file(root, raw)?;
    let repository_path = repository_path_string(&relative);
    ensure!(
        tracked_files.contains(&repository_path),
        "repository evidence is not tracked by git: {repository_path}"
    );
    Ok(canonical_candidate)
}

fn validate_execution_receipt_path(raw: &str, kind: &str) -> Result<PathBuf> {
    let relative = validate_relative_path(raw)?;
    let required_root = match kind {
        "actual_client" => Path::new(ACTUAL_CLIENT_RECEIPT_ROOT),
        "packaged_product" => Path::new(PACKAGED_PRODUCT_RECEIPT_ROOT),
        other => bail!("{other} is not an executable evidence kind"),
    };
    ensure!(
        relative.starts_with(required_root),
        "{kind} evidence must live under {}: {raw}",
        required_root.display()
    );
    ensure!(
        relative.extension().and_then(|extension| extension.to_str()) == Some("json"),
        "{kind} evidence must be a JSON receipt: {raw}"
    );
    Ok(relative)
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_artifact(artifact: &ReceiptArtifact) -> Result<()> {
    let _ = validate_relative_path(&artifact.path)
        .context("packaged-product artifact path must be a bounded relative locator")?;
    ensure!(
        is_lower_hex(&artifact.sha256, 64),
        "artifact sha256 must be 64 lowercase hexadecimal characters"
    );
    ensure!(
        artifact.size_bytes > 0,
        "packaged-product artifact must have a non-zero size"
    );
    Ok(())
}

fn validate_execution_receipt(
    path: &Path,
    expected_client_id: &str,
    expected_kind: &str,
) -> Result<()> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("read executable client receipt {}", path.display()))?;
    let receipt: ExecutionReceipt = serde_json::from_str(&source)
        .with_context(|| format!("parse executable client receipt {}", path.display()))?;

    ensure!(
        receipt.schema == EXECUTABLE_RECEIPT_SCHEMA,
        "receipt schema mismatch in {}: {}",
        path.display(),
        receipt.schema
    );
    ensure!(
        receipt.client_id == expected_client_id,
        "receipt client mismatch in {}: expected {expected_client_id}, got {}",
        path.display(),
        receipt.client_id
    );
    ensure!(
        receipt.evidence_kind == expected_kind,
        "receipt evidence kind mismatch in {}: expected {expected_kind}, got {}",
        path.display(),
        receipt.evidence_kind
    );
    ensure!(
        receipt.recorded_at_utc.contains('T') && receipt.recorded_at_utc.ends_with('Z'),
        "receipt recorded_at_utc must be a UTC timestamp ending in Z"
    );
    ensure!(
        is_lower_hex(&receipt.source_revision, 40),
        "receipt source_revision must be a 40-character lowercase Git SHA"
    );
    ensure!(
        !receipt.client_executable.trim().is_empty(),
        "receipt client_executable must not be empty"
    );
    ensure!(
        !receipt.client_version.trim().is_empty(),
        "receipt client_version must not be empty"
    );
    ensure!(
        !receipt.server_command.is_empty()
            && receipt
                .server_command
                .iter()
                .all(|argument| !argument.trim().is_empty()),
        "receipt server_command must contain non-empty arguments"
    );
    ensure!(
        receipt.transport == "stdio",
        "receipt transport must be stdio, got {}",
        receipt.transport
    );
    ensure!(
        receipt.exit_code == 0,
        "executable client receipt did not exit successfully: {}",
        receipt.exit_code
    );
    ensure!(
        is_lower_hex(&receipt.stdout_sha256, 64),
        "receipt stdout_sha256 must be 64 lowercase hexadecimal characters"
    );
    ensure!(
        is_lower_hex(&receipt.stderr_sha256, 64),
        "receipt stderr_sha256 must be 64 lowercase hexadecimal characters"
    );
    ensure!(
        !receipt.assertions.is_empty(),
        "executable client receipt must contain assertions"
    );

    let mut assertion_names = BTreeSet::new();
    for assertion in &receipt.assertions {
        ensure!(
            !assertion.name.trim().is_empty(),
            "receipt assertion name must not be empty"
        );
        ensure!(
            assertion_names.insert(assertion.name.as_str()),
            "receipt repeats assertion {}",
            assertion.name
        );
        ensure!(
            assertion.passed,
            "receipt assertion failed: {}",
            assertion.name
        );
        ensure!(
            !assertion.evidence.trim().is_empty(),
            "receipt assertion {} has no evidence locator",
            assertion.name
        );
    }

    if let Some(artifact) = &receipt.artifact {
        validate_artifact(artifact)?;
    }
    if expected_kind == "packaged_product" {
        let artifact = receipt
            .artifact
            .as_ref()
            .context("packaged-product receipt must include an artifact")?;
        validate_artifact(artifact)?;
    }
    Ok(())
}

fn editor_table_names(markdown: &str) -> Vec<String> {
    let mut in_table = false;
    let mut skipped_separator = false;
    let mut names = Vec::new();

    for line in markdown.lines() {
        if line.starts_with("| Editor |") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if !line.starts_with('|') {
            break;
        }
        if !skipped_separator {
            skipped_separator = true;
            continue;
        }
        if let Some(name) = line.split('|').nth(1).map(str::trim)
            && !name.is_empty()
        {
            names.push(name.to_string());
        }
    }

    names
}

fn render_status(registry: &Registry) -> String {
    let mut rendered = String::from(
        "# LSP client support evidence\n\n\
         > Generated from `policy/lsp-client-support.toml`. Setup prose, synthetic client profiles, actual clients, and packaged products are different evidence classes.\n\n\
         | Client | Integration mode | Earned tier | Claim boundary |\n\
         | --- | --- | --- | --- |\n",
    );

    for client in &registry.client {
        let boundary = client.claim_boundary.replace('|', "\\|");
        rendered.push_str(&format!(
            "| {} | `{}` | `{}` | {} |\n",
            client.display_name, client.integration_mode, client.tier, boundary
        ));
    }

    let promoted = registry
        .client
        .iter()
        .filter(|client| {
            matches!(
                client.tier.as_str(),
                "packaged_product_proven" | "real_generic_client_proven"
            )
        })
        .map(|client| format!("`{}` (`{}`)", client.display_name, client.tier))
        .collect::<Vec<_>>();

    if promoted.is_empty() {
        rendered.push_str(
            "\nNo row is currently promoted to `packaged_product_proven` or `real_generic_client_proven`. Promotion requires typed actual-client or packaged-product evidence under issue #6739.\n",
        );
    } else {
        rendered.push_str(&format!(
            "\nCurrent executable promotions: {}. Each promotion is validated against its typed evidence class.\n",
            promoted.join(", ")
        ));
    }
    rendered
}

#[test]
fn client_registry_matches_documented_editor_population() -> Result<()> {
    let root = workspace_root()?;
    let registry = load_registry(&root)?;
    let tracked_files = load_tracked_files(&root)?;
    ensure!(registry.meta.schema == "lsp-client-support.v1");
    ensure!(registry.meta.owner_issue == 6739);

    let docs_path = validate_repository_file(
        &root,
        &registry.meta.source_document,
        &tracked_files,
    )?;
    let docs = fs::read_to_string(&docs_path)
        .with_context(|| format!("read editor setup guide at {}", docs_path.display()))?;
    let documented_rows = editor_table_names(&docs);
    ensure!(
        !documented_rows.is_empty(),
        "editor setup table was not found or was empty"
    );
    let documented = documented_rows.iter().cloned().collect::<BTreeSet<_>>();
    ensure!(
        documented.len() == documented_rows.len(),
        "editor setup table contains duplicate client names: {documented_rows:?}"
    );

    let registered_rows = registry
        .client
        .iter()
        .map(|client| client.display_name.clone())
        .collect::<Vec<_>>();
    let registered = registered_rows.iter().cloned().collect::<BTreeSet<_>>();
    ensure!(
        registered.len() == registered_rows.len(),
        "client registry contains duplicate display names: {registered_rows:?}"
    );
    ensure!(
        documented == registered,
        "editor support registry drift: documented_only={:?} registry_only={:?}",
        documented.difference(&registered).collect::<Vec<_>>(),
        registered.difference(&documented).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn client_registry_rejects_claim_inflation_and_missing_evidence() -> Result<()> {
    let root = workspace_root()?;
    let registry = load_registry(&root)?;
    let tracked_files = load_tracked_files(&root)?;
    let allowed_tiers = registry
        .meta
        .allowed_tiers
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let allowed_evidence = registry
        .meta
        .allowed_evidence_kinds
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();

    for client in &registry.client {
        ensure!(!client.id.is_empty(), "client ID must not be empty");
        ensure!(
            ids.insert(client.id.as_str()),
            "duplicate client ID: {}",
            client.id
        );
        ensure!(
            allowed_tiers.contains(client.tier.as_str()),
            "unknown tier for {}: {}",
            client.id,
            client.tier
        );
        ensure!(
            client.owner_issue > 0,
            "{} must name an owning issue",
            client.id
        );
        ensure!(
            !client.claim_boundary.trim().is_empty(),
            "{} must state a claim boundary",
            client.id
        );
        ensure!(
            !client.evidence.is_empty(),
            "{} must name typed evidence",
            client.id
        );

        let mut evidence_kinds = BTreeSet::new();
        let mut evidence_paths = BTreeSet::new();
        let mut validated_execution_kinds = BTreeSet::new();
        for evidence in &client.evidence {
            ensure!(
                allowed_evidence.contains(evidence.kind.as_str()),
                "{} has unknown evidence kind {}",
                client.id,
                evidence.kind
            );
            ensure!(
                evidence_paths.insert(evidence.path.as_str()),
                "{} repeats evidence path {}",
                client.id,
                evidence.path
            );
            evidence_kinds.insert(evidence.kind.as_str());

            if matches!(evidence.kind.as_str(), "actual_client" | "packaged_product") {
                let expected_relative =
                    validate_execution_receipt_path(&evidence.path, &evidence.kind)?;
                let receipt_path =
                    validate_repository_file(&root, &evidence.path, &tracked_files)?;
                ensure!(
                    repository_path_string(&expected_relative)
                        == repository_path_string(&validate_relative_path(&evidence.path)?),
                    "executable receipt path normalization drifted"
                );
                validate_execution_receipt(
                    &receipt_path,
                    &client.id,
                    &evidence.kind,
                )?;
                validated_execution_kinds.insert(evidence.kind.as_str());
            } else {
                let _ = validate_repository_file(&root, &evidence.path, &tracked_files)?;
            }
        }
        for override_text in &client.known_overrides {
            ensure!(
                !override_text.trim().is_empty(),
                "{} contains an empty override",
                client.id
            );
        }
        if client.synthetic_profile {
            ensure!(
                evidence_kinds.contains("protocol_profile"),
                "{} declares a synthetic profile without protocol-profile evidence",
                client.id
            );
        }

        match client.tier.as_str() {
            "packaged_product_proven" => {
                ensure!(
                    !client.requires_actual_client_receipt,
                    "{} is promoted while still declaring its actual-client receipt missing",
                    client.id
                );
                ensure!(
                    !client.synthetic_profile,
                    "{} cannot use a synthetic profile to satisfy packaged product proof",
                    client.id
                );
                ensure!(
                    validated_execution_kinds.contains("packaged_product"),
                    "{} packaged-product tier lacks a validated packaged-product receipt",
                    client.id
                );
            }
            "real_generic_client_proven" => {
                ensure!(
                    !client.requires_actual_client_receipt,
                    "{} is promoted while still declaring its actual-client receipt missing",
                    client.id
                );
                ensure!(
                    !client.synthetic_profile,
                    "{} cannot use a synthetic profile to satisfy actual-client proof",
                    client.id
                );
                ensure!(
                    validated_execution_kinds.contains("actual_client"),
                    "{} real-client tier lacks a validated actual-client receipt",
                    client.id
                );
            }
            "protocol_profile_proven" => {
                ensure!(
                    client.synthetic_profile,
                    "{} protocol-profile tier lacks a declared synthetic profile",
                    client.id
                );
                ensure!(
                    evidence_kinds.contains("protocol_profile"),
                    "{} protocol-profile tier lacks protocol-profile evidence",
                    client.id
                );
            }
            "bridge_or_plugin_dependency" => {
                ensure!(
                    !client.external_dependency.trim().is_empty(),
                    "{} bridge/plugin tier must name the dependency",
                    client.id
                );
                ensure!(
                    evidence_kinds.contains("documentation"),
                    "{} bridge/plugin tier lacks integration documentation",
                    client.id
                );
            }
            "configuration_documented" => {
                ensure!(
                    evidence_kinds.contains("documentation"),
                    "{} configuration tier lacks documentation evidence",
                    client.id
                );
            }
            "not_proven_unsupported" => {}
            other => bail!("unhandled client tier {other}"),
        }
    }
    Ok(())
}

#[test]
fn bridge_boundaries_do_not_misrepresent_lsp_as_mcp_or_native_zed_registration() -> Result<()> {
    let registry = load_registry(&workspace_root()?)?;
    let codex = registry
        .client
        .iter()
        .find(|client| client.id == "codex_cli")
        .context("missing Codex CLI row")?;
    ensure!(codex.integration_mode == "lsp_to_mcp_bridge");
    ensure!(codex.tier == "bridge_or_plugin_dependency");
    ensure!(codex.claim_boundary.contains("unsupported"));

    let zed = registry
        .client
        .iter()
        .find(|client| client.id == "zed")
        .context("missing Zed row")?;
    ensure!(
        zed.integration_mode == "extension_registered_language_server",
        "Zed must remain an extension-registered language-server integration"
    );
    ensure!(zed.tier == "bridge_or_plugin_dependency");
    ensure!(zed.claim_boundary.contains("extension"));
    Ok(())
}

#[test]
fn generated_client_status_is_fresh() -> Result<()> {
    let root = workspace_root()?;
    let registry = load_registry(&root)?;
    let expected = render_status(&registry);
    let status_path = root.join("docs/project/status/lsp_clients.md");
    let actual = fs::read_to_string(&status_path)
        .with_context(|| format!("read generated client status at {}", status_path.display()))?;
    ensure!(
        actual.replace("\r\n", "\n") == expected,
        "docs/project/status/lsp_clients.md is stale; regenerate it from policy/lsp-client-support.toml"
    );
    Ok(())
}

#[test]
fn repository_paths_reject_absolute_traversal_and_untracked_files() -> Result<()> {
    let absolute = std::env::current_exe().context("resolve current test executable")?;
    ensure!(
        validate_relative_path(&absolute.to_string_lossy()).is_err(),
        "absolute path was accepted"
    );
    ensure!(
        validate_relative_path("../outside.json").is_err(),
        "parent traversal was accepted"
    );
    ensure!(
        validate_relative_path("docs/./receipt.json").is_err(),
        "current-directory traversal was accepted"
    );

    let root = workspace_root()?;
    let tracked_files = load_tracked_files(&root)?;
    let untracked = tempfile::NamedTempFile::new_in(&root)
        .context("create untracked evidence negative control")?;
    let relative = untracked
        .path()
        .strip_prefix(&root)
        .context("derive untracked evidence path")?;
    ensure!(
        validate_repository_file(
            &root,
            &repository_path_string(relative),
            &tracked_files,
        )
        .is_err(),
        "untracked regular file was accepted as evidence"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn repository_paths_reject_symlink_escape() -> Result<()> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().context("create path-validation root")?;
    let outside = tempfile::tempdir().context("create outside directory")?;
    let outside_file = outside.path().join("receipt.json");
    fs::write(&outside_file, "{}\n").context("write outside receipt")?;
    symlink(&outside_file, root.path().join("escape.json"))
        .context("create escaping symlink")?;

    ensure!(
        resolve_repository_file(root.path(), "escape.json").is_err(),
        "symlink escape was accepted"
    );
    Ok(())
}

fn write_receipt(path: &Path, value: &serde_json::Value) -> Result<()> {
    let encoded = serde_json::to_vec_pretty(value).context("encode receipt fixture")?;
    fs::write(path, encoded)
        .with_context(|| format!("write receipt fixture {}", path.display()))
}

fn valid_receipt_value(
    client_id: &str,
    evidence_kind: &str,
    artifact: Option<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "schema": EXECUTABLE_RECEIPT_SCHEMA,
        "client_id": client_id,
        "evidence_kind": evidence_kind,
        "recorded_at_utc": "2026-08-11T20:00:00Z",
        "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "client_executable": "client-under-test",
        "client_version": "1.0.0",
        "server_command": ["perllsp", "--stdio"],
        "transport": "stdio",
        "exit_code": 0,
        "stdout_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "stderr_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "assertions": [
            {
                "name": "initialize_shutdown_exit",
                "passed": true,
                "evidence": "transcript.jsonl#lifecycle"
            }
        ],
        "artifact": artifact
    })
}

#[test]
fn executable_receipt_schema_accepts_actual_client_and_rejects_relabeling() -> Result<()> {
    let directory = tempfile::tempdir().context("create receipt fixture directory")?;
    let path = directory.path().join("actual.json");
    let value = valid_receipt_value("neovim", "actual_client", None);
    write_receipt(&path, &value)?;
    validate_execution_receipt(&path, "neovim", "actual_client")?;

    ensure!(
        validate_execution_receipt(&path, "helix", "actual_client").is_err(),
        "receipt was accepted for a different client"
    );
    ensure!(
        validate_execution_receipt(&path, "neovim", "packaged_product").is_err(),
        "actual-client receipt was relabeled as packaged-product proof"
    );
    ensure!(
        validate_execution_receipt_path(
            "Cargo.toml",
            "actual_client",
        )
        .is_err(),
        "arbitrary repository file was accepted as executable evidence"
    );
    Ok(())
}

#[test]
fn executable_receipt_schema_rejects_failed_assertions_and_missing_product_artifact() -> Result<()> {
    let directory = tempfile::tempdir().context("create receipt fixture directory")?;
    let failed_path = directory.path().join("failed.json");
    let mut failed = valid_receipt_value("neovim", "actual_client", None);
    let assertion = failed
        .get_mut("assertions")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|assertions| assertions.first_mut())
        .context("receipt fixture omitted assertion")?;
    assertion["passed"] = serde_json::json!(false);
    write_receipt(&failed_path, &failed)?;
    ensure!(
        validate_execution_receipt(&failed_path, "neovim", "actual_client").is_err(),
        "receipt with a failed assertion was accepted"
    );

    let packaged_path = directory.path().join("packaged.json");
    let packaged = valid_receipt_value("vscode", "packaged_product", None);
    write_receipt(&packaged_path, &packaged)?;
    ensure!(
        validate_execution_receipt(&packaged_path, "vscode", "packaged_product").is_err(),
        "packaged-product receipt without an artifact was accepted"
    );

    let valid_packaged = valid_receipt_value(
        "vscode",
        "packaged_product",
        Some(serde_json::json!({
            "path": "dist/perllsp.vsix",
            "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "size_bytes": 1024
        })),
    );
    write_receipt(&packaged_path, &valid_packaged)?;
    validate_execution_receipt(&packaged_path, "vscode", "packaged_product")?;
    Ok(())
}
