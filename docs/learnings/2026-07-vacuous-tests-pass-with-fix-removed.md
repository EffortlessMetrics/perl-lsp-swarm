---
tags: [test-quality, test-validity, verification, green-tdd]
repos: [perl-lsp-swarm]
related: ["#3618", "#3765"]
portable: true
article_asset: true
search_terms: [vacuous test, test passes with fix removed, mutation check, Minimal-snapshot, ast.is_none guard, rollback test, construction-count assertion, never-true condition, test-quality]
---

# Vacuous tests: passing with the fix removed, or guarded by impossible conditions

**Date**: 2026-07
**Hazard class**: test-quality / test-validity
**Portable lesson**: [docs/concepts/verify-the-instrument.md](../concepts/verify-the-instrument.md)

## What happened

PR #3618 (off-lock parse worker) added 3 rollback tests that all passed with the rollback logic deleted — asserted behavior on a field the fix did not touch, providing no proof the fix worked. PR #3765 (generation-owned lazy analyzer) included a "Minimal-snapshot" test guarded by `if ast.is_none()`, a condition the v3 parser's recovery behavior never makes true in the test environment, leaving the core assertion untested.

Both patterns represent vacuous tests: the test runs (passing) while the guard or mutation causes it to assert nothing meaningful. The test count increases, CI appears greener, but coverage of the fix remains zero.

## Why

Test validity requires two independent checks:

1. **Mutation check**: remove or invert the fix, and the test must go RED. If the test passes with the fix removed, the test does not discriminate between "fix present" and "fix absent."

2. **Construction-count / guard-truth**: when a test guesses at its own preconditions (`if ast.is_none()`), verify the guard is reachable under the inputs the test constructs. A guard that is never true provides no proof of the guarded behavior.

Neither check is automatic. A test that compiles and runs will report green regardless. Vacuous tests are the most insidious test failure: they increase test count and pass CI while providing zero proof.

## Fix

Two targeted changes:

1. **#3618 rollback tests**: rewrote to directly assert the field the rollback modifies. Inverted the fix (removed the rollback) and verified all three tests went RED.

2. **#3765 Minimal-snapshot test**: removed the `if ast.is_none()` guard (unreachable under v3 recovery) and added a direct construction ensuring the snapshot was built under the new analyzer path.

Both fixes are examples of **adversarial test design**: the test must fail in the absence of the fix, and must exercise the specific code path the fix modifies.

## Spec impact

- [docs/agents/SPEC_UPDATE_CHECKLIST.md](../agents/SPEC_UPDATE_CHECKLIST.md): added acceptance criterion under "Test validity" — every test MUST include a mutation check (remove fix → test goes RED) and guards must be verified reachable under the test's inputs.
- [docs/concepts/verify-the-instrument.md](../concepts/verify-the-instrument.md): extended with the "vacuous test" pattern and cheap counter-move: direct-accessor assertions instead of indirect caller chains.

## Portable lesson

A test passing is an instrument reading. The instrument failure is a guard that never fires or an assertion on a field unrelated to the fix. Verify the test against two ground truths:

1. Remove the fix, run the test, confirm it goes RED (mutation check).
2. Confirm the guard/assertion path is actually exercised by the test inputs (construction-count assertion or direct accessor call).

- **Pattern**: [docs/concepts/verify-the-instrument.md](../concepts/verify-the-instrument.md)
- **Class**: Test-validity / test-quality instrument failure
- **Generalization**: A passing test is evidence that the test compiles and runs, not evidence that the fix works. Verify both directions: (a) test goes RED when fix is removed (mutation check); (b) test's guarded path is reachable under the test's inputs (guard-truth verification).

## Related PRs

- [#3618](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3618) — off-lock parse worker, vacuous rollback tests
- [#3765](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3765) — generation-owned lazy analyzer, unreachable Minimal-snapshot guard
