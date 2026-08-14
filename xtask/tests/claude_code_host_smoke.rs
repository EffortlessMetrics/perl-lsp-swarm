#[path = "support/agent_client_compat.rs"]
mod agent_client_compat;

use agent_client_compat::{
    AgentClientCompatReceipt, EvidenceStage, HostIdentity, HostProduct, IntegrationIdentity,
    IntegrationMode, JourneyCell, ObservationResult, PlatformIdentity, Protocol, SCHEMA_VERSION,
    ServerIdentity, WorkspaceFixtureIdentity, fixture_digest,
};
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use walkdir::WalkDir;

const PLUGIN_ROOT: &str = "integrations/claude-code/plugins/perl-lsp-rs";
const FIXTURE_ROOT: &str = "crates/perl-lsp-ux-tests/fixtures/agent-client-compat";
const MARKETPLACE_NAME: &str = "effortlessmetrics";
const PLUGIN_NAME: &str = "perl-lsp-rs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LspOperation {
    Definition,
    References,
    DocumentSymbols,
    WorkspaceSymbols,
    Hover,
    Implementation,
    CallHierarchy,
    Other,
}

#[derive(Debug, Clone)]
struct LspInvocation {
    id: String,
    operation: LspOperation,
    input: Value,
    result: Option<Value>,
}

#[derive(Debug)]
struct ClaudeStreamSummary {
    init_tools: BTreeSet<String>,
    plugin_paths: BTreeMap<String, PathBuf>,
    plugin_errors: Vec<String>,
    lsp_invocations: Vec<LspInvocation>,
}

#[derive(Debug)]
struct HostCommandPlan {
    program: OsString,
    args: Vec<OsString>,
}

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below repository root")
}

fn navigation_plan(program: OsString, debug_file: &Path) -> HostCommandPlan {
    HostCommandPlan {
        program,
        args: vec![
            "-p".into(),
            "Use only the LSP tool. Do not infer answers without LSP calls. Run all four checks: (1) find the definition of Widget->new used in app.pl, (2) find references to greet used in app.pl, (3) list document symbols in lib/Widget.pm, and (4) search workspace symbols for Widget. Complete every LSP operation before answering.".into(),
            "--tools".into(),
            "LSP".into(),
            "--allowedTools".into(),
            "LSP".into(),
            "--strict-mcp-config".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--debug-file".into(),
            debug_file.as_os_str().to_owned(),
            "--no-session-persistence".into(),
            "--max-turns".into(),
            "12".into(),
        ],
    }
}

fn run_output(command: &mut Command, label: &str) -> Result<Output> {
    let output = command.output().with_context(|| format!("running {label}"))?;
    ensure!(
        output.status.success(),
        "{label} failed with {}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn claude_program() -> OsString {
    std::env::var_os("CLAUDE_CODE_BIN").unwrap_or_else(|| OsString::from("claude"))
}

fn configured_auth_dir(run: &TempDir) -> Result<Option<PathBuf>> {
    if let Some(path) = std::env::var_os("CLAUDE_CODE_SMOKE_CONFIG_DIR") {
        let path = PathBuf::from(path);
        ensure!(path.is_dir(), "CLAUDE_CODE_SMOKE_CONFIG_DIR is not a directory");
        return Ok(Some(path));
    }

    let token_auth = std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_some()
        || std::env::var_os("ANTHROPIC_API_KEY").is_some();
    if token_auth {
        let path = run.path().join("claude-config");
        fs::create_dir_all(&path)?;
        return Ok(Some(path));
    }

    bail!(
        "actual Claude smoke requires CLAUDE_CODE_SMOKE_CONFIG_DIR pointing at a dedicated authenticated config, or CLAUDE_CODE_OAUTH_TOKEN/ANTHROPIC_API_KEY"
    )
}

fn child_path(candidate_bin_dir: &Path) -> Result<OsString> {
    let mut paths = vec![candidate_bin_dir.to_path_buf()];
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    std::env::join_paths(paths).context("constructing isolated child PATH")
}

fn apply_claude_env(
    command: &mut Command,
    plugin_cache: &Path,
    config_dir: Option<&Path>,
    candidate_bin_dir: &Path,
    server_log: &Path,
) -> Result<()> {
    command.env("CLAUDE_CODE_PLUGIN_CACHE_DIR", plugin_cache);
    command.env("CLAUDE_CODE_SYNC_PLUGIN_INSTALL", "1");
    command.env("PATH", child_path(candidate_bin_dir)?);
    command.env("PERL_LSP_LOG", "perl_lsp=debug,perl_lsp_rs_core=debug");
    command.env("PERL_LSP_LOG_FILE", server_log);
    command.env("PERL_LSP_QUIET", "1");
    if let Some(config_dir) = config_dir {
        command.env("CLAUDE_CONFIG_DIR", config_dir);
    }
    Ok(())
}

fn copy_fixture(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        ensure!(!entry.file_type().is_symlink(), "fixture contains symlink");
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).with_context(|| format!("reading {}", path.display()))?);
    let digest = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok(format!("sha256:{digest}"))
}

