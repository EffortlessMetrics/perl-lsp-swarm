#!/usr/bin/env python3
"""Apply the review-approved PR #6847 gate-family improvement atomically."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GATE_POLICY = ROOT / ".ci/gate-policy.yaml"
PROFILE_TEST = ROOT / "xtask/tests/gate_policy_profile_tests.rs"
GATES_RS = ROOT / "xtask/src/tasks/gates.rs"

GATE_NAMES = (
    "inline_completion_registration",
    "lsp_registration_contract",
    "lsp_capability_snapshots",
    "inline_completion_core",
)


def replace_once(text: str, pattern: str, replacement: str, label: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.DOTALL)
    if count != 1:
        raise SystemExit(f"{label}: expected one replacement, got {count}")
    return updated


def update_gate_policy() -> None:
    text = GATE_POLICY.read_text(encoding="utf-8")
    for gate_name in GATE_NAMES:
        pattern = rf"(  - name: {re.escape(gate_name)}\n.*?)(?=\n  - name: )"
        match = re.search(pattern, text, flags=re.DOTALL)
        if match is None:
            raise SystemExit(f"missing gate block: {gate_name}")
        block = match.group(1)
        if block.count("timeout_seconds: 180") != 1:
            raise SystemExit(f"{gate_name}: expected one 180-second timeout")
        if block.count("max_duration_ms: 120000") != 1:
            raise SystemExit(f"{gate_name}: expected one 120000ms budget")
        block = block.replace("timeout_seconds: 180", "timeout_seconds: 150")
        block = block.replace("max_duration_ms: 120000", "max_duration_ms: 135000")
        block, package_count = re.subn(
            r"(    planning:\n      role: rust_package_scoped\n      packages:\n)(?:        - [^\n]+\n?)+",
            r"\1        - perl-lsp-rs\n        - perl-lsp-rs-core\n",
            block,
            count=1,
        )
        if package_count != 1:
            raise SystemExit(f"{gate_name}: package scope was not replaced")
        text = text[: match.start(1)] + block + text[match.end(1) :]

    GATE_POLICY.write_text(text, encoding="utf-8")


PROFILE_REPLACEMENT = r'''/// (issue #6845) The former `inline_completion_contract` gate chained four
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

    let ordered_names: Vec<_> = parsed.gates.iter().map(|gate| gate.name.as_str()).collect();
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
        (
            "lsp_capability_snapshots",
            "cargo test -p perl-lsp-rs --locked --test lsp_cap_snap",
        ),
        (
            "inline_completion_core",
            "cargo test -p perl-lsp-rs-core --locked --lib inline_completion",
        ),
    ];
    let expected_names: Vec<_> = expected.iter().map(|(name, _)| *name).collect();
    let family_start = ordered_names
        .windows(expected_names.len())
        .position(|window| window == expected_names.as_slice())
        .ok_or("inline-completion gate family must be contiguous and ordered")?;
    assert_eq!(
        ordered_names.get(family_start + expected_names.len()),
        Some(&"inline_completion_quality_receipt"),
        "quality receipt must remain the family boundary immediately after the four children"
    );

    let mut timeout_total = 0_u64;
    let mut budget_total = 0_u64;
    for &(gate_name, expected_command) in expected {
        let gate = gates.get(gate_name).ok_or_else(|| {
            format!("gate '{gate_name}' not found in gate-policy.yaml")
        })?;
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
    let quality_planning = quality
        .planning
        .as_ref()
        .ok_or("inline_completion_quality_receipt missing planning")?;
    assert_eq!(quality.tier, "pr_fast");
    assert!(quality.required, "quality receipt must remain required within pr_fast");
    assert_eq!(quality_planning.role, "rust_package_scoped");
    assert_eq!(quality_planning.packages, vec!["perl-lsp-rs-core", "xtask"]);

    Ok(())
}

'''


def update_profile_test() -> None:
    text = PROFILE_TEST.read_text(encoding="utf-8")
    pattern = (
        r"/// \(issue #6845\).*?"
        r"#\[test\]\n"
        r"fn inline_completion_gates_are_split_and_correctly_scoped\(\).*?\n"
        r"}\n\n"
        r"(?=#\[test\]\nfn gate_registry_alignment_prevents_stale_parser_wiring)"
    )
    text = replace_once(text, pattern, PROFILE_REPLACEMENT, "profile contract")
    PROFILE_TEST.write_text(text, encoding="utf-8")


SCOPING_REPLACEMENT = r'''    #[test]
    fn inline_completion_gates_are_required_within_tier_and_family_scoped()
    -> color_eyre::eyre::Result<()> {
        let root = crate::utils::project_root()?;
        let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;

        for &gate_name in INLINE_COMPLETION_GATE_NAMES {
            let gate = policy
                .gates
                .iter()
                .find(|gate| gate.name == gate_name)
                .ok_or_else(|| color_eyre::eyre::eyre!("gate '{gate_name}' not found"))?;

            assert!(
                gate.required,
                "Gate '{gate_name}' must remain required within the pr_fast runner; \
                 this field does not claim that GitHub protects the containing PR Smoke job"
            );
            let planning = gate.planning.as_ref().ok_or_else(|| {
                color_eyre::eyre::eyre!("Gate '{gate_name}' must have planning metadata")
            })?;
            assert_eq!(planning.role, GatePlanningRole::RustPackageScoped);
            let packages: Vec<_> = planning.packages.iter().map(String::as_str).collect();
            assert_eq!(
                packages,
                vec!["perl-lsp-rs", "perl-lsp-rs-core"],
                "every split child must preserve the former family's selection on either package"
            );
        }

        Ok(())
    }

    #[test]
    fn inline_completion_gates_cover_the_same_packages_as_former_composite'''

RUNNER_REPLACEMENT = r'''    #[test]
    fn gate_runner_reports_independent_results_when_a_peer_fails()
    -> color_eyre::eyre::Result<()> {
        let failing_gate = tier_gate("gate_a_fails", "pr_fast", "exit 1");
        let passing_gate = tier_gate("gate_b_still_runs", "pr_fast", "exit 0");
        let policy = policy_with_gates(vec![failing_gate.clone(), passing_gate.clone()]);
        let plan = static_gate_plan(
            GateTier::PrFast,
            "HEAD".to_string(),
            vec![failing_gate, passing_gate],
            None,
        );
        let config = GateRunnerConfig {
            tier: GateTier::PrFast,
            output_format: OutputFormat::Summary,
            fail_fast: false,
            ..GateRunnerConfig::default()
        };

        let receipt = run_gate_plan(&plan, &policy, &config)?;

        assert_eq!(receipt.gates.len(), 2, "the real plan must emit both terminal rows");
        assert_eq!(receipt.gates[0].gate_name, "gate_a_fails");
        assert_eq!(receipt.gates[0].status, "fail");
        assert_eq!(receipt.gates[1].gate_name, "gate_b_still_runs");
        assert_eq!(receipt.gates[1].status, "pass");
        assert!(
            receipt.gates.iter().all(|gate| gate.log_path.is_some()),
            "each terminal row must retain its independent log path"
        );
        assert_eq!(receipt.summary.total_gates, 2);
        assert_eq!(receipt.summary.failed, 1);
        assert_eq!(receipt.summary.passed, 1);
        assert_eq!(
            receipt.summary.blocking_failures.as_deref(),
            Some(&["gate_a_fails".to_string()][..])
        );

        Ok(())
    }
'''


def update_gates_rs() -> None:
    text = GATES_RS.read_text(encoding="utf-8")
    scoping_pattern = (
        r"    #\[test\]\n"
        r"    fn inline_completion_gates_are_required_and_package_scoped\(\).*?\n"
        r"    }\n\n"
        r"    #\[test\]\n"
        r"    fn inline_completion_gates_cover_the_same_packages_as_former_composite"
    )
    text = replace_once(text, scoping_pattern, SCOPING_REPLACEMENT, "scoping unit test")

    runner_pattern = (
        r"    #\[test\]\n"
        r"    fn gate_runner_reports_independent_results_when_a_peer_fails\(\).*?\n"
        r"    }\n(?=})"
    )
    text = replace_once(text, runner_pattern, RUNNER_REPLACEMENT, "runner unit test")
    GATES_RS.write_text(text, encoding="utf-8")


def main() -> None:
    update_gate_policy()
    update_profile_test()
    update_gates_rs()


if __name__ == "__main__":
    main()
