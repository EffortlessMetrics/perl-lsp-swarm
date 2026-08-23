//! Drift, path-safety, and claim-boundary checks for
//! `policy/lsp-client-support.toml`.

use anyhow::{Context, Result, bail, ensure};
use chrono::DateTime;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const EXECUTABLE_RECEIPT_SCHEMA: &str = "lsp-client-execution-receipt.v1";
const ACTUAL_CLIENT_RECEIPT_ROOT: &str = "docs/receipts/lsp-clients/actual";
const PACKAGED_PRODUCT_RECEIPT_ROOT: &str = "docs/receipts/lsp-clients/packaged";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    meta: Meta,
    client: Vec<Client>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    schema: String,
    source_document: String,
    owner_issue: u64,
    allowed_tiers: Vec<String>,
    allowed_evidence_kinds: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct Evidence {
    path: String,
    kind: String,
    #[serde(default)]
    profile: Option<ProfileEvidence>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileEvidence {
    client_id: String,
    scenario: String,
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
    transcript_path: String,
    transcript_sha256: String,
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

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ReceiptArtifact {
    path: String,
    sha256: String,
    size_bytes: u64,
}

const REQUIRED_EXECUTION_ASSERTIONS: &[&str] =
    &["initialize", "request_response", "clean_shutdown"];
const TRANSCRIPT_ROOT: &str = "docs/receipts/lsp-clients";

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
    ensure!(
        !raw.replace('\\', "/").split('/').any(|component| component == "."),
        "repository path must not contain '.': {raw}"
    );
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
    ensure!(!normalized.as_os_str().is_empty(), "repository path must name a file: {raw}");
    Ok(normalized)
}

fn repository_path_string(path: &Path) -> String {
    path.iter().map(|part| part.to_string_lossy()).collect::<Vec<_>>().join("/")
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

fn validate_protocol_profile_evidence(
    root: &Path,
    raw: &str,
    tracked_files: &BTreeSet<String>,
    client_id: &str,
    profile: &ProfileEvidence,
) -> Result<()> {
    ensure!(
        profile.client_id == client_id,
        "protocol-profile evidence names client {}, expected {}",
        profile.client_id,
        client_id
    );
    ensure!(!profile.scenario.trim().is_empty(), "protocol-profile scenario must not be empty");
    let relative = validate_relative_path(raw)?;
    ensure!(
        relative.extension().and_then(|extension| extension.to_str()) == Some("rs"),
        "protocol-profile evidence must be a Rust test source: {raw}"
    );
    ensure!(
        relative.components().any(|component| component.as_os_str() == "tests"),
        "protocol-profile evidence must live under a tests directory: {raw}"
    );
    let path = validate_repository_file(root, raw, tracked_files)?;
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read protocol-profile evidence {}", path.display()))?;
    let source_lower = source.to_ascii_lowercase();
    let client_marker = match client_id {
        "intellij_lsp4ij" => "lsp4ij",
        other => other,
    };
    ensure!(
        source_lower.contains(client_marker),
        "protocol-profile evidence {} is not bound to client {}",
        raw,
        client_id
    );
    for scenario_marker in profile.scenario.split('_').filter(|marker| !marker.is_empty()) {
        let scenario_marker = scenario_marker.to_ascii_lowercase();
        ensure!(
            source_lower.contains(&scenario_marker),
            "protocol-profile evidence {} does not identify scenario {}",
            raw,
            profile.scenario
        );
    }
    ensure!(
        source.contains("#[test]"),
        "protocol-profile evidence must define at least one Rust test: {raw}"
    );
    ensure!(
        ["initialize", "capabilit", "stdio"].iter().any(|marker| source.contains(marker)),
        "protocol-profile evidence must exercise a recognizable LSP protocol seam: {raw}"
    );
    Ok(())
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
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_artifact_metadata(artifact: &ReceiptArtifact) -> Result<()> {
    let _ = validate_relative_path(&artifact.path)
        .context("packaged-product artifact path must be a bounded relative locator")?;
    ensure!(
        is_lower_hex(&artifact.sha256, 64),
        "artifact sha256 must be 64 lowercase hexadecimal characters"
    );
    ensure!(artifact.size_bytes > 0, "packaged-product artifact must have a non-zero size");
    Ok(())
}

fn validate_artifact(
    root: &Path,
    tracked_files: &BTreeSet<String>,
    artifact: &ReceiptArtifact,
) -> Result<()> {
    validate_artifact_metadata(artifact)?;
    let artifact_path = validate_repository_file(root, &artifact.path, tracked_files)?;
    let bytes = fs::read(&artifact_path)
        .with_context(|| format!("read packaged-product artifact {}", artifact.path))?;
    ensure!(
        bytes.len() as u64 == artifact.size_bytes,
        "packaged-product artifact size mismatch for {}: receipt={}, actual={}",
        artifact.path,
        artifact.size_bytes,
        bytes.len()
    );
    ensure!(
        sha256_hex(&bytes) == artifact.sha256,
        "packaged-product artifact digest mismatch for {}",
        artifact.path
    );
    Ok(())
}

fn validate_source_revision(root: &Path, source_revision: &str) -> Result<()> {
    ensure!(
        is_lower_hex(source_revision, 40),
        "receipt source_revision must be a 40-character lowercase Git SHA"
    );
    let revision = format!("{source_revision}^{{commit}}");
    let resolved = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(&revision)
        .output()
        .context("resolve receipt source revision")?;
    ensure!(
        resolved.status.success(),
        "receipt source_revision is not a reachable commit: {source_revision}"
    );
    ensure!(
        String::from_utf8_lossy(&resolved.stdout).trim() == source_revision,
        "receipt source_revision did not resolve to its canonical commit: {source_revision}"
    );
    let head = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .context("resolve current candidate revision")?;
    ensure!(head.status.success(), "current candidate revision could not be resolved");
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    let ancestry = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", source_revision, &head])
        .output()
        .context("check receipt source revision ancestry")?;
    ensure!(
        ancestry.status.success(),
        "receipt source_revision must be an ancestor of the current candidate: {source_revision}"
    );
    Ok(())
}

fn validate_transcript(
    root: &Path,
    tracked_files: &BTreeSet<String>,
    receipt: &ExecutionReceipt,
) -> Result<()> {
    let transcript_relative = validate_relative_path(&receipt.transcript_path)?;
    ensure!(
        transcript_relative.starts_with(Path::new(TRANSCRIPT_ROOT)),
        "transcript must live under {TRANSCRIPT_ROOT}: {}",
        receipt.transcript_path
    );
    ensure!(
        transcript_relative.extension().and_then(|extension| extension.to_str()) == Some("jsonl"),
        "transcript must be a JSONL file: {}",
        receipt.transcript_path
    );
    let transcript_path = validate_repository_file(root, &receipt.transcript_path, tracked_files)?;
    let bytes = fs::read(&transcript_path)
        .with_context(|| format!("read execution transcript {}", receipt.transcript_path))?;
    ensure!(
        sha256_hex(&bytes) == receipt.transcript_sha256,
        "execution transcript digest mismatch for {}",
        receipt.transcript_path
    );
    let transcript = String::from_utf8(bytes).context("execution transcript must be UTF-8")?;
    let mut event_names = BTreeSet::<String>::new();
    for line in transcript.lines().filter(|line| !line.trim().is_empty()) {
        let event: serde_json::Value =
            serde_json::from_str(line).context("execution transcript must be JSONL")?;
        let name = event
            .get("event")
            .and_then(serde_json::Value::as_str)
            .context("execution transcript events must name an event")?
            .to_owned();
        ensure!(
            REQUIRED_EXECUTION_ASSERTIONS.contains(&name.as_str()),
            "execution transcript contains an undeclared event {name}"
        );
        ensure!(event_names.insert(name.clone()), "execution transcript repeats event {name}");
    }
    for required_event in REQUIRED_EXECUTION_ASSERTIONS {
        ensure!(
            event_names.contains(*required_event),
            "execution transcript is missing required event {required_event}"
        );
    }
    for assertion in &receipt.assertions {
        ensure!(
            event_names.contains(assertion.name.as_str()),
            "execution transcript does not contain assertion {}",
            assertion.name
        );
    }
    Ok(())
}

fn validate_execution_receipt(
    path: &Path,
    expected_client_id: &str,
    expected_kind: &str,
) -> Result<ExecutionReceipt> {
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
    let timestamp = DateTime::parse_from_rfc3339(&receipt.recorded_at_utc).with_context(|| {
        format!("receipt recorded_at_utc is not RFC3339: {}", receipt.recorded_at_utc)
    })?;
    ensure!(
        receipt.recorded_at_utc.ends_with('Z') && timestamp.offset().local_minus_utc() == 0,
        "receipt recorded_at_utc must be an RFC3339 UTC timestamp ending in Z"
    );
    ensure!(
        is_lower_hex(&receipt.source_revision, 40),
        "receipt source_revision must be a 40-character lowercase Git SHA"
    );
    ensure!(
        !receipt.client_executable.trim().is_empty(),
        "receipt client_executable must not be empty"
    );
    ensure!(!receipt.client_version.trim().is_empty(), "receipt client_version must not be empty");
    ensure!(
        !receipt.server_command.is_empty()
            && receipt.server_command.iter().all(|argument| !argument.trim().is_empty()),
        "receipt server_command must contain non-empty arguments"
    );
    let launched =
        receipt.server_command.first().context("receipt server_command must name an executable")?;
    let executable = launched.replace('\\', "/");
    let executable = executable.rsplit('/').next().unwrap_or(executable.as_str());
    let executable = executable.strip_suffix(".exe").unwrap_or(executable);
    ensure!(
        executable.eq_ignore_ascii_case("perllsp")
            && receipt.server_command.iter().skip(1).any(|argument| argument == "--stdio"),
        "receipt server_command must launch perllsp with --stdio"
    );
    ensure!(
        is_lower_hex(&receipt.transcript_sha256, 64),
        "transcript_sha256 must be 64 lowercase hexadecimal characters"
    );
    let _ = validate_relative_path(&receipt.transcript_path)
        .context("transcript_path must be a bounded relative locator")?;
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
    ensure!(!receipt.assertions.is_empty(), "executable client receipt must contain assertions");

    let mut assertion_names = BTreeSet::new();
    for assertion in &receipt.assertions {
        ensure!(!assertion.name.trim().is_empty(), "receipt assertion name must not be empty");
        ensure!(
            assertion_names.insert(assertion.name.as_str()),
            "receipt repeats assertion {}",
            assertion.name
        );
        ensure!(assertion.passed, "receipt assertion failed: {}", assertion.name);
        ensure!(
            !assertion.evidence.trim().is_empty(),
            "receipt assertion {} has no evidence locator",
            assertion.name
        );
        ensure!(
            assertion.evidence == format!("{}#{}", receipt.transcript_path, assertion.name),
            "receipt assertion {} must point to its named transcript event",
            assertion.name
        );
    }
    let required_assertions =
        REQUIRED_EXECUTION_ASSERTIONS.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        assertion_names == required_assertions,
        "receipt assertions must be exactly {REQUIRED_EXECUTION_ASSERTIONS:?}, got {assertion_names:?}"
    );

    if expected_kind == "packaged_product" {
        ensure!(receipt.artifact.is_some(), "packaged-product receipt must include an artifact");
    }
    Ok(receipt)
}

fn validate_executable_receipt(
    root: &Path,
    tracked_files: &BTreeSet<String>,
    path: &Path,
    expected_client_id: &str,
    expected_kind: &str,
) -> Result<()> {
    let receipt = validate_execution_receipt(path, expected_client_id, expected_kind)?;
    validate_source_revision(root, &receipt.source_revision)?;
    validate_transcript(root, tracked_files, &receipt)?;
    if let Some(artifact) = &receipt.artifact {
        validate_artifact(root, tracked_files, artifact)?;
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
            matches!(client.tier.as_str(), "packaged_product_proven" | "real_generic_client_proven")
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

    let docs_path =
        validate_repository_file(&root, &registry.meta.source_document, &tracked_files)?;
    let docs = fs::read_to_string(&docs_path)
        .with_context(|| format!("read editor setup guide at {}", docs_path.display()))?;
    let documented_rows = editor_table_names(&docs);
    ensure!(!documented_rows.is_empty(), "editor setup table was not found or was empty");
    let documented = documented_rows.iter().cloned().collect::<BTreeSet<_>>();
    ensure!(
        documented.len() == documented_rows.len(),
        "editor setup table contains duplicate client names: {documented_rows:?}"
    );

    let registered_rows =
        registry.client.iter().map(|client| client.display_name.clone()).collect::<Vec<_>>();
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
    let allowed_tiers =
        registry.meta.allowed_tiers.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let allowed_evidence =
        registry.meta.allowed_evidence_kinds.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();

    for client in &registry.client {
        ensure!(!client.id.is_empty(), "client ID must not be empty");
        ensure!(ids.insert(client.id.as_str()), "duplicate client ID: {}", client.id);
        ensure!(
            allowed_tiers.contains(client.tier.as_str()),
            "unknown tier for {}: {}",
            client.id,
            client.tier
        );
        ensure!(client.owner_issue > 0, "{} must name an owning issue", client.id);
        ensure!(
            !client.claim_boundary.trim().is_empty(),
            "{} must state a claim boundary",
            client.id
        );
        ensure!(!client.evidence.is_empty(), "{} must name typed evidence", client.id);

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
                validate_execution_receipt_path(&evidence.path, &evidence.kind)?;
                let receipt_path = validate_repository_file(&root, &evidence.path, &tracked_files)?;
                validate_executable_receipt(
                    &root,
                    &tracked_files,
                    &receipt_path,
                    &client.id,
                    &evidence.kind,
                )?;
                validated_execution_kinds.insert(evidence.kind.as_str());
            } else if evidence.kind == "protocol_profile" {
                let profile = evidence.profile.as_ref().context(format!(
                    "{} protocol-profile evidence must name its client/profile binding",
                    client.id
                ))?;
                validate_protocol_profile_evidence(
                    &root,
                    &evidence.path,
                    &tracked_files,
                    &client.id,
                    profile,
                )?;
            } else {
                ensure!(
                    evidence.profile.is_none(),
                    "{} non-profile evidence must not carry a profile binding",
                    client.id
                );
                let _ = validate_repository_file(&root, &evidence.path, &tracked_files)?;
            }
        }
        for override_text in &client.known_overrides {
            ensure!(!override_text.trim().is_empty(), "{} contains an empty override", client.id);
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
fn protocol_profiles_are_bound_to_the_named_client_and_scenario() -> Result<()> {
    let root = workspace_root()?;
    let tracked_files = load_tracked_files(&root)?;
    let intellij_profile = ProfileEvidence {
        client_id: "intellij_lsp4ij".to_string(),
        scenario: "lsp4ij_inline_completion".to_string(),
    };
    validate_protocol_profile_evidence(
        &root,
        "crates/perl-lsp-rs/tests/lsp_inline_completion_registration_tests.rs",
        &tracked_files,
        "intellij_lsp4ij",
        &intellij_profile,
    )?;
    ensure!(
        validate_protocol_profile_evidence(
            &root,
            "crates/perl-lsp-rs/tests/lsp_inline_completion_registration_tests.rs",
            &tracked_files,
            "neovim",
            &ProfileEvidence {
                client_id: "neovim".to_string(),
                scenario: "neovim_lean_startup".to_string(),
            },
        )
        .is_err(),
        "a profile source must not be relabeled for another client"
    );
    Ok(())
}

#[test]
fn registry_schema_rejects_unknown_persisted_fields() -> Result<()> {
    let source = r#"
schema = "lsp-client-support.v1"
source_document = "docs/how-to/EDITOR_SETUP.md"
owner_issue = 6739
allowed_tiers = []
allowed_evidence_kinds = []
unexpected_future_claim = true
"#;
    ensure!(
        toml::from_str::<Meta>(source).is_err(),
        "registry metadata accepted an undeclared field"
    );
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

    let codex_desktop = registry
        .client
        .iter()
        .find(|client| client.id == "codex_desktop")
        .context("missing Codex Desktop row")?;
    ensure!(codex_desktop.tier == "not_proven_unsupported");
    ensure!(codex_desktop.claim_boundary.contains("unsupported"));

    let zed =
        registry.client.iter().find(|client| client.id == "zed").context("missing Zed row")?;
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
    ensure!(validate_relative_path("../outside.json").is_err(), "parent traversal was accepted");
    ensure!(
        validate_relative_path("docs/./receipt.json").is_err(),
        "current-directory traversal was accepted"
    );

    let root = workspace_root()?;
    let tracked_files = load_tracked_files(&root)?;
    let empty_tracked_files = BTreeSet::new();
    ensure!(
        validate_repository_file(&root, "Cargo.toml", &empty_tracked_files).is_err(),
        "a file absent from the tracked-file set was accepted as evidence"
    );
    ensure!(
        validate_protocol_profile_evidence(
            &root,
            "Cargo.toml",
            &tracked_files,
            "neovim",
            &ProfileEvidence {
                client_id: "neovim".to_string(),
                scenario: "lean_startup".to_string(),
            },
        )
        .is_err(),
        "an arbitrary tracked manifest was accepted as protocol-profile evidence"
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
    symlink(&outside_file, root.path().join("escape.json")).context("create escaping symlink")?;

    ensure!(
        resolve_repository_file(root.path(), "escape.json").is_err(),
        "symlink escape was accepted"
    );
    Ok(())
}

fn write_receipt(path: &Path, value: &serde_json::Value) -> Result<()> {
    let encoded = serde_json::to_vec_pretty(value).context("encode receipt fixture")?;
    fs::write(path, encoded).with_context(|| format!("write receipt fixture {}", path.display()))
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
        "transcript_path": "docs/receipts/lsp-clients/actual/transcript.jsonl",
        "transcript_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "transport": "stdio",
        "exit_code": 0,
        "stdout_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "stderr_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "assertions": [
            {"name": "initialize", "passed": true, "evidence": "docs/receipts/lsp-clients/actual/transcript.jsonl#initialize"},
            {"name": "request_response", "passed": true, "evidence": "docs/receipts/lsp-clients/actual/transcript.jsonl#request_response"},
            {"name": "clean_shutdown", "passed": true, "evidence": "docs/receipts/lsp-clients/actual/transcript.jsonl#clean_shutdown"}
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
        validate_execution_receipt_path("Cargo.toml", "actual_client",).is_err(),
        "arbitrary repository file was accepted as executable evidence"
    );
    let mut malformed_timestamp = value.clone();
    malformed_timestamp["recorded_at_utc"] = serde_json::json!("2026-99-99TZ");
    write_receipt(&path, &malformed_timestamp)?;
    ensure!(
        validate_execution_receipt(&path, "neovim", "actual_client").is_err(),
        "malformed UTC timestamp was accepted"
    );

    let mut arbitrary_command = value.clone();
    arbitrary_command["server_command"] = serde_json::json!(["echo", "--stdio"]);
    write_receipt(&path, &arbitrary_command)?;
    ensure!(
        validate_execution_receipt(&path, "neovim", "actual_client").is_err(),
        "arbitrary server command was accepted"
    );
    Ok(())
}

#[test]
fn executable_receipt_schema_rejects_failed_assertions_and_missing_product_artifact() -> Result<()>
{
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

    let mut invented_assertion = valid_receipt_value("neovim", "actual_client", None);
    invented_assertion["assertions"] = serde_json::json!([{
        "name": "process_started",
        "passed": true,
        "evidence": "docs/receipts/lsp-clients/actual/transcript.jsonl#process_started"
    }]);
    write_receipt(&failed_path, &invented_assertion)?;
    ensure!(
        validate_execution_receipt(&failed_path, "neovim", "actual_client").is_err(),
        "invented assertion set was accepted"
    );

    let mut unknown_receipt_field = valid_receipt_value("neovim", "actual_client", None);
    unknown_receipt_field["unreviewed_claim"] = serde_json::json!(true);
    write_receipt(&failed_path, &unknown_receipt_field)?;
    ensure!(
        validate_execution_receipt(&failed_path, "neovim", "actual_client").is_err(),
        "receipt accepted an undeclared field"
    );
    Ok(())
}

#[test]
fn executable_subjects_require_reachable_source_and_bound_bytes() -> Result<()> {
    let root = workspace_root()?;
    let head = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("resolve current test candidate")?;
    ensure!(head.status.success(), "failed to resolve current test candidate");
    let head = String::from_utf8(head.stdout).context("candidate revision was not UTF-8")?;
    validate_source_revision(&root, head.trim())?;
    ensure!(
        validate_source_revision(&root, &"a".repeat(40)).is_err(),
        "unreachable source revision was accepted"
    );

    let fixture_root = tempfile::tempdir().context("create executable subject fixture")?;
    let artifact_path = fixture_root.path().join("dist/perllsp.vsix");
    fs::create_dir_all(artifact_path.parent().context("artifact fixture has no parent")?)?;
    let artifact_bytes = b"packaged-candidate";
    fs::write(&artifact_path, artifact_bytes)?;
    let tracked_files = ["dist/perllsp.vsix".to_string()].into_iter().collect();
    let artifact = ReceiptArtifact {
        path: "dist/perllsp.vsix".to_string(),
        sha256: sha256_hex(artifact_bytes),
        size_bytes: artifact_bytes.len() as u64,
    };
    validate_artifact(fixture_root.path(), &tracked_files, &artifact)?;
    let mut wrong_size = artifact.clone();
    wrong_size.size_bytes += 1;
    ensure!(
        validate_artifact(fixture_root.path(), &tracked_files, &wrong_size).is_err(),
        "artifact with an incorrect size was accepted"
    );
    let mut wrong_digest = artifact.clone();
    wrong_digest.sha256 = "a".repeat(64);
    ensure!(
        validate_artifact(fixture_root.path(), &tracked_files, &wrong_digest).is_err(),
        "artifact with an incorrect digest was accepted"
    );
    let missing = ReceiptArtifact { path: "dist/missing.vsix".to_string(), ..artifact };
    ensure!(
        validate_artifact(fixture_root.path(), &tracked_files, &missing).is_err(),
        "missing artifact was accepted"
    );
    Ok(())
}

#[test]
fn executable_transcripts_require_existing_matching_bytes() -> Result<()> {
    let root = tempfile::tempdir().context("create transcript fixture")?;
    let transcript_path = root.path().join("docs/receipts/lsp-clients/actual/transcript.jsonl");
    fs::create_dir_all(transcript_path.parent().context("transcript fixture has no parent")?)?;
    let transcript = concat!(
        "{\"event\":\"initialize\"}\n",
        "{\"event\":\"request_response\"}\n",
        "{\"event\":\"clean_shutdown\"}\n"
    );
    fs::write(&transcript_path, transcript)?;
    let tracked_files =
        ["docs/receipts/lsp-clients/actual/transcript.jsonl".to_string()].into_iter().collect();
    let mut value = valid_receipt_value("neovim", "actual_client", None);
    value["transcript_sha256"] = serde_json::json!(sha256_hex(transcript.as_bytes()));
    let receipt_path = root.path().join("receipt.json");
    write_receipt(&receipt_path, &value)?;
    let receipt = validate_execution_receipt(&receipt_path, "neovim", "actual_client")?;
    validate_transcript(root.path(), &tracked_files, &receipt)?;
    value["transcript_sha256"] = serde_json::json!("a".repeat(64));
    write_receipt(&receipt_path, &value)?;
    let receipt = validate_execution_receipt(&receipt_path, "neovim", "actual_client")?;
    ensure!(
        validate_transcript(root.path(), &tracked_files, &receipt).is_err(),
        "transcript with an incorrect digest was accepted"
    );
    Ok(())
}