fn plugin_version(plugin_root: &Path) -> Result<String> {
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(plugin_root.join(".claude-plugin/plugin.json"))?)?;
    manifest["version"]
        .as_str()
        .map(str::to_string)
        .context("Claude plugin manifest missing version")
}

fn assert_launch_contract(plugin_root: &Path) -> Result<()> {
    let config: Value = serde_json::from_str(&fs::read_to_string(plugin_root.join(".lsp.json"))?)?;
    ensure!(config["perl"]["command"] == "perllsp", "Claude plugin command drifted");
    ensure!(config["perl"]["args"] == serde_json::json!(["--stdio"]), "Claude plugin args drifted");
    ensure!(
        config["perl"]["workspaceFolder"] == "${CLAUDE_PROJECT_DIR}",
        "Claude plugin workspaceFolder drifted"
    );
    Ok(())
}

fn collect_typed_objects(value: &Value, target_type: &str, out: &mut Vec<Value>) {
    match value {
        Value::Array(values) => {
            for child in values {
                collect_typed_objects(child, target_type, out);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some(target_type) {
                out.push(value.clone());
            }
            for child in object.values() {
                collect_typed_objects(child, target_type, out);
            }
        }
        _ => {}
    }
}

fn classify_lsp_operation(input: &Value) -> LspOperation {
    let Some(operation) = input.get("operation").and_then(Value::as_str) else {
        return LspOperation::Other;
    };
    let operation = operation
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match operation.as_str() {
        "definition" | "gotodefinition" => LspOperation::Definition,
        "reference" | "references" | "findreferences" => LspOperation::References,
        "documentsymbol" | "documentsymbols" => LspOperation::DocumentSymbols,
        "workspacesymbol" | "workspacesymbols" => LspOperation::WorkspaceSymbols,
        "hover" | "typeinformation" => LspOperation::Hover,
        "implementation" | "gotoimplementation" => LspOperation::Implementation,
        "callhierarchy" | "preparecallhierarchy" => LspOperation::CallHierarchy,
        _ => LspOperation::Other,
    }
}

fn tool_result_content(value: &Value) -> Value {
    value
        .get("content")
        .cloned()
        .or_else(|| value.get("result").cloned())
        .unwrap_or_else(|| value.clone())
}

fn parse_claude_stream(stdout: &[u8]) -> Result<ClaudeStreamSummary> {
    let mut init_tools = BTreeSet::new();
    let mut plugin_paths = BTreeMap::new();
    let mut plugin_errors = Vec::new();
    let mut tool_uses = Vec::new();
    let mut tool_results = BTreeMap::<String, Value>::new();

    for line in String::from_utf8_lossy(stdout).lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("invalid Claude stream-json line: {line}"))?;

        if value["type"] == "system" && value["subtype"] == "init" {
            if let Some(tools) = value["tools"].as_array() {
                for tool in tools.iter().filter_map(Value::as_str) {
                    init_tools.insert(tool.to_string());
                }
            }
            if let Some(plugins) = value["plugins"].as_array() {
                for plugin in plugins {
                    if let Some(name) = plugin.as_str() {
                        plugin_paths.entry(name.to_string()).or_default();
                    } else if let Some(name) = plugin.get("name").and_then(Value::as_str) {
                        let path = plugin
                            .get("path")
                            .and_then(Value::as_str)
                            .map(PathBuf::from)
                            .unwrap_or_default();
                        plugin_paths.insert(name.to_string(), path);
                    }
                }
            }
            if let Some(errors) = value["plugin_errors"].as_array() {
                plugin_errors.extend(errors.iter().map(|error| error.to_string()));
            }
        }

        let mut uses = Vec::new();
        collect_typed_objects(&value, "tool_use", &mut uses);
        for tool_use in uses {
            if tool_use.get("name").and_then(Value::as_str) == Some("LSP") {
                tool_uses.push(tool_use);
            }
        }

        let mut results = Vec::new();
        collect_typed_objects(&value, "tool_result", &mut results);
        for result in results {
            if let Some(id) = result.get("tool_use_id").and_then(Value::as_str) {
                tool_results.insert(id.to_string(), tool_result_content(&result));
            }
        }
    }

    let lsp_invocations = tool_uses
        .into_iter()
        .filter_map(|tool_use| {
            let id = tool_use.get("id")?.as_str()?.to_string();
            let input = tool_use.get("input").cloned().unwrap_or(Value::Null);
            Some(LspInvocation {
                operation: classify_lsp_operation(&input),
                result: tool_results.get(&id).cloned(),
                id,
                input,
            })
        })
        .collect();

    Ok(ClaudeStreamSummary { init_tools, plugin_paths, plugin_errors, lsp_invocations })
}

