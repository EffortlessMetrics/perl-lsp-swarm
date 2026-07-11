---
tags: [multi-lane, coordination, held-state, merge-gate]
repos: [perl-lsp-swarm]
related: ["#3627", "#3650", "#3659", "#3637"]
portable: true
article_asset: true
search_terms: [held PR, needs-deep-review label, already merged, parallel merge lanes, label not a block, verify PR state, gh pr view, stale label]
---

# Held-PR state goes stale in multi-lane repo: label is not an inter-lane merge-block

**Date**: 2026-07
**Hazard class**: multi-lane / coordination gap
**Portable lesson**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)

## What happened

PRs #3627 (unparenthesized declarations), #3650 (heredoc offsets), #3659 (diagnostics PL404), and #3637 (hir classify) were each labeled `needs-deep-review` and a deep-review agent was routed to them. Hours later, all four PRs had already been merged to main by a parallel merge lane. The deep-review agent discovered the PRs were no longer current (base branch was main, PR was merged), and the held-state routing decision was stale.

The root cause: a label like `needs-deep-review` affects routing within a single decision-loop, but in a multi-lane high-velocity repo, another lane can merge a PR while the label is in effect. The label is not an inter-lane merge-block; it only gates actions within the lane that set it.

## Why

In a repo with multiple concurrent merge lanes and rapid velocity (3+ PRs merging per hour), a label's presence does not guarantee the PR's state is current. A PR labeled `needs-deep-review` at time T can be merged by another lane at time T+30min. An agent acting on the label at time T+60min is acting on stale state.

The distinction: a label is a routing signal *within* a lane (gates the next action in the same decision loop); it is not a cross-lane merge-block (does not prevent another lane from merging).

## Fix

Verification step added to agent pre-flight:

1. **Before acting on a PR's label state**, verify the PR is still open and on a current base branch: `gh pr view <PR> --json state,baseRefName`
2. If state = "MERGED", the PR is no longer current; skip the action and log (do not treat as error).
3. If base branch has drifted from `main`, rebase or close the PR.

This is cheap ground-truth verification: confirm the signal (the label) matches the underlying state (PR open, base = main).

## Spec impact

- [docs/reference/MAINTAINER_AGENT_DOCTRINE.md](../reference/MAINTAINER_AGENT_DOCTRINE.md): added guidance on multi-lane coordination — labels are routing signals within a lane, not inter-lane merge-blocks. Before acting on a label, verify the PR's current state (open/merged, base branch).
- [.claude/agents/AGENT_CATALOG.md](.claude/agents/AGENT_CATALOG.md): added pre-flight check for all agents routing on label state — "Verify PR is open and on main branch before proceeding."

## Portable lesson

A label's presence is a signal that the PR had a certain state at label-creation time. In a multi-lane system, that state can become stale. Verify the underlying fact (PR open, base = main) before routing, not just the label.

- **Pattern**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)
- **Class**: Multi-lane coordination / signal-truth divergence
- **Generalization**: A label is an instrument reading of PR state at creation time. In a high-velocity multi-lane system, that reading ages quickly. Before acting on a label, verify the underlying state is current.

## Related PRs

- [#3627](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3627) — unparenthesized declarations, labeled held but already merged
- [#3650](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3650) — heredoc offsets, labeled held but already merged
- [#3659](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3659) — diagnostics PL404, labeled held but already merged
- [#3637](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3637) — hir classify, labeled held but already merged
