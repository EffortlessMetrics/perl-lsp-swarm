//! `cargo xtask publication-sync project` — deterministic producer for the
//! projected publication tree `P`.
//!
//! Takes immutable publication base `R`, prepared swarm source `S`,
//! blocker-free reconciliation evidence, and a reviewed projection manifest.
//! Starts from the complete tree of `S`, applies only manifest-owned
//! changes, and builds the core join `J0` and PR head `J` with one fixed
//! commit identity. Generation is repeated internally; any difference
//! between the two runs, any unlisted `S`→`P` difference, stale input, or
//! mismatched identity fails closed. No ref, branch, tag, or working tree
//! is mutated: all construction uses Git plumbing with a temporary index.

use color_eyre::eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INPUT_SCHEMA_VERSION: &str = "publication_sync_input.v1";
const DEFAULT_PACKET_SCHEMA_VERSION: &str = "v0.18_publication_sync.v2";
const DEFAULT_LEDGER_SCHEMA_VERSION: &str = "v0.18_reconciliation.v1";
const DEFAULT_MANIFEST_SCHEMA_VERSION: &str = "v0.18_publication_sync_manifest.v1";
const CONTROL_DIR: &str = ".github/publication-sync/";
const REQUIRED_INVARIANT_IDS: [&str; 5] =
    ["destination-context", "version", "topology", "artifact-reachability", "effective-identity"];

