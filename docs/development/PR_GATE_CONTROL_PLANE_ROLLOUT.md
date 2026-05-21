# PR Gate Control Plane Burndown

> **Substrate (already built)**: CI Gate aggregate, PR Smoke, merge-gate shards,
> UX regression gate, required-check policy, workflow-trigger lint,
> methodology gate, merge-ready receipts, PR title checker, PR Plan,
> workflow policy lint, and CI policy ledgers.
>
> **Connector gap**: the PR gate is operational but not yet contract-aware:
> required-check policy consumers disagree, merge-ready receipts are not fully
> evidence-backed, PR bodies do not carry a validated work contract, and PR Plan
> proof requirements are advisory rather than connected to gate verdicts.
>
> **0.14.0 upside**: Codex and humans can open a PR with a precise contract,
> run the right proof, avoid fake merge rules, and get a reproducible gate
> verdict tied to head SHA, base SHA, gate policy version, and declared claim
> boundary.

## Current gate map

| Layer | Current state |
|---|---|
| Hard CI gate | `.github/workflows/ci.yml` runs PR smoke, shard gates, UX tests, Windows, all-target checks, and aggregates under `CI Gate (Merge-Blocking)` / `ci/merge-gate`. |
| Required-check policy | `.ci/policies/required-checks.toml` is intended source-of-truth for required checks. |
| Merge-ready receipt | `merge-ready` emits/verifies receipts tied to PR head SHA, base lineage, and gate graph version. |
| Gate graph hash | `merge_ready.rs` hashes required-check policy, `.ci/policies/**`, `.ci/gates.d/**`, and required-style workflows. |
| Workflow trigger lint | Required-style workflows are validated for `pull_request`, `merge_group`, push to master, no path filters, and safe concurrency. |
| Methodology gate | Label contradictions and closeout hygiene are checked, currently advisory unless explicitly enforced. |
| PR Plan / CI economics | PR Plan computes risk packs, lanes, budgets, trust-lane class, skipped checks, and proof expectations, but is advisory. |
| Policy ledgers | Lanes, budgets, risk packs, allowlists, and exceptions already follow governance ledger patterns. |
| Workflow policy lint | Unsafe workflow patterns are checked; lane-whitelist validation exists as advisory extension. |

## Requirements (R0–R9)

### R0 — Normalize required-check policy

Acceptance:
- `workflow-trigger-lint` and `merge-ready` consume one parsed model.
- Mixed `[[check]]` / `[[checks]]` schema is migrated or rejected.
- Only `required = true` checks enter required-check receipts.
- Advisory checks remain representable but never masquerade as required.

### R1 — Required-check policy validator

Add command:

```bash
cargo xtask required-checks-lint \
  --policy .ci/policies/required-checks.toml \
  --receipt target/receipts/required-checks.json
```

Validator must cover parseability, uniqueness, required metadata, schema shape, and trigger policy enforcement for required checks.

### R2 — Merge-ready consumes normalized policy

`merge-ready` must use the same parser/model as workflow-trigger lint and emit structured required-check receipt entries (name/workflow/context/required), not names alone.

### R3 — Merge-ready evidence is real

Replace static review evidence with sourced evidence fields:
- `ci_evidence`
- `methodology_evidence`
- `review_evidence`
- `blocker_label_evidence`
- `pr_contract_evidence`

Early rollout may emit `advisory`/`unavailable` status, but receipts must be explicit about source quality.

### R4 — PR body contract template

Add or update `.github/pull_request_template.md` with required sections:
- Issue / Work item
- Rail / Spec
- Scope
- Non-goals
- Proof
- Claim boundary
- Rollback
- Policy impact
- Support-tier impact

### R5 — PR body contract checker

Add command:

```bash
cargo xtask pr body-check \
  --body-file <path> \
  --receipt target/receipts/pr-body.json
```

Checks: required headings, non-empty Proof/Claim boundary/Rollback, explicit policy/support-tier impact, and close-keyword restrictions for partial/scaffold PRs.

### R6 — PR Plan proof obligations connector

Connect `PR Plan required proof -> PR body Proof section -> receipts/artifacts`.

Rollout: advisory first; later only high-risk PRs block on missing proof declaration.

