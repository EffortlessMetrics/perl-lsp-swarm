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
            "cargo build -p perllsp --locked && cargo test -p perl-lsp-rs --locked --test lsp_inline_completion_registration_tests",
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
        if *gate_name == "inline_completion_registration" {
            assert!(
                gate.command.starts_with("cargo build -p perllsp --locked && "),
                "server-spawning inline completion tests must use the prebuilt perllsp binary"
            );
        } else {
            assert!(!gate.command.contains("&&"));
        }
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

