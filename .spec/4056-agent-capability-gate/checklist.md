# Implementation Checklist: #4056 - Agent capability gate runner routing

## Change order

### Step 1: Encode the contract

- **Files:** `.spec/4056-agent-capability-gate/{context,checklist,acceptance}.md`
- **Change:** Record trust routing, fallback reasons, preserved workflow
  surfaces, proof commands, and the external-prerequisite boundary.
- **Verify:** Review against the current workflow, runner labels, and policy
  conventions before editing implementation.

### Step 2: Add workflow-policy coverage

- **File:** `xtask/tests/agent_capability_gate_workflow_policy.rs`
- **Change:** Parse the workflow and verify static runner labels, route outputs,
  fork/bot isolation, fallback branches, preserved triggers/permissions/
  concurrency, and non-optional command failure propagation.
- **Verify:** `rtk cargo test -p xtask --test agent_capability_gate_workflow_policy`

### Step 3: Route execution

- **File:** `.github/workflows/agent-capability-gate.yml`
- **Change:** Add the hosted router, self-hosted `workflow-nano` job, and
  `ubuntu-24.04` fallback job. Keep the existing capability command unchanged.
- **Verify:** workflow-policy lint plus the focused policy test.

### Step 4: Reconcile governed documentation

- **Files:** `policy/ci-lane-whitelist.toml`, `docs/ci/inventory.md`,
  `docs/ci/ci-lane-map.md`, `docs/reference/PIPELINE_GATES.md`
- **Change:** Describe the mixed runner route, explicit fallback, and claim
  boundary without promoting the lane to a required merge check.
- **Verify:** `python3 scripts/ci/validate_risk_packs.py --strict`,
  `python3 scripts/ci/validate_gate_lane_mapping.py --strict`, TOML parse,
  workflow-policy lint, and documentation diff review.

### Step 5: Exact-head proof

- **Verify:** focused policy test, `rtk cargo fmt --all -- --check`,
  `rtk git diff --check`, `rtk cargo allow diff --base origin/main`, relevant
  `rtk cargo allow check` baseline classification, then exact-head CI and live
  runner route evidence.

## Scope boundary

In scope: the agent-capability workflow, its policy ledger and CI reference
surfaces, the focused workflow contract test, and this spec bundle.

Out of scope: changes to `.claude/agents/*.md`, capability-policy semantics,
other workflows, runner provisioning, secret/token permissions, branch rules,
and promotion of this advisory lane to a required check.
