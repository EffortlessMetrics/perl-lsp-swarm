---
tags: [review-integrity, multi-agent, resolve-behavior, review-thread]
repos: [perl-lsp-swarm]
related: ["#3647", "#3701", "#3637", "#3703", "#3659", "#3705", "#3740", "#3753"]
portable: true
article_asset: true
search_terms: [resolveReviewThread, resolved-to-clear, disposition, reply-context, thread resolve, 15 threads, goals selector, hir classify, P1 live, silent resolve]
---

# Resolved review threads without disposition reply ship live defects

**Date**: 2026-07
**Hazard class**: review-integrity / review-process gap
**Portable lesson**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)

## What happened

PR #3647 (goals selector) merged after reaching "3-green CI + merge-ready label" with 6 live P1 defects in production. Post-merge investigation revealed that the PR's review process had silently resolved 15 review threads with no reply or disposition. Each resolution cleared the thread from the reviewer's checklist (unresolved-thread count → 0) without addressing the concern or documenting the decision (reply-with-evidence / reply-with-plan / reply-dismiss).

Similar patterns appeared in #3637 (hir classify) and #3659 (diagnostics PL404): test-integrity issues that passed review and merged because review threads were resolved-without-disposition. Remediated by #3701, #3703, #3705 (fix-forward PRs), and the disposition convention encoded in #3740 and embedded in the convergence gate (#3753).

## Why

A resolved thread unblocks the merge gate (GitHub's required-check `Merge blocked by outdated review` clears when all threads are resolved, regardless of how). The "resolved" state is a signal that means "thread is complete and accounted for" in the review-protocol sense, but has no enforcement that the resolution includes evidence or reasoning. Reviewers and responders can resolve threads programmatically without posting a reply.

The risk: a thread is resolved (gate clears) without the disposition being recorded (reply posted). The next reviewer, the maintainer, and the operator cannot see why the concern was dismissed. Code ships with unaddressed review feedback.

## Fix

Three changes landed in sequence:

1. **#3740** teaches the disposition convention: every resolved thread MUST include a reply (evidence-based or plan-based), not just a resolve action. The reply documents the decision.

2. **#3753** embeds disposition-reply verification in the convergence gate (review-protocol R1): the gate now checks `resolved_without_disposition` (resolved threads lacking a reply post by the resolving agent). Gate applies the check advisory-first (does not block), defaulting to `REVIEW_PROTOCOL_ENFORCE=false`.

3. Agents are routed to understand: resolving a thread without replying is a signal of incomplete work, not completion.

## Spec impact

- [docs/agents/SPEC_UPDATE_CHECKLIST.md](../agents/SPEC_UPDATE_CHECKLIST.md): added acceptance criterion under "Review integrity" — every resolved thread must include a reply documenting the disposition.
- [docs/reference/MAINTAINER_AGENT_DOCTRINE.md](../reference/MAINTAINER_AGENT_DOCTRINE.md): added guidance on thread-resolution discipline and the distinction between "thread resolved in GitHub UI" and "review feedback addressed via reply."
- Convergence gate (#3753): `resolved_without_disposition` check rolls out advisory (does not block) with a follow-up to enforce after dogfood.

## Portable lesson

In review systems where "resolved" is a UI action decoupled from "reply posted," the gate sees only the UI state. A resolution without a reply-post is a signal failure: the thread state (resolved) diverges from the evidence state (no reply). This unifies with the broader "verify live truth over the reported signal" pattern.

- **Pattern**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)
- **Class**: Review-process gap / signal-truth divergence
- **Generalization**: A gate's "clear" state may diverge from the gate's *intent*. A thread count of "0 unresolved" does not mean "all feedback addressed" — verify the signal against the underlying fact (reply posts, not just resolution actions).

## Related PRs

- [#3647](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3647) — goals selector, merged with unaddressed threads
- [#3701](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3701) — fix-forward: goals input validation gaps
- [#3637](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3637) — hir classify, test rewrites, fixed by #3703
- [#3703](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3703) — fix-forward: symbolic-ref deref classification
- [#3659](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3659) — diagnostics PL404, fixed by #3705
- [#3705](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3705) — fix-forward: PL404 scope tie-breaker
- [#3740](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3740) — teaches disposition convention
- [#3753](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3753) — convergence gate R1 protocol
