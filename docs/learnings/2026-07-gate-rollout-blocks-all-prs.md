---
tags: [rollout, control-plane, gate-logic, enforcement]
repos: [perl-lsp-swarm]
related: ["#3732", "#3740", "#3753"]
portable: true
article_asset: true
search_terms: [gate rollout, resolved_without_disposition check, advisory flag, REVIEW_PROTOCOL_ENFORCE, all PRs blocked, dogfood, enforcement, gate shipped wrong]
---

# A correct gate rolled out too strict: blocks all in-flight PRs until skill taught

**Date**: 2026-07
**Hazard class**: rollout / control-plane incident
**Portable lesson**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)

## What happened

PR #3732 (convergence gate) introduced a `resolved_without_disposition` check to enforce the disposition convention — every resolved thread must include a reply. The gate worked correctly but was shipped with `enforce=true` by default. This would have blocked ~all in-flight PRs because no skill had yet taught reviewers/responders how to comply with the disposition convention (i.e., what constitutes a "disposition reply" and how to format it).

Rolling out a new gate in enforce-mode without the enabling guidance creates an outage: the gate is correct code, but the humans and agents using the system are not yet equipped to pass it.

## Why

Gate correctness (the check executes as designed) is independent of gate readiness (the system is prepared to pass it). A gate that enforces a new discipline must be shipped behind a default-off flag, with teaching and dogfood first, then enforcement in a follow-up. Shipping enforcement first creates a breaking change without a migration path.

## Fix

Three changes, in sequence:

1. **#3740**: writes the disposition convention — teaches what a "disposition reply" is, what formats are acceptable (evidence-based, plan-based, dismiss-with-reasoning), and how to post one.

2. **#3732 revised**: re-ships with `resolved_without_disposition` check behind `REVIEW_PROTOCOL_ENFORCE=false` (advisory by default). Gate runs, reports findings, but does not block merges.

3. **#3753**: embeds the disposition convention in the convergence gate (review-protocol R1) with the advisory flag. Follow-up (not yet filed) to flip the flag to `enforce=true` after dogfood.

## Spec impact

- [docs/reference/MAINTAINER_AGENT_DOCTRINE.md](../reference/MAINTAINER_AGENT_DOCTRINE.md): added gate-rollout pattern — teach before enforce, ship behind advisory flag, dogfood, then flip to enforcement.
- [.ci/policies/required-checks.toml](.ci/policies/required-checks.toml): documented the advisory-first philosophy for new checks; new required checks must start `required=false` for one release cycle.

## Portable lesson

When a gate is correct but new, deploy it advisory-first. An enforcing gate that the system is not yet prepared to pass creates cascading blockages. Ship the gate, report findings, teach the discipline, then switch to enforcement.

- **Pattern**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)
- **Class**: Rollout / control-plane incident
- **Generalization**: A gate's correctness and a gate's readiness are independent concerns. A correct gate can cause an outage if shipped before the system is ready to pass it. Use advisory-first + dogfood + enforcement-later for gates that introduce new disciplines.

## Related PRs

- [#3732](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3732) — convergence gate (revised with advisory flag)
- [#3740](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3740) — teaches disposition convention
- [#3753](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3753) — convergence gate R1 protocol (disposition check embedded)
