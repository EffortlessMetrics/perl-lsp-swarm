# Historical review-receipt control plane

This document described an earlier Swarm review protocol built around:

- exact-head `review-run:v1` receipts;
- material-claim digests;
- independent verification receipts;
- lifecycle projections derived from those comments.

That protocol is **not the current development method**.

## Current authority

Use:

- [`AGENTS.md`](../../AGENTS.md) or [`CLAUDE.md`](../../CLAUDE.md);
- [`docs/agents/DEVELOPMENT_METHOD.md`](../agents/DEVELOPMENT_METHOD.md);
- [`docs/agents/REVIEW_CURRENTNESS.md`](../agents/REVIEW_CURRENTNESS.md);
- live GitHub reviews, inline threads, required checks, rulesets, conflicts, and mergeability.

Review is cumulative and semantic. A later commit does not invalidate review merely because the SHA changed. Review repairs are checked at the affected finding, proof, and semantic seam. Material claim, production-route, authority, risk, rollback, or compatibility changes receive proportionate review of the affected dimensions.

Do not post `Review pass (...) at head ... and claim ...` comments or require claim-digest receipts in fresh work.

## What remains valid from the earlier work

The incidents that motivated the earlier protocol remain real:

- unresolved review threads must not be silently ignored;
- findings need evidence-backed dispositions;
- a clean review is valid;
- required checks and GitHub protection remain authoritative;
- expected-head compare-and-swap at merge time prevents racing a moving branch.

Expected-head merge safety is not review currentness.

## Retained compatibility surfaces

Legacy scripts, schemas, fixtures, and historical receipts may remain temporarily so old records and self-tests stay readable. They are compatibility/history surfaces, not instructions for fresh Claude or Codex lanes. New work must not extend the retired receipt lifecycle.

Tracking correction: #5778.
