//! Contract tests for first blocking proof-lane CI wiring.

use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use assert_cmd::Command;
use perl_tdd_support::{must, must_some};
use serde_yaml_ng::Value;
use toml::Value as TomlValue;

#[test]
fn ignored_test_issue_reference_gate_is_required_on_prs() {
    let root = repo_root();
    let policy = must(fs::read_to_string(root.join(".ci/gate-policy.yaml")));
    let gate_start = must_some(policy.find("  - name: ignored_tests_check_refs"));
    let gate_tail = &policy[gate_start..];
    let gate_end = gate_tail.find("\n  - name:").unwrap_or(gate_tail.len());
    let gate = &gate_tail[..gate_end];
    assert!(
        gate.contains("tier: pr_fast")
            && gate.contains("required: true")
            && gate.contains("command: just ignored-tests-check-refs")
            && gate.contains("timeout_seconds: 180")
            && gate.contains("quarantine: false")
            && gate.contains("role: always_on"),
        "gate policy must keep the ignored-test issue-reference gate required on the PR fast lane"
    );

    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci.yml")));
    let shards_start = must_some(workflow.find("merge-gate-shards:"));
    let shards = &workflow[shards_start..];
    let next_job = shards.find("\nmerge-gate:").unwrap_or(shards.len());
    assert!(
        shards[..next_job].contains("ignored_tests_check_refs"),
        "merge-gate-shards must execute the ignored-test issue-reference gate"
    );

    let smoke_start = must_some(workflow.find("  pr-smoke:"));
    let smoke = &workflow[smoke_start..];
    let target_start = must_some(smoke.find("- name: Select PR Smoke Cargo target"));
    let warm_start = must_some(smoke.find("- name: Warm xtask"));
    let target_step = must_some(workflow_step(smoke, "Select PR Smoke Cargo target"));
    assert!(
        target_start < warm_start,
        "PR Smoke must select CARGO_TARGET_DIR before warming xtask"
    );
    assert!(
        target_step.contains("CARGO_TARGET_DIR=$target_dir") && target_step.contains("GITHUB_ENV"),
        "PR Smoke must persist its cargo target for the shared gate runner"
    );
    assert!(
        target_step.contains("PR_SMOKE_RUN_ID: ${{ github.run_id }}")
            && target_step.contains("PR_SMOKE_RUN_ATTEMPT: ${{ github.run_attempt }}")
            && target_step.contains("pr-smoke-${PR_SMOKE_RUN_ID}-${PR_SMOKE_RUN_ATTEMPT}"),
        "PR Smoke must pass run identity through step env into the cargo target path"
    );
    assert!(
        smoke.contains("\"$CARGO_TARGET_DIR/debug/xtask\" gates --tier pr-fast"),
        "PR Smoke must invoke the warmed xtask from its selected cargo target"
    );
    let scope_step = must_some(workflow_step(smoke, "Determine inline-completion warm-up scope"));
    assert!(
        scope_step.contains("id: inline-completion-scope")
            && scope_step.contains(
                "\"$CARGO_TARGET_DIR/debug/xtask\" ci-scope --subject target/receipts/ci-subject.json --format json"
            )
            && scope_step.contains("fail-closed"),
        "PR Smoke must use the warmed xtask's ci-scope JSON with fail-closed warm-up fallback"
    );
    for validation in [
        "any(.direct_crates[]?; type != \"object\" or (.name | type) != \"string\")",
        "any(.reverse_dep_closure[]?; type != \"object\" or (.name | type) != \"string\")",
        "any(.architecture_wideners[]?; type != \"object\" or (.name | type) != \"string\")",
        "any(.selected_lanes[]?; type != \"object\" or (.scope | type) != \"array\" or any(.scope[]?; type != \"string\"))",
    ] {
        assert!(
            scope_step.contains(validation),
            "PR Smoke ci-scope schema must reject malformed nested data: {validation}"
        );
    }
    assert!(
        scope_step.contains("code|mixed")
            && scope_step.contains("jq -r '")
            && scope_step.contains("any(. == \"perl-lsp-rs\" or . == \"perl-lsp-rs-core\")")
            && scope_step.contains("elif [ \"$relevant\" = \"false\" ]"),
        "PR Smoke must evaluate code/mixed relevance while retaining fail-closed jq fallback"
    );
    assert!(
        scope_step.contains("PR_SMOKE_RUN_ID: ${{ github.run_id }}")
            && scope_step.contains("PR_SMOKE_RUN_ATTEMPT: ${{ github.run_attempt }}")
            && scope_step
                .contains("pr-smoke-ci-scope-${PR_SMOKE_RUN_ID}-${PR_SMOKE_RUN_ATTEMPT}.json"),
        "PR Smoke must pass run identity through step env into the temporary scope path"
    );
    assert!(
        scope_step.contains("warm_inline_completion=true"),
        "PR Smoke must default inline-completion warm-up to enabled"
    );
    assert!(
        scope_step
            .contains("warm_inline_completion=$warm_inline_completion\" >> \"$GITHUB_OUTPUT\""),
        "PR Smoke must publish the inline-completion warm-up decision"
    );
    let warm_targets_start = must_some(smoke.find("- name: Warm inline-completion test targets"));
    let run_start = must_some(smoke.find("- name: Run PR-fast via shared xtask gate runner"));
    assert!(
        warm_start < warm_targets_start,
        "PR Smoke must warm inline-completion targets after warming xtask"
    );
    assert!(
        warm_targets_start < run_start,
        "PR Smoke must warm inline-completion targets before the actual shared PR-fast gate step"
    );
    let warm_targets = must_some(workflow_step(smoke, "Warm inline-completion test targets"));
    assert!(
        warm_targets
            .contains("if: steps.inline-completion-scope.outputs.warm_inline_completion == 'true'"),
        "PR Smoke must condition inline-completion warm-up on the fail-closed scope decision"
    );
    for command in [
        "cargo test -p perl-lsp-rs --locked --test lsp_inline_completion_registration_tests --no-run",
        "cargo test -p perl-lsp-rs-core --locked --lib inline_completion --no-run",
    ] {
        assert!(
            warm_targets.contains(command),
            "PR Smoke must prebuild `{command}` before running independent gates"
        );
    }
}

#[test]
fn ripr_workflow_blocks_new_gaps_and_requires_receipts() {
    let root = repo_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ripr.yml")));

    assert!(workflow.contains("name: ripr+ New Gap Gate"), "RIPR check name must be explicit");
    let gate_step = must_some(workflow_step(&workflow, "Enforce new RIPR gap quality gate"));
    assert!(
        !gate_step.contains("continue-on-error: true"),
        "RIPR new-gap gate must be blocking after PR8"
    );
    for required in [
        "- name: Enforce new RIPR gap quality gate",
        "cargo xtask quality-gate",
        "--mode enforce-new-ripr",
        "--ripr-receipt target/receipts/quality/ripr-plus.json",
        "--ripr-pr-receipt target/ripr/pr/repo-exposure.json",
        "--review-receipt target/ripr/review/comments.json",
        "--receipt target/receipts/quality/quality-gate-ripr.json",
        "--summary target/receipts/quality/quality-gate-ripr.md",
        "--check",
        "target/ripr/pr/**",
        "target/ripr/review/**",
        "target/receipts/quality/ripr-plus.json",
        "target/receipts/quality/quality-gate-ripr.json",
        "target/receipts/quality/quality-gate-ripr.md",
        "if-no-files-found: error",
    ] {
        assert!(workflow.contains(required), "RIPR workflow missing `{required}`");
    }
    assert!(
        workflow.contains("target/receipts/quality/quality-gate-ripr.md")
            && workflow.contains("GITHUB_STEP_SUMMARY"),
        "RIPR workflow must append quality-gate Markdown to the GitHub summary"
    );
}