fn result_text(invocation: &LspInvocation) -> String {
    invocation
        .result
        .as_ref()
        .map(|value| serde_json::to_string(value).unwrap_or_default())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn input_path(input: &Value) -> Option<&str> {
    input
        .get("filePath")
        .or_else(|| input.get("path"))
        .or_else(|| input.get("uri"))
        .and_then(Value::as_str)
}

fn input_position(input: &Value) -> Option<(u64, u64)> {
    let position = input.get("position").unwrap_or(input);
    Some((position.get("line")?.as_u64()?, position.get("character")?.as_u64()?))
}

fn path_ends_with(input: &Value, expected: &str) -> bool {
    input_path(input).map(|path| path.replace('\\', "/").ends_with(expected)).unwrap_or(false)
}

fn assert_positive_result(invocation: &LspInvocation, required_fragments: &[&str]) -> Result<()> {
    let result = invocation.result.as_ref().context("LSP operation had no observed tool result")?;
    let non_empty = match result {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(_) => true,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
    };
    ensure!(non_empty, "LSP operation {:?} returned an empty result", invocation.operation);

    let text = result_text(invocation);
    for negative in [
        "not found",
        "no results",
        "no symbols",
        "no workspace symbols",
        "0 workspace symbols",
        "no locations",
        "no references",
        "no definition",
        "error:",
    ] {
        ensure!(
            !text.contains(negative),
            "LSP operation {:?} returned a negative result: {text}",
            invocation.operation
        );
    }
    for fragment in required_fragments {
        ensure!(
            text.contains(fragment),
            "LSP operation {:?} result missed oracle fragment {fragment:?}: {text}",
            invocation.operation
        );
    }
    Ok(())
}

fn assert_navigation_oracles(summary: &ClaudeStreamSummary) -> Result<Vec<JourneyCell>> {
    ensure!(summary.init_tools.contains("LSP"), "Claude system/init did not expose LSP tool");
    ensure!(
        summary.plugin_errors.is_empty(),
        "Claude reported plugin errors: {:?}",
        summary.plugin_errors
    );

    let mut cells = Vec::new();
    for (operation, id, required_fragments) in [
        (LspOperation::Definition, "definition.cross_file", &["widget.pm", "new"][..]),
        (
            LspOperation::References,
            "references.widget_greet",
            &["widget.pm", "app.pl", "greet"][..],
        ),
        (LspOperation::DocumentSymbols, "document_symbols.widget", &["new", "greet"][..]),
        (LspOperation::WorkspaceSymbols, "workspace_symbols.widget", &["widget", "widget.pm"][..]),
    ] {
        let invocation = summary
            .lsp_invocations
            .iter()
            .find(|invocation| invocation.operation == operation)
            .with_context(|| {
                format!("Claude did not issue required LSP operation {operation:?}")
            })?;
        match operation {
            LspOperation::Definition => {
                ensure!(
                    path_ends_with(&invocation.input, "app.pl"),
                    "definition request did not target app.pl"
                );
                let (line, character) = input_position(&invocation.input)
                    .context("definition request had no source position")?;
                ensure!(
                    matches!(line, 4 | 5) && (13..=24).contains(&character),
                    "definition request did not target Widget->new in app.pl: {line}:{character}"
                );
            }
            LspOperation::References => {
                ensure!(
                    path_ends_with(&invocation.input, "app.pl"),
                    "references request did not target app.pl"
                );
                let (line, character) = input_position(&invocation.input)
                    .context("references request had no source position")?;
                ensure!(
                    matches!(line, 5 | 6) && (13..=22).contains(&character),
                    "references request did not target greet in app.pl: {line}:{character}"
                );
            }
            LspOperation::DocumentSymbols => ensure!(
                path_ends_with(&invocation.input, "lib/Widget.pm"),
                "document-symbol request did not target lib/Widget.pm"
            ),
            LspOperation::WorkspaceSymbols => ensure!(
                invocation.input.get("query").and_then(Value::as_str) == Some("Widget"),
                "workspace-symbol request did not query Widget"
            ),
            _ => bail!("unexpected required LSP operation {operation:?}"),
        }
        assert_positive_result(invocation, required_fragments)?;
        cells.push(JourneyCell {
            id: id.to_string(),
            result: ObservationResult::Pass,
            evidence: vec![format!("lsp-tool-use:{}", invocation.id)],
            limitation: None,
        });
    }
    Ok(cells)
}

fn lsp_extensions(config_path: &Path) -> Result<BTreeSet<String>> {
    let config: Value = serde_json::from_str(&fs::read_to_string(config_path)?)?;
    Ok(config
        .as_object()
        .into_iter()
        .flat_map(|servers| servers.values())
        .filter_map(|server| server.get("extensionToLanguage"))
        .filter_map(Value::as_object)
        .flat_map(|extensions| extensions.keys().cloned())
        .collect())
}

fn conflicting_plugin_paths(
    loaded_plugins: &BTreeMap<String, PathBuf>,
    candidate_name: &str,
    candidate_extensions: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let mut conflicts = Vec::new();
    for (name, path) in loaded_plugins {
        if name == candidate_name || path.as_os_str().is_empty() {
            continue;
        }
        let lsp_path = path.join(".lsp.json");
        if !lsp_path.is_file() {
            continue;
        }
        let extensions = lsp_extensions(&lsp_path)?;
        if !extensions.is_disjoint(candidate_extensions) {
            conflicts.push(name.clone());
        }
    }
    conflicts.sort();
    Ok(conflicts)
}

fn git_head(root: &Path) -> Result<String> {
    let output = run_output(
        Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root),
        "git rev-parse HEAD",
    )?;
    let sha = String::from_utf8(output.stdout)?.trim().to_string();
    ensure!(sha.len() == 40, "git HEAD is not a full SHA");
    Ok(sha)
}

