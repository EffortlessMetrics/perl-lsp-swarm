If Git reports a real conflict, the later lane resolves it and refreshes only the
affected proof/review. If an explicit stack or combined-tree check exposes a real
interaction, repair that interaction rather than predicting overlap in advance.

## Check attribution

A failing check is evidence about a candidate only if it ran on that candidate and
does not fail without it. Two questions come before recording one as a finding:

1. **Did it run on the live head?** A check anchored to a superseded SHA describes a
   commit that no longer exists in the branch. Cancelled lanes are the common case —
   a newer push supersedes an in-flight run, and the cancellation surfaces as a
   failure. Read the run: lanes that know they were cancelled usually say so.
2. **Does it reproduce on the base?** Run the same command on a clean base checkout.
   A gate already red on `main` is a repository condition — name it, file it, and
   record it as not attributable. It is not a defect in a candidate that never
   touched the failing crate.

A failure that appears on both trees is not automatically the same failure. Before
classifying it as a base condition, compare the command, relevant environment, exit
status, and failure signature at the PR merge base (or the nearest equivalent
candidate/base pair). If the base fails a different test, error, or path, the
candidate still owns its own failure; preserve both findings instead of cancelling
one as a reproduction.

Likewise, an unrelated later commit does not erase a genuine candidate failure.
Carry that failure forward until a later head changes the affected seam, removes the
failure, or supplies discriminating proof that the failure was an attribution
artifact. Review currentness is about the claim and its evidence, not merely whether
the branch received another commit.

Both checks are cheap and both failures are expensive in the same way: they send an
author to repair code that is not broken, and the real defect stays unfiled.

A gate that fails on `main`, blocks nothing, and is labelled flaky at a 100% failure
rate is worse than a missing gate. A missing gate is visibly absent; this one looks
like coverage while carrying no signal, so genuine regressions land behind it
unnoticed. Treat a persistently red non-blocking gate as an open question about
whether it should be required, repaired, or explicitly marked advisory.

## Expected-head merge safety

At the instant of merge, use the current PR head SHA as compare-and-swap protection so
a branch cannot move between inspection and merge:

```text
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

This is merge race protection. It is not review currentness and does not justify
exact-head review comments.

## Landed reconciliation

After squash merge, verify the landed effect on current `main`, update the controlling
issue and durable claims, preserve residual work, and clean the branch/worktree. The
future squash commit was not—and did not need to be—the formal review subject.