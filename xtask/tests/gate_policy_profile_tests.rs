use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

fn project_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir
}

#[derive(Debug, Deserialize)]
struct GatePolicyDoc {
    gates: Vec<PolicyGate>,
}

#[derive(Debug, Deserialize)]
struct PolicyGate {
    name: String,
    tier: String,
    description: String,
    #[serde(default = "default_true")]
    required: bool,
    #[serde(default)]
    command: String,
    #[serde(default)]
    quarantine: bool,
    timeout_seconds: Option<u64>,
    #[serde(default)]
    retry_count: Option<u32>,
    budgets: Option<GateBudgets>,
    planning: Option<GatePlanning>,
}

#[derive(Debug, Deserialize)]
struct GateBudgets {
    max_duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GatePlanning {
    role: String,
    #[serde(default)]
    packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GateRegistryDoc {
    gate: Vec<RegistryGate>,
}

#[derive(Debug, Deserialize)]
struct RegistryGate {
    id: String,
    #[serde(default)]
    blocking: bool,
}

fn default_true() -> bool {
    true
}

#[test]
fn parser_corpus_pr_policy_is_unambiguous() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    let common = gates.get("common_corpus_clean").ok_or("missing common_corpus_clean gate")?;
    let parser = gates.get("parser_corpus_ratchet").ok_or("missing parser_corpus_ratchet gate")?;
    let cpan = gates.get("cpan_corpus_ratchet").ok_or("missing cpan_corpus_ratchet gate")?;

    assert_eq!(common.tier, "merge_gate");
    assert!(common.required, "common_corpus_clean must stay PR-blocking");
    assert!(
        common.timeout_seconds.unwrap_or_default() >= 240,
        "common_corpus_clean timeout must include cold CI xtask startup"
    );
    assert!(
        common.budgets.as_ref().and_then(|budget| budget.max_duration_ms).unwrap_or_default()
            >= 180_000,
        "common_corpus_clean duration budget must reflect CI startup overhead"
    );

    assert_eq!(parser.tier, "merge_gate");
    assert!(!parser.required, "parser_corpus_ratchet must be advisory in PR merge-gate profile");

    assert_eq!(cpan.tier, "merge_gate");
    assert!(!cpan.required, "cpan_corpus_ratchet must never be PR-blocking");

    Ok(())
}

#[test]
fn release_history_pr_gates_have_realistic_timeout_headroom()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    for gate_name in ["release_history", "release_history_check"] {
        let gate = gates.get(gate_name).ok_or_else(|| format!("missing {gate_name} gate"))?;
        assert_eq!(gate.tier, "pr_fast", "{gate_name} must stay in pr-fast");
        assert!(gate.required, "{gate_name} must stay PR-blocking");
        assert!(
            gate.timeout_seconds.unwrap_or_default() >= 120,
            "{gate_name} timeout must include cold or partially-cold xtask startup"
        );
        assert!(
            gate.budgets.as_ref().and_then(|budget| budget.max_duration_ms).unwrap_or_default()
                >= 90_000,
            "{gate_name} duration budget must reflect observed pr-fast runtime"
        );
    }

    Ok(())
}

#[test]
fn conflict_marker_gate_has_local_runtime_headroom() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gate = parsed
        .gates
        .into_iter()
        .find(|gate| gate.name == "check_conflict_markers")
        .ok_or("missing check_conflict_markers gate")?;

    assert_eq!(gate.tier, "pr_fast", "conflict marker scan must stay in pr-fast");
    assert!(gate.required, "conflict marker scan must stay PR-blocking");
    assert!(
        gate.timeout_seconds.unwrap_or_default() >= 120,
        "conflict marker timeout must include Windows and mounted-worktree headroom"
    );
    assert!(
        gate.budgets.as_ref().and_then(|budget| budget.max_duration_ms).unwrap_or_default()
            >= 120_000,
        "conflict marker budget must reflect observed local runtime"
    );

    Ok(())
}

