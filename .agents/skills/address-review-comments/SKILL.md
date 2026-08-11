---
name: address-review-comments
description: Explicit atomic skill for verifying every substantive human, bot, and CI finding against the current candidate and dispositioning it through the repository's sanctioned GitHub review helper.
---

# Address review comments

Read the current PR head, controlling issue, governing contract, proof, submitted
reviews, inline threads, and relevant CI findings.

For each substantive finding choose one supported class:

```text
fixed
refuted
superseded
follow-up
```

## Orchestration affordances

### Lane-root decisions

The lane root retains whether a finding is valid, stale, refuted, superseded, or a
bounded follow-up; whether it changes claim/owner/proof/risk; the accepted repair; and
which proof/review dimensions become stale.

### Delegable read-only work

Use focused workers where useful for:

- complete thread/review/finding inventory;
- source and external-authority verification;
- high-output CI log/artifact classification;
- reproduction and production-path tracing;
- proof/oracle challenge;
- checking whether an apparent fix weakens a test, ratchet, support claim, or policy.

Workers return finding identity, direct/contradictory evidence, searched scope,
classification, suggested disposition, uncertainty, and affected dimensions. They do
not resolve threads or authorize repair.

### Mutation owner and join

One candidate writer integrates accepted repairs. The lane root joins duplicate or
conflicting findings, verifies dispositions against current source/evidence, and decides
which findings are repaired, refuted, superseded, followed up, blocked, or not proven.

The repair join is complete only when every substantive finding has one supported
visible disposition, accepted mutations have current affected proof, and any material
claim/owner change has returned to the proper earlier route.

### Return packet

Return candidate/head identity, complete substantive finding set, classifications and
dispositions, evidence, commits/issues used by the helper, affected proof/review
results, unresolved contradictions, limitations, and typed result.

## Procedure

1. Enumerate the PR's review threads with `scripts/reviews/threads <pr> [owner/repo] [--unresolved-only] [--json]`. This read-only enumerator is the sanctioned source of the `<threadId>` required by `disposition --thread` in step 5. `scripts/reviews/state` returns an aggregate classification without per-thread identity and cannot supply it; do not hand-roll a `reviewThreads` GraphQL query.
2. Verify each finding against current source and authority; do not patch comments literally.
   Refute, do not comply with, a finding the currentness contract already answers.
   Base staleness, head-SHA movement, and "rebuild on current main" for a conflict-free
   candidate are settled by [`REVIEW_CURRENTNESS.md`](../../../docs/agents/REVIEW_CURRENTNESS.md):
   this repository squash-merges. Reply `Disposition: refuted` citing the contract.
   Rebasing to satisfy such a request costs a full re-proof cycle, draws fresh
   stale-head objections against the new head, and can absorb concurrent branch edits
   unnoticed. A check anchored to a superseded SHA is not a finding about the current head unless
   its failure is revalidated on the unchanged current seam. A genuine failure on an
   older head remains actionable when later work did not change its subject; revalidate
   that seam instead of dismissing it merely because an unrelated push superseded the
   run. A check reproducing on the base is likewise not attributable without matching
   the failure signature against the candidate's merge base.
3. Batch accepted repairs through one integrating writer.
4. Run affected focused proof.
5. Compose the canonical human reply with `Disposition: <class>` and `Evidence: <claim-bounded evidence summary>` lines, then pass that complete text through `--reply` to `scripts/reviews/disposition` with the PR, thread ID, lowercase class, and class-specific evidence (`--commit`, `--argument`, `--superseded-by`, or `--issue`).
6. Let the helper append the `<!-- disposition:v1 ... -->` marker to that supplied reply, post it, and only then resolve the thread.
7. Re-run the enumerator to confirm no substantive thread remains unresolved.
8. If the reviewer applied a repair, treat the resulting head as a new authored candidate and invalidate affected review dimensions.

Do not call raw thread-resolution APIs, resolve performatively, or use pr-responded or
reviewer-persona labels as evidence.

## GitHub boundary

Localized findings and dispositions belong in their inline threads. Cross-cutting
finding classes, material premise changes, or bounded follow-up decisions may update the
PR/issue synthesis. Preserve the human-readable `Disposition:` / `Evidence:` reply and
helper marker before resolution.

Keep worker topology, raw logs, duplicate findings, temporary reproduction output,
retries, and routine progress runtime-local. Do not post one agent summary per finding
or resolve merely to make the thread count green.

## Routes

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `$final-challenge`
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `PROOF_WEAKENED` → `$prepare-proof`
- `FOLLOW_UP_ACCEPTED` → create/link the bounded follow-up and continue this PR within its claim
- `DISPOSITION_INSTRUMENT_FAILURE` / `BLOCKED` / `NOT_PROVEN` → preserve the unresolved finding or missing evidence
