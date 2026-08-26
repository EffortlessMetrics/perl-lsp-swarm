---
name: ci-failure-triage
description: Diagnose failing CI workflows, builds, lint jobs, typechecks, tests, flaky jobs, pipeline regressions, or failed GitHub Actions using logs, reproduction, and minimal fixes.
---

# CI failure triage

Fix CI from evidence instead of guesses. Establish whose red it is before assigning
repair ownership or mutating anything.

## Required checks

- Capture the failing workflow, job, command, file, and error message.
- Reproduce locally where the repo supports it.
- Distinguish deterministic failure from flaky behavior.
- Avoid broad rewrites unless the root cause demands it.
- Prefer the smallest fix that makes the failing signal pass.
- Do not weaken tests, lint rules, or type checks without explicit justification.
- Record unreproduced failures clearly.

## Merge-tree checks and base movement

Merge-tree-evaluated checks evaluate a GitHub-computed merge snapshot, not the branch
head:

- `gh run rerun` replays that run's ORIGINAL merge snapshot. After material base
  movement (release bumps, fmt/clippy sweeps landing on main) it can never unblock a
  red merge-tree check, because it keeps re-evaluating the stale tree.
- After material base movement, the reliable fresh trigger is an empty-commit head
  bump: `git commit --allow-empty -m "ci: re-request fresh merge-tree checks"` and
  push. This is not status churn: the rerun replays the stale snapshot while the
  fresh trigger re-evaluates the current merge tree (#12174, #12251, #12256, and
  #12258 each unblocked only through the fresh trigger).
- Advisory CI-Gate shard redness on a FRESH merge tree is a main-red signal, not
  noise. Query main's own head check-runs via
  `gh api repos/OWNER/REPO/commits/main/check-runs` before repairing branch-locally:
  #12357 located main's stale `references_pir_shadow.rs` expectation through such
  red, #12311 was repaired on main by #12312, and #12374 is the same class.
- Ownership still requires evidence, not inference from redness. A shard that runs
  once on the candidate's merge tree exercises candidate code too, so attribute by
  comparing the delta against main's head and the merge base before naming the
  repair owner: #12652's meta-shard red was candidate-owned `.index_file()` additions
  initially misread as a main failure.

## Definition of done

- Root cause is stated.
- Fix is tied to the failing signal.
- Relevant command passes or remaining blocker is documented.