#[test]
fn coverage_workflow_is_manual_or_nightly_only_and_requires_receipts() {
    let root = repo_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci-nightly.yml")));
    let policy = must(fs::read_to_string(root.join(".ci/policies/required-checks.toml")));
    let lane_economics = must(fs::read_to_string(root.join("policy/ci-lanes.toml")));
    let lane_whitelist = must(fs::read_to_string(root.join("policy/ci-lane-whitelist.toml")));
    let risk_pack_policy = must(fs::read_to_string(root.join("policy/ci-risk-packs.toml")));
    let inventory = must(fs::read_to_string(root.join("docs/ci/inventory.md")));
    let coverage_guide = must(fs::read_to_string(root.join("docs/how-to/COVERAGE.md")));
    let evidence_lanes_doc = must(fs::read_to_string(root.join("docs/ci/test-evidence-lanes.md")));
    let verification_ladder_doc =
        must(fs::read_to_string(root.join("docs/ci/verification-ladder.md")));
    let risk_pack_doc = must(fs::read_to_string(root.join("docs/ci/risk-packs.md")));
    let codecov_rollout = must(fs::read_to_string(root.join("docs/ci/codecov-rollout.md")));
    let codecov_config = must(fs::read_to_string(root.join("codecov.yml")));
    let justfile = must(fs::read_to_string(root.join("justfile")));
    let codecov_router = must(fs::read_to_string(root.join("scripts/ci/route-codecov-packs.py")));
    let coverage_start = must_some(workflow.find("  test-coverage:"));
    let coverage_tail = &workflow[coverage_start..];
    let coverage_end = must_some(coverage_tail.find("\n  tautology-check:"));
    let coverage_job = &coverage_tail[..coverage_end];

    let workflow_document: Value = must(serde_yaml_ng::from_str(&workflow));
    must(coverage_workflow_contract(&workflow_document));
    let coverage_yaml_job = must_some(mapping_value(
        must_some(mapping_value(&workflow_document, "jobs")),
        "test-coverage",
    ));
    let coverage_if = must_some(must_some(mapping_value(coverage_yaml_job, "if")).as_str())
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert_eq!(
        coverage_if,
        "github.event_name=='schedule'||(github.event_name=='workflow_dispatch'&&inputs.run_coverage)",
        "coverage job must be structurally limited to schedule/manual dispatch"
    );

    let whitelist_document: TomlValue = must(toml::from_str(&lane_whitelist));
    let required_checks_document: TomlValue = must(toml::from_str(&policy));
    let economics_document: TomlValue = must(toml::from_str(&lane_economics));
    let risk_pack_document: TomlValue = must(toml::from_str(&risk_pack_policy));
    let codecov_document: Value = must(serde_yaml_ng::from_str(&codecov_config));
    must(coverage_required_check_contract(&required_checks_document));
    must(coverage_whitelist_contract(&whitelist_document));
    must(coverage_risk_pack_contract(&risk_pack_document, &economics_document));
    must(codecov_posture_contract(&codecov_document));
    must(codecov_threshold_contract(&codecov_document));
    let coverage_whitelist_table = must_some(toml_lane(&whitelist_document, "coverage"));
    let allowed_triggers =
        must_some(must_some(coverage_whitelist_table.get("allowed_triggers")).as_array())
            .iter()
            .filter_map(TomlValue::as_str)
            .collect::<Vec<_>>();
    assert_eq!(
        allowed_triggers,
        vec!["schedule", "workflow_dispatch"],
        "coverage whitelist must have exactly the executable trigger set"
    );
    assert!(
        toml_array_is_empty(coverage_whitelist_table.get("labels"))
            && toml_array_is_empty(coverage_whitelist_table.get("branches")),
        "coverage whitelist must not carry label or branch selectors"
    );

    let coverage_economics_table = must_some(
        economics_document
            .get("lane")
            .and_then(TomlValue::as_table)
            .and_then(|lanes| lanes.get("coverage"))
            .and_then(TomlValue::as_table),
    );
    assert!(
        toml_array_is_empty(coverage_economics_table.get("labels"))
            && toml_array_is_empty(coverage_economics_table.get("branches")),
        "coverage economics must not carry label or branch selectors"
    );
    assert!(
        coverage_economics_table.get("default_pr").and_then(TomlValue::as_bool) == Some(false),
        "coverage economics must remain outside the default PR lane"
    );
    assert!(
        coverage_economics_table.get("workflow").and_then(TomlValue::as_str)
            == Some(".github/workflows/ci-nightly.yml")
            && coverage_economics_table.get("job").and_then(TomlValue::as_str)
                == Some("test-coverage"),
        "coverage economics must bind the planner lane to the executable job"
    );

    assert!(
        workflow.contains("types: [opened, synchronize, reopened, ready_for_review, labeled]"),
        "ci-nightly may still host other label-gated jobs"
    );
    assert!(
        coverage_job.contains("name: Codecov / Patch 95"),
        "coverage job keeps the familiar advisory check name"
    );
    let policy_start = must_some(policy.find("name = \"Codecov / Patch 95\""));
    let policy_tail = &policy[policy_start..];
    let policy_end = policy_tail.find("\n[[checks]]").unwrap_or(policy_tail.len());
    let coverage_policy = &policy_tail[..policy_end];
    assert!(
        coverage_policy.contains("events = [\"schedule\", \"workflow_dispatch\"]")
            && !coverage_policy.contains("pull_request")
            && !coverage_policy.contains("labelled"),
        "coverage policy must describe only the executable schedule/manual route"
    );
    let lane_start = must_some(lane_whitelist.find("id = \"coverage\""));
    let lane_tail = &lane_whitelist[lane_start..];
    let lane_end = lane_tail.find("\n[[lane]]").unwrap_or(lane_tail.len());
    let coverage_lane = &lane_tail[..lane_end];
    assert!(
        coverage_lane.contains("allowed_triggers = [\"schedule\", \"workflow_dispatch\"]")
            && coverage_lane.contains("Codecov / Patch 95 is advisory")
            && coverage_lane.contains("labels = []")
            && !coverage_lane.contains("pull_request")
            && !coverage_lane.contains("merge_group")
            && !coverage_lane.contains("ci:coverage")
            && !coverage_lane.contains("full-ci")
            && !coverage_lane.contains("required Codecov"),
        "coverage lane whitelist must match the schedule/manual-only advisory workflow"
    );
    let economics_start = must_some(lane_economics.find("[lane.coverage]"));
    let economics_tail = &lane_economics[economics_start..];
    let economics_end = economics_tail.find("\n[lane.").unwrap_or(economics_tail.len());
    let coverage_economics = &economics_tail[..economics_end];
    assert!(
        coverage_economics.contains(
            "description = \"Coverage collection for scheduled nightly or explicitly manual runs.\""
        ) && coverage_economics.contains("labels = []")
            && coverage_economics.contains("branches = []")
            && !coverage_economics.contains("master")
            && !coverage_economics.contains("ci:coverage")
            && !coverage_economics.contains("full-ci"),
        "coverage lane economics must not advertise label or master-branch routing"
    );
    assert!(
        evidence_lanes_doc.contains(
            "| Coverage | scheduled nightly run or explicit `workflow_dispatch` with coverage enabled | Advisory Codecov upload; it is not a PR or merge-queue lane. |"
        ) && verification_ladder_doc
            .contains("| coverage | nightly / manual dispatch | execution surface |")
            && !evidence_lanes_doc.contains("| `coverage` |")
            && !evidence_lanes_doc.contains("coverage.yml` Codecov upload on PR")
            && !evidence_lanes_doc.contains("Coverage on `master`")
            && !evidence_lanes_doc.contains("label-gated PR (`coverage`)")
            && !verification_ladder_doc.contains("main / `coverage` label"),
        "coverage reference docs must describe only scheduled/manual execution"
    );
    assert!(
        coverage_guide.contains("stays stable on current main")
            && coverage_guide.contains("current coverage on the `main` branch")
            && coverage_guide.contains("/branch/main/graph/badge.svg")
            && !coverage_guide.contains("current master")
            && !coverage_guide.contains("/branch/master/"),
        "coverage guide must describe the live main branch and badge"
    );
    must(coverage_reference_rows_contract(&evidence_lanes_doc, "test-evidence-lanes.md"));
    must(coverage_reference_rows_contract(&verification_ladder_doc, "verification-ladder.md"));
    must(coverage_reference_rows_contract(&inventory, "inventory.md"));
    must(coverage_reference_rows_contract(&coverage_guide, "COVERAGE.md"));
    must(coverage_risk_pack_docs_contract(&risk_pack_doc));
    assert!(
        codecov_rollout
            .contains("Codecov patch gate remains advisory; project coverage remains burn-down:")
            && !codecov_rollout.contains("Codecov patch gate remains blocking"),
        "Codecov rollout checklist must keep patch coverage advisory"
    );
    must(coverage_rollout_docs_contract(&codecov_rollout));
    // The contract only inspects lines that mention coverage or Codecov, so the
    // negative control must mutate such a line: the Codecov badge URL.
    let stale_codecov_rollout = codecov_rollout.replacen(
        "/branch/main/graph/badge.svg",
        "/branch/master/graph/badge.svg",
        1,
    );
    assert!(
        stale_codecov_rollout != codecov_rollout,
        "negative control must mutate a Codecov badge line in the rollout doc"
    );
    assert!(
        coverage_rollout_docs_contract(&stale_codecov_rollout).is_err(),
        "Codecov rollout documentation contract must reject stale master guidance"
    );
    assert!(
        inventory.contains(
            "| `ci-nightly.yml` (test-coverage) | `schedule`, `workflow_dispatch` | no | `ubuntu-24.04` | Coverage | 45 | `coverage` | keep |"
        ) && !inventory.contains(
            "| `ci-nightly.yml` (test-coverage) | `schedule`, `workflow_dispatch`, label |"
        ),
        "CI inventory must describe coverage as schedule/manual-only"
    );
    assert!(
        inventory.contains("| Default branch | `main` |")
            && !inventory.contains("| Default branch | `master` |"),
        "CI inventory must name the live default branch"
    );
    must(coverage_baseline_contract(coverage_job, coverage_yaml_job, &justfile));

    let workflow_mutations = [
        (
            "label route",
            "      (github.event_name == 'workflow_dispatch' && inputs.run_coverage)",
            "      (github.event_name == 'workflow_dispatch' && inputs.run_coverage) ||\n      (github.event_name == 'pull_request' && contains(github.event.pull_request.labels.*.name, 'coverage'))",
        ),
        (
            "branch route",
            "      (github.event_name == 'workflow_dispatch' && inputs.run_coverage)",
            "      (github.event_name == 'workflow_dispatch' && inputs.run_coverage) ||\n      (github.ref == 'refs/heads/main' && inputs.run_coverage)",
        ),
        (
            "push route",
            "      (github.event_name == 'workflow_dispatch' && inputs.run_coverage)",
            "      (github.event_name == 'workflow_dispatch' && inputs.run_coverage) ||\n      (github.event_name == 'push' && inputs.run_coverage)",
        ),
    ];
    for (route, original, replacement) in workflow_mutations {
        let mutated_source = workflow.replacen(original, replacement, 1);
        assert_ne!(mutated_source, workflow, "{route} negative control must change the workflow");
        let mutated_workflow: Value = must(serde_yaml_ng::from_str(&mutated_source));
        assert!(
            coverage_workflow_contract(&mutated_workflow).is_err(),
            "coverage workflow contract must reject {route}"
        );
    }
    let shared_trigger_workflow =
        workflow.replacen("on:\n", "on:\n  push:\n    branches: [main]\n  merge_group:\n", 1);
    let shared_trigger_workflow: Value = must(serde_yaml_ng::from_str(&shared_trigger_workflow));
    assert!(
        coverage_workflow_contract(&shared_trigger_workflow).is_ok(),
        "coverage contract must preserve legitimate shared workflow triggers"
    );
    let duplicate_checks_source = format!(
        "{policy}\n[[checks]]\nname = \"Codecov / Patch 95 (nightly alias)\"\nworkflow = \".github/workflows/ci-nightly.yml\"\njob = \"test-coverage\"\nevents = [\"schedule\", \"workflow_dispatch\"]\n"
    );
    let duplicate_checks: TomlValue = must(toml::from_str(&duplicate_checks_source));
    assert!(
        coverage_required_check_contract(&duplicate_checks).is_err(),
        "coverage policy contract must reject duplicate coverage entries"
    );
    let duplicate_lanes_source = format!(
        "{lane_whitelist}\n[[lane]]\nid = \"coverage\"\nworkflow = \".github/workflows/ci-nightly.yml\"\njob = \"test-coverage\"\n"
    );
    let duplicate_lanes: TomlValue = must(toml::from_str(&duplicate_lanes_source));
    assert!(
        coverage_whitelist_contract(&duplicate_lanes).is_err(),
        "coverage whitelist contract must reject duplicate coverage entries"
    );
    let duplicate_alias_lanes_source = format!(
        "{lane_whitelist}\n[[lane]]\nid = \"coverage-nightly\"\nworkflow = \".github/workflows/ci-nightly.yml\"\njob = \"test-coverage\"\n"
    );
    let duplicate_alias_lanes: TomlValue = must(toml::from_str(&duplicate_alias_lanes_source));
    assert!(
        coverage_whitelist_contract(&duplicate_alias_lanes).is_err(),
        "coverage whitelist contract must reject renamed semantic duplicates"
    );
    let stale_reason_source = policy.replacen(
        "scheduled or explicitly manual coverage runs",
        "pull_request coverage runs",
        1,
    );
    let stale_reason: TomlValue = must(toml::from_str(&stale_reason_source));
    assert!(
        coverage_required_check_contract(&stale_reason).is_err(),
        "coverage policy contract must reject stale route wording in free text"
    );
    let stale_inventory = inventory.replacen(
        "| `ci-nightly.yml` (test-coverage) | `schedule`, `workflow_dispatch` | no | `ubuntu-24.04` | Coverage | 45 | `coverage` | keep |",
        "| `ci-nightly.yml` (test-coverage) | `schedule`, `workflow_dispatch` | no | `ubuntu-24.04` | Coverage runs on PRs | 45 | `coverage` | keep |",
        1,
    );
    assert!(
        coverage_reference_rows_contract(&stale_inventory, "mutated inventory").is_err(),
        "coverage documentation contract must reject stale route wording in alternate rows"
    );
    let wrapped_stale_route = "Coverage is advisory, but\nruns on pull requests.";
    assert!(
        coverage_reference_rows_contract(wrapped_stale_route, "wrapped reference").is_err(),
        "coverage documentation contract must reject stale route wording across wrapped lines"
    );
    let wrapped_connector_on_second_line = "Coverage is advisory,\nbut runs on pull requests.";
    assert!(
        coverage_reference_rows_contract(
            wrapped_connector_on_second_line,
            "connector-led wrapped reference"
        )
        .is_err(),
        "coverage documentation contract must reject a connector starting the wrapped line"
    );
    let wrapped_required_route = "Coverage is advisory,\nbut required for pull requests.";
    assert!(
        coverage_reference_rows_contract(wrapped_required_route, "wrapped required reference")
            .is_err(),
        "coverage documentation contract must reject a wrapped required route"
    );
    let wrapped_gated_route = "Coverage is advisory,\nbut gated for pull requests.";
    assert!(
        coverage_reference_rows_contract(wrapped_gated_route, "wrapped gated reference").is_err(),
        "coverage documentation contract must reject a wrapped gated route"
    );
    let wrapped_gated_negative = "Coverage is advisory,\nbut not gated for pull requests.";
    assert!(
        coverage_reference_rows_contract(wrapped_gated_negative, "wrapped gated negative").is_ok(),
        "coverage documentation contract must allow a wrapped negated gated route"
    );
    let wrapped_connector_sentence_boundary = "Coverage is advisory.\nbut runs on pull requests.";
    assert!(
        coverage_reference_rows_contract(
            wrapped_connector_sentence_boundary,
            "connector sentence boundary"
        )
        .is_ok(),
        "coverage documentation contract must not join across a completed sentence"
    );
    let wrapped_connector_table_boundary =
        "| Coverage is advisory,\n| but runs on pull requests.\n";
    assert!(
        coverage_reference_rows_contract(
            wrapped_connector_table_boundary,
            "connector table boundary"
        )
        .is_ok(),
        "coverage documentation contract must not join separate table rows"
    );
    let wrapped_three_line_stale_route =
        "Coverage is advisory, but\nruns through the\nfull CI deep lane.";
    assert!(
        coverage_reference_rows_contract(
            wrapped_three_line_stale_route,
            "three-line wrapped reference"
        )
        .is_err(),
        "coverage documentation contract must reject stale route wording across three wrapped lines"
    );
    let wrapped_three_line_connector_on_second_line =
        "Coverage is advisory,\nbut runs through the\nfull CI deep lane.";
    assert!(
        coverage_reference_rows_contract(
            wrapped_three_line_connector_on_second_line,
            "three-line connector continuation"
        )
        .is_err(),
        "coverage documentation contract must reject connector-led three-line route wording"
    );
    let wrapped_boundary_control =
        "Coverage is advisory, but\nthis sentence ends here.\nfull CI deep lane.";
    assert!(
        coverage_reference_rows_contract(wrapped_boundary_control, "wrapped boundary control")
            .is_ok(),
        "coverage documentation contract must not join across a completed sentence"
    );
    let stale_risk_doc =
        risk_pack_doc.replacen("`mutation`, `fuzz` |", "`mutation`, `fuzz`, `coverage` |", 1);
    assert!(
        coverage_risk_pack_docs_contract(&stale_risk_doc).is_err(),
        "risk-pack documentation contract must reject coverage as a full-ci lane"
    );
    let stale_risk_sentence = risk_pack_doc.replacen("must not select it", "must select it", 1);
    assert!(
        coverage_risk_pack_docs_contract(&stale_risk_sentence).is_err(),
        "risk-pack documentation contract must reject stale coverage route wording"
    );
    let wrapped_stale_risk_route = format!(
        "{risk_pack_doc}\nCoverage is advisory,\nbut runs through the\nfull CI deep lane.\n"
    );
    assert!(
        coverage_risk_pack_docs_contract(&wrapped_stale_risk_route).is_err(),
        "risk-pack documentation contract must reject stale coverage wording across three wrapped lines"
    );
    assert!(
        has_positive_stale_route_claim(
            "Coverage does not run on PRs, but coverage runs on PRs for every pull request."
        ),
        "contradictory coverage prose must retain the positive stale-route finding"
    );
    assert!(
        has_positive_stale_route_claim(
            "Coverage does not run on PRs but coverage runs on PRs for every pull request."
        ),
        "unpunctuated contradictory coverage prose must retain the positive stale-route finding"
    );
    assert!(
        has_positive_stale_route_claim("Coverage runs on PRs, without labels."),
        "positive stale coverage prose must not be hidden by a comma and `without`"
    );
    assert!(
        has_positive_stale_route_claim("Coverage runs on PRs without labels."),
        "positive stale coverage prose must not be hidden by `without labels`"
    );
    assert!(
        has_positive_stale_route_claim("Coverage runs on PRs; never blocks."),
        "positive stale coverage prose must not be hidden by a semicolon and `never`"
    );
    assert!(
        has_positive_stale_route_claim("Coverage runs on PRs and never blocks."),
        "positive stale coverage prose must not be hidden by `and never`"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage is advisory for PRs."),
        "a bare advisory `for PRs` phrase must not be classified as a route"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage is optional for PRs."),
        "an optional `for PRs` phrase must not match the `on` route token"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage for PRs."),
        "a bare `coverage for PRs` phrase must not be classified as a route"
    );
    assert!(
        has_positive_stale_route_claim("Coverage is required for PRs."),
        "an explicit required `for PRs` route must remain classified as stale"
    );
    assert!(
        has_positive_stale_route_claim("Coverage runs for PRs."),
        "an explicit runs `for PRs` route must remain classified as stale"
    );
    assert!(
        has_positive_stale_route_claim("Coverage does not run on PRs and coverage runs on PRs."),
        "a positive conjunction must not be hidden by an unrelated negative clause"
    );
    assert!(
        has_positive_stale_route_claim("Coverage does not run on PRs or coverage runs on PRs."),
        "a positive disjunction must not be hidden by an unrelated negative clause"
    );
    assert!(
        has_positive_stale_route_claim("Coverage does not run on PRs and\ncoverage runs on PRs."),
        "a newline-separated positive conjunction must not be hidden by an unrelated negative clause"
    );
    assert!(
        has_positive_stale_route_claim("Coverage does not run on PRs or\ncoverage runs on PRs."),
        "a newline-separated positive disjunction must not be hidden by an unrelated negative clause"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage does not run on PRs or merge queues."),
        "wholly negative coverage prose must remain allowed"
    );
    assert!(
        !has_positive_stale_route_claim(
            "Coverage is advisory, and maintainers can run routed proof locally."
        ),
        "legitimate negative coverage prose must remain allowed"
    );
    assert!(
        !has_positive_stale_route_claim(
            "Coverage is advisory, and pull requests use the required Rust gates."
        ),
        "advisory prose must not be classified as a positive coverage route"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage is advisory, never a PR gate."),
        "legitimate negative coverage prose with `never` must remain allowed"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage doesn't run on PRs."),
        "negative coverage prose with an ASCII contraction must remain allowed"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage doesn’t run on PRs."),
        "negative coverage prose with a Unicode apostrophe contraction must remain allowed"
    );
    assert!(
        !has_positive_stale_route_claim(
            "Coverage is advisory, but the full CI deep lane is disabled."
        ),
        "disabled coverage routes must remain negative in compatibility prose"
    );
    assert!(
        has_positive_stale_route_claim("Coverage is disabled, but coverage runs on pull requests."),
        "a positive coverage route must remain visible beside a disabled clause"
    );
    assert!(
        has_positive_stale_route_claim(
            "Coverage runs on PRs even though the coverage job is disabled."
        ),
        "a positive route must remain visible before an `even though` negative clause"
    );
    assert!(
        !has_positive_stale_route_claim(
            "Coverage does not run on PRs even though the coverage job is disabled."
        ),
        "an entirely negative `even though` sentence must remain allowed"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage is advisory, and never a full CI deep lane."),
        "advisory prose must not be classified as a positive full-ci deep-lane route"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage is advisory, and\nnever a full CI deep lane."),
        "wrapped advisory prose must not be classified as a positive full-ci deep-lane route"
    );
    assert!(
        !has_positive_stale_route_claim("Coverage runs, but\nnot on pull requests."),
        "wrapped post-run negation must not be classified as a positive coverage route"
    );
    assert!(
        coverage_reference_rows_contract(
            "Coverage runs, but\ndoesn't run on pull requests.",
            "wrapped contraction negation"
        )
        .is_ok(),
        "wrapped ASCII contraction negation must not be classified as a positive route"
    );
    assert!(
        coverage_reference_rows_contract(
            "Coverage runs, but\nthe coverage job is disabled on pull requests.",
            "wrapped disabled negation"
        )
        .is_ok(),
        "wrapped disabled negation must not be classified as a positive route"
    );
    assert!(
        has_positive_stale_route_claim(
            "Coverage does not run, but\ncoverage runs on pull requests."
        ),
        "a wrapped positive contradiction must retain the stale-route finding"
    );
    let stale_lane_alias_source = format!(
        "{lane_economics}\n[lane.coverage_alias]\nworkflow = \".github/workflows/ci-nightly.yml\"\njob = \"test-coverage\"\nlabels = [\"coverage-alias\"]\nbranches = [\"schedule\", \"workflow_dispatch\"]\n"
    );
    let stale_lane_alias: TomlValue = must(toml::from_str(&stale_lane_alias_source));
    assert!(
        coverage_risk_pack_contract(&risk_pack_document, &stale_lane_alias).is_err(),
        "risk-pack contract must reject renamed semantic coverage lanes"
    );
    let stale_deep_lane_source = risk_pack_policy.replacen(
        "deep_lanes = [\"mutation\", \"fuzz\"]",
        "deep_lanes = [\"mutation\", \"fuzz\", \"coverage\"]",
        1,
    );
    let stale_deep_lanes: TomlValue = must(toml::from_str(&stale_deep_lane_source));
    assert!(
        coverage_risk_pack_contract(&stale_deep_lanes, &economics_document).is_err(),
        "risk-pack contract must reject coverage as a full-ci deep lane"
    );
    let stale_risk_label_source = risk_pack_policy.replacen(
        "labels = [\"ci:parser\", \"ci:mutation\"]",
        "labels = [\"ci:parser\", \"ci:mutation\", \"ci:coverage\"]",
        1,
    );
    let stale_risk_labels: TomlValue = must(toml::from_str(&stale_risk_label_source));
    assert!(
        coverage_risk_pack_contract(&stale_risk_labels, &economics_document).is_err(),
        "risk-pack contract must reject coverage label routing"
    );
    let stale_risk_description_source = risk_pack_policy.replacen(
        "description = \"Parser, lexer, token, AST, tree-sitter, corpus, POD, regex, source-position, and parser support changes.\"",
        "description = \"Coverage runs on PRs.\"",
        1,
    );
    let stale_risk_description: TomlValue = must(toml::from_str(&stale_risk_description_source));
    assert!(
        coverage_risk_pack_contract(&stale_risk_description, &economics_document).is_err(),
        "risk-pack contract must reject positive stale coverage prose"
    );
    let contradictory_risk_sentence =
        format!("{risk_pack_doc}\nCoverage does not run on PRs but coverage runs on PRs.\n");
    assert!(
        coverage_risk_pack_docs_contract(&contradictory_risk_sentence).is_err(),
        "risk-pack docs contract must reject contradictory stale coverage prose"
    );
    let negative_risk_sentence =
        format!("{risk_pack_doc}\nCoverage does not run on PRs or merge queues.\n");
    assert!(
        coverage_risk_pack_docs_contract(&negative_risk_sentence).is_ok(),
        "risk-pack docs contract must allow wholly negative coverage prose"
    );
    let weak_codecov_target = codecov_config.replacen("target: 95%", "target: 90%", 1);
    let weak_codecov_target: Value = must(serde_yaml_ng::from_str(&weak_codecov_target));
    assert!(
        codecov_threshold_contract(&weak_codecov_target).is_err(),
        "Codecov contract must reject an altered informational target"
    );
    let blocking_codecov_status =
        codecov_config.replacen("informational: true", "informational: false", 1);
    let blocking_codecov_status: Value = must(serde_yaml_ng::from_str(&blocking_codecov_status));
    assert!(
        codecov_threshold_contract(&blocking_codecov_status).is_err(),
        "Codecov contract must reject a blocking status regression"
    );
    let blocking_codecov_root =
        codecov_config.replacen("require_ci_to_pass: false", "require_ci_to_pass: true", 1);
    let blocking_codecov_root: Value = must(serde_yaml_ng::from_str(&blocking_codecov_root));
    assert!(
        codecov_posture_contract(&blocking_codecov_root).is_err(),
        "Codecov contract must reject a blocking top-level CI posture"
    );
    let proof_recipe_start = must_some(justfile.find("coverage-proof base='origin/main':"));
    let (justfile_prefix, proof_recipe) = justfile.split_at(proof_recipe_start);
    let weak_baseline_enforcement = format!(
        "{justfile_prefix}{}",
        proof_recipe.replacen("--mode enforce-patch-coverage", "--mode report-patch-coverage", 1)
    );
    assert!(
        coverage_baseline_contract(coverage_job, coverage_yaml_job, &weak_baseline_enforcement)
            .is_err(),
        "coverage contract must reject non-enforcing baseline quality-gate mode"
    );
    let checkout_ref = must_some(checkout_action_ref(coverage_job));
    assert!(
        is_full_sha(checkout_ref) && coverage_job.contains("persist-credentials: false"),
        "coverage proof checkout must be pinned and must not persist write credentials"
    );
    assert!(
        coverage_job.contains("github.event_name == 'schedule'")
            && coverage_job.contains("github.event_name == 'workflow_dispatch'")
            && !coverage_job.contains("github.event_name == 'pull_request'")
            && !coverage_job.contains("github.event.pull_request")
            && !coverage_job.contains("github.event_name == 'push'")
            && !coverage_job.contains("ci:coverage")
            && !coverage_job.contains("github.event_name == 'merge_group'")
            // The shared workflow keeps a top-level `pull_request:` trigger for
            // its label-gated sibling jobs; only the coverage job is restricted.
            && workflow.contains("\n  pull_request:"),
        "patch coverage must be advisory: only the coverage job is schedule/workflow_dispatch-only"
    );
    let route_step =
        must_some(coverage_job.find("- name: Emit changed-file coverage route summary"));
    let install_rust_step = must_some(coverage_job.find("- name: Install Rust"));
    assert!(
        route_step < install_rust_step,
        "coverage routing must run before Rust/cargo setup so skipped PRs avoid expensive setup"
    );
    for setup_step in [
        "Install Rust",
        "Cache cargo dependencies",
        "Install just",
        "Install cargo-llvm-cov",
        "Create legacy LSP fixtures (CI-only)",
    ] {
        let step = must_some(workflow_step(coverage_job, setup_step));
        assert!(
            !step.contains("merge_group")
                && !step.contains("pull_request")
                && !step.contains("coverage_required"),
            "coverage setup step `{setup_step}` must not carry PR routing conditions"
        );
    }
    let codecov_upload_start = must_some(coverage_job.find("- name: Upload coverage to Codecov"));
    let enforced_coverage_steps = &coverage_job[..codecov_upload_start];
    let after_codecov_upload = &coverage_job[codecov_upload_start..];
    let codecov_upload_end =
        must_some(after_codecov_upload.find("\n      - name: Upload coverage proof artifacts"));
    let codecov_upload_step = &after_codecov_upload[..codecov_upload_end];
    assert_eq!(
        codecov_upload_step.matches("continue-on-error: true").count(),
        1,
        "Codecov upload must not fail the job on integration errors (non-fatal telemetry)"
    );
    assert!(
        codecov_upload_step.contains("uses: codecov/codecov-action@")
            && codecov_upload_step.contains("files: target/lcov.info"),
        "Codecov upload step must upload the workspace library plus xtask proof-lane LCOV receipt"
    );
    for required in [
        "BASE_REF: ${{ github.base_ref || github.event.repository.default_branch }}",
        "base_ref=\"$BASE_REF\"",
        "id: coverage_route",
        "coverage_required=$coverage_required",
        "changed-file routing selected no LCOV coverage proof packs",
        "just coverage-proof \"origin/$base_ref\"",
        "cache-targets: false",
        "RUSTFLAGS: \"-Cdebuginfo=0\"",
        "CARGO_BUILD_JOBS: 1",
        "- name: Emit changed-file coverage route summary",
        "python3 scripts/ci/route-codecov-packs.py",
        "--manifest .ci/coverage-packs.toml",
        "--receipt target/receipts/quality/ci-route.json",
        "--summary target/receipts/quality/ci-route.md",
        "target/receipts/quality/ci-route.md",
        "target/receipts/quality/quality-gate-coverage.md",
        "GITHUB_STEP_SUMMARY",
        "name: coverage-proof-${{ github.sha }}",
        "target/lcov.info",
        "target/receipts/quality/ci-route.json",
        "target/receipts/quality/ci-route.md",
        "target/receipts/quality/coverage-baseline.json",
        "target/receipts/quality/quality-gate-coverage.json",
        "target/receipts/quality/quality-gate-coverage.md",
        "if-no-files-found: error",
    ] {
        assert!(coverage_job.contains(required), "coverage job missing `{required}`");
    }
    for forbidden in [
        "Run routed PR coverage proof",
        "coverage-proof-routed",
        "github.event_name == 'pull_request'",
        "github.event.pull_request",
        "ci:coverage",
    ] {
        assert!(
            !coverage_job.contains(forbidden),
            "coverage job must not include PR coverage path `{forbidden}`"
        );
    }
    assert!(
        coverage_job
            .contains("uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"),
        "coverage proof artifact upload must pin the action SHA"
    );
    assert!(
        !enforced_coverage_steps.contains("continue-on-error: true"),
        "coverage patch proof must stay enforced before non-fatal Codecov telemetry upload"
    );
    assert!(
        codecov_router.contains("Advisory lightweight Codecov coverage-pack route")
            && codecov_router.contains("feed manual routed coverage diagnostics")
            && !codecov_router.contains("CI-enforced lightweight Codecov coverage-pack route"),
        "Codecov route receipt must describe routed patch proof as advisory"
    );
    for required in [
        "coverage-proof-routed base='origin/main' head='HEAD':",
        "cargo xtask ci route",
        "--receipt target/receipts/quality/ci-route.json",
        "--summary target/receipts/quality/ci-route.md",
        "coverage-pack-commands.sh",
        "scripts/ci/generate-coverage-pack-commands.py",
        "changed-file routing selected no coverage proof packs",
        "cargo llvm-cov report --profile agent --lcov --output-path target/lcov.info",
        "--scope routed-coverage-packs",
    ] {
        assert!(justfile.contains(required), "coverage-proof-routed missing `{required}`");
    }
    let routed_recipe_start =
        must_some(justfile.find("coverage-proof-routed base='origin/main' head='HEAD':"));
    let routed_recipe_end = must_some(justfile.find("# Refresh the checked-in coverage baseline"));
    let routed_recipe = &justfile[routed_recipe_start..routed_recipe_end];
    assert_eq!(
        routed_recipe.matches("cargo xtask coverage-baseline").count(),
        1,
        "manual routed coverage proof should generate the coverage receipt once"
    );
    assert_eq!(
        routed_recipe.matches("cargo xtask quality-gate").count(),
        1,
        "manual routed coverage proof should evaluate the patch gate once"
    );
    assert!(
        !routed_recipe.contains("--check"),
        "manual routed coverage proof writes task-owned receipts and should not immediately recheck them"
    );
    for required in [
        "coverage-proof base='origin/main':",
        "coverage_target=\"${CARGO_TARGET_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/perl-lsp-swarm-coverage-target}\"",
        "export CARGO_TARGET_DIR=\"$coverage_target\"",
        "cargo llvm-cov clean --workspace",
        "coverage_env=\"$coverage_target/llvm-cov-env.sh\"",
        "cargo llvm-cov show-env --sh",
        "source \"$coverage_env\"",
        "cargo test --workspace --lib --locked",
        "cargo test -p xtask --bin xtask quality_baseline --locked",
        "cargo test -p xtask --bin xtask merge_ready --locked",
        "cargo test -p xtask --bin xtask gates --locked",
        "cargo test -p xtask --bin xtask ci_route --locked",
        "cargo test -p xtask --bin xtask workflow_policy_lint --locked",
        "cargo test -p xtask --bin xtask allocation_tracker --locked",
        "cargo test -p xtask --bin xtask agent_ledgers --locked",
        "cargo test -p xtask --bin xtask agent_receipt --locked",
        "cargo test -p xtask --bin xtask active_goal_manifest --locked",
        "cargo test -p xtask --bin xtask file_policy --locked",
        "cargo test -p xtask --bin xtask ripr --locked",
        "cargo test -p xtask --bin xtask inline_completion_quality --locked",
        "cargo test -p xtask --bin xtask semantic_inline_receipts --locked",
        "cargo test -p xtask --bin xtask semantic_inline_next_edit --locked",
        "cargo test -p xtask --locked",
        "--test active_goal_manifest_cli",
        "--test ci_route_cli",
        "--test quality_ci_wiring_policy",
        "--test quality_gate_patch_coverage_cli_policy",
        "--test semantic_inline_receipts_cli",
        "--test semantic_inline_next_edit_cli",
        "cargo llvm-cov report --lcov --output-path target/lcov.info",
        "--lcov --output-path target/lcov.info",
        "cargo xtask coverage-baseline",
        "--patch-base \"{{base}}\"",
        "--scope workspace-lib-xtask-quality",
        "cargo xtask quality-gate",
        "--mode enforce-patch-coverage",
        "--receipt target/receipts/quality/quality-gate-coverage.json",
        "--summary target/receipts/quality/quality-gate-coverage.md",
        "--check",
    ] {
        assert!(justfile.contains(required), "coverage-proof missing `{required}`");
    }
}

