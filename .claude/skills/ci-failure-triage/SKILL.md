---
name: ci-failure-triage
description: Diagnose failing CI workflows, builds, lint jobs, typechecks, tests, flaky jobs, pipeline regressions, or failed GitHub Actions using logs, reproduction, and minimal fixes.
user-invocable: false
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
  bump. Confirm the index is clean first (`git status --porcelain` empty:
  `--allow-empty` publishes whatever is staged under the trigger message), commit
  with a message such as `"ci: re-request fresh merge-tree checks"`, verify the new
  commit carries exactly nothing (`git diff --quiet HEAD^ HEAD` succeeds), and push.
  This is not status churn: the rerun replays the stale snapshot while the fresh
  trigger re-evaluates the current merge tree (#12174, #12251, #12256, and #12258
  each unblocked only through the fresh trigger).
- Advisory CI-Gate shard redness on a FRESH merge tree is a main-red signal, not
  noise. Enumerate ALL of main's own head check-runs before repairing
  branch-locally, using pagination so a red shard cannot hide past page one — mirror
  the production classifier's query shape:
  `gh api --paginate --slurp "repos/OWNER/REPO/commits/main/check-runs?per_page=100"`
  (see `.github/workflows/em-ci-routed-rust.yml`). Receipts: #12357 located main's
  stale `references_pir_shadow.rs` expectation through such red, #12311 was repaired
  on main by #12312, and #12374 is the same class.
- Ownership still requires evidence, not inference from redness. A shard that runs
  once on the candidate's merge tree exercises candidate code too, so attribute by
  comparing the delta against main's head and the merge base before naming the
  repair owner: #12652's meta-shard red was candidate-owned `.index_file()` additions
  initially misread as a main failure.

## Definition of done

- Root cause is stated.
- Fix is tied to the failing signal.
- Relevant command passes or remaining blocker is documented.
