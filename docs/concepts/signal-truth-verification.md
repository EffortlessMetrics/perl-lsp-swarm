# Signal-Truth Verification

## The thesis

Every reporting layer can diverge from ground truth. A resolved-thread count, a CI "green" badge, a label's presence, a recon line number, a bot's advisory flag, an agent's "done" claim, a held-PR state — each is a *signal* (what the system reports) distinct from *truth* (the actual current state of record). When a signal looks suspicious or a decision hangs on it, verify the signal against the underlying truth before acting.

This is not paranoia. It is a recognition that in a multi-lane high-velocity repo, or in a system where multiple reporting layers feed into decisions, signals age faster than the decisions that depend on them. A label set at time T reflects PR state at T; at time T+1hr in a multi-lane merge, the PR state may have changed. A resolved-thread count of "0" means threads are marked resolved; it does not mean review feedback was addressed. A CI badge shows the last completed run; it does not guarantee the run was on the current HEAD SHA.

---

## Incidents where signal diverged from truth

### Resolved threads without disposition (#3647, #3637, #3659)

The signal: unresolved-thread count = 0 (merge blocker clears).
The truth: 15 review threads marked "resolved" with no reply posted.
The gap: GitHub's UI treats "resolved" as a completion signal, but resolution is a UI action decoupled from reply-post. A thread can be resolved without disposition being documented.

**Lesson**: a zero unresolved-thread count does not mean all feedback was addressed. Verify the signal (thread count) against the underlying fact (reply posts) before assuming review is complete.

### Vacuous tests (#3618, #3765)

The signal: 3 new tests added, all pass.
The truth: tests pass with the fix removed, or guards never fire under test inputs.
The gap: test-passing is a signal that the test compiles and runs; it does not guarantee the fix is tested. Verify by mutation (remove fix, confirm test goes RED) and guard-truth (confirm guard is reachable).

**Lesson**: a passing test is an instrument reading. Verify both directions: test must fail when fix is absent, and test's guarded path must be exercised.

### CI "green" on stale SHA (#3701, #3732)

The signal: `gh pr checks` reports all required checks pass.
The truth: checks never ran on current HEAD SHA; only older commit has results.
The gap: PR-summary API aggregates check results but can report stale data when pushes are in flight. The signal is evidence-quality until verified against the underlying check-run object.

**Lesson**: match the CI result's SHA to current HEAD before trusting the "green" signal. Read the check-run object; do not rely on the PR-summary aggregation.

### Bot threads as advisory noise (#3768)

The signal: 12 review threads from bots.
The truth: 12/12 are placeholder or factually wrong.
The gap: automated bots post advisory checks; their threads are not binding review feedback. A review system that treats all threads equally conflates advisory (bot) with binding (review). Resolve-only is appropriate for advisory; it masks genuine review feedback if applied to binding threads.

**Lesson**: distinguish advisory signals (bot) from binding signals (review) before deciding how to respond. Resolve-only breaks churn loops on advisory noise; it must never be used on binding review threads.

### Gate shipped before system ready (#3732, #3740, #3753)

The signal: new `resolved_without_disposition` check deployed.
The truth: no skill taught agents/humans how to comply; check would block all PRs.
The gap: gate correctness (the check executes as designed) is independent of gate readiness (the system is prepared to pass it). Shipping enforcement before teaching creates a breaking change.

**Lesson**: advisory-first + dogfood + enforcement-later. A correct gate can cause an outage if shipped before the system is ready to pass it.

### Held-PR label stale in multi-lane (#3627, #3650, #3659, #3637)

The signal: label `needs-deep-review` set at time T.
The truth: PR merged by another lane at time T+30min; label is stale.
The gap: labels are routing signals within a lane, not cross-lane merge-blocks. In a multi-lane high-velocity repo, a label's presence does not guarantee the PR's state is current.

**Lesson**: before acting on a PR's label state, verify the PR is still open and on the current base branch. A label is stale evidence; verify ground truth before routing.

### Recon citations from stale checkout (phase5-recon)

The signal: recon provides file:line references.
The truth: references are valid relative to the recon's 568-commit-old checkout, not current origin/main.
The gap: a recon pass extracts coordinates relative to its source. When the builder works from a different checkout, the coordinates require re-verification. Line numbers age quickly.

**Lesson**: recon citations are signposts, not ground-truth coordinates. Before using them, grep the symbol fresh on origin/main to confirm the line number is current.

---

## Recognition heuristic

> When a decision hinges on a signal, verify the signal against ground truth. When a signal and expected ground truth diverge, suspect the signal first.

Concretely:

- **Unresolved-thread count = 0** → verify reply posts exist on resolved threads
- **CI "green"** → match the check-run's SHA to current HEAD
- **Label present** → verify the underlying state (PR open, base branch current)
- **Test passes** → remove the fix and confirm test goes RED
- **Bot advisory** → resolve-only to break churn; do not use on binding threads
- **Recon line number** → grep the symbol fresh on origin/main

---

## Cheap counter-moves

These checks cost seconds and catch the most common signal-truth divergences:

**Verify the ground-truth system of record.**

- Thread disposition: read the reply post on the resolved thread, not just the "resolved" UI flag.
- CI result: match the check-run's SHA to current HEAD; read the check-run object, not the PR-summary.
- Label state: `gh pr view --json state,baseRefName` before acting on a label.
- Test validity: remove the fix and run the test; confirm it goes RED.
- Bot threads: resolve-only if advisory (no binding gate); reply + dispose if binding (review feedback).
- Recon coordinates: `git show origin/main:path/to/file | grep -n symbol` before building at a cited line.

**Distinguish signals by type.**

- Advisory signal (bot thread, recon signpost, label from another lane) — verify before routing, re-verify before building.
- Binding signal (review thread, required check, core state like "PR open") — verify and do not trust stale readings.

**Read the primary artifact, not the summary.**

- Do not trust aggregated summaries (PR-summary check status, thread-count UI). Read the primary artifacts (check-run objects, reply posts, PR state API).

---

## Position in the pipeline

This pattern is a tactic under the broader posture of adversarial-by-default verification — treat every artifact as evidence with a reliability profile. "Signal-truth verification" is the application of that posture to reporting layers and aggregated summaries specifically.

It is also a tactic under the slow stochastic compiler model: a pipeline of translation stages (scouts → planners → builders → reviewers → CI → operators) where each stage translates and reports to the next. Each translation can diverge from the underlying fact. The operator's job includes distinguishing "the signal is wrong" from "the underlying state is wrong." These require different interventions: a signal-truth divergence requires verification and re-reading the primary artifact, not a new build pass.

Relation to other patterns:

- **Verify the instrument** ([verify-the-instrument.md](verify-the-instrument.md)) — a parent pattern. Signal-truth verification is the application of verification to reporting layers specifically.
- **Human corrects substrate** ([human-corrects-substrate.md](human-corrects-substrate.md)) — when a signal-truth divergence is systemic (e.g., CI badge is always stale, resolved-thread checks never verify disposition), correcting it is a substrate fix, not a one-time merge exception.
- **Triage as claim audit** ([triage-as-claim-audit.md](triage-as-claim-audit.md)) — when triaging an incident ("why did we merge a broken PR?"), trace back through the signals that were trusted at merge time and verify each one against ground truth. The root cause is often a signal that diverged from truth at a critical decision point.

---

## Summary

| Signal | Divergence risk | Ground-truth check |
|--------|-----------------|-------------------|
| Unresolved-thread count | Resolved without reply | Read reply posts, not UI flag |
| CI "green" badge | Stale SHA or wrong scope | Match SHA to HEAD; read check-run |
| Label presence | Stale after merge or drift | `gh pr view --json state` |
| Test pass | Vacuous or wrong-path guard | Remove fix, confirm test goes RED |
| Bot thread count | Advisory noise as binding | Resolve-only for advisory, not binding |
| Recon line number | Stale checkout | Grep symbol fresh on origin/main |
| Held-PR state | Merged by another lane | Verify PR is still open before acting |

When a signal and expected ground truth diverge, verify the signal against the primary artifact before acting. The signal is evidence-quality until verified. The check costs seconds. The divergence caught is expensive.

---

## Decision rules

1. **Before merge**: verify CI result's SHA matches HEAD; read the check-run, do not trust PR-summary.
2. **Before deep-review routing**: verify PR is open and on current base branch; a label is not a cross-lane merge-block.
3. **Before accepting test coverage**: remove the fix and run the test; confirm it goes RED.
4. **Before accepting "review complete"**: read reply posts on resolved threads; zero unresolved-thread count is not sufficient.
5. **Before using recon citations**: grep the symbol fresh on origin/main; recon is evidence relative to its checkout, not current state.
6. **Before shipping a new gate**: ship advisory-first, dogfood, then flip to enforcement. Gate correctness ≠ gate readiness.

---

## Relation to other patterns

- **Verify the instrument** — parent pattern; signal-truth is one instance
- **External truth gate** — for user-visible semantic claims, verify against external oracle (perldoc, spec)
- **Human corrects substrate** — systemic signal-truth divergences are substrate bugs, not one-time exceptions
- **Triage as claim audit** — when triaging an incident, verify each signal that was trusted at the critical decision point