#[test]
fn routed_integration_test_gate_has_cold_ci_headroom() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gate = parsed
        .gates
        .into_iter()
        .find(|gate| gate.name == "unit_routed_full")
        .ok_or("missing unit_routed_full gate")?;

    assert_eq!(gate.tier, "pr_fast", "unit_routed_full must stay in pr-fast");
    assert!(gate.required, "unit_routed_full must stay PR-blocking");
    assert_eq!(
        gate.planning.as_ref().map(|planning| planning.role.as_str()),
        Some("rust_scoped"),
        "unit_routed_full must stay routed to changed Rust packages"
    );
    assert!(
        gate.timeout_seconds.unwrap_or_default() >= 1_500,
        "unit_routed_full timeout must include cold integration-test build headroom"
    );
    assert!(
        gate.budgets.as_ref().and_then(|budget| budget.max_duration_ms).unwrap_or_default()
            >= 1_320_000,
        "unit_routed_full duration budget must reflect observed cold PR-fast runtime"
    );

    Ok(())
}

/// Contract guard for issue #5934: `unit_parser_stack_full` must remain a required
/// merge_gate tier gate that is not quarantined.
///
/// This gate covers lib tests for perl-parser, perl-lexer, and perl-parser-core —
/// the only required gate that runs these packages. Quarantining or demoting it
/// would silently remove coverage from the entire parser stack.
#[test]
fn parser_stack_gate_stays_required_merge_gate() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gate = parsed.gates.into_iter().find(|gate| gate.name == "unit_parser_stack_full").ok_or(
        "missing unit_parser_stack_full gate — it covers parser/lexer/parser-core lib tests",
    )?;

    assert_eq!(gate.tier, "merge_gate", "unit_parser_stack_full must stay in the merge_gate tier");
    assert!(gate.required, "unit_parser_stack_full must stay PR-blocking");
    assert!(
        !gate.quarantine,
        "unit_parser_stack_full must not be quarantined — quarantine silently removes \
         the only required coverage for the parser/lexer/parser-core lib surface"
    );

    // Verify the gate exercises --lib with locking (not --tests, which is deferred to
    // the scoped integration lane; not unlocked, which would allow dep drift).
    let tokens: Vec<&str> = gate.command.split_whitespace().collect();
    for flag in ["--lib", "--locked"] {
        assert!(
            tokens.contains(&flag),
            "unit_parser_stack_full must keep {flag} as an exact argument"
        );
    }

    Ok(())
}

/// Contract guard for issue #6107: the bounded parser integration proof stays
/// required, manifest-driven, and above its initial denominator.
#[test]
fn parser_integration_gate_is_required_and_manifest_driven()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;
    let gate = parsed
        .gates
        .into_iter()
        .find(|gate| gate.name == "parser_integration")
        .ok_or("missing parser_integration gate — #6107 proof is not wired")?;

    assert_eq!(gate.tier, "merge_gate");
    assert!(gate.required, "parser integration proof must be required");
    assert!(!gate.quarantine, "parser integration proof must not be quarantined");
    assert!(
        gate.command.contains("scripts/ci/run_parser_integration.py"),
        "parser integration gate must use the manifest-driven runner; current command: {}",
        gate.command
    );
    assert!(
        gate.timeout_seconds.unwrap_or_default() >= 900,
        "parser integration gate needs cold-build headroom"
    );

    let manifest = root.join(".ci/parser-integration-targets.json");
    let manifest_content = fs::read_to_string(manifest)?;
    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_content)?;
    let targets = manifest_json
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("parser integration manifest must contain targets")?;
    assert!(
        targets.len() >= 7,
        "parser integration manifest must not shrink below its initial proof set"
    );

    Ok(())
}

/// (issue #6845) The former `inline_completion_contract` gate chained four
/// Cargo commands with `&&`, masking which contract failed and preventing
/// later contracts from running. It is now one ordered four-row family:
/// every change to either owning package selects every child. The family
/// envelope was re-sized for cold-cache PR Smoke compilation (#11797): the
/// two members that compile the perl-lsp-rs-core dependency graph
/// (`inline_completion_registration`, `inline_completion_core`) carry
/// 240s/210000ms each, while `lsp_registration_contract` and
/// `lsp_capability_snapshots` keep their original 150s/135000ms, for a
/// 780-second timeout / 690000ms budget envelope.
#[test]
fn inline_completion_gates_are_split_scoped_ordered_and_budgeted()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let ordered_names: Vec<String> = parsed.gates.iter().map(|gate| gate.name.clone()).collect();
    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    assert!(
        !gates.contains_key("inline_completion_contract"),
        "inline_completion_contract must be removed — the && composite masks children"
    );

    let expected: &[(&str, &str)] = &[
        (
            "inline_completion_registration",
            "cargo test -p perl-lsp-rs --locked --test lsp_inline_completion_registration_tests",
        ),
        (
            "lsp_registration_contract",
            "cargo test -p perl-lsp-rs --locked --test lsp_registration_tests",
        ),
        ("lsp_capability_snapshots", "cargo test -p perl-lsp-rs --locked --test lsp_cap_snap"),
        (
            "inline_completion_core",
            "cargo test -p perl-lsp-rs-core --locked --lib inline_completion",
        ),
    ];
    let expected_names: Vec<_> = expected.iter().map(|(name, _)| *name).collect();
    let family_start = ordered_names
        .windows(expected_names.len())
        .position(|window| window.iter().eq(expected_names.iter()))
        .ok_or("inline-completion gate family must be contiguous and ordered")?;
    assert_eq!(
        ordered_names.get(family_start + expected_names.len()).map(String::as_str),
        Some("inline_completion_quality_receipt"),
        "quality receipt must remain the family boundary immediately after the four children"
    );

    let mut timeout_total = 0_u64;
    let mut budget_total = 0_u64;
    for &(gate_name, expected_command) in expected {
        let gate = gates
            .get(gate_name)
            .ok_or_else(|| format!("gate '{gate_name}' not found in gate-policy.yaml"))?;
        let planning = gate
            .planning
            .as_ref()
            .ok_or_else(|| format!("gate '{gate_name}' missing planning field"))?;

        assert_eq!(gate.tier, "pr_fast", "gate '{gate_name}' must remain in pr_fast");
        assert!(
            gate.required,
            "gate '{gate_name}' must remain required within the pr_fast runner; this does not claim GitHub protection"
        );
        assert_eq!(planning.role, "rust_package_scoped");
        assert_eq!(
            planning.packages,
            vec!["perl-lsp-rs", "perl-lsp-rs-core"],
            "every child must be selected by a change to either formerly governed package"
        );
        assert_eq!(gate.command, expected_command);
        assert!(!gate.command.contains("&&"));
        timeout_total += gate.timeout_seconds.ok_or("child timeout must be explicit")?;
        budget_total += gate
            .budgets
            .as_ref()
            .and_then(|budget| budget.max_duration_ms)
            .ok_or("child budget must be explicit")?;
    }
    // 240 + 150 + 150 + 240: the two perl-lsp-rs-core-compiling members were
    // re-sized for cold-cache PR Smoke compilation (#11797); the two smaller
    // lsp_registration_contract / lsp_capability_snapshots members keep the
    // original split envelope. Pinning the exact sum keeps any future drift
    // in any member a deliberate, review-visible change.
    assert_eq!(timeout_total, 780, "family hard timeout must match the #11797 cold-cache sizing");
    assert_eq!(
        budget_total, 690_000,
        "family duration budget must match the #11797 cold-cache sizing"
    );

    let quality = gates
        .get("inline_completion_quality_receipt")
        .ok_or("missing inline_completion_quality_receipt gate")?;
    let quality_planning =
        quality.planning.as_ref().ok_or("inline_completion_quality_receipt missing planning")?;
    assert_eq!(quality.tier, "pr_fast");
    assert!(quality.required, "quality receipt must remain required within pr_fast");
    assert_eq!(quality_planning.role, "rust_package_scoped");
    assert_eq!(quality_planning.packages, vec!["perl-lsp-rs-core", "xtask"]);

    Ok(())
}

#[test]
fn gate_registry_alignment_prevents_stale_parser_wiring() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();

    let policy: GatePolicyDoc =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join(".ci/gate-policy.yaml"))?)?;
    let registry: GateRegistryDoc =
        toml::from_str(&fs::read_to_string(root.join(".ci/GATE_REGISTRY.toml"))?)?;

    let policy_by_name: HashMap<_, _> =
        policy.gates.into_iter().map(|gate| (gate.name.clone(), gate.required)).collect();
    let registry_by_id: HashMap<_, _> =
        registry.gate.into_iter().map(|gate| (gate.id.clone(), gate.blocking)).collect();

    let pairs = [
        ("parser_corpus_ratchet", "parser-corpus-ratchet"),
        ("cpan_corpus_ratchet", "cpan-corpus-ratchet"),
        ("parser_audit_closeout", "parser-audit-closeout"),
    ];

    for (policy_name, registry_id) in pairs {
        let required = policy_by_name
            .get(policy_name)
            .ok_or_else(|| format!("missing policy gate: {policy_name}"))?;
        let blocking = registry_by_id
            .get(registry_id)
            .ok_or_else(|| format!("missing registry gate: {registry_id}"))?;
        assert_eq!(
            required, blocking,
            "gate-policy and gate-registry must agree for {policy_name}/{registry_id}"
        );
    }

    Ok(())
}