#[test]
fn nightly_manual_dispatch_routes_each_expensive_job_through_its_typed_input() {
    let root = repo_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci-nightly.yml")));

    let routed_jobs = [
        ("mutation", "run_mutation"),
        ("benchmark", "run_benchmarks"),
        ("real-repo-latency", "run_real_repo_latency"),
        ("corpus-differential", "run_corpus_differential"),
        ("lsp-memory-plateau", "run_memory"),
        ("test-coverage", "run_coverage"),
        ("tautology-check", "run_tautology"),
        ("semver-check", "run_semver"),
        ("public-api-check", "run_public_api"),
        ("scorecard-ratchet-check", "run_scorecard"),
        ("clippy-strict", "run_clippy_strict"),
        ("perl-kwalitee", "run_perl_kwalitee"),
        ("fuzz", "run_fuzz"),
    ];

    assert!(
        !workflow.contains("github.event.inputs."),
        "manual dispatch routing must use typed boolean inputs instead of string event payloads"
    );
    let fuzz = must_some(workflow_job(&workflow, "fuzz"));
    assert!(
        fuzz.contains("DURATION=600")
            && !fuzz.contains("inputs.fuzz_duration")
            && !fuzz.contains("github.event.inputs.duration"),
        "dispatch routing must not make the 600-second fuzz proof duration configurable"
    );

    for (job, input) in routed_jobs {
        let input_contract = format!("      {input}:\n");
        assert!(
            workflow.contains(&input_contract),
            "nightly dispatch must declare `{input}` for `{job}`"
        );

        let job_contract = must_some(workflow_job(&workflow, job));
        let selector = format!("(github.event_name == 'workflow_dispatch' && inputs.{input})");
        assert!(
            job_contract.contains(&selector),
            "nightly job `{job}` must be gated by its typed `{input}` selector"
        );
        assert_eq!(
            job_contract.matches("github.event_name == 'workflow_dispatch'").count(),
            1,
            "nightly job `{job}` must not retain an unconditional manual-dispatch route"
        );
    }

    let coverage = must_some(workflow_job(&workflow, "test-coverage"));
    assert!(
        coverage.contains("inputs.run_coverage")
            && routed_jobs
                .iter()
                .filter(|(job, _)| *job != "test-coverage")
                .all(|(_, input)| !coverage.contains(&format!("inputs.{input}"))),
        "coverage-only dispatch must select coverage independently of every other expensive pack"
    );

    for job in [
        "mutation",
        "benchmark",
        "real-repo-latency",
        "corpus-differential",
        "lsp-memory-plateau",
        "semver-check",
        "public-api-check",
        "scorecard-ratchet-check",
        "perl-kwalitee",
        "fuzz",
    ] {
        assert!(
            must_some(workflow_job(&workflow, job)).contains("github.event_name == 'schedule'"),
            "scheduled nightly behavior must remain enabled for `{job}`"
        );
    }

    for (job, label) in [
        ("mutation", "ci:mutation"),
        ("benchmark", "ci:bench"),
        ("real-repo-latency", "ci:real-repo-latency"),
        ("corpus-differential", "ci:corpus-differential"),
        ("lsp-memory-plateau", "ci:memory"),
        ("tautology-check", "ci:strict"),
        ("semver-check", "ci:semver"),
        ("public-api-check", "ci:public-api"),
        ("scorecard-ratchet-check", "ci:metrics-ratchet"),
        ("clippy-strict", "ci:strict"),
        ("perl-kwalitee", "ci:kwalitee"),
    ] {
        assert!(
            must_some(workflow_job(&workflow, job)).contains(label),
            "PR label route `{label}` must remain enabled for `{job}`"
        );
    }
}