fn version_output(program: &Path) -> Result<String> {
    let output = run_output(Command::new(program).arg("--version"), "perllsp --version")?;
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn prove_build_revision(version: &str, root: &Path, candidate_sha: &str) -> Result<String> {
    let revision_line = version
        .lines()
        .find(|line| line.starts_with("Git "))
        .context("perllsp --version omitted its source revision")?;
    let (kind, revision) =
        revision_line.split_once(": ").context("malformed perllsp source revision")?;
    ensure!(
        !revision.trim().is_empty() && revision != "unknown",
        "perllsp source revision is not proven"
    );

    match kind {
        "Git commit" => {
            ensure!(
                revision.len() >= 7
                    && revision.chars().all(|character| character.is_ascii_hexdigit())
                    && candidate_sha.starts_with(revision),
                "perllsp was built from commit {revision}, not candidate {candidate_sha}"
            );
        }
        "Git tag" => {
            let tagged = run_output(
                Command::new("git")
                    .args(["rev-parse", &format!("{revision}^{{commit}}")])
                    .current_dir(root),
                "git rev-parse perllsp tag",
            )?;
            ensure!(
                String::from_utf8(tagged.stdout)?.trim() == candidate_sha,
                "perllsp tag {revision} does not identify candidate {candidate_sha}"
            );
        }
        _ => bail!("unsupported perllsp revision label {kind:?}"),
    }
    Ok(candidate_sha.to_string())
}

#[cfg(target_os = "linux")]
fn matching_linux_processes(canonical_candidate: &Path) -> Result<Vec<u32>> {
    let mut pids = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(exe) = fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        if fs::canonicalize(exe).ok().as_deref() == Some(canonical_candidate) {
            pids.push(pid);
        }
    }
    Ok(pids)
}

