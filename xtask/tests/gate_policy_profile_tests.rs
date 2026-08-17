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
/// every change to either owning package selects every child, and the family
/// preserves the former 600-second timeout / 540-second budget envelope.
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
    assert_eq!(timeout_total, 600, "split family must preserve the old hard timeout");
    assert_eq!(budget_total, 540_000, "split family must preserve the old duration budget");

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
    // unit_analysis_full, unit_dap_support_full, and lsp_smoke all sit at
    // exactly 0.80 (240000/300s), as do both LSP lanes (336000/420s). The
    // enforced band below is deliberately wider than that single observed
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