#[test]
fn nightly_manual_dispatch_inputs_are_boolean_and_job_selectors_are_exclusive() -> Result<()> {
    let root = repo_root();
    let workflow_source = fs::read_to_string(root.join(".github/workflows/ci-nightly.yml"))?;
    let workflow: Value = serde_yaml_ng::from_str(&workflow_source)?;
    let triggers = yaml_mapping_entry(&workflow, "on")?;
    let schedule = yaml_mapping_entry(triggers, "schedule")?;
    ensure!(
        schedule.as_sequence().is_some_and(|entries| !entries.is_empty()),
        "nightly workflow must declare at least one top-level schedule trigger"
    );
    let inputs = yaml_mapping_entry(yaml_mapping_entry(triggers, "workflow_dispatch")?, "inputs")?;
    let routed_jobs = [
        ("mutation", "run_mutation"),
        ("benchmark", "run_benchmarks"),
        ("real-repo-latency", "run_real_repo_latency"),
        ("corpus-differential", "run_corpus_differential"),
        ("lsp-memory-plateau", "run_memory"),
        ("test-coverage", "run_coverage"),
        ("tautology-check", "run_tautology"),
        ("semver-check", "run_semver"),
        ("public-api-check", "run_public_api"),
        ("scorecard-ratchet-check", "run_scorecard"),
        ("clippy-strict", "run_clippy_strict"),
        ("perl-kwalitee", "run_perl_kwalitee"),
        ("fuzz", "run_fuzz"),
    ];

    // GitHub's workflow syntax reference documents a 25 top-level `inputs`
    // maximum; this guards platform validity without forcing unrelated
    // expensive packs into grouped selectors.
    ensure!(
        inputs.as_mapping().is_some_and(|mapping| mapping.len() <= 25),
        "workflow_dispatch must stay within GitHub's 25-input platform limit"
    );

    for (_, input) in routed_jobs {
        let definition = yaml_mapping_entry(inputs, input)?;
        ensure!(
            definition.get("type").and_then(Value::as_str) == Some("boolean"),
            "nightly dispatch input `{input}` must declare `type: boolean`"
        );
        ensure!(
            definition.get("default").and_then(Value::as_bool) == Some(false),
            "nightly dispatch input `{input}` must default to false for fail-safe manual routing"
        );
    }

    for (job, input) in routed_jobs {
        let job_contract = workflow_job(&workflow_source, job)
            .ok_or_else(|| anyhow!("nightly job `{job}` must be present"))?;
        ensure!(
            nightly_job_selector_is_exclusive(job_contract, input),
            "nightly job `{job}` must select exactly `{input}` for manual dispatch"
        );
    }

    let original_selector = "(github.event_name == 'workflow_dispatch' && inputs.run_mutation)";
    let mutated_selector = format!("{original_selector} || inputs.run_benchmarks");
    let mutated_source = workflow_source.replacen(original_selector, &mutated_selector, 1);
    ensure!(
        mutated_source != workflow_source,
        "exclusivity negative control must mutate the mutation selector"
    );
    let mutated_job = workflow_job(&mutated_source, "mutation")
        .ok_or_else(|| anyhow!("mutated mutation job must be present"))?;
    ensure!(
        !nightly_job_selector_is_exclusive(mutated_job, "run_mutation"),
        "a second run_* selector must invalidate the per-job exclusivity contract"
    );

    Ok(())
}