/// The six crates that `unit_lsp_full` covered before #5425 split the lane.
///
/// This list is the contract. If a crate legitimately leaves the LSP unit
/// surface, change it here deliberately — do not let it drift by editing one
/// `command:` string.
/// The crate the split isolates. It carries 2815 lib tests — nearly twice the
/// other five combined — which is what pushed the original single lane into its
/// ceiling. Keeping it alone is the split's load-bearing property.
const LSP_CORE_CRATE: &str = "perl-lsp-rs-core";

const LSP_UNIT_SURFACE: [&str; 6] = [
    "perl-lsp-perltidy",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perl-lsp-ux-tests",
    "perl-subprocess-runtime",
    "perllsp",
];

/// Extract the `-p <crate>` package arguments from a gate command string.
fn package_args(command: &str) -> Vec<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| **token == "-p")
        .filter_map(|(index, _)| tokens.get(index + 1))
        .map(|package| (*package).to_string())
        .collect()
}

const DAP_HELPER_TARGETS: [&str; 4] = [
    "eval_ref_cache_miss_resume_tests",
    "dap_evaluate_comprehensive_tests",
    "dap_variable_reference_hardening_tests",
    "pause_signal_delivery_tests",
];

fn dap_helper_command_error(command: &str) -> Option<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let separator_count = tokens.iter().filter(|token| **token == "&&").count();
    if separator_count != 1 {
        return Some("DAP helper command must have exactly one && separator".to_string());
    }
    if tokens.iter().any(|token| matches!(*token, ";" | "||")) {
        return Some("DAP helper command must not swallow failures".to_string());
    }

    let separator = tokens.iter().position(|token| *token == "&&")?;
    let helper_tokens = &tokens[separator + 1..];

    // Cargo's `--` boundary: everything after the first standalone `--` is
    // harness arguments, not package/target selection. Selectors are only
    // honored in the pre-`--` prefix, and selector-shaped words in the
    // harness region are refused, so relocating a required selector behind
    // `--` cannot keep this contract green.
    let harness_start = helper_tokens.iter().position(|token| *token == "--");
    let (selection_tokens, harness_tokens) = match harness_start {
        Some(boundary) => (&helper_tokens[..boundary], &helper_tokens[boundary + 1..]),
        None => (helper_tokens, &helper_tokens[helper_tokens.len()..]),
    };
    if harness_tokens
        .iter()
        .any(|token| *token == "--test" || *token == "-p" || *token == "--features")
    {
        return Some(
            "DAP helper command must keep package/target selection before `--`".to_string(),
        );
    }

    if !selection_tokens.windows(2).any(|window| window[0] == "-p" && window[1] == "perl-dap") {
        return Some("DAP helper command must target perl-dap".to_string());
    }
    if !selection_tokens
        .windows(2)
        .any(|window| window[0] == "--features" && window[1] == "test-helpers")
    {
        return Some("DAP helper command must enable test-helpers".to_string());
    }

    for target in DAP_HELPER_TARGETS {
        let occurrences = selection_tokens
            .windows(2)
            .filter(|window| window[0] == "--test" && window[1] == target)
            .count();
        if occurrences != 1 {
            return Some(format!("DAP helper command must bind exactly one --test {target}"));
        }
    }
    // Exact target set: an additional `--test` pair would silently widen
    // this supposedly exact gate while every named pair still binds once.
    let selector_count = selection_tokens.windows(2).filter(|window| window[0] == "--test").count();
    if selector_count != DAP_HELPER_TARGETS.len() {
        return Some(format!(
            "DAP helper command must bind exactly {} --test targets",
            DAP_HELPER_TARGETS.len()
        ));
    }
    None
}

fn dap_support_gate(root: &PathBuf) -> Result<PolicyGate, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(root.join(".ci/gate-policy.yaml"))?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;
    parsed
        .gates
        .into_iter()
        .find(|gate| gate.name == "unit_dap_support_full")
        .ok_or_else(|| "missing unit_dap_support_full gate".into())
}

