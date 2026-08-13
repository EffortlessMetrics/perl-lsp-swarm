//! Claude Code integration lifecycle owned by the public `perllsp` product.
//!
//! Claude-specific process/marketplace mechanics stay behind [`ClaudeRunner`]
//! so CLI parsing, tests, installers, and future presentation adapters consume
//! one control plane instead of reimplementing Claude state discovery.

use serde_json::{Map, Value, json};
use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_VERSION: &str = "perllsp.claude_status.v1";
const MARKETPLACE_NAME: &str = "effortlessmetrics";
const MARKETPLACE_SOURCE: &str = "https://github.com/EffortlessMetrics/perl-lsp.git";
const MARKETPLACE_REPO_TOKEN: &str = "effortlessmetrics/perl-lsp";
const PLUGIN_SLUG: &str = "perl-lsp-rs";
const PLUGIN_ID: &str = "perl-lsp-rs@effortlessmetrics";
const PLUGIN_PATH: &str = "integrations/claude-code/plugins/perl-lsp-rs";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum HostState {
    Present,
    Missing,
    Error,
}

impl HostState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ResourceState {
    Present,
    Absent,
    Unsupported,
    Error,
}

impl ResourceState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unsupported => "unsupported",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Verdict {
    Ready,
    Degraded,
    ActionRequired,
    Unsupported,
    InstrumentError,
}

impl Verdict {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::ActionRequired => "action_required",
            Self::Unsupported => "unsupported",
            Self::InstrumentError => "instrument_error",
        }
    }

    const fn exit_code(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Degraded | Self::ActionRequired | Self::Unsupported => 2,
            Self::InstrumentError => 1,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct HostStatus {
    state: HostState,
    version: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ServerStatus {
    version: &'static str,
    path_visible_from_current_environment: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct MarketplaceStatus {
    state: ResourceState,
    source_matches_expected: Option<bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PluginStatus {
    state: ResourceState,
    enabled: Option<bool>,
    version: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct IntegrationStatus {
    host: HostStatus,
    server: ServerStatus,
    marketplace: MarketplaceStatus,
    plugin: PluginStatus,
    verdict: Verdict,
    reasons: Vec<&'static str>,
    next_actions: Vec<&'static str>,
}

impl IntegrationStatus {
    fn to_json(&self) -> Value {
        json!({
            "schema_version": SCHEMA_VERSION,
            "integration": "claude_code",
            "control_plane_version": env!("CARGO_PKG_VERSION"),
            "distribution_source": "first_party_effortlessmetrics",
            "host": {
                "state": self.host.state.as_str(),
                "version": self.host.version,
            },
            "server": {
                "binary": "perllsp",
                "version": self.server.version,
                "health": "healthy_current_process",
                "path_visible_from_current_environment": self.server.path_visible_from_current_environment,
            },
            "marketplace": {
                "name": MARKETPLACE_NAME,
                "state": self.marketplace.state.as_str(),
                "source_matches_expected": self.marketplace.source_matches_expected,
            },
            "plugin": {
                "slug": PLUGIN_SLUG,
                "id": PLUGIN_ID,
                "state": self.plugin.state.as_str(),
                "enabled": self.plugin.enabled,
                "version": self.plugin.version,
            },
            "ownership": {
                "active_provider": Value::Null,
                "competing_providers": [],
                "observation": "not_exposed_by_noninteractive_plugin_list",
            },
            "workspace": {
                "expected_root_contract": "${CLAUDE_PROJECT_DIR}",
            },
            "verdict": self.verdict.as_str(),
            "reasons": self.reasons,
            "next_actions": self.next_actions,
        })
    }
}

#[derive(Debug)]
struct CommandResult {
    success: bool,
    stdout: String,
    stderr: String,
}

trait ClaudeRunner {
    fn run(&mut self, args: &[&str]) -> io::Result<CommandResult>;
}

struct SystemClaudeRunner;

impl ClaudeRunner for SystemClaudeRunner {
    fn run(&mut self, args: &[&str]) -> io::Result<CommandResult> {
        let output = Command::new("claude").args(args).output()?;
        Ok(CommandResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Action {
    Status,
    Doctor,
    Install,
    Update,
    Uninstall,
    Help,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct Invocation {
    action: Action,
    json: bool,
}

/// Intercept the product-level `perllsp claude ...` command family before the
/// generic LSP launcher parses transport/server options.
pub(crate) fn try_run(args: &[String]) -> Option<u8> {
    if args.get(1).map(String::as_str) != Some("claude") {
        return None;
    }

    let invocation = match parse_invocation(&args[2..]) {
        Ok(invocation) => invocation,
        Err(reason) => {
            eprintln!("{reason}");
            print_help();
            return Some(1);
        }
    };

    if invocation.action == Action::Help {
        print_help();
        return Some(0);
    }

    let mut runner = SystemClaudeRunner;
    let path_visible = current_binary_visible_on_path();
    Some(run_invocation(&mut runner, invocation, path_visible))
}

fn parse_invocation(args: &[String]) -> Result<Invocation, &'static str> {
    let mut action = None;
    let mut json_output = false;

    for arg in args {
        match arg.as_str() {
            "status" => set_action(&mut action, Action::Status)?,
            "doctor" => set_action(&mut action, Action::Doctor)?,
            "install" => set_action(&mut action, Action::Install)?,
            "update" => set_action(&mut action, Action::Update)?,
            "uninstall" | "remove" | "rm" => set_action(&mut action, Action::Uninstall)?,
            "--json" => json_output = true,
            "--help" | "-h" => set_action(&mut action, Action::Help)?,
            "--scope" | "user" => {
                // User scope is the only admitted lifecycle scope in v1. The
                // token pair is accepted for script readability; project/local
                // scopes would mutate repository state and need their own proof.
            }
            "project" | "local" | "managed" => {
                return Err("perllsp claude currently supports user scope only");
            }
            _ => return Err("unknown perllsp claude argument"),
        }
    }

    Ok(Invocation {
        action: action.unwrap_or(Action::Help),
        json: json_output,
    })
}

fn set_action(slot: &mut Option<Action>, action: Action) -> Result<(), &'static str> {
    if slot.is_some() {
        return Err("perllsp claude accepts exactly one lifecycle action");
    }
    *slot = Some(action);
    Ok(())
}

fn run_invocation<R: ClaudeRunner>(runner: &mut R, invocation: Invocation, path_visible: bool) -> u8 {
    match invocation.action {
        Action::Status | Action::Doctor => {
            let status = collect_status(runner, path_visible);
            render_status(&status, invocation.json);
            status.verdict.exit_code()
        }
        Action::Install => run_install(runner, invocation.json, path_visible),
        Action::Update => run_update(runner, invocation.json, path_visible),
        Action::Uninstall => run_uninstall(runner, invocation.json, path_visible),
        Action::Help => 0,
    }
}

fn run_install<R: ClaudeRunner>(runner: &mut R, json_output: bool, path_visible: bool) -> u8 {
    let initial = collect_status(runner, path_visible);
    if !mutation_preconditions_met(&initial) {
        render_status(&initial, json_output);
        return initial.verdict.exit_code();
    }

    if initial.marketplace.state == ResourceState::Absent {
        let result = run_mutation(
            runner,
            &[
                "plugin",
                "marketplace",
                "add",
                MARKETPLACE_SOURCE,
                "--scope",
                "user",
                "--sparse",
                ".claude-plugin",
                PLUGIN_PATH,
            ],
        );
        if !result {
            render_operation_failure("claude_marketplace_add_failed", json_output);
            return 1;
        }
    }

    match initial.plugin.state {
        ResourceState::Absent => {
            if !run_mutation(
                runner,
                &["plugin", "install", PLUGIN_ID, "--scope", "user"],
            ) {
                render_operation_failure("claude_plugin_install_failed", json_output);
                return 1;
            }
        }
        ResourceState::Present if initial.plugin.enabled == Some(false) => {
            if !run_mutation(runner, &["plugin", "enable", PLUGIN_ID, "--scope", "user"]) {
                render_operation_failure("claude_plugin_enable_failed", json_output);
                return 1;
            }
        }
        ResourceState::Present => {}
        ResourceState::Unsupported | ResourceState::Error => {
            render_status(&initial, json_output);
            return initial.verdict.exit_code();
        }
    }

    let final_status = collect_status(runner, path_visible);
    render_status(&final_status, json_output);
    final_status.verdict.exit_code()
}

fn run_update<R: ClaudeRunner>(runner: &mut R, json_output: bool, path_visible: bool) -> u8 {
    let initial = collect_status(runner, path_visible);
    if !mutation_preconditions_met(&initial) {
        render_status(&initial, json_output);
        return initial.verdict.exit_code();
    }

    if initial.marketplace.state != ResourceState::Present
        || initial.plugin.state != ResourceState::Present
    {
        render_status(&initial, json_output);
        return 2;
    }

    if !run_mutation(runner, &["plugin", "marketplace", "update", MARKETPLACE_NAME]) {
        render_operation_failure("claude_marketplace_update_failed", json_output);
        return 1;
    }
    if !run_mutation(runner, &["plugin", "update", PLUGIN_ID, "--scope", "user"]) {
        render_operation_failure("claude_plugin_update_failed", json_output);
        return 1;
    }

    let final_status = collect_status(runner, path_visible);
    render_status(&final_status, json_output);
    final_status.verdict.exit_code()
}

fn run_uninstall<R: ClaudeRunner>(runner: &mut R, json_output: bool, path_visible: bool) -> u8 {
    let initial = collect_status(runner, path_visible);
    if initial.host.state != HostState::Present {
        render_status(&initial, json_output);
        return initial.verdict.exit_code();
    }

    match initial.plugin.state {
        ResourceState::Absent => {
            render_operation_result("already_absent", json_output);
            0
        }
        ResourceState::Present => {
            if !run_mutation(runner, &["plugin", "uninstall", PLUGIN_ID, "--scope", "user"]) {
                render_operation_failure("claude_plugin_uninstall_failed", json_output);
                return 1;
            }
            render_operation_result("uninstalled", json_output);
            0
        }
        ResourceState::Unsupported | ResourceState::Error => {
            render_status(&initial, json_output);
            initial.verdict.exit_code()
        }
    }
}

fn mutation_preconditions_met(status: &IntegrationStatus) -> bool {
    status.host.state == HostState::Present
        && status.marketplace.state != ResourceState::Unsupported
        && status.marketplace.state != ResourceState::Error
        && status.plugin.state != ResourceState::Unsupported
        && status.plugin.state != ResourceState::Error
        && status.marketplace.source_matches_expected != Some(false)
}

fn run_mutation<R: ClaudeRunner>(runner: &mut R, args: &[&str]) -> bool {
    matches!(runner.run(args), Ok(result) if result.success)
}

fn collect_status<R: ClaudeRunner>(runner: &mut R, path_visible: bool) -> IntegrationStatus {
    let host = probe_host(runner);
    let server = ServerStatus {
        version: env!("CARGO_PKG_VERSION"),
        path_visible_from_current_environment: path_visible,
    };

    if host.state != HostState::Present {
        return finish_status(
            host,
            server,
            MarketplaceStatus {
                state: ResourceState::Absent,
                source_matches_expected: None,
            },
            PluginStatus {
                state: ResourceState::Absent,
                enabled: None,
                version: None,
            },
        );
    }

    let marketplace = probe_marketplace(runner);
    let plugin = probe_plugin(runner);
    finish_status(host, server, marketplace, plugin)
}

fn probe_host<R: ClaudeRunner>(runner: &mut R) -> HostStatus {
    match runner.run(&["--version"]) {
        Ok(result) if result.success => HostStatus {
            state: HostState::Present,
            version: first_nonempty_line(&result.stdout).or_else(|| first_nonempty_line(&result.stderr)),
        },
        Ok(_) => HostStatus {
            state: HostState::Error,
            version: None,
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => HostStatus {
            state: HostState::Missing,
            version: None,
        },
        Err(_) => HostStatus {
            state: HostState::Error,
            version: None,
        },
    }
}

fn probe_marketplace<R: ClaudeRunner>(runner: &mut R) -> MarketplaceStatus {
    let result = match runner.run(&["plugin", "marketplace", "list", "--json"]) {
        Ok(result) => result,
        Err(_) => {
            return MarketplaceStatus {
                state: ResourceState::Error,
                source_matches_expected: None,
            };
        }
    };

    if !result.success {
        return MarketplaceStatus {
            state: if command_surface_unsupported(&result) {
                ResourceState::Unsupported
            } else {
                ResourceState::Error
            },
            source_matches_expected: None,
        };
    }

    parse_marketplace_list(&result.stdout).unwrap_or(MarketplaceStatus {
        state: ResourceState::Error,
        source_matches_expected: None,
    })
}

fn probe_plugin<R: ClaudeRunner>(runner: &mut R) -> PluginStatus {
    let result = match runner.run(&["plugin", "list", "--json"]) {
        Ok(result) => result,
        Err(_) => {
            return PluginStatus {
                state: ResourceState::Error,
                enabled: None,
                version: None,
            };
        }
    };

    if !result.success {
        return PluginStatus {
            state: if command_surface_unsupported(&result) {
                ResourceState::Unsupported
            } else {
                ResourceState::Error
            },
            enabled: None,
            version: None,
        };
    }

    parse_plugin_list(&result.stdout).unwrap_or(PluginStatus {
        state: ResourceState::Error,
        enabled: None,
        version: None,
    })
}

fn parse_marketplace_list(raw: &str) -> Option<MarketplaceStatus> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let objects = collection_objects(&value, "marketplaces")?;
    let mut recognized = 0usize;

    for object in objects {
        let Some(identity) = object_string(object, &["name", "id", "marketplace"]) else {
            continue;
        };
        recognized += 1;
        if identity.eq_ignore_ascii_case(MARKETPLACE_NAME) {
            let source_matches_expected = object
                .get("source")
                .map(value_contains_expected_repo);
            return Some(MarketplaceStatus {
                state: ResourceState::Present,
                source_matches_expected,
            });
        }
    }

    if recognized > 0 || collection_is_empty(&value, "marketplaces") {
        Some(MarketplaceStatus {
            state: ResourceState::Absent,
            source_matches_expected: None,
        })
    } else {
        None
    }
}

fn parse_plugin_list(raw: &str) -> Option<PluginStatus> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let objects = collection_objects(&value, "plugins")?;
    let mut recognized = 0usize;

    for object in objects {
        let Some(identity) = object_string(object, &["id", "name", "plugin"]) else {
            continue;
        };
        recognized += 1;
        if !plugin_identity_matches(object, identity) {
            continue;
        }

        return Some(PluginStatus {
            state: ResourceState::Present,
            enabled: plugin_enabled(object),
            version: object_string(object, &["version"]).map(str::to_owned),
        });
    }

    if recognized > 0 || collection_is_empty(&value, "plugins") {
        Some(PluginStatus {
            state: ResourceState::Absent,
            enabled: None,
            version: None,
        })
    } else {
        None
    }
}

fn collection_objects<'a>(value: &'a Value, key: &str) -> Option<Vec<&'a Map<String, Value>>> {
    let values = match value {
        Value::Array(values) => values,
        Value::Object(object) => object.get(key)?.as_array()?,
        _ => return None,
    };

    let mut objects = Vec::with_capacity(values.len());
    for value in values {
        objects.push(value.as_object()?);
    }
    Some(objects)
}

fn collection_is_empty(value: &Value, key: &str) -> bool {
    match value {
        Value::Array(values) => values.is_empty(),
        Value::Object(object) => {
            object.get(key).and_then(Value::as_array).is_some_and(Vec::is_empty)
        }
        _ => false,
    }
}

fn object_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| object.get(*key).and_then(Value::as_str))
}

fn plugin_identity_matches(object: &Map<String, Value>, identity: &str) -> bool {
    if identity.eq_ignore_ascii_case(PLUGIN_ID) {
        return true;
    }
    if !identity.eq_ignore_ascii_case(PLUGIN_SLUG) {
        return false;
    }

    object_string(object, &["marketplace", "marketplaceName", "sourceMarketplace"])
        .is_some_and(|marketplace| marketplace.eq_ignore_ascii_case(MARKETPLACE_NAME))
}

fn plugin_enabled(object: &Map<String, Value>) -> Option<bool> {
    if let Some(enabled) = object.get("enabled").and_then(Value::as_bool) {
        return Some(enabled);
    }

    object_string(object, &["status", "state"]).and_then(|state| {
        if state.eq_ignore_ascii_case("enabled") || state.eq_ignore_ascii_case("active") {
            Some(true)
        } else if state.eq_ignore_ascii_case("disabled") || state.eq_ignore_ascii_case("inactive") {
            Some(false)
        } else {
            None
        }
    })
}

fn value_contains_expected_repo(value: &Value) -> bool {
    match value {
        Value::String(value) => value.to_ascii_lowercase().contains(MARKETPLACE_REPO_TOKEN),
        Value::Array(values) => values.iter().any(value_contains_expected_repo),
        Value::Object(object) => object.values().any(value_contains_expected_repo),
        _ => false,
    }
}

fn command_surface_unsupported(result: &CommandResult) -> bool {
    let diagnostic = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
    diagnostic.contains("unknown command")
        || diagnostic.contains("unknown option")
        || diagnostic.contains("unrecognized")
        || diagnostic.contains("not a command")
}

fn first_nonempty_line(value: &str) -> Option<String> {
    value.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
}

fn finish_status(
    host: HostStatus,
    server: ServerStatus,
    marketplace: MarketplaceStatus,
    plugin: PluginStatus,
) -> IntegrationStatus {
    let mut reasons = Vec::new();
    let mut next_actions = Vec::new();

    if !server.path_visible_from_current_environment {
        reasons.push("perllsp_not_path_visible");
        next_actions.push("repair_perllsp_path");
    }

    match host.state {
        HostState::Missing => {
            reasons.push("claude_not_found");
            next_actions.push("install_claude_code");
        }
        HostState::Error => reasons.push("claude_probe_failed"),
        HostState::Present => {}
    }

    match marketplace.state {
        ResourceState::Absent => {
            reasons.push("effortlessmetrics_marketplace_missing");
            next_actions.push("perllsp_claude_install");
        }
        ResourceState::Unsupported => reasons.push("claude_marketplace_cli_unsupported"),
        ResourceState::Error => reasons.push("claude_marketplace_state_unreadable"),
        ResourceState::Present => match marketplace.source_matches_expected {
            Some(false) => {
                reasons.push("unexpected_effortlessmetrics_marketplace_source");
                next_actions.push("inspect_claude_marketplace_source");
            }
            None => reasons.push("claude_marketplace_source_unknown"),
            Some(true) => {}
        },
    }

    match plugin.state {
        ResourceState::Absent => {
            reasons.push("perl_lsp_rs_plugin_missing");
            next_actions.push("perllsp_claude_install");
        }
        ResourceState::Unsupported => reasons.push("claude_plugin_cli_unsupported"),
        ResourceState::Error => reasons.push("claude_plugin_state_unreadable"),
        ResourceState::Present => match plugin.enabled {
            Some(false) => {
                reasons.push("perl_lsp_rs_plugin_disabled");
                next_actions.push("perllsp_claude_install");
            }
            None => reasons.push("perl_lsp_rs_enable_state_unknown"),
            Some(true) => {}
        },
    }

    dedup_stable(&mut next_actions);

    let verdict = if host.state == HostState::Error
        || marketplace.state == ResourceState::Error
        || plugin.state == ResourceState::Error
    {
        Verdict::InstrumentError
    } else if marketplace.state == ResourceState::Unsupported
        || plugin.state == ResourceState::Unsupported
    {
        Verdict::Unsupported
    } else if marketplace.source_matches_expected.is_none()
        && marketplace.state == ResourceState::Present
        || plugin.enabled.is_none() && plugin.state == ResourceState::Present
    {
        Verdict::Degraded
    } else if reasons.is_empty() {
        Verdict::Ready
    } else {
        Verdict::ActionRequired
    };

    IntegrationStatus {
        host,
        server,
        marketplace,
        plugin,
        verdict,
        reasons,
        next_actions,
    }
}

fn dedup_stable(values: &mut Vec<&'static str>) {
    let mut index = 0usize;
    while index < values.len() {
        if values[..index].contains(&values[index]) {
            values.remove(index);
        } else {
            index += 1;
        }
    }
}

fn current_binary_visible_on_path() -> bool {
    let current = match env::current_exe().and_then(|path| path.canonicalize()) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let Some(path_value) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&path_value).any(|directory| path_candidate_matches(&current, &directory))
}

fn path_candidate_matches(current: &Path, directory: &Path) -> bool {
    let candidates: &[&str] = if cfg!(windows) {
        &["perllsp.exe", "perllsp"]
    } else {
        &["perllsp"]
    };

    candidates.iter().any(|name| {
        let candidate: PathBuf = directory.join(name);
        candidate.canonicalize().is_ok_and(|path| path == current)
    })
}

fn render_status(status: &IntegrationStatus, json_output: bool) {
    if json_output {
        match serde_json::to_string_pretty(&status.to_json()) {
            Ok(rendered) => println!("{rendered}"),
            Err(_) => eprintln!("failed to render perllsp Claude status JSON"),
        }
        return;
    }

    println!("Claude Code integration");
    println!("  Host:        {}", status.host.state.as_str());
    if let Some(version) = &status.host.version {
        println!("  Host version: {version}");
    }
    println!(
        "  Server PATH: {}",
        if status.server.path_visible_from_current_environment {
            "visible"
        } else {
            "not visible"
        }
    );
    println!("  Marketplace: {}", status.marketplace.state.as_str());
    println!("  Plugin:      {}", status.plugin.state.as_str());
    println!("  Verdict:     {}", status.verdict.as_str());
    for reason in &status.reasons {
        println!("  Reason:      {reason}");
    }
    for action in &status.next_actions {
        println!("  Next action: {action}");
    }
}

fn render_operation_failure(reason: &'static str, json_output: bool) {
    if json_output {
        println!(
            "{}",
            json!({
                "schema_version": SCHEMA_VERSION,
                "operation_result": "failed",
                "reason": reason,
            })
        );
    } else {
        eprintln!("Claude integration operation failed: {reason}");
    }
}

fn render_operation_result(result: &'static str, json_output: bool) {
    if json_output {
        println!(
            "{}",
            json!({
                "schema_version": SCHEMA_VERSION,
                "operation_result": result,
            })
        );
    } else {
        println!("Claude integration: {result}");
    }
}

fn print_help() {
    println!(
        "perllsp claude <status|doctor|install|update|uninstall> [--json]\n\
         \n\
         Manage the first-party Claude Code native-LSP integration.\n\
         status/doctor are read-only; install/update/uninstall use Claude's supported plugin CLI.\n\
         User scope is the only lifecycle scope admitted by this version."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const MARKETPLACE_READY: &str = r#"[{"name":"effortlessmetrics","source":{"source":"github","repo":"EffortlessMetrics/perl-lsp"}}]"#;
    const PLUGIN_READY: &str = r#"[{"name":"perl-lsp-rs","marketplace":"effortlessmetrics","version":"0.1.0","enabled":true}]"#;

    struct FakeRunner {
        responses: VecDeque<(Vec<String>, io::Result<CommandResult>)>,
    }

    impl FakeRunner {
        fn new(responses: Vec<(Vec<String>, io::Result<CommandResult>)>) -> Self {
            Self {
                responses: responses.into(),
            }
        }
    }

    impl ClaudeRunner for FakeRunner {
        fn run(&mut self, args: &[&str]) -> io::Result<CommandResult> {
            let Some((expected, response)) = self.responses.pop_front() else {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected Claude command"));
            };
            let actual = args.iter().map(|value| (*value).to_string()).collect::<Vec<_>>();
            if actual != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("expected {expected:?}, got {actual:?}"),
                ));
            }
            response
        }
    }

    fn command(args: &[&str], stdout: &str) -> (Vec<String>, io::Result<CommandResult>) {
        (
            args.iter().map(|value| (*value).to_string()).collect(),
            Ok(CommandResult {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            }),
        )
    }

    #[test]
    fn ready_status_requires_expected_marketplace_plugin_and_path() {
        let mut runner = FakeRunner::new(vec![
            command(&["--version"], "2.1.231 (Claude Code)\n"),
            command(&["plugin", "marketplace", "list", "--json"], MARKETPLACE_READY),
            command(&["plugin", "list", "--json"], PLUGIN_READY),
        ]);

        let status = collect_status(&mut runner, true);
        assert_eq!(status.verdict, Verdict::Ready);
        assert!(status.reasons.is_empty());
    }

    #[test]
    fn missing_claude_is_action_required_not_a_fake_ready_state() {
        let mut runner = FakeRunner::new(vec![(
            vec!["--version".to_string()],
            Err(io::Error::new(io::ErrorKind::NotFound, "claude missing")),
        )]);

        let status = collect_status(&mut runner, true);
        assert_eq!(status.verdict, Verdict::ActionRequired);
        assert!(status.reasons.contains(&"claude_not_found"));
    }

    #[test]
    fn install_adds_missing_marketplace_and_plugin_then_rechecks() {
        let mut runner = FakeRunner::new(vec![
            command(&["--version"], "2.1.231 (Claude Code)\n"),
            command(&["plugin", "marketplace", "list", "--json"], "[]"),
            command(&["plugin", "list", "--json"], "[]"),
            command(
                &[
                    "plugin",
                    "marketplace",
                    "add",
                    MARKETPLACE_SOURCE,
                    "--scope",
                    "user",
                    "--sparse",
                    ".claude-plugin",
                    PLUGIN_PATH,
                ],
                "",
            ),
            command(&["plugin", "install", PLUGIN_ID, "--scope", "user"], ""),
            command(&["--version"], "2.1.231 (Claude Code)\n"),
            command(&["plugin", "marketplace", "list", "--json"], MARKETPLACE_READY),
            command(&["plugin", "list", "--json"], PLUGIN_READY),
        ]);

        let code = run_install(&mut runner, true, true);
        assert_eq!(code, 0);
    }

    #[test]
    fn changed_json_shape_fails_closed_as_instrument_error() {
        let mut runner = FakeRunner::new(vec![
            command(&["--version"], "2.1.231 (Claude Code)\n"),
            command(
                &["plugin", "marketplace", "list", "--json"],
                r#"{"unexpected":"shape"}"#,
            ),
            command(&["plugin", "list", "--json"], PLUGIN_READY),
        ]);

        let status = collect_status(&mut runner, true);
        assert_eq!(status.verdict, Verdict::InstrumentError);
        assert!(status.reasons.contains(&"claude_marketplace_state_unreadable"));
    }

    #[test]
    fn wrong_marketplace_source_is_action_required_and_never_mutated() {
        let marketplace = r#"[{"name":"effortlessmetrics","source":{"source":"github","repo":"someone/else"}}]"#;
        let mut runner = FakeRunner::new(vec![
            command(&["--version"], "2.1.231 (Claude Code)\n"),
            command(&["plugin", "marketplace", "list", "--json"], marketplace),
            command(&["plugin", "list", "--json"], PLUGIN_READY),
        ]);

        let status = collect_status(&mut runner, true);
        assert_eq!(status.verdict, Verdict::ActionRequired);
        assert!(status.reasons.contains(&"unexpected_effortlessmetrics_marketplace_source"));
        assert!(!mutation_preconditions_met(&status));
    }
}