#[test]
fn coverage_proof_exercises_lsp_318_claim_guard() {
    let root = repo_root();

    must(Command::cargo_bin("xtask"))
        .current_dir(root)
        .arg("check-lsp-318-claims")
        .assert()
        .success();
}

#[test]
fn docs_describe_transitional_blocking_contract() {
    let root = repo_root();
    let ripr_doc = must(fs::read_to_string(root.join("docs/ci/ripr.md")));
    let coverage_doc = must(fs::read_to_string(root.join("docs/how-to/COVERAGE.md")));
    let status_doc =
        must(fs::read_to_string(root.join("docs/project/status/coverage_and_ripr_enforcement.md")));

    assert!(
        ripr_doc.contains("blocks PRs that introduce")
            && ripr_doc.contains("not require repo-wide RIPR+ total zero"),
        "RIPR docs must distinguish new-gap blocking from final total-zero enforcement"
    );
    assert!(
        coverage_doc.contains("Patch coverage is an advisory coverage signal")
            && coverage_doc.contains("Coverage does not run on PRs or merge queues")
            && coverage_doc.contains("just coverage-proof <base>")
            && coverage_doc.contains("Project coverage remains informational during burn-down"),
        "coverage docs must describe advisory patch coverage and transitional project coverage"
    );
    assert!(
        status_doc.contains("quality-gate")
            && status_doc.contains("Markdown summaries")
            && status_doc.contains("Current Blocking Proof Floor")
            && status_doc.contains("Codecov / Patch 95")
            && status_doc.contains("advisory coverage contexts")
            && status_doc.contains("coverage proof artifacts are advisory")
            && status_doc.contains(
                "generated quality-gate receipts are freshness-checked for patch, new-RIPR,"
            )
            && status_doc.contains("project coverage is visible in coverage receipts")
            && status_doc.contains("total active RIPR+ unresolved gaps")
            && status_doc.contains("Active RIPR+ Inventory")
            && status_doc.contains("active_unresolved")
            && status_doc.contains("suppressed_unresolved")
            && status_doc.contains("top_active_gap_kinds")
            && status_doc.contains("recommended_first_clusters")
            && status_doc.contains("Project Coverage Burn-Down Inventory")
            && status_doc.contains("project_burndown.remaining_percentage_points")
            && status_doc.contains("project_files_below_target")
            && status_doc.contains("top_project_files")
            && status_doc.contains("recommended_project_clusters")
            && status_doc.contains("An active temporary exception is not a final-enforcement pass"),
        "status doc must keep final targets separate from transitional enforcement"
    );
}