#[test]
fn dap_support_gate_binds_all_helper_targets_and_propagates_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let gate = dap_support_gate(&root)?;
    assert_eq!(gate.tier, "merge_gate");
    assert!(gate.required, "DAP support gate must stay required");
    assert!(!gate.quarantine, "DAP support gate must not be quarantined");
    assert!(
        gate.description.contains("Windows-only pause runtime"),
        "the Linux claim boundary must remain explicit"
    );
    if let Some(error) = dap_helper_command_error(&gate.command) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error).into());
    }

    for target in DAP_HELPER_TARGETS {
        let mutated = gate.command.replacen(&format!(" --test {target}"), "", 1);
        assert!(
            dap_helper_command_error(&mutated).is_some(),
            "removing {target} must fail the policy contract"
        );
    }
    let missing_feature = gate.command.replace("--features test-helpers", "--features default");
    assert!(dap_helper_command_error(&missing_feature).is_some());
    let swallowed_failure = gate.command.replacen("&&", ";", 1);
    assert!(dap_helper_command_error(&swallowed_failure).is_some());
    // Cargo `--` boundary mutation: the ONLY copy of a required selector is
    // moved behind the harness separator. Removing it from the selection
    // region keeps the exact-occurrence check satisfied under a
    // boundary-blind validator, so this mutation specifically protects the
    // selection/harness split.
    let relocated = format!(
        "{} --test pause_signal_delivery_tests",
        gate.command.replacen(" --test pause_signal_delivery_tests", "", 1),
    );
    assert!(
        dap_helper_command_error(&relocated).is_some(),
        "selector behind `--` must fail the policy contract"
    );
    // Exact target set: an extra `--test` pair widens the gate and must fail.
    let widened = gate.command.replace(
        "--test eval_ref_cache_miss_resume_tests",
        "--test eval_ref_cache_miss_resume_tests --test extra_target_tests",
    );
    assert!(
        dap_helper_command_error(&widened).is_some(),
        "expanding the target set must fail the policy contract"
    );
    Ok(())
}

#[test]
fn dap_support_retry_envelope_leaves_terminal_receipt_headroom()
-> Result<(), Box<dyn std::error::Error>> {
    const SHARD_WATCHDOG_SECONDS: u64 = 1_200;
    const LINUX_CLEANUP_GRACE_SECONDS: u64 = 75;
    const TERMINAL_RECEIPT_RESERVE_SECONDS: u64 = 120;
    const EXPECTED_TIMEOUT_SECONDS: u64 = 450;
    const EXPECTED_BUDGET_MS: u64 = 360_000;

    let root = project_root();
    let gate = dap_support_gate(&root)?;
    assert_eq!(gate.timeout_seconds, Some(EXPECTED_TIMEOUT_SECONDS));
    assert_eq!(gate.retry_count, Some(1));
    let attempts = u64::from(gate.retry_count.unwrap_or_default()) + 1;
    let worst_case = attempts * (EXPECTED_TIMEOUT_SECONDS + LINUX_CLEANUP_GRACE_SECONDS);
    assert!(
        worst_case + TERMINAL_RECEIPT_RESERVE_SECONDS <= SHARD_WATCHDOG_SECONDS,
        "retry envelope must leave time for terminal receipts"
    );
    assert_eq!(gate.budgets.and_then(|budgets| budgets.max_duration_ms), Some(EXPECTED_BUDGET_MS));

    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    assert!(
        workflow.contains("timeout --signal=TERM --kill-after=30s 1200s"),
        "the policy test must bind its envelope to the shard watchdog"
    );
    Ok(())
}

