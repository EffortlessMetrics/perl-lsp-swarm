# Review convergence

## Current contract

Review convergence is determined from useful GitHub state, not an exact-head receipt lifecycle.

A PR is review-converged when:

- no substantive review thread remains unresolved;
- no current `CHANGES_REQUESTED` review remains;
- no deliberately requested reviewer is still pending where that review is part of the claim;
- later repairs have been checked against the findings, proof, and semantic seams they changed.

A review does not become stale merely because another commit changed the PR head SHA. Stale human or bot review timestamps may be reported as context; they do not block by themselves.

## Review-forward repair

After a repair:

- verify the addressed finding and affected proof;
- review any changed claim, production path, authority, risk, rollback, or compatibility dimension;
- do not restart a full review for formatting, editorial cleanup, generated receipt refresh, or stronger tests unless the conclusion changed;
- review actual conflict or integration repairs at the affected seam.

A clean review is valid. Publish useful findings or a concise clean conclusion through GitHub reviews and threads.

## Retired protocol

The following are retired from fresh Claude work:

- `scripts/reviews/run review-start` / `review-done` comments;
- `Review pass (...) at head ... and claim ...` comments;
- material-claim digests as review gates;
- full re-review solely because the head SHA changed;
- `REVIEW_PROTOCOL_ENFORCE=1` as a default PR-completion step.

Legacy scripts, schemas, fixtures, and historical comments may remain readable for compatibility and incident history. They are not current instructions.

## Merge safety

GitHub branch protection, unresolved-conversation rules, required checks, draft state, conflicts, mergeability, and queue/ruleset state remain authoritative.

The current head SHA may be used as compare-and-swap protection at merge time. That prevents racing a moving branch; it is not review currentness.

Current method: [`docs/agents/REVIEW_CURRENTNESS.md`](../../docs/agents/REVIEW_CURRENTNESS.md).
Tracking correction: #5778.