#[test]
fn conventional_required_checks_record_live_proof_floor() {
    let root = repo_root();
    let policy = must(fs::read_to_string(root.join(".ci/policies/required-checks.toml")));
    let merge_ready_doc = must(fs::read_to_string(root.join("docs/ci/merge-ready-protocol.md")));
    let status_doc =
        must(fs::read_to_string(root.join("docs/project/status/coverage_and_ripr_enforcement.md")));
    let parsed: toml::Value = must(toml::from_str(&policy));

    for required in ["Perl LSP Rust Small Result", "ripr+ New Gap Gate"] {
        assert!(
            policy_required_check(&parsed, required),
            "required-check policy must mark `{required}` as required under GitHub enforcement"
        );
        assert!(
            merge_ready_doc.contains(required) && status_doc.contains(required),
            "docs must name live required proof context `{required}`"
        );
    }
    for advisory in ["Codecov / Patch 95", "codecov/patch"] {
        assert!(
            !policy_required_check(&parsed, advisory),
            "required-check policy must not mark advisory coverage context `{advisory}` as required"
        );
        assert!(
            merge_ready_doc.contains(advisory) && status_doc.contains(advisory),
            "docs must name advisory coverage context `{advisory}`"
        );
    }
}

fn policy_required_check(policy: &toml::Value, name: &str) -> bool {
    policy.get("checks").and_then(toml::Value::as_array).into_iter().flatten().any(|item| {
        item.get("name").and_then(toml::Value::as_str) == Some(name)
            && item.get("required").and_then(toml::Value::as_bool) == Some(true)
            && item.get("enforcement").and_then(toml::Value::as_str) == Some("github-ruleset")
    })
}

fn coverage_workflow_contract(workflow: &Value) -> Result<()> {
    let triggers = yaml_mapping_entry(workflow, "on")?;
    let schedule = yaml_mapping_entry(triggers, "schedule")?;
    ensure!(
        schedule.as_sequence().is_some_and(|entries| !entries.is_empty()),
        "coverage workflow must declare a non-empty top-level schedule trigger"
    );
    yaml_mapping_entry(triggers, "workflow_dispatch")?;

    let jobs = yaml_mapping_entry(workflow, "jobs")?;
    let coverage_job = yaml_mapping_entry(jobs, "test-coverage")?;
    let condition = yaml_mapping_entry(coverage_job, "if")?
        .as_str()
        .ok_or_else(|| anyhow!("coverage job must declare a string if expression"))?
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    ensure!(
        condition
            == "github.event_name=='schedule'||(github.event_name=='workflow_dispatch'&&inputs.run_coverage)",
        "coverage job must select only the top-level schedule or manual coverage route"
    );
    Ok(())
}

fn coverage_required_check_contract(policy: &TomlValue) -> Result<()> {
    let coverage_checks =
        toml_target_tables(policy, "checks", ".github/workflows/ci-nightly.yml", "test-coverage")?;
    ensure!(
        coverage_checks.len() == 1,
        "required-checks policy must contain exactly one Codecov / Patch 95 entry"
    );
    let coverage = coverage_checks
        .first()
        .ok_or_else(|| anyhow!("coverage required-check entry is missing"))?;
    ensure!(
        coverage.get("name").and_then(TomlValue::as_str) == Some("Codecov / Patch 95"),
        "coverage required-check entry must retain the canonical display name"
    );
    ensure!(
        coverage.get("workflow").and_then(TomlValue::as_str)
            == Some(".github/workflows/ci-nightly.yml")
            && coverage.get("job").and_then(TomlValue::as_str) == Some("test-coverage"),
        "coverage required-check entry must identify the nightly test-coverage job"
    );
    let events = coverage
        .get("events")
        .and_then(TomlValue::as_array)
        .ok_or_else(|| anyhow!("coverage required-check entry must declare events"))?
        .iter()
        .map(|event| event.as_str())
        .collect::<Option<Vec<_>>>();
    ensure!(
        events.as_deref() == Some(["schedule", "workflow_dispatch"].as_slice()),
        "coverage required-check entry must list only schedule and workflow_dispatch events"
    );
    ensure!(
        coverage.get("required").and_then(TomlValue::as_bool) == Some(false)
            && coverage.get("policy_role").and_then(TomlValue::as_str) == Some("advisory")
            && coverage.get("applicability").and_then(TomlValue::as_str) == Some("conditional")
            && coverage.get("enforcement").and_then(TomlValue::as_str) == Some("neither"),
        "coverage required-check entry must remain advisory and conditional"
    );
    ensure_no_positive_stale_route_claim(coverage, "required-check coverage entry")?;
    Ok(())
}

fn coverage_whitelist_contract(policy: &TomlValue) -> Result<()> {
    let coverage_lanes =
        toml_target_tables(policy, "lane", ".github/workflows/ci-nightly.yml", "test-coverage")?;
    ensure!(coverage_lanes.len() == 1, "coverage whitelist must contain exactly one coverage lane");
    let coverage =
        coverage_lanes.first().ok_or_else(|| anyhow!("coverage whitelist entry is missing"))?;
    ensure!(
        coverage.get("id").and_then(TomlValue::as_str) == Some("coverage"),
        "coverage whitelist entry must retain the canonical lane id"
    );
    ensure!(
        coverage.get("workflow").and_then(TomlValue::as_str)
            == Some(".github/workflows/ci-nightly.yml")
            && coverage.get("job").and_then(TomlValue::as_str) == Some("test-coverage"),
        "coverage whitelist entry must identify the nightly test-coverage job"
    );
    let triggers = coverage
        .get("allowed_triggers")
        .and_then(TomlValue::as_array)
        .ok_or_else(|| anyhow!("coverage whitelist must declare allowed_triggers"))?
        .iter()
        .map(|trigger| trigger.as_str())
        .collect::<Option<Vec<_>>>();
    ensure!(
        triggers.as_deref() == Some(["schedule", "workflow_dispatch"].as_slice()),
        "coverage whitelist must allow only schedule and workflow_dispatch"
    );
    for field in ["labels", "branches"] {
        match coverage.get(field) {
            None => {}
            Some(values) => ensure!(
                values.as_array().is_some_and(|values| values.is_empty()),
                "coverage whitelist must not route coverage by `{field}`"
            ),
        }
    }
    ensure_no_positive_stale_route_claim(coverage, "coverage whitelist entry")?;
    Ok(())
}