#[cfg(target_os = "linux")]
fn assert_no_orphan(candidate: &Path, timeout: Duration) -> Result<()> {
    let candidate = fs::canonicalize(candidate)?;
    let deadline = Instant::now() + timeout;
    loop {
        let pids = matching_linux_processes(&candidate)?;
        if pids.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("orphan perllsp processes remain after Claude exit: {pids:?}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a locally installed/authenticated Claude Code host and exact candidate perllsp"]
fn claude_code_exact_source_smoke() -> Result<()> {
    let root = repository_root()?;
    let plugin_root = root.join(PLUGIN_ROOT);
    let fixture_source = root.join(FIXTURE_ROOT);
    ensure!(plugin_root.is_dir(), "#7231 Claude package is not present in this candidate");
    assert_launch_contract(&plugin_root)?;

    let candidate_source = std::env::var_os("PERL_LSP_BIN")
        .map(PathBuf::from)
        .context("PERL_LSP_BIN must point at the exact candidate perllsp")?;
    ensure!(candidate_source.is_file(), "PERL_LSP_BIN is not a file");

    let run = TempDir::new()?;
    let bin_dir = run.path().join("bin");
    let plugin_cache = run.path().join("plugin-cache");
    let workspace = run.path().join("workspace");
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&plugin_cache)?;
    fs::create_dir_all(&workspace)?;
    copy_fixture(&fixture_source, &workspace)?;

    let candidate = bin_dir.join("perllsp");
    fs::copy(&candidate_source, &candidate)?;
    let server_hash = file_sha256(&candidate)?;
    let server_version = version_output(&candidate)?;
    let plugin_hash = fixture_digest(&plugin_root)?;
    let fixture_hash = fixture_digest(&fixture_source)?;
    let plugin_version = plugin_version(&plugin_root)?;
    let candidate_sha = git_head(&root)?;
    let build_revision = prove_build_revision(&server_version, &root, &candidate_sha)?;
    let config_dir = configured_auth_dir(&run)?;
    let server_log = run.path().join("perllsp.log");
    let debug_file = run.path().join("claude-debug.log");
    let claude = claude_program();

    let version = run_output(Command::new(&claude).arg("--version"), "claude --version")?;
    let host_version = String::from_utf8(version.stdout)?.trim().to_string();

    let mut add = Command::new(&claude);
    add.args(["plugin", "marketplace", "add"]).arg(&root).current_dir(&root);
    apply_claude_env(&mut add, &plugin_cache, config_dir.as_deref(), &bin_dir, &server_log)?;
    run_output(&mut add, "claude plugin marketplace add")?;

    let mut install = Command::new(&claude);
    install
        .args([
            "plugin",
            "install",
            &format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}"),
            "--scope",
            "user",
        ])
        .current_dir(&root);
    apply_claude_env(&mut install, &plugin_cache, config_dir.as_deref(), &bin_dir, &server_log)?;
    run_output(&mut install, "claude plugin install")?;

    let plan = navigation_plan(claude.clone(), &debug_file);
    let mut host = Command::new(&plan.program);
    host.args(&plan.args).current_dir(&workspace);
    apply_claude_env(&mut host, &plugin_cache, config_dir.as_deref(), &bin_dir, &server_log)?;
    let output = run_output(&mut host, "Claude native-LSP navigation journey")?;
    let summary = parse_claude_stream(&output.stdout)?;

    ensure!(
        summary.plugin_paths.contains_key(PLUGIN_NAME),
        "Claude system/init did not load perl-lsp-rs plugin"
    );
    let installed_path = summary
        .plugin_paths
        .get(PLUGIN_NAME)
        .filter(|path| !path.as_os_str().is_empty() && path.is_dir())
        .context("Claude did not report a usable installed perl-lsp-rs plugin path")?;
    ensure!(
        fixture_digest(installed_path)? == plugin_hash,
        "installed Claude plugin package differs from source candidate"
    );

    let candidate_extensions = lsp_extensions(&plugin_root.join(".lsp.json"))?;
    let conflicts =
        conflicting_plugin_paths(&summary.plugin_paths, PLUGIN_NAME, &candidate_extensions)?;
    ensure!(
        conflicts.is_empty(),
        "another loaded Claude plugin claims Perl extensions: {conflicts:?}"
    );

    let mut journey = assert_navigation_oracles(&summary)?;
    assert_no_orphan(&candidate, Duration::from_secs(3))?;
    journey.push(JourneyCell {
        id: "lifecycle.shutdown".to_string(),
        result: ObservationResult::Pass,
        evidence: vec!["linux-proc-exe-cleanup".to_string()],
        limitation: None,
    });

    let receipt = AgentClientCompatReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        observed_at: Utc::now().to_rfc3339(),
        stage: EvidenceStage::ExactSourceLocal,
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        candidate_sha: candidate_sha.clone(),
        platform: PlatformIdentity {
            os: "linux".to_string(),
            os_version: std::env::var("RUNNER_OS").unwrap_or_else(|_| "local-linux".to_string()),
            arch: std::env::consts::ARCH.to_string(),
        },
        host: HostIdentity {
            product: HostProduct::ClaudeCode,
            version: host_version,
            instrument_model: None,
        },
        integration: IntegrationIdentity {
            mode: IntegrationMode::NativeLspPlugin,
            plugin_name: PLUGIN_NAME.to_string(),
            plugin_version,
            marketplace_source: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            marketplace_ref: candidate_sha,
            package_sha256: plugin_hash,
        },
        server: ServerIdentity {
            executable: "perllsp".to_string(),
            version: server_version,
            build_revision,
            artifact_sha256: server_hash,
            protocol: Protocol::Lsp,
            protocol_or_schema_version: "lsp-3.17".to_string(),
        },
        workspace_fixture: WorkspaceFixtureIdentity {
            id: "perl-agent-client-v1".to_string(),
            digest: fixture_hash,
        },
        journey,
        result: ObservationResult::Pass,
        failure_class: None,
        limitations: vec![
            "This exact-source smoke proves navigation and Linux process cleanup; post-edit diagnostic injection remains a separate #7238 cell.".to_string(),
        ],
        artifacts: Vec::new(),
        claim_boundary: "Exact-source local Claude Code native-LSP navigation on this Linux host only.".to_string(),
    };
    receipt.validate()?;

    let receipt_path = root.join("target/receipts/agent-clients/claude-code-exact-source.json");
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    if debug_file.exists() {
        fs::remove_file(&debug_file)?;
    }
    Ok(())
}

