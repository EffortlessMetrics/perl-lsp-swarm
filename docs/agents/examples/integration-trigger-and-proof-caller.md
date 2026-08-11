# Integration trigger and bounded proof caller

This example is a real, narrow control-plane lane from PRs [#5713](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5713) and [#5717](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5717). It is useful because the two PRs separate decision authority from execution, and the recorded proof boundary includes an unresolved instrument result.

## User-selected outcome

Determine whether one candidate needed combined-tree proof, then—only when the trigger authority selected it—run the selected proof commands in a synthetic squash worktree.

This was not a request to build a scheduler, poll GitHub, authorize a merge, or create a second affected-test database.

## Durable starting artifacts

- Issue [#4588](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4588) owned the trigger decision.
- Issue [#4589](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4589) owned the bounded caller.
- PR #5713 merged the `integration-trigger.v1` evaluator.
- PR #5717 merged the caller and typed command-evidence boundary.

## Entry and first decision

The trigger evaluator was deliberately pure. It accepted candidate identities,
source interaction evidence, and an existing proof-pack selection. It did not
collect GitHub state or invent the command matrix.

The important outcomes were explicit:

- irrelevant conditions such as behind-only, age-only, commit-distance-only, or
  same-file-independent changes did not trigger combined proof;
- missing authority evidence returned `NOT_PROVEN`;
- a moved candidate head returned `RETURN_TO_REVIEW`;
- a real interaction with no existing bounded proof selection remained
  `NOT_PROVEN`;
- a selected textual conflict remained `BLOCKED`.

That reduced the decision to a testable evidence function before any subprocess
or synthetic-tree work existed.

## Bounded execution slice

PR #5717 consumed the trigger result rather than re-deciding it. For a required
trigger, it constructed one synthetic squash tree and ran only the commands in
the supplied proof set. The command runner bound relative working directories to that synthetic tree and rejected absolute or parent-component escapes. This was lexical containment only: the lane did not prove that a selected directory containing a symlink could not resolve outside the synthetic tree, so symlink-based containment remains `NOT_PROVEN`.

The caller preserved upstream results:

- `NOT_REQUIRED` stayed outside integration proof;
- `RETURN_TO_REVIEW` routed back before execution;
- `BLOCKED` stayed blocked;
- incomplete selection or identity remained `NOT_PROVEN`.

## Evidence that changed the result

The later repair pass found a concrete safety gap: a proof command could appear
to run in the synthetic tree while using an unsafe working-directory shape. The
repair added rejection for absolute paths and parent traversal and added focused
regression tests.

The PR body records the key instrument boundary honestly: the focused Cargo
rerun on the repaired head timed out after 305 seconds without output and was
`NOT_PROVEN`; hosted proof was required before merge. The earlier repair head's
format, focused tests, and diff hygiene had passed, but those passes do not prove
the repaired head.

The PR subsequently merged, which is a durable repository event. It is not
itself a substitute for the missing focused-run receipt, and this example does
not claim that the hosted checks proved every execution path.

## What the lane established

- The trigger decision has a pure, typed contract with explicit negative cases.
- The caller consumes that decision and does not become a second authority.
- Synthetic-proof commands reject absolute paths and parent traversal; canonical symlink containment remains an explicit unproven boundary.
- Instrument failure is represented as `NOT_PROVEN` rather than silently
  converted into product pass or fail.

## What it did not establish

- It did not prove that all repository integration cases are detected.
- It did not prove that every selected command passes on every platform.
- It did not authorize merges or replace branch/review policy.
- It did not establish release readiness or a universal workflow engine.

## Retained operating lesson

Split the decision from the expensive action, preserve the source identities,
and make missing or stale evidence route backward. A merged PR can establish a
repository change while a specific verification result remains `NOT_PROVEN`.
That distinction is part of the artifact, not cleanup prose to be removed later.