### R7 — Workflow lane whitelist promotion

Promotion ladder:
1. advisory warning;
2. advisory receipt with missing lane count;
3. fail for new workflows missing lanes;
4. fail for all PR-visible workflows missing lanes.

### R8 — Gate graph version includes PR-contract policy

When PR contract files exist, include these in gate-graph hash inputs:
- `.github/pull_request_template.md`
- `docs/development/PR_GATE_CONTROL_PLANE_ROLLOUT.md`
- `policy/pr-body-contract.toml`

### R9 — Advisory before blocking

Promotion rule:

```text
advisory receipt -> clean burn-in -> required in policy -> blocking workflow
```

No branch-protection theater before empirical burn-in.

## PR sequence

1. **PR 1 — rail doc only**: add this rail doc + index row; proof: `git diff --check`.
2. **PR 2 — required-check schema cleanup**: normalize `.ci/policies/required-checks.toml`; preserve CI Gate requiredness; mark advisory checks.
3. **PR 3 — required-checks-lint**: add `cargo xtask required-checks-lint`.
4. **PR 4 — merge-ready policy reader**: use shared parser and emit structured required checks.
5. **PR 5 — PR body template**: add contract-oriented `.github/pull_request_template.md`.
6. **PR 6 — PR body checker**: add `cargo xtask pr body-check`.
7. **PR 7 — advisory CI wiring**: run body-check in methodology/CI and emit advisory receipt.
8. **PR 8 — PR Plan proof connector**: connect plan proof obligations to PR body proof declaration.
9. **PR 9 — lane-whitelist advisory receipt**: emit workflow lane-whitelist receipt.
10. **PR 10 — merge-ready evidence sourcing**: replace static evidence fields with sourced evidence.
11. **PR 11 — gate-graph PR contract inputs**: hash PR contract policy/template files.
12. **PR 12 — narrow blocker promotion**: only promote hardened schema/body checks to blocking.

## Receipts

Target receipt artifacts for this rail include:
- `target/receipts/workflow-trigger-lint.json`
- `target/receipts/required-checks.json`
- `target/receipts/merge-readiness.json`
- `target/receipts/pr-body.json`
- `target/receipts/methodology-gate.json`
- `target/receipts/workflow-policy.json`
- `target/ci/ci-plan.json`

## Lane assignment

- **Primary lane**: `codex`
- **Secondary lane (if split needed later)**: `builder`

## Do not combine

Do not combine this rail work with:
- LSP latency work
- parser behavior changes
- clippy cleanup rails
- codecov/evidence rail items outside this PR-gate scope
- file-policy rollout items
- release-prep/version-sync work

One semantic change per PR across this ladder.

## Exit criteria

- [ ] `.ci/policies/required-checks.toml` has one schema.
- [ ] `workflow-trigger-lint` and `merge-ready` consume one required-check model.
- [ ] `cargo xtask required-checks-lint` exists and passes.
- [ ] Merge-ready receipt includes structured required-check evidence.
- [ ] Merge-ready receipt no longer uses fake/static review evidence.
- [ ] PR body template exists.
- [ ] `cargo xtask pr body-check` exists.
- [ ] PR body checker validates proof, claim boundary, rollback, and policy/support-tier impact.
- [ ] PR Plan proof obligations are visible in PR-body validation (at least advisory).
- [ ] Workflow lane whitelist receipt exists.
- [ ] Gate graph version includes PR-contract policy inputs.
- [ ] New gates are advisory before blocking.
- [ ] Blocking promotion is documented and narrow.
- [ ] Claim boundary is recorded.

## Claim boundary

This rail proves:
- one required-check source-of-truth model for gate consumers;
- merge-ready receipts tied to real gate policy inputs;
- PR contracts declare proof, scope, claim boundary, rollback, and policy/support-tier impact;
- Codex can inspect repo artifacts to derive required PR proof;
- gate policy changes stale older merge-ready receipts.

This rail does **not** prove:
- full support-tier validation for every feature claim;
- full proposal/spec/ADR/plan artifact linkage;
- active-goal manifest completeness;
- tokmd proof-stack implementation;
- complete CI economics enforcement;
- cost-optimal lane allocation;
- live Neovim latency improvements.