fn toml_target_tables<'a>(
    document: &'a TomlValue,
    array_key: &str,
    workflow: &str,
    job: &str,
) -> Result<Vec<&'a toml::value::Table>> {
    let records = document
        .get(array_key)
        .and_then(TomlValue::as_array)
        .ok_or_else(|| anyhow!("policy must contain a `{array_key}` array"))?;
    Ok(records
        .iter()
        .filter_map(TomlValue::as_table)
        .filter(|record| {
            record.get("workflow").and_then(TomlValue::as_str) == Some(workflow)
                && record.get("job").and_then(TomlValue::as_str) == Some(job)
        })
        .collect())
}

fn ensure_no_positive_stale_route_claim(table: &toml::value::Table, context: &str) -> Result<()> {
    for value in table.values() {
        ensure_no_positive_stale_route_value(value, context)?;
    }
    Ok(())
}

fn ensure_no_positive_stale_route_value(value: &TomlValue, context: &str) -> Result<()> {
    match value {
        TomlValue::String(text) => {
            ensure!(
                !has_positive_stale_route_claim(text),
                "{context} contains a positive stale PR, merge-queue, label, or branch claim"
            );
            Ok(())
        }
        TomlValue::Array(values) => {
            for value in values {
                ensure_no_positive_stale_route_value(value, context)?;
            }
            Ok(())
        }
        TomlValue::Table(values) => {
            for value in values.values() {
                ensure_no_positive_stale_route_value(value, context)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn has_positive_stale_route_claim(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase().replace(['_', '-', '\n', '\r'], " ");
    let has_pr_token = has_token(&normalized, "pr") || has_token(&normalized, "prs");
    route_prose_clauses(&normalized).iter().any(|clause| has_positive_stale_route_clause(clause))
        || (has_token(&normalized, "coverage")
            && has_tokenized_phrase(&normalized, "full ci")
            && (has_tokenized_phrase(&normalized, "deep lane") || has_token(&normalized, "label"))
            && !has_negative_route_prose(&normalized))
        || (has_token(&normalized, "coverage")
            && has_pr_token
            && (has_tokenized_phrase(&normalized, "deep lane")
                || has_tokenized_phrase(&normalized, "risk pack")
                || has_tokenized_phrase(&normalized, "pr smoke"))
            && !has_negative_route_prose(&normalized))
}

fn has_token(normalized: &str, expected: &str) -> bool {
    normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| token == expected)
}

fn has_tokenized_phrase(normalized: &str, phrase: &str) -> bool {
    let expected = phrase
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if expected.is_empty() {
        return false;
    }
    let tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.windows(expected.len()).any(|window| window == expected.as_slice())
}

fn is_direct_route_token(token: &str) -> bool {
    matches!(token, "run" | "runs" | "required" | "gate" | "gated" | "trigger" | "on")
}

fn route_prose_clauses(normalized: &str) -> Vec<&str> {
    let mut clauses = Vec::new();
    for punctuation_clause in normalized.split(['.', ',', ';', ':', '!', '?']) {
        let mut remaining = punctuation_clause;
        loop {
            let Some((separator, separator_text)) =
                [" but ", " without ", " never ", " even though ", " and ", " or "]
                    .iter()
                    .filter_map(|separator| {
                        remaining.find(separator).map(|index| (index, *separator))
                    })
                    .min_by_key(|(index, _)| *index)
            else {
                let clause = remaining.trim();
                if !clause.is_empty() {
                    clauses.push(clause);
                }
                break;
            };
            let clause = remaining[..separator].trim();
            if !clause.is_empty() {
                clauses.push(clause);
            }
            remaining = &remaining[separator + separator_text.len()..];
        }
    }
    clauses
}

fn has_positive_stale_route_clause(normalized: &str) -> bool {
    if has_negative_route_prose(normalized) {
        return false;
    }
    let has_pr_token = has_token(normalized, "pr") || has_token(normalized, "prs");
    let has_route_term = [
        "pull request",
        "pull requests",
        "merge queue",
        "merge queues",
        "merge group",
        "merge groups",
        "label",
        "full ci",
        "master",
    ]
    .iter()
    .any(|marker| has_tokenized_phrase(normalized, marker))
        || (has_pr_token
            && ["run", "runs", "required", "gate", "gated", "trigger", "on"]
                .iter()
                .any(|marker| has_token(normalized, marker)));
    let has_coverage_term = has_token(normalized, "coverage") || has_token(normalized, "codecov");
    [
        "pull request coverage",
        "pull requests coverage",
        "pr coverage",
        "prs coverage",
        "coverage pull request",
        "coverage runs on pull request",
        "coverage required for pull request",
        "merge queue coverage",
        "merge group coverage",
        "coverage merge queue",
        "coverage merge group",
        "label gated coverage",
        "labeled coverage",
        "coverage label",
        "coverage labels",
        "full ci coverage",
        "coverage on master",
        "coverage branch master",
    ]
    .iter()
    .any(|marker| has_tokenized_phrase(normalized, marker))
        || (has_route_term && has_coverage_term)
}

fn has_negative_route_prose(normalized: &str) -> bool {
    let has_negative_token =
        normalized.split(|character: char| !character.is_ascii_alphanumeric()).any(|token| {
            matches!(token, "not" | "without" | "never" | "absent" | "neither" | "disabled")
        });
    let has_negative_phrase = ["does not", "doesn't", "doesn’t", "must not", "instead of"]
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_no_route_phrase = normalized.contains("no pull request")
        || normalized.contains("no merge queue")
        || normalized.contains("no merge group")
        || (normalized
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| token == "no")
            && normalized
                .split(|character: char| !character.is_ascii_alphanumeric())
                .any(|token| matches!(token, "pr" | "prs")));
    let has_route_context = normalized.contains("coverage")
        || normalized.contains("codecov")
        || normalized.contains("pull request")
        || (normalized.contains("full ci") && normalized.contains("label"))
        || normalized
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| matches!(token, "pr" | "prs"));
    let has_contextual_negative =
        ["coverage proof routed", "outside the pr", "schedule/manual only"]
            .iter()
            .any(|marker| normalized.contains(marker));
    has_contextual_negative
        || (has_route_context && (has_negative_token || has_negative_phrase || has_no_route_phrase))
}

/// Stricter detector for lines that sit in coverage context (the line itself
/// or its predecessor mentions coverage), where a bare PR/label/merge-queue
/// route verb is already a stale claim.
fn has_positive_stale_route_claim_in_context(text: &str) -> bool {
    if has_positive_stale_route_claim(text) {
        return true;
    }
    let normalized = text.to_ascii_lowercase().replace(['_', '-'], " ");
    if has_negative_route_prose(&normalized) {
        return false;
    }
    (has_tokenized_phrase(&normalized, "full ci") && has_token(&normalized, "label"))
        || ((has_tokenized_phrase(&normalized, "pull request")
            || has_tokenized_phrase(&normalized, "pull requests")
            || has_tokenized_phrase(&normalized, "merge queue")
            || has_tokenized_phrase(&normalized, "merge queues")
            || has_tokenized_phrase(&normalized, "merge group")
            || has_tokenized_phrase(&normalized, "merge groups")
            || has_token(&normalized, "label"))
            && ["run", "runs", "required", "select", "selected", "route", "trigger"]
                .iter()
                .any(|marker| has_token(&normalized, marker)))
}

fn has_positive_stale_route_claim_across_wrap(lines: &[&str]) -> bool {
    let normalized = lines
        .join(" ")
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .replace("doesn't", "does not")
        .replace("doesn’t", "does not");
    let tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    for (coverage_index, token) in tokens.iter().enumerate() {
        if !matches!(*token, "coverage" | "codecov") {
            continue;
        }
        for run_index in coverage_index + 1..tokens.len() {
            if !is_direct_route_token(tokens[run_index]) {
                continue;
            }
            if tokens[coverage_index + 1..run_index]
                .iter()
                .any(|token| matches!(*token, "not" | "no" | "never" | "without" | "disabled"))
            {
                continue;
            }
            let following = &tokens[run_index + 1..tokens.len().min(run_index + 7)];
            let route_index = following.iter().enumerate().find_map(|(index, token)| {
                if matches!(*token, "pr" | "prs" | "merge" | "queue" | "group" | "label")
                    || (index + 1 < following.len()
                        && (([*token, following[index + 1]] == ["pull", "request"])
                            || ([*token, following[index + 1]] == ["pull", "requests"])
                            || ([*token, following[index + 1]] == ["deep", "lane"])
                            || ([*token, following[index + 1]] == ["risk", "pack"])))
                {
                    Some(index)
                } else {
                    None
                }
            });
            let Some(route_index) = route_index else {
                continue;
            };
            if following[..route_index]
                .iter()
                .any(|token| matches!(*token, "not" | "no" | "never" | "without" | "disabled"))
            {
                continue;
            }
            return true;
        }
    }
    false
}

fn coverage_reference_rows_contract(document: &str, context: &str) -> Result<()> {
    let lines = document.lines().collect::<Vec<_>>();
    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if lower.contains("coverage") || lower.contains("codecov") {
            ensure!(
                !has_positive_stale_route_claim(line),
                "{context} contains stale coverage route wording: {line}"
            );
        }
    }
    for window in lines.windows(2) {
        let first_line = window[0].trim_end();
        let second_line = window[1].trim_start();
        let first_word = first_line
            .split(|character: char| !character.is_ascii_alphanumeric())
            .rfind(|word: &&str| !word.is_empty());
        let second_first_word = second_line
            .split(|character: char| !character.is_ascii_alphanumeric())
            .find(|word: &&str| !word.is_empty());
        let first_line_continues =
            matches!(first_word, Some("but" | "and" | "or" | "without" | "never"));
        let connector_starts_second_line =
            matches!(second_first_word, Some("but" | "and" | "or" | "without" | "never"))
                && first_line.ends_with(',');
        if matches!(first_line.chars().last(), Some('|' | '.' | '!' | '?' | ':'))
            || window.iter().any(|line| line.contains('|'))
            || (!first_line_continues && !connector_starts_second_line)
        {
            continue;
        }
        let joined = format!("{} {}", window[0], window[1]);
        let lower = joined.to_ascii_lowercase();
        if lower.contains("coverage") || lower.contains("codecov") {
            ensure!(
                !has_positive_stale_route_claim_across_wrap(window),
                "{context} contains stale coverage route wording across wrapped lines: {joined}"
            );
        }
    }
    for window in lines.windows(3) {
        let first_line = window[0].trim_end();
        let second_line = window[1].trim_end();
        let first_word = first_line
            .split(|character: char| !character.is_ascii_alphanumeric())
            .rfind(|word: &&str| !word.is_empty());
        let second_first_word = window[1]
            .split(|character: char| !character.is_ascii_alphanumeric())
            .find(|word: &&str| !word.is_empty());
        let first_line_continues =
            matches!(first_word, Some("but" | "and" | "or" | "without" | "never"));
        let connector_starts_second_line =
            matches!(second_first_word, Some("but" | "and" | "or" | "without" | "never"))
                && first_line.ends_with(',')
                && !matches!(second_line.chars().last(), Some('|' | '.' | '!' | '?' | ':'));
        if matches!(first_line.chars().last(), Some('|' | '.' | '!' | '?' | ':'))
            || matches!(second_line.chars().last(), Some('|' | '.' | '!' | '?' | ':'))
            || window[1].trim_start().starts_with('|')
            || window[2].trim_start().starts_with('|')
            || (!first_line_continues && !connector_starts_second_line)
        {
            continue;
        }
        let joined = format!("{} {} {}", window[0], window[1], window[2]);
        let lower = joined.to_ascii_lowercase();
        if lower.contains("coverage") || lower.contains("codecov") {
            ensure!(
                !has_positive_stale_route_claim_across_wrap(window),
                "{context} contains stale coverage route wording across three wrapped lines: {joined}"
            );
        }
    }
    Ok(())
}