/// The LSP unit lanes must partition `LSP_UNIT_SURFACE` exactly: every crate
/// covered once, none covered twice, none lost.
///
/// Without this, the split is only guarded by
/// `no_duplicate_gate_definitions_across_tiers`, which asserts gate *names* are
/// unique. That would still pass if a crate were dropped from both lanes — the
/// gates would go green while silently testing less. #5425 split this lane by
/// editing two `command:` strings, and a future rebalance will edit them again.
#[test]
fn lsp_unit_lanes_partition_the_surface_exactly() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let content = fs::read_to_string(root.join(".ci/gate-policy.yaml"))?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let lane_names = ["unit_lsp_core_full", "unit_lsp_full"];
    let mut seen: Vec<String> = Vec::new();

    for lane_name in lane_names {
        let lane = parsed
            .gates
            .iter()
            .find(|gate| gate.name == lane_name)
            .ok_or_else(|| format!("gate '{lane_name}' must exist in .ci/gate-policy.yaml"))?;

        assert!(
            lane.required,
            "{lane_name} must stay required — the LSP unit surface is never-skippable"
        );
        assert_eq!(lane.tier, "merge_gate", "{lane_name} must stay in the merge_gate tier");
        // `required` and `tier` do not encode never-skippable on their own: a
        // quarantined gate stays required and merge_gate while no longer
        // blocking merge, which is the exact behaviour this lane must not gain.
        assert!(
            !lane.quarantine,
            "{lane_name} must not be quarantined — a quarantined lane still reads as \
             required but stops blocking merge, which silently un-gates the LSP surface"
        );

        // Both lanes must run under identical flags. The split only preserves
        // coverage if the two invocations differ solely in their package set:
        // --lib pins which targets run, --locked pins the dependency graph, and
        // --test-threads=4 pins the concurrency the ceilings were measured at.
        // Match whole whitespace-delimited tokens, not substrings: a substring
        // check on "--test-threads=4" would also accept "--test-threads=40".
        let tokens: Vec<&str> = lane.command.split_whitespace().collect();
        for flag in ["--lib", "--locked", "--test-threads=4"] {
            assert!(
                tokens.contains(&flag),
                "{lane_name} must keep {flag} as an exact argument; the LSP lanes \
                 differ only in their package set, and changing a flag silently \
                 changes what is executed"
            );
        }

        // The union check below is satisfied by any partition of the surface,
        // including swapping the two lanes or collapsing everything into one.
        // The split is only meaningful if core stays alone: it is the crate
        // whose 2815 lib tests forced the original lane against its ceiling.
        let packages = package_args(&lane.command);
        if lane_name == "unit_lsp_core_full" {
            assert_eq!(
                packages,
                vec![LSP_CORE_CRATE.to_string()],
                "unit_lsp_core_full must run exactly {LSP_CORE_CRATE} — isolating it \
                 is the whole point of the split; adding crates here rebuilds the \
                 oversized lane #5425 broke up"
            );
        } else {
            let mut others: Vec<String> = LSP_UNIT_SURFACE
                .iter()
                .filter(|c| **c != LSP_CORE_CRATE)
                .map(|c| (*c).to_string())
                .collect();
            others.sort();
            let mut got = packages.clone();
            got.sort();
            assert_eq!(got, others, "unit_lsp_full must run exactly the five non-core LSP crates");
        }

        seen.extend(packages);
    }

    let mut deduped = seen.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(
        deduped.len(),
        seen.len(),
        "LSP unit lanes must not run any crate twice; got {seen:?}"
    );

    let mut expected: Vec<String> = LSP_UNIT_SURFACE.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(
        deduped, expected,
        "LSP unit lanes must cover exactly the crates in LSP_UNIT_SURFACE. \
         A crate missing here is coverage silently dropped; an extra crate is \
         coverage silently duplicated or moved from another lane."
    );

    Ok(())
}