#[test]
fn navigation_command_plan_is_lsp_only_and_bounded() -> Result<()> {
    let plan = navigation_plan("claude".into(), Path::new("debug.log"));
    let args = plan.args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>();
    ensure!(args.windows(2).any(|pair| pair == ["--tools", "LSP"]));
    ensure!(args.windows(2).any(|pair| pair == ["--allowedTools", "LSP"]));
    ensure!(args.iter().any(|arg| arg == "--strict-mcp-config"));
    ensure!(args.windows(2).any(|pair| pair == ["--output-format", "stream-json"]));
    ensure!(args.windows(2).any(|pair| pair == ["--max-turns", "12"]));
    ensure!(!args.iter().any(|arg| arg == "Read" || arg == "Grep" || arg == "Bash"));
    Ok(())
}

#[test]
fn stream_parser_requires_real_lsp_tool_results_for_oracles() -> Result<()> {
    let stream = [
        serde_json::json!({
            "type": "system",
            "subtype": "init",
            "tools": ["LSP"],
            "plugins": [{"name":"perl-lsp-rs","path":"plugin-cache/perl-lsp-rs"}],
            "plugin_errors": []
        }),
        serde_json::json!({"type":"assistant","message":{"content":[
            {"type":"tool_use","id":"d1","name":"LSP","input":{"operation":"goToDefinition","filePath":"app.pl","line":4,"character":20}},
            {"type":"tool_use","id":"r1","name":"LSP","input":{"operation":"findReferences","filePath":"app.pl","position":{"line":5,"character":15}}},
            {"type":"tool_use","id":"ds1","name":"LSP","input":{"operation":"documentSymbol","filePath":"lib/Widget.pm"}},
            {"type":"tool_use","id":"ws1","name":"LSP","input":{"operation":"workspaceSymbol","query":"Widget"}}
        ]}}),
        serde_json::json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"d1","content":"lib/Widget.pm:5 sub new"},
            {"type":"tool_result","tool_use_id":"r1","content":"greet app.pl:6 lib/Widget.pm:10"},
            {"type":"tool_result","tool_use_id":"ds1","content":"new greet"},
            {"type":"tool_result","tool_use_id":"ws1","content":"Widget lib/Widget.pm"}
        ]}}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value))
    .collect::<std::result::Result<Vec<_>, _>>()?
    .join("\n");

    let summary = parse_claude_stream(stream.as_bytes())?;
    let cells = assert_navigation_oracles(&summary)?;
    assert_eq!(cells.len(), 4);
    assert!(summary.plugin_errors.is_empty());
    assert!(summary.plugin_paths.contains_key(PLUGIN_NAME));
    Ok(())
}