fn coverage_risk_pack_contract(policy: &TomlValue, lane_policy: &TomlValue) -> Result<()> {
    let lanes = lane_policy
        .get("lane")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| anyhow!("lane policy must contain a lane table"))?;
    let coverage_lanes = lanes
        .iter()
        .filter_map(|(id, lane)| {
            let lane = lane.as_table()?;
            (lane.get("workflow").and_then(TomlValue::as_str)
                == Some(".github/workflows/ci-nightly.yml")
                && lane.get("job").and_then(TomlValue::as_str) == Some("test-coverage"))
            .then_some((id.as_str(), lane))
        })
        .collect::<Vec<_>>();
    ensure!(
        coverage_lanes.len() == 1 && coverage_lanes[0].0 == "coverage",
        "lane policy must contain exactly one canonical test-coverage lane"
    );
    let packs = policy
        .get("risk_pack")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| anyhow!("risk-pack policy must contain a risk_pack table"))?;
    for (pack_id, pack) in packs {
        let Some(pack) = pack.as_table() else {
            continue;
        };
        let context = format!("risk pack `{pack_id}`");
        for value in pack.values() {
            ensure_no_positive_stale_route_value(value, &context)?;
        }
        for field in ["lanes", "deep_lanes", "labels"] {
            if let Some(values) = pack.get(field).and_then(TomlValue::as_array) {
                ensure!(
                    !values.iter().filter_map(TomlValue::as_str).any(|value| {
                        value == "coverage"
                            || (field == "labels" && value.eq_ignore_ascii_case("ci:coverage"))
                            || (field != "labels"
                                && lanes.get(value).and_then(TomlValue::as_table).is_some_and(
                                    |lane| {
                                        lane.get("workflow").and_then(TomlValue::as_str)
                                            == Some(".github/workflows/ci-nightly.yml")
                                            && lane.get("job").and_then(TomlValue::as_str)
                                                == Some("test-coverage")
                                    },
                                ))
                    }),
                    "risk pack `{pack_id}` must not route schedule/manual-only coverage through `{field}`"
                );
            }
        }
    }
    Ok(())
}

fn coverage_risk_pack_docs_contract(document: &str) -> Result<()> {
    let parser_row = document
        .lines()
        .find(|line| line.contains("| `parser` |"))
        .ok_or_else(|| anyhow!("risk-pack docs must contain the parser catalog row"))?;
    ensure!(
        !has_positive_stale_route_claim(parser_row),
        "parser risk-pack docs must not advertise coverage as a PR deep lane"
    );
    coverage_reference_rows_contract(document, "risk-pack docs")?;
    let mut previous_coverage_line = false;
    for line in document.lines() {
        let lower = line.to_ascii_lowercase();
        let coverage_context =
            lower.contains("coverage") || lower.contains("codecov") || previous_coverage_line;
        if coverage_context && has_positive_stale_route_claim_in_context(line) {
            ensure!(false, "risk-pack docs contain stale coverage route wording: {line}");
        }
        previous_coverage_line = !line.trim().is_empty()
            && (lower.contains("coverage")
                || lower.contains("codecov")
                || (previous_coverage_line && !has_positive_stale_route_claim(line)));
    }
    Ok(())
}

fn coverage_rollout_docs_contract(document: &str) -> Result<()> {
    let has_stale_coverage_branch = document.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("master") && (lower.contains("coverage") || lower.contains("codecov"))
    });
    ensure!(
        !has_stale_coverage_branch,
        "Codecov rollout docs must use the live main branch for coverage references"
    );
    Ok(())
}

fn codecov_posture_contract(config: &Value) -> Result<()> {
    let codecov = yaml_mapping_entry(config, "codecov")?;
    ensure!(
        mapping_value(codecov, "require_ci_to_pass").and_then(Value::as_bool) == Some(false),
        "Codecov must remain independent of unrelated CI failures"
    );
    Ok(())
}

fn codecov_threshold_contract(config: &Value) -> Result<()> {
    let status = yaml_mapping_entry(yaml_mapping_entry(config, "coverage")?, "status")?;
    for (section, target, threshold) in [("project", "95%", "2%"), ("patch", "95%", "0%")] {
        let default = yaml_mapping_entry(yaml_mapping_entry(status, section)?, "default")?;
        ensure!(
            mapping_value(default, "target").and_then(Value::as_str) == Some(target)
                && mapping_value(default, "threshold").and_then(Value::as_str) == Some(threshold)
                && mapping_value(default, "informational").and_then(Value::as_bool) == Some(true)
                && mapping_value(default, "if_ci_failed").and_then(Value::as_str) == Some("ignore"),
            "Codecov `{section}` status must retain its informational {target} target and {threshold} threshold"
        );
    }
    Ok(())
}

fn coverage_baseline_contract(
    coverage_job: &str,
    coverage_yaml_job: &Value,
    justfile: &str,
) -> Result<()> {
    ensure!(
        mapping_value(coverage_yaml_job, "continue-on-error").is_none(),
        "nightly/manual coverage baseline enforcement must remain job-blocking"
    );
    ensure!(
        coverage_job.contains("just coverage-proof \"origin/$base_ref\""),
        "nightly/manual coverage must execute the shared baseline proof recipe"
    );
    let recipe_start = justfile
        .find("coverage-proof base='origin/main':")
        .ok_or_else(|| anyhow!("justfile must define the workspace coverage proof recipe"))?;
    let recipe_tail = &justfile[recipe_start..];
    let recipe_end =
        recipe_tail.find("\n# Generate route-selected coverage").unwrap_or(recipe_tail.len());
    let recipe = &recipe_tail[..recipe_end];
    for required in [
        "cargo xtask coverage-baseline",
        "--codecov codecov.yml",
        "--receipt target/receipts/quality/coverage-baseline.json",
        "cargo xtask quality-gate",
        "--mode enforce-patch-coverage",
        "--receipt target/receipts/quality/quality-gate-coverage.json",
        "--summary target/receipts/quality/quality-gate-coverage.md",
    ] {
        ensure!(recipe.contains(required), "coverage proof recipe must contain `{required}`");
    }
    ensure!(
        recipe.matches("--mode enforce-patch-coverage").count() == 2,
        "coverage proof recipe must enforce the patch gate on write and check passes"
    );
    Ok(())
}

fn mapping_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_mapping().and_then(|mapping| mapping.get(Value::String(key.to_owned())))
}

fn toml_lane<'a>(document: &'a TomlValue, id: &str) -> Option<&'a toml::value::Table> {
    document.get("lane").and_then(TomlValue::as_array).and_then(|lanes| {
        lanes.iter().find_map(|lane| {
            let table = lane.as_table()?;
            (table.get("id").and_then(TomlValue::as_str) == Some(id)).then_some(table)
        })
    })
}

fn toml_array_is_empty(value: Option<&TomlValue>) -> bool {
    match value {
        None => true,
        Some(value) => value.as_array().is_some_and(|array| array.is_empty()),
    }
}

fn checkout_action_ref(step: &str) -> Option<&str> {
    step.lines().find_map(|line| {
        let trimmed = line.trim_start();
        trimmed.strip_prefix("- uses: actions/checkout@")?.split_whitespace().next()
    })
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    must_some(manifest.parent()).to_path_buf()
}

fn workflow_step<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("- name: {name}");
    let start = content.find(&needle)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(needle.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(
            |(offset, line)| {
                if line.trim_start().starts_with("- name:") { Some(offset) } else { None }
            },
        )
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn workflow_job<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("\n  {name}:\n");
    let start = content.find(&needle)? + 1;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(rest.lines().next()?.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| {
            let bytes = line.as_bytes();
            if line.starts_with("  ")
                && !line.starts_with("    ")
                && bytes.get(2).is_some_and(u8::is_ascii_alphanumeric)
            {
                Some(offset)
            } else {
                None
            }
        })
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn nightly_job_selector_is_exclusive(job: &str, expected_input: &str) -> bool {
    let selector = format!("(github.event_name == 'workflow_dispatch' && inputs.{expected_input})");
    job.contains(&selector)
        && job.matches("github.event_name == 'workflow_dispatch'").count() == 1
        && job.matches("inputs.run_").count() == 1
}

fn yaml_mapping_entry<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_mapping()
        .ok_or_else(|| anyhow!("expected a YAML mapping while looking for `{key}`"))?
        .get(Value::String(key.to_owned()))
        .ok_or_else(|| anyhow!("missing YAML key `{key}`"))
}