/// Arguments for the deterministic projection producer.
pub struct ProjectConfig {
    /// Exact publication base commit-ish (`R`).
    pub base: String,
    /// Exact prepared swarm commit-ish (`S`).
    pub source: String,
    /// Reviewed input projection manifest (JSON).
    pub manifest: PathBuf,
    /// Blocker-free reconciliation ledger; its canonical bytes are
    /// committed as `.github/publication-sync/reconciliation-ledger.json`.
    pub ledger: PathBuf,
    /// Reviewed commit message file used for both `J0` and `J`.
    pub message_file: PathBuf,
    /// Output directory for packet, committed manifest, ledger, receipt.
    pub out_dir: PathBuf,
    /// Fixed author/committer name for `J0`/`J`.
    pub author_name: String,
    /// Fixed author/committer email for `J0`/`J`.
    pub author_email: String,
    /// Fixed author/committer timestamp (Unix seconds, UTC).
    pub timestamp: i64,
    /// Repository to run against; defaults to the process working directory.
    pub working_directory: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputManifest {
    schema_version: String,
    release: String,
    #[serde(default)]
    packet_schema_version: Option<String>,
    #[serde(default)]
    ledger_schema_version: Option<String>,
    #[serde(default)]
    manifest_schema_version: Option<String>,
    #[serde(default)]
    translations: Vec<Translation>,
    #[serde(default)]
    exclusions: Vec<String>,
    required_invariants: Vec<Invariant>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Translation {
    from: String,
    to: String,
    #[serde(default)]
    content: ContentSource,
    /// Optional sha256 (hex, unprefixed) of the blob at `from` in `S`;
    /// when present, a stale `S` fails closed.
    #[serde(default)]
    source_sha256: Option<String>,
    /// Optional sha256 (hex, unprefixed) of the resulting blob at `to`;
    /// when present, unexpected content fails closed.
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ContentSource {
    /// Carry the blob (and mode) found at `from` in `S`.
    Swarm,
    /// Take the blob at `path` (default: `to`) from `R`.
    Release { path: Option<String> },
    /// Take an existing blob object by id.
    Blob { sha: String },
    /// Use literal UTF-8 text as the new blob.
    Inline { text: String },
}

impl Default for ContentSource {
    fn default() -> Self {
        Self::Swarm
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invariant {
    id: String,
    status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evidence: Option<String>,
}

#[derive(Debug, Serialize)]
struct CommittedManifest {
    schema_version: String,
    release: String,
    translations: Vec<CommittedTranslation>,
    exclusions: Vec<String>,
    required_invariants: Vec<Invariant>,
}

#[derive(Debug, Serialize)]
struct CommittedTranslation {
    from: String,
    to: String,
}

#[derive(Debug, Serialize)]
struct Packet {
    schema_version: String,
    release: String,
    release_base_sha: String,
    prepared_swarm_sha: String,
    sync_join_sha: String,
    expected_join_parents: Vec<String>,
    reconciliation_digest: String,
    publication_sync_manifest_digest: String,
    expected_projected_tree: String,
    published_channels: Vec<String>,
    release_cut: bool,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: String,
    release: String,
    release_base_sha: String,
    prepared_swarm_sha: String,
    expected_projected_tree: String,
    core_sync_join_sha: String,
    pr_head_join_sha: String,
    reconciliation_digest: String,
    publication_sync_manifest_digest: String,
    listed_paths: Vec<String>,
    published_channels: Vec<String>,
    release_cut: bool,
    deterministic_double_generation: bool,
}

struct Generation {
    projected_tree: String,
    core_join: String,
    pr_head: String,
    packet_bytes: Vec<u8>,
    manifest_bytes: Vec<u8>,
    ledger_bytes: Vec<u8>,
    listed_paths: Vec<String>,
}

/// Produce `P`, `J0`, and `J` deterministically and write the packet,
/// committed manifest, ledger, and receipt into `out_dir`.
pub fn project(config: ProjectConfig) -> Result<()> {
    let repo = config.working_directory.clone().unwrap_or_else(|| PathBuf::from("."));
    let base = resolve_commit(&repo, &config.base, "base (R)")?;
    let source = resolve_commit(&repo, &config.source, "source (S)")?;
    if base == source {
        return Err(eyre!("base (R) and source (S) must differ"));
    }
    let manifest_raw = fs::read(&config.manifest)
        .with_context(|| format!("reading input manifest {}", config.manifest.display()))?;
    let input: InputManifest =
        serde_json::from_slice(&manifest_raw).context("parsing input projection manifest")?;
    validate_input(&input)?;
    let ledger_raw = fs::read(&config.ledger)
        .with_context(|| format!("reading reconciliation ledger {}", config.ledger.display()))?;
    let ledger_bytes = canonical_ledger(&ledger_raw, &input)?;
    let message = fs::read(&config.message_file)
        .with_context(|| format!("reading message file {}", config.message_file.display()))?;
    if message.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Err(eyre!("commit message file must not be empty"));
    }

    let first = generate(&repo, &config, &input, &base, &source, &ledger_bytes, &message)?;
    let second = generate(&repo, &config, &input, &base, &source, &ledger_bytes, &message)?;
    if first.projected_tree != second.projected_tree
        || first.core_join != second.core_join
        || first.pr_head != second.pr_head
        || first.packet_bytes != second.packet_bytes
        || first.manifest_bytes != second.manifest_bytes
    {
        return Err(eyre!("projection is not deterministic: repeated generation differed"));
    }
    verify_against_contract(&repo, &source, &first)?;

    for name in ["packet.yaml", "projection-manifest.json", "reconciliation-ledger.json"] {
        if config.out_dir.join(name).exists() {
            return Err(eyre!(
                "refusing to overwrite existing {} in {}",
                name,
                config.out_dir.display()
            ));
        }
    }
    fs::create_dir_all(&config.out_dir)
        .with_context(|| format!("creating output directory {}", config.out_dir.display()))?;
    fs::write(config.out_dir.join("packet.yaml"), &first.packet_bytes)?;
    fs::write(config.out_dir.join("projection-manifest.json"), &first.manifest_bytes)?;
    fs::write(config.out_dir.join("reconciliation-ledger.json"), &first.ledger_bytes)?;
    let receipt = Receipt {
        schema_version: "publication_sync_producer_receipt.v1".to_string(),
        release: input.release.clone(),
        release_base_sha: base,
        prepared_swarm_sha: source,
        expected_projected_tree: first.projected_tree.clone(),
        core_sync_join_sha: first.core_join.clone(),
        pr_head_join_sha: first.pr_head.clone(),
        reconciliation_digest: sha256_prefixed(&first.ledger_bytes),
        publication_sync_manifest_digest: sha256_prefixed(&first.manifest_bytes),
        listed_paths: first.listed_paths.clone(),
        published_channels: Vec::new(),
        release_cut: false,
        deterministic_double_generation: true,
    };
    let receipt_bytes = canonical_json(&serde_json::to_value(&receipt)?)?;
    fs::write(config.out_dir.join("projection-receipt.json"), receipt_bytes)?;
    println!(
        "publication-sync: P={} J0={} J={}",
        first.projected_tree, first.core_join, first.pr_head
    );
    Ok(())
}

fn validate_input(input: &InputManifest) -> Result<()> {
    if input.schema_version != INPUT_SCHEMA_VERSION {
        return Err(eyre!(
            "input manifest schema_version must be {INPUT_SCHEMA_VERSION}, got {}",
            input.schema_version
        ));
    }
    if input.release.trim().is_empty() {
        return Err(eyre!("input manifest release must not be empty"));
    }
    let mut listed = BTreeSet::new();
    let mut froms = BTreeSet::new();
    for translation in &input.translations {
        validate_repo_path(&translation.from)?;
        validate_repo_path(&translation.to)?;
        if translation.from.starts_with(CONTROL_DIR) || translation.to.starts_with(CONTROL_DIR) {
            return Err(eyre!("translations must not touch the control directory"));
        }
        if !froms.insert(translation.from.clone()) {
            return Err(eyre!("duplicate translation from: {}", translation.from));
        }
        for path in [&translation.from, &translation.to] {
            if !listed.insert(path.clone()) {
                return Err(eyre!("path listed more than once: {path}"));
            }
        }
    }
    for exclusion in &input.exclusions {
        validate_repo_path(exclusion)?;
        if exclusion.starts_with(CONTROL_DIR) {
            return Err(eyre!("exclusions must not touch the control directory"));
        }
    }
    let mut seen = BTreeSet::new();
    for invariant in &input.required_invariants {
        if !REQUIRED_INVARIANT_IDS.contains(&invariant.id.as_str()) {
            return Err(eyre!("unknown required invariant: {}", invariant.id));
        }
        if !seen.insert(invariant.id.clone()) {
            return Err(eyre!("duplicate required invariant: {}", invariant.id));
        }
        if invariant.status != "pass" {
            return Err(eyre!(
                "required invariant {} has status {}; only pass is acceptable",
                invariant.id,
                invariant.status
            ));
        }
        if invariant.evidence.as_deref().unwrap_or("").trim().is_empty() {
            return Err(eyre!("required invariant {} needs non-empty evidence", invariant.id));
        }
    }
    for required in REQUIRED_INVARIANT_IDS {
        if !seen.contains(required) {
            return Err(eyre!("input manifest omits required invariant: {required}"));
        }
    }
    Ok(())
}

fn validate_repo_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(eyre!("unsafe repo-relative path: {path:?}"));
    }
    Ok(())
}

fn canonical_ledger(raw: &[u8], input: &InputManifest) -> Result<Vec<u8>> {
    let value: serde_json::Value =
        serde_json::from_slice(raw).context("parsing reconciliation ledger")?;
    let object = value.as_object().ok_or_else(|| eyre!("ledger must be a JSON object"))?;
    let expected_ledger_schema =
        input.ledger_schema_version.as_deref().unwrap_or(DEFAULT_LEDGER_SCHEMA_VERSION);
    if object.get("schema_version").and_then(|v| v.as_str()) != Some(expected_ledger_schema) {
        return Err(eyre!("ledger schema_version must be {expected_ledger_schema}"));
    }
    if object.get("release").and_then(|v| v.as_str()) != Some(input.release.as_str()) {
        return Err(eyre!("ledger release does not match input manifest release"));
    }
    let blockers = object
        .get("blockers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| eyre!("ledger blockers must be an array"))?;
    if !blockers.is_empty() {
        return Err(eyre!("ledger has blockers; reconciliation is not blocker-free"));
    }
    let decisions = object
        .get("decisions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| eyre!("ledger decisions must be an array"))?;
    for decision in decisions {
        if decision.get("status").and_then(|v| v.as_str()) != Some("resolved") {
            return Err(eyre!("every ledger decision must be resolved"));
        }
    }
    canonical_json(&value)
}

/// Canonical JSON: object keys sorted recursively, two-space indent,
/// trailing newline. Identical inputs give identical bytes.
fn canonical_json(value: &serde_json::Value) -> Result<Vec<u8>> {
    fn sort(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let sorted: BTreeMap<String, serde_json::Value> =
                    map.iter().map(|(k, v)| (k.clone(), sort(v))).collect();
                serde_json::Value::Object(sorted.into_iter().collect())
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(sort).collect())
            }
            other => other.clone(),
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&sort(value))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn sha256_prefixed(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    format!("sha256:{}", hex(&digest))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn resolve_commit(repo: &Path, input: &str, role: &str) -> Result<String> {
    if input.trim().is_empty() || input.starts_with('-') {
        return Err(eyre!("invalid {role} subject: {input:?}"));
    }
    let sha = git(repo, &["rev-parse", "--verify", &format!("{input}^{{commit}}")], None, None)?;
    let sha = sha.trim().to_string();
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(eyre!("{role} did not resolve to a full commit id: {sha}"));
    }
    Ok(sha)
}

fn git(
    repo: &Path,
    args: &[&str],
    stdin: Option<&[u8]>,
    extra_env: Option<&[(&str, &str)]>,
) -> Result<String> {
    run_git(repo, args, stdin, extra_env).map(|out| String::from_utf8_lossy(&out).into_owned())
}

fn run_git(
    repo: &Path,
    args: &[&str],
    stdin: Option<&[u8]>,
    extra_env: Option<&[(&str, &str)]>,
) -> Result<Vec<u8>> {
    let mut command = xtask::git_environment::isolate(Command::new("git"));
    command
        .current_dir(repo)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(pairs) = extra_env {
        for (key, value) in pairs {
            command.env(key, value);
        }
    }
    if stdin.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command.spawn().context("spawning git")?;
    if let Some(bytes) = stdin {
        use std::io::Write;
        let mut pipe = child.stdin.take().ok_or_else(|| eyre!("git stdin unavailable"))?;
        pipe.write_all(bytes).context("writing git stdin")?;
    }
    let output = child.wait_with_output().context("waiting for git")?;
    if !output.status.success() {
        return Err(eyre!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn generate(
    repo: &Path,
    config: &ProjectConfig,
    input: &InputManifest,
    base: &str,
    source: &str,
    ledger_bytes: &[u8],
    message: &[u8],
) -> Result<Generation> {
    // Expand exclusions: a directory exclusion covers every file under it.
    let source_files = list_files(repo, source)?;
    let mut expanded_exclusions: Vec<String> = Vec::new();
    for exclusion in &input.exclusions {
        let matched: Vec<String> = source_files
            .iter()
            .filter(|path| *path == exclusion || path.starts_with(&format!("{exclusion}/")))
            .cloned()
            .collect();
        if matched.is_empty() {
            return Err(eyre!("exclusion matches nothing in S: {exclusion}"));
        }
        expanded_exclusions.extend(matched);
    }
    expanded_exclusions.sort();
    expanded_exclusions.dedup();

    let mut invariants = input.required_invariants.clone();
    invariants.sort_by(|a, b| a.id.cmp(&b.id));
    let committed = CommittedManifest {
        schema_version: input
            .manifest_schema_version
            .clone()
            .unwrap_or_else(|| DEFAULT_MANIFEST_SCHEMA_VERSION.to_string()),
        release: input.release.clone(),
        translations: input
            .translations
            .iter()
            .map(|t| CommittedTranslation { from: t.from.clone(), to: t.to.clone() })
            .collect(),
        exclusions: expanded_exclusions.clone(),
        required_invariants: invariants,
    };
    let manifest_bytes = canonical_json(&serde_json::to_value(&committed)?)?;

    let mut listed: Vec<String> = Vec::new();
    for translation in &input.translations {
        listed.push(translation.from.clone());
        listed.push(translation.to.clone());
    }
    listed.extend(expanded_exclusions.iter().cloned());
    listed.sort();
    listed.dedup();
    if listed.len()
        != input.translations.len() * 2 - count_same_path(input) + expanded_exclusions.len()
    {
        return Err(eyre!("manifest paths overlap; listed set is ambiguous"));
    }

    let projected_tree =
        build_projected_tree(repo, config, input, base, source, &expanded_exclusions)?;

    let env = commit_env(config);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let core_join = commit_tree(repo, &projected_tree, base, source, message, &env_refs)?;

    let packet = Packet {
        schema_version: input
            .packet_schema_version
            .clone()
            .unwrap_or_else(|| DEFAULT_PACKET_SCHEMA_VERSION.to_string()),
        release: input.release.clone(),
        release_base_sha: base.to_string(),
        prepared_swarm_sha: source.to_string(),
        sync_join_sha: core_join.clone(),
        expected_join_parents: vec![base.to_string(), source.to_string()],
        reconciliation_digest: sha256_prefixed(ledger_bytes),
        publication_sync_manifest_digest: sha256_prefixed(&manifest_bytes),
        expected_projected_tree: projected_tree.clone(),
        published_channels: Vec::new(),
        release_cut: false,
    };
    let packet_bytes = canonical_json(&serde_json::to_value(&packet)?)?;

    // PR head tree: projection plus exactly the three control files.
    let pr_tree =
        with_control_files(repo, &projected_tree, &packet_bytes, ledger_bytes, &manifest_bytes)?;
    let pr_head = commit_tree(repo, &pr_tree, base, source, message, &env_refs)?;

    Ok(Generation {
        projected_tree,
        core_join,
        pr_head,
        packet_bytes,
        manifest_bytes,
        ledger_bytes: ledger_bytes.to_vec(),
        listed_paths: listed,
    })
}

fn count_same_path(input: &InputManifest) -> usize {
    input.translations.iter().filter(|t| t.from == t.to).count()
}

fn list_files(repo: &Path, commit: &str) -> Result<Vec<String>> {
    let out = git(repo, &["ls-tree", "-r", "--name-only", commit], None, None)?;
    Ok(out.lines().map(str::to_string).collect())
}

fn tree_entry(repo: &Path, commit: &str, path: &str) -> Result<Option<(String, String)>> {
    let out = git(repo, &["ls-tree", commit, "--", path], None, None)?;
    let line = out.lines().next();
    match line {
        None => Ok(None),
        Some(line) => {
            // "100644 blob <sha>\t<path>"
            let mut parts = line.splitn(4, ' ');
            let mode = parts.next().unwrap_or("");
            let _kind = parts.next().unwrap_or("");
            let sha = parts.next().unwrap_or("");
            if mode.is_empty() || sha.is_empty() {
                return Err(eyre!("unparseable ls-tree line: {line}"));
            }
            Ok(Some((mode.to_string(), sha.to_string())))
        }
    }
}

fn build_projected_tree(
    repo: &Path,
    _config: &ProjectConfig,
    input: &InputManifest,
    base: &str,
    source: &str,
    expanded_exclusions: &[String],
) -> Result<String> {
    let index_dir = tempfile::tempdir().context("creating temporary index directory")?;
    let index_path = index_dir.path().join("index");
    let index_env =
        [("GIT_INDEX_FILE", index_path.to_str().ok_or_else(|| eyre!("non-UTF8 temp path"))?)];
    let index_refs: Vec<(&str, &str)> = index_env.iter().map(|(k, v)| (*k, *v)).collect();
    git(repo, &["read-tree", source], None, Some(&index_refs))?;
    for path in expanded_exclusions {
        git(repo, &["update-index", "--force-remove", "--", path], None, Some(&index_refs))?;
    }
    for translation in &input.translations {
        if let Some(expected_source) = &translation.source_sha256 {
            let (_, source_blob) = tree_entry(repo, source, &translation.from)?
                .ok_or_else(|| eyre!("translation source missing in S: {}", translation.from))?;
            let actual = blob_sha256(repo, &source_blob)?;
            if actual != expected_source.to_ascii_lowercase() {
                return Err(eyre!(
                    "stale input: source digest for {} is {actual}, manifest binds {expected_source}",
                    translation.from
                ));
            }
        }
        let (mode, sha) = resolve_content(repo, translation, base, source)?;
        if let Some(expected) = &translation.expected_sha256 {
            let actual = blob_sha256(repo, &sha)?;
            if actual != expected.to_ascii_lowercase() {
                return Err(eyre!(
                    "unexpected content: digest for {} is {actual}, manifest binds {expected}",
                    translation.to
                ));
            }
        }
        if translation.from != translation.to {
            git(
                repo,
                &["update-index", "--force-remove", "--", &translation.from],
                None,
                Some(&index_refs),
            )?;
        }
        if tree_entry(repo, source, &translation.to)?.is_some()
            && translation.to != translation.from
            && !expanded_exclusions.contains(&translation.to)
        {
            return Err(eyre!("translation target already exists in S: {}", translation.to));
        }
        git(
            repo,
            &["update-index", "--add", "--cacheinfo", &format!("{mode},{sha},{}", translation.to)],
            None,
            Some(&index_refs),
        )?;
    }
    let tree = git(repo, &["write-tree"], None, Some(&index_refs))?;
    Ok(tree.trim().to_string())
}

fn blob_sha256(repo: &Path, blob: &str) -> Result<String> {
    let bytes = run_git(repo, &["cat-file", "blob", blob], None, None)?;
    Ok(hex(&Sha256::digest(&bytes)))
}

fn resolve_content(
    repo: &Path,
    translation: &Translation,
    base: &str,
    source: &str,
) -> Result<(String, String)> {
    match &translation.content {
        ContentSource::Swarm => tree_entry(repo, source, &translation.from)?
            .ok_or_else(|| eyre!("translation source missing in S: {}", translation.from)),
        ContentSource::Release { path } => {
            let release_path = path.as_deref().unwrap_or(&translation.to);
            tree_entry(repo, base, release_path)?
                .ok_or_else(|| eyre!("translation release source missing in R: {release_path}"))
        }
        ContentSource::Blob { sha } => {
            let kind = git(repo, &["cat-file", "-t", sha], None, None)?;
            if kind.trim() != "blob" {
                return Err(eyre!("content blob {sha} is not a blob"));
            }
            Ok(("100644".to_string(), sha.clone()))
        }
        ContentSource::Inline { text } => {
            let sha = git(repo, &["hash-object", "-w", "--stdin"], Some(text.as_bytes()), None)?;
            Ok(("100644".to_string(), sha.trim().to_string()))
        }
    }
}

fn with_control_files(
    repo: &Path,
    projected_tree: &str,
    packet: &[u8],
    ledger: &[u8],
    manifest: &[u8],
) -> Result<String> {
    let index_dir = tempfile::tempdir().context("creating temporary index directory")?;
    let index_path = index_dir.path().join("index");
    let index_env =
        [("GIT_INDEX_FILE", index_path.to_str().ok_or_else(|| eyre!("non-UTF8 temp path"))?)];
    let index_refs: Vec<(&str, &str)> = index_env.iter().map(|(k, v)| (*k, *v)).collect();
    git(repo, &["read-tree", projected_tree], None, Some(&index_refs))?;
    for (name, bytes) in [
        ("packet.yaml", packet),
        ("reconciliation-ledger.json", ledger),
        ("projection-manifest.json", manifest),
    ] {
        let sha = git(repo, &["hash-object", "-w", "--stdin"], Some(bytes), None)?;
        git(
            repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{},.github/publication-sync/{name}", sha.trim()),
            ],
            None,
            Some(&index_refs),
        )?;
    }
    let tree = git(repo, &["write-tree"], None, Some(&index_refs))?;
    Ok(tree.trim().to_string())
}

fn commit_env(config: &ProjectConfig) -> Vec<(&'static str, String)> {
    let date = format!("{} +0000", config.timestamp);
    vec![
        ("GIT_AUTHOR_NAME", config.author_name.clone()),
        ("GIT_AUTHOR_EMAIL", config.author_email.clone()),
        ("GIT_COMMITTER_NAME", config.author_name.clone()),
        ("GIT_COMMITTER_EMAIL", config.author_email.clone()),
        ("GIT_AUTHOR_DATE", date.clone()),
        ("GIT_COMMITTER_DATE", date),
    ]
}

fn commit_tree(
    repo: &Path,
    tree: &str,
    base: &str,
    source: &str,
    message: &[u8],
    env: &[(&str, &str)],
) -> Result<String> {
    let sha =
        git(repo, &["commit-tree", tree, "-p", base, "-p", source], Some(message), Some(env))?;
    Ok(sha.trim().to_string())
}

/// Arguments for verifying an existing audited join `J`.
pub struct VerifyJoinConfig {
    pub base: String,
    pub source: String,
    pub join: String,
    /// Committed projection manifest (`.github/publication-sync/projection-manifest.json` shape).
    pub manifest: PathBuf,
    pub expected_tree: String,
    pub working_directory: Option<PathBuf>,
}

/// Verify `J`: parents `[R, S]`, projected tree, and exact manifest diff.
pub fn verify_join(config: VerifyJoinConfig) -> Result<()> {
    let repo = config.working_directory.clone().unwrap_or_else(|| PathBuf::from("."));
    let base = resolve_commit(&repo, &config.base, "base (R)")?;
    let source = resolve_commit(&repo, &config.source, "source (S)")?;
    let join = resolve_commit(&repo, &config.join, "join (J)")?;
    let manifest_raw = fs::read(&config.manifest)
        .with_context(|| format!("reading manifest {}", config.manifest.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_raw).context("parsing committed manifest")?;
    let mut listed = BTreeSet::new();
    for translation in manifest["translations"]
        .as_array()
        .ok_or_else(|| eyre!("manifest translations must be an array"))?
    {
        listed.insert(
            translation["from"].as_str().ok_or_else(|| eyre!("translation from"))?.to_string(),
        );
        listed
            .insert(translation["to"].as_str().ok_or_else(|| eyre!("translation to"))?.to_string());
    }
    for exclusion in manifest["exclusions"]
        .as_array()
        .ok_or_else(|| eyre!("manifest exclusions must be an array"))?
    {
        listed.insert(exclusion.as_str().ok_or_else(|| eyre!("exclusion"))?.to_string());
    }
    for invariant in manifest["required_invariants"]
        .as_array()
        .ok_or_else(|| eyre!("manifest required_invariants must be an array"))?
    {
        if invariant["status"].as_str() != Some("pass") {
            return Err(eyre!("required invariant is not pass"));
        }
    }
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", &join], None, None)?;
    let parts: Vec<&str> = parents.split_whitespace().collect();
    if parts.len() != 3 || parts[1] != base || parts[2] != source {
        return Err(eyre!("J parents are not exactly [R, S]"));
    }
    // Projected tree: tree(J) minus the control directory must equal P.
    // Reuse the packet-free check: diff S..J excluding control == listed,
    // and tree(J) with control stripped is recomputed by the validator;
    // here we require the caller-bound expected tree via ls-tree of J
    // compared after removing control entries through a temp index.
    let projected = strip_control_tree(&repo, &join)?;
    if projected != config.expected_tree {
        return Err(eyre!("projected tree {projected} != expected {}", config.expected_tree));
    }
    let diff = git(&repo, &["diff", "--name-only", &source, &join], None, None)?;
    let changed: BTreeSet<String> =
        diff.lines().map(str::to_string).filter(|path| !path.starts_with(CONTROL_DIR)).collect();
    if changed != listed {
        return Err(eyre!("S..J diff does not equal manifest-listed paths"));
    }
    println!("publication-sync verify-join: PASS J={join} P={projected}");
    Ok(())
}

fn strip_control_tree(repo: &Path, commit: &str) -> Result<String> {
    let index_dir = tempfile::tempdir().context("creating temporary index directory")?;
    let index_path = index_dir.path().join("index");
    let index_env =
        [("GIT_INDEX_FILE", index_path.to_str().ok_or_else(|| eyre!("non-UTF8 temp path"))?)];
    let index_refs: Vec<(&str, &str)> = index_env.iter().map(|(k, v)| (*k, *v)).collect();
    git(repo, &["read-tree", commit], None, Some(&index_refs))?;
    git(
        repo,
        &["update-index", "--force-remove", "--", ".github/publication-sync/packet.yaml"],
        None,
        Some(&index_refs),
    )
    .ok();
    git(
        repo,
        &[
            "update-index",
            "--force-remove",
            "--",
            ".github/publication-sync/reconciliation-ledger.json",
        ],
        None,
        Some(&index_refs),
    )
    .ok();
    git(
        repo,
        &[
            "update-index",
            "--force-remove",
            "--",
            ".github/publication-sync/projection-manifest.json",
        ],
        None,
        Some(&index_refs),
    )
    .ok();
    let tree = git(repo, &["write-tree"], None, Some(&index_refs))?;
    Ok(tree.trim().to_string())
}

/// Arguments for verifying a landed wrapper merge `M`.
pub struct VerifyLandedConfig {
    pub base: String,
    pub source: String,
    pub join: String,
    pub merge: String,
    /// Current publication head that must equal `M` (commit-ish).
    pub master: String,
    pub working_directory: Option<PathBuf>,
}

/// Verify landed `M`: `J`/`R`/`S` ancestry, `tree(M) == tree(J)`, head == `M`.
pub fn verify_landed(config: VerifyLandedConfig) -> Result<()> {
    let repo = config.working_directory.clone().unwrap_or_else(|| PathBuf::from("."));
    let base = resolve_commit(&repo, &config.base, "base (R)")?;
    let source = resolve_commit(&repo, &config.source, "source (S)")?;
    let join = resolve_commit(&repo, &config.join, "join (J)")?;
    let merge = resolve_commit(&repo, &config.merge, "merge (M)")?;
    let master = resolve_commit(&repo, &config.master, "master")?;
    for (label, ancestor) in [("J", &join), ("R", &base), ("S", &source)] {
        let ok = git(&repo, &["merge-base", "--is-ancestor", ancestor, &merge], None, None);
        if ok.is_err() {
            return Err(eyre!("{label} is not an ancestor of M"));
        }
    }
    let tree_m = git(&repo, &["rev-parse", &format!("{merge}^{{tree}}")], None, None)?;
    let tree_j = git(&repo, &["rev-parse", &format!("{join}^{{tree}}")], None, None)?;
    if tree_m.trim() != tree_j.trim() {
        return Err(eyre!("tree(M) != tree(J)"));
    }
    if master != merge {
        return Err(eyre!("current publication head {master} != M {merge}"));
    }
    println!("publication-sync verify-landed: PASS M={merge}");
    Ok(())
}

fn verify_against_contract(repo: &Path, source: &str, generation: &Generation) -> Result<()> {
    // Parents of J must be exactly [R, S] in order.
    let parents =
        git(repo, &["rev-list", "--parents", "-n", "1", &generation.pr_head], None, None)?;
    let parts: Vec<&str> = parents.split_whitespace().collect();
    if parts.len() != 3 || parts[2] != source {
        return Err(eyre!("generated J does not have source S as second parent"));
    }
    // diff S..J excluding the control directory must equal the listed set.
    let diff = git(repo, &["diff", "--name-only", source, &generation.pr_head], None, None)?;
    let changed: BTreeSet<String> =
        diff.lines().map(str::to_string).filter(|path| !path.starts_with(CONTROL_DIR)).collect();
    let listed: BTreeSet<String> = generation.listed_paths.iter().cloned().collect();
    if changed != listed {
        return Err(eyre!(
            "S..J diff does not equal manifest-listed paths: changed={changed:?} listed={listed:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_fixture(repo: &Path, args: &[&str]) -> Result<String> {
        let mut command = xtask::git_environment::isolate(Command::new("git"));
        command.current_dir(repo).args(args);
        let output = command.output()?;
        if !output.status.success() {
            return Err(eyre!("fixture git failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    fn commit_files(repo: &Path, files: &[(&str, &str)], message: &str) -> Result<String> {
        git_fixture(repo, &["read-tree", "--empty"])?;
        for (path, content) in files {
            let blob = {
                let mut command = xtask::git_environment::isolate(Command::new("git"));
                command
                    .current_dir(repo)
                    .args(["hash-object", "-w", "--stdin"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped());
                let mut child = command.spawn()?;
                use std::io::Write;
                child.stdin.take().ok_or_else(|| eyre!("stdin"))?.write_all(content.as_bytes())?;
                let out = child.wait_with_output()?;
                String::from_utf8(out.stdout)?.trim().to_string()
            };
            git_fixture(
                repo,
                &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")],
            )?;
        }
        let tree = git_fixture(repo, &["write-tree"])?;
        let sha = {
            let mut command = xtask::git_environment::isolate(Command::new("git"));
            command
                .current_dir(repo)
                .args(["commit-tree", &tree])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped());
            let mut child = command.spawn()?;
            use std::io::Write;
            child.stdin.take().ok_or_else(|| eyre!("stdin"))?.write_all(message.as_bytes())?;
            let out = child.wait_with_output()?;
            String::from_utf8(out.stdout)?.trim().to_string()
        };
        Ok(sha)
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        repo: PathBuf,
        base: String,
        source: String,
        manifest: PathBuf,
        ledger: PathBuf,
        message: PathBuf,
    }

    fn fixture() -> Result<Fixture> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo)?;
        git_fixture(&repo, &["init", "--quiet", "--initial-branch=main"])?;
        git_fixture(&repo, &["config", "user.email", "test@example.com"])?;
        git_fixture(&repo, &["config", "user.name", "producer-test"])?;
        let base = commit_files(&repo, &[("README.md", "perl-lsp base\n")], "R\n")?;
        let source = commit_files(
            &repo,
            &[
                ("app/main.pl", "use strict;\n"),
                ("docs/old.md", "old docs\n"),
                ("tmp/skip.txt", "scratch\n"),
                (".github/workflows/ci.yml", "name: CI\n"),
            ],
            "S\n",
        )?;
        let manifest = dir.path().join("manifest.json");
        fs::write(
            &manifest,
            r#"{
  "schema_version": "publication_sync_input.v1",
  "release": "0.18.0-rc.1",
  "translations": [{"from": "docs/old.md", "to": "docs/new.md", "content": {"kind": "inline", "text": "new docs\n"}}],
  "exclusions": ["tmp/skip.txt"],
  "required_invariants": [
    {"id": "destination-context", "status": "pass", "evidence": "fixture"},
    {"id": "version", "status": "pass", "evidence": "fixture"},
    {"id": "topology", "status": "pass", "evidence": "fixture"},
    {"id": "artifact-reachability", "status": "pass", "evidence": "fixture"},
    {"id": "effective-identity", "status": "pass", "evidence": "fixture"}
  ]
}"#,
        )?;
        let ledger = dir.path().join("ledger.json");
        fs::write(
            &ledger,
            r#"{"schema_version":"v0.18_reconciliation.v1","release":"0.18.0-rc.1","decisions":[{"id":"d1","status":"resolved"}],"blockers":[]}"#,
        )?;
        let message = dir.path().join("message.txt");
        fs::write(&message, "release: history-preserving publication sync\n")?;
        Ok(Fixture { _dir: dir, repo, base, source, manifest, ledger, message })
    }

    fn config(fixture: &Fixture, out_dir: PathBuf) -> ProjectConfig {
        ProjectConfig {
            base: fixture.base.clone(),
            source: fixture.source.clone(),
            manifest: fixture.manifest.clone(),
            ledger: fixture.ledger.clone(),
            message_file: fixture.message.clone(),
            out_dir,
            author_name: "Producer Test".to_string(),
            author_email: "test@example.com".to_string(),
            timestamp: 1_700_000_000,
            working_directory: Some(fixture.repo.clone()),
        }
    }

    #[test]
    fn deterministic_projection_satisfies_contract_shape() -> Result<()> {
        let fixture = fixture()?;
        let out1 = fixture.repo.parent().ok_or_else(|| eyre!("parent"))?.join("out1");
        let out2 = fixture.repo.parent().ok_or_else(|| eyre!("parent"))?.join("out2");
        project(config(&fixture, out1.clone()))?;
        project(config(&fixture, out2.clone()))?;
        let receipt1: serde_json::Value =
            serde_json::from_slice(&fs::read(out1.join("projection-receipt.json"))?)?;
        let receipt2: serde_json::Value =
            serde_json::from_slice(&fs::read(out2.join("projection-receipt.json"))?)?;
        assert_eq!(receipt1, receipt2);
        assert_eq!(receipt1["deterministic_double_generation"], true);
        assert_eq!(receipt1["published_channels"], serde_json::json!([]));
        assert_eq!(receipt1["release_cut"], false);
        let head = receipt1["pr_head_join_sha"].as_str().ok_or_else(|| eyre!("head"))?;
        let diff = git_fixture(&fixture.repo, &["diff", "--name-only", &fixture.source, head])?;
        let mut changed: Vec<&str> =
            diff.lines().filter(|line| !line.starts_with(".github/publication-sync/")).collect();
        changed.sort_unstable();
        assert_eq!(changed, vec!["docs/new.md", "docs/old.md", "tmp/skip.txt"]);
        let control = git_fixture(
            &fixture.repo,
            &["ls-tree", "--name-only", &format!("{head}:.github/publication-sync")],
        )?;
        let mut names: Vec<&str> = control.lines().collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["packet.yaml", "projection-manifest.json", "reconciliation-ledger.json"]
        );
        Ok(())
    }

    #[test]
    fn blocked_ledger_fails_closed() -> Result<()> {
        let fixture = fixture()?;
        fs::write(
            &fixture.ledger,
            r#"{"schema_version":"v0.18_reconciliation.v1","release":"0.18.0-rc.1","decisions":[],"blockers":["open"]}"#,
        )?;
        let out = fixture.repo.parent().ok_or_else(|| eyre!("parent"))?.join("out-blocked");
        let error = project(config(&fixture, out)).expect_err("blocked ledger must fail");
        assert!(error.to_string().contains("blocker"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn not_proven_invariant_fails_closed() -> Result<()> {
        let fixture = fixture()?;
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.manifest)?)?;
        value["required_invariants"][0]["status"] = serde_json::json!("not_proven");
        fs::write(&fixture.manifest, serde_json::to_vec(&value)?)?;
        let out = fixture.repo.parent().ok_or_else(|| eyre!("parent"))?.join("out-np");
        let error = project(config(&fixture, out)).expect_err("not_proven invariant must fail");
        assert!(error.to_string().contains("only pass"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn missing_exclusion_target_fails_closed() -> Result<()> {
        let fixture = fixture()?;
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.manifest)?)?;
        value["exclusions"] = serde_json::json!(["nope/missing.txt"]);
        fs::write(&fixture.manifest, serde_json::to_vec(&value)?)?;
        let out = fixture.repo.parent().ok_or_else(|| eyre!("parent"))?.join("out-missing");
        let error = project(config(&fixture, out)).expect_err("missing exclusion must fail");
        assert!(error.to_string().contains("matches nothing"), "unexpected error: {error}");
        Ok(())
    }
}