#[test]
fn stream_parser_does_not_accept_model_prose_as_lsp_evidence() -> Result<()> {
    let stream = serde_json::json!({
        "type":"assistant",
        "message":{"content":[{"type":"text","text":"Widget.pm defines new and app.pl references greet"}]}
    });
    let summary = parse_claude_stream(serde_json::to_string(&stream)?.as_bytes())?;
    assert!(assert_navigation_oracles(&summary).is_err());
    Ok(())
}

#[test]
fn operation_classification_uses_only_the_operation_field() -> Result<()> {
    let definition_in_path = serde_json::json!({
        "operation": "workspaceSymbol",
        "query": "definition",
        "filePath": "definition/app.pl"
    });
    ensure!(classify_lsp_operation(&definition_in_path) == LspOperation::WorkspaceSymbols);
    ensure!(
        classify_lsp_operation(&serde_json::json!({"query": "definition"})) == LspOperation::Other
    );
    Ok(())
}

#[test]
fn navigation_oracles_reject_wrong_targets_and_negative_results() -> Result<()> {
    let mut summary = parse_claude_stream(
        serde_json::to_string(&serde_json::json!({
            "type": "system",
            "subtype": "init",
            "tools": ["LSP"],
            "plugins": [],
            "plugin_errors": []
        }))?
        .as_bytes(),
    )?;
    summary.lsp_invocations = vec![LspInvocation {
        id: "d1".to_string(),
        operation: LspOperation::Definition,
        input: serde_json::json!({
            "operation": "goToDefinition",
            "filePath": "definition.pm",
            "line": 4,
            "character": 20
        }),
        result: Some(Value::String("no results found for Widget.pm new".to_string())),
    }];
    ensure!(assert_navigation_oracles(&summary).is_err());
    Ok(())
}

#[test]
fn build_revision_must_identify_candidate_head() -> Result<()> {
    let root = repository_root()?;
    let candidate_sha = git_head(&root)?;
    let short = candidate_sha.get(..12).context("candidate SHA was unexpectedly short")?;
    let version = format!("perllsp test\nGit commit: {short}");
    ensure!(prove_build_revision(&version, &root, &candidate_sha)? == candidate_sha);
    ensure!(
        prove_build_revision("perllsp test\nGit commit: deadbee", &root, &candidate_sha).is_err(),
        "a mismatched perllsp revision must not produce exact-source evidence"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_process_matching_distinguishes_the_candidate_executable() -> Result<()> {
    let current = fs::canonicalize(std::env::current_exe()?)?;
    let pids = matching_linux_processes(&current)?;
    ensure!(pids.contains(&std::process::id()), "process matcher did not detect its candidate");

    let temp = TempDir::new()?;
    let wrong_candidate = temp.path().join("perllsp");
    fs::write(&wrong_candidate, b"not the running candidate")?;
    let wrong_candidate = fs::canonicalize(wrong_candidate)?;
    ensure!(
        !matching_linux_processes(&wrong_candidate)?.contains(&std::process::id()),
        "process matcher attributed this test process to the wrong candidate"
    );
    Ok(())
}

#[test]
fn same_extension_conflict_is_detected_from_loaded_plugin_packages() -> Result<()> {
    let temp = TempDir::new()?;
    let ours = temp.path().join("ours");
    let other = temp.path().join("other");
    fs::create_dir_all(&ours)?;
    fs::create_dir_all(&other)?;
    fs::write(
        other.join(".lsp.json"),
        r#"{"other":{"command":"other-lsp","extensionToLanguage":{".pl":"perl"}}}"#,
    )?;

    let plugins = BTreeMap::from([
        (PLUGIN_NAME.to_string(), ours),
        ("competing-perl-test-plugin".to_string(), other),
    ]);
    let extensions = BTreeSet::from([".pl".to_string(), ".pm".to_string()]);
    assert_eq!(
        conflicting_plugin_paths(&plugins, PLUGIN_NAME, &extensions)?,
        vec!["competing-perl-test-plugin".to_string()]
    );
    Ok(())
}
