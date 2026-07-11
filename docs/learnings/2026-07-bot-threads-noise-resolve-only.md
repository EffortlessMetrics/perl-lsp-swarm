---
tags: [review-noise, review-integrity, churn-loop]
repos: [perl-lsp-swarm]
related: ["#3768"]
portable: true
article_asset: true
search_terms: [bot threads, advisory noise, 12/12 bogus, placeholder test body, wrong citation, issue-vs-PR conflation, re-fire churn, resolve-only pattern, evidence reply]
---

# Auto-review bots post advisory noise; resolve-only breaks churn loops

**Date**: 2026-07
**Hazard class**: review-noise / review-process anti-pattern
**Portable lesson**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)

## What happened

PR #3768 (changelog) accumulated 12 review threads from automated bots, and 12/12 were advisory noise: 8 threads contained literal placeholder text ("test", "todo", "fix this"), and 4 threads cited factually-wrong claims conflating issue numbers with PR numbers. Each push re-triggered the entire bot fleet, re-firing the same 12 threads (churn loop).

The reviewer's options: (a) push a commit to silence the bots (heavy), or (b) resolve the threads without a reply (dismissive but effective). Option (b) used the resolve-only pattern — marking threads complete without engaging — to break the churn loop and clear the unresolved-thread blocker.

## Why

Automated bots are advisory by nature; they post pre-canned checks that may be inapplicable to a specific PR. In high-velocity PRs, bot feedback is often noise (template bodies, generic citations, copy-paste errors). Engaging each bot thread with a reply-and-resolve is expensive; ignoring them risks missing genuine bot feedback. The resolve-only pattern (resolve without replying) clears the thread from the reviewer's checklist without authoring a reply, breaking the re-fire churn.

However, resolve-only used carelessly can mask genuine review feedback (the #3647 incident: resolved threads without disposition). The key distinction: bot threads are *advisory* (no review gating), while agent/human review threads are *binding* (gated by unresolved count).

## Fix

Governance distinction:

1. **Bot threads**: advisory, no merge blocker. Resolve-only is acceptable (breaks churn, clears checklist).

2. **Agent/human review threads**: binding, require disposition reply. Resolve only after replying (evidence-based or plan-based decision).

The pattern: **resolve-only (evidence reply + resolve, NO new commit) reaches 0-unresolved without re-triggering the bot fleet**, breaking the churn loop on advisory noise.

## Spec impact

- [docs/reference/MAINTAINER_AGENT_DOCTRINE.md](../reference/MAINTAINER_AGENT_DOCTRINE.md): added distinction between bot-thread resolve-only (advisory churn-breaker) and agent-thread resolve-with-disposition (binding review gate).
- [docs/agents/SPEC_UPDATE_CHECKLIST.md](../agents/SPEC_UPDATE_CHECKLIST.md): added guidance on thread classification (bot vs agent) and appropriate resolution strategy.

## Portable lesson

Review bots are advisory aggregators, not correctness gates. Resolving bot threads without reply is noise-suppression, not review abdication. The cost of the resolve-only pattern on advisory threads is low; the cost of the same pattern on binding threads is high (unaddressed feedback).

- **Pattern**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)
- **Class**: Review-noise / signal-filtering pattern
- **Generalization**: In a system where bots and reviewers both post to the same notification channel, distinguish advisory (bot) from binding (review) before deciding how to resolve. Resolve-only is a noise-suppression tactic appropriate only for advisory signals.

## Related PRs

- [#3768](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3768) — changelog, 12 bot threads (all noise), resolved-only to break churn
