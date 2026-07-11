---
tags: [ci, ci-truth, signal-truth-verification, draft-skip]
repos: [perl-lsp-swarm]
related: ["#3701", "#3732"]
portable: true
article_asset: true
search_terms: [CI green, check-run, head SHA, draft-skip neutral, base branch policy prohibits, cancellation CASCADE, ripr gap quality gate, re-run, stale green]
---

# CI "green" signal diverges from actual check-run state on current HEAD

**Date**: 2026-07
**Hazard class**: ci-truth / signal-truth divergence
**Portable lesson**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)

## What happened

During merge operations on PR #3701 (goals input validation fix-forward), the `gh pr checks` command reported "pass" for all required checks. However, the actual GitHub check-run objects showed that checks had never run on the current HEAD SHA — only on an older commit. The signal ("all checks pass") diverged from ground truth (no checks ran on current HEAD).

Additionally, two distinct failure modes were conflated: (a) a check marked "CANCELLED" at "Generate PR evidence" stage, which indicates a cancellation cascade from concurrent merges (re-run resolves it), and (b) a check marked "FAILED" at "Enforce new RIPR gap quality gate" stage, which indicates a real gap detection (add coverage to fix it). A re-run of a cascaded check re-triggering the gate resolves the cascade; a re-run of a real gap simply re-fails until the coverage is added.

The merge-gate decision (proceed or wait?) depends on distinguishing the failure class before acting. The operator must read the check-run conclusion on the exact HEAD SHA, not trust the PR-summary signal.

## Why

GitHub's PR-summary API (`gh pr checks`) aggregates check results but can report stale data (a prior push's check state) when new pushes are in flight. Additionally, check statuses use status strings ("pass", "fail", "pending", "cancelled") that are compressed summaries — the underlying check-run object's `conclusion` and `status` fields carry the detailed state (e.g., "cancelled at step X" vs "failed at step Y").

Relying on the aggregated summary without reading the check-run details introduces a signal-truth gap: the summary is evidence-quality until verified against the underlying check-run object.

## Fix

Three corrective moves:

1. **#3701 operator experience**: before proceeding past CI results, verify `gh pr view --json headRefOid` and read the real check-run objects via `gh run view <run-id>`. Match the SHA; read the `conclusion` and `status` fields.

2. **Cascade vs gap classification**: when a check is "CANCELLED", read the cancel reason (check-run `conclusion` = "cancelled", step logs for where). When a check is "FAILED", read the failure logs. Cascades re-run; gaps require coverage changes.

3. **Merge gate (#3732 convergence feature)**: added automated SHA verification to the merge-ready gate — checks required status on current HEAD, not a stale result from an earlier push.

## Spec impact

- [docs/reference/CI_GATE_PLAYBOOK.md](../reference/CI_GATE_PLAYBOOK.md): added troubleshooting section on distinguishing cascades (re-run) from real gaps (coverage/fix). Taught the `gh run view` command and the importance of reading check-run `conclusion` fields, not PR-summary statuses.
- [docs/agents/SPEC_UPDATE_CHECKLIST.md](../agents/SPEC_UPDATE_CHECKLIST.md): added merge-gate acceptance criterion — "verify each required check's conclusion on current HEAD SHA; stale results do not unblock merge."

## Portable lesson

A CI badge and an aggregated PR-summary status are instrument readings. The underlying check-run objects are ground truth. When a status looks wrong, read the check-run. When a failure re-occurs on re-run, confirm it's not a cascade by reading the step logs.

- **Pattern**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)
- **Class**: CI-truth / signal-truth divergence
- **Generalization**: A reporting layer (PR summary, CI badge, agent claim) condenses ground truth into a signal. The signal may diverge from ground truth (stale SHA, wrong scope, cascaded-vs-real failure mode). Verify the signal against the primary artifact (the check-run, the git log, the actual state of record) before acting.

## Related PRs

- [#3701](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3701) — fix-forward experiencing CI-green divergence
- [#3732](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3732) — convergence gate: automated SHA verification for merge