/// Both LSP unit lanes carry the same ceiling and the same budget.
///
/// #5425 split one lane that had been raised 300s -> 420s and still timed out.
/// The first hosted receipt measured 177s and 219s, i.e. the lanes are close
/// enough in wall time that differentiating their budgets would be invention.
#[test]
fn lsp_unit_lanes_share_ceiling_and_budget() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let content = fs::read_to_string(root.join(".ci/gate-policy.yaml"))?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let lanes: Vec<&PolicyGate> = parsed
        .gates
        .iter()
        .filter(|gate| gate.name == "unit_lsp_core_full" || gate.name == "unit_lsp_full")
        .collect();
    assert_eq!(lanes.len(), 2, "both LSP unit lanes must be defined");

    let timeouts: Vec<u64> = lanes.iter().filter_map(|gate| gate.timeout_seconds).collect();
    assert_eq!(timeouts.len(), 2, "both LSP unit lanes must declare timeout_seconds");
    assert_eq!(
        timeouts[0], timeouts[1],
        "LSP unit lanes must share one ceiling; asymmetry needs measured justification"
    );

    let budgets: Vec<u64> = lanes
        .iter()
        .filter_map(|gate| gate.budgets.as_ref())
        .filter_map(|budget| budget.max_duration_ms)
        .collect();
    assert_eq!(budgets.len(), 2, "both LSP unit lanes must declare a budget");
    assert_eq!(budgets[0], budgets[1], "LSP unit lane budgets must match each other");

    // Equality and the ratio band below are both satisfied by matching-but-wrong
    // values (e.g. both lanes dropped to 300s/240000). Pin the measured pair:
    // 420s is the ceiling #5425 kept rather than relaxed, and 336000 is the
    // 0.80 budget the first hosted receipt confirmed for both lanes.
    assert_eq!(
        timeouts[0], 420,
        "LSP unit lane ceiling must stay 420s — #5425 split the lane precisely so \
         the ceiling would not have to move again"
    );
    assert_eq!(
        budgets[0], 336_000,
        "LSP unit lane budget must stay 336000ms (0.80 x 420s), the value the \
         hosted receipt measured both lanes comfortably inside"
    );

    // Keep the budget:ceiling ratio in line with the sibling test lanes.
    // unit_analysis_full and unit_dap_support_full sit at 0.80, as do both
    // LSP lanes (336000/420s). lsp_smoke keeps its 0.80 declared budget
    // (576000/720s); the shared Linux watchdog's 75s Rust backstop grace is
    // cleanup allowance, not a reason to shorten the execution window.
    // The enforced band below is deliberately wider than that single observed
    // value so a considered retune does not trip the guard, but narrow enough
    // to catch a budget set without reference to its ceiling. One band, stated
    // once: the assertion, this comment, and the failure message must agree.
    const MIN_BUDGET_RATIO: f64 = 0.75;
    const MAX_BUDGET_RATIO: f64 = 0.85;

    let ratio = budgets[0] as f64 / (timeouts[0] as f64 * 1000.0);
    assert!(
        (MIN_BUDGET_RATIO..=MAX_BUDGET_RATIO).contains(&ratio),
        "LSP unit lane budget:ceiling ratio {ratio:.2} is outside the enforced \
         {MIN_BUDGET_RATIO:.2}-{MAX_BUDGET_RATIO:.2} band; the sibling test lanes \
         all sit at 0.80"
    );

    Ok(())
}

/// #8063: `lsp_smoke` must stay one atomic-child harness invocation — never a
/// `&&` composite — with an outer runaway guard that accounts for the shared
/// watchdog's cleanup grace rather than pretending to cover the sum of all
/// child budgets, and with no gate-level `retry_count` (retry policy is
/// executable only inside the typed child harness, where setup/compile
/// watchdog timeouts retry once and behavior children never retry). The child
/// set itself is pinned by the xtask bin tests
/// (`lsp_smoke_atomic::tests::child_set_is_pinned_and_ordered`).
#[test]
fn lsp_smoke_is_atomic_bounded_and_independently_terminal() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gate = parsed
        .gates
        .into_iter()
        .find(|gate| gate.name == "lsp_smoke")
        .ok_or("missing lsp_smoke gate")?;

    assert_eq!(gate.tier, "merge_gate");
    assert!(gate.required, "lsp_smoke must stay PR-blocking");
    let command = gate.command.trim().to_string();
    assert_eq!(
        command,
        "cargo run --locked -p xtask -- lsp-smoke-atomic \
         --receipt target/receipts/artifacts/lsp_smoke_children.json",
        "lsp_smoke must invoke the atomic child harness, not a composite"
    );
    assert!(!command.contains("&&"), "the #8063 decomposition forbids composites");

    assert!(
        gate.retry_count.is_none(),
        "gate-level retry_count must stay absent: a whole-suite rerun on outer \
         timeout is exactly the twice-retried-600s fleet symptom #8063 fixes"
    );

    // Outer runaway guard: 720s is the declared execution window. The Linux
    // helper's 75s Rust backstop grace is cleanup allowance after that window,
    // not a reason to shorten it. This is deliberately NOT the worst-case sum
    // of child budgets (3 x 2 x 300s retrying compiles + 6 x 120s behavior =
    // 2520s): the guard bounds the suite and leaves CANCELLED marks in the
    // child receipt, it does not promise unreachable headroom.
    assert_eq!(
        gate.timeout_seconds,
        Some(720),
        "declared outer guard must preserve the full hosted lsp execution window"
    );
    let budget = gate
        .budgets
        .and_then(|budgets| budgets.max_duration_ms)
        .ok_or("lsp_smoke must declare a duration budget")?;
    assert_eq!(budget, 576_000, "budget must stay at the 0.80 ratio (576000/720s)");

    Ok(())
}
