---
name: address-review-comments
description: Verify and disposition every substantive human, bot, and CI finding through the repository's sanctioned GitHub review helper.
user-invocable: false
---

# Address review comments

Read the current PR head, controlling issue, governing contract, proof, submitted
reviews, inline threads, and relevant CI findings.

For each substantive finding choose one supported lowercase class:

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

### Useful subagent work

Use focused subagents, context forks, or an Agent Team only where useful for:

- complete thread/review/finding inventory;
- source and external-authority verification;
- high-output CI log/artifact classification;
- reproduction and production-path tracing;
- proof/oracle challenge;
- detecting a test, ratchet, support claim, or policy weakening disguised as a fix.

Children return finding identity, direct/contradictory evidence, searched scope,
classification, suggested disposition, uncertainty, and affected dimensions. They do
not resolve threads or authorize repair.

### Mutation owner and join

One candidate writer integrates accepted repairs. The lane root joins duplicate or
conflicting findings, verifies dispositions against current evidence, and decides which
findings are repaired, refuted, superseded, followed up, blocked, or not proven.

The join is complete only when every substantive finding has one supported visible
disposition, accepted mutations have current affected proof, and material claim/owner
changes have returned to the proper earlier route.

### Return packet

Return candidate/head identity, complete substantive finding set, classifications and
dispositions, evidence, commits/issues used by the helper, affected proof/review
results, unresolved contradictions, limitations, and typed result.

## Procedure

1. Enumerate the PR's review threads with `scripts/reviews/threads <pr> [owner/repo] [--unresolved-only] [--json]`. This is the sanctioned read-only enumerator and the source of the `<threadId>` that step 5 passes to `disposition --thread`; `scripts/reviews/state` returns an aggregate classification with no per-thread identity and cannot supply it. Do not hand-roll a `reviewThreads` GraphQL query.
2. Verify each finding against current source and authority; do not patch comments literally.
   A finding the currentness contract already answers is refuted, not complied with.
   Base staleness, "behind by N", head-SHA movement, and a demand to rebuild on current
   main for a conflict-free candidate are all answered by
   [`REVIEW_CURRENTNESS.md`](../../../docs/agents/REVIEW_CURRENTNESS.md) —
   this repository squash-merges, so unrelated base movement changes nothing. Reply
   `Disposition: refuted` citing the contract. Rebasing to satisfy such a request costs
   a full re-proof cycle, invites a fresh set of stale-head objections against the new
   head, and can silently absorb concurrent edits made to the branch meanwhile.
   Likewise, a failing check anchored to a superseded SHA, or one that reproduces on
   the base branch, is not a finding about this candidate.
3. Batch accepted repairs through one writer on the selected candidate.
4. Run affected focused proof.
5. Compose the canonical human reply with `Disposition: <class>` and `Evidence: <claim-bounded evidence summary>` lines, then pass that complete text through `--reply` to `scripts/reviews/disposition` with the PR, thread ID, lowercase class, and required class-specific evidence (`--commit`, `--argument`, `--superseded-by`, or `--issue`).
6. Let the helper append the `<!-- disposition:v1 ... -->` marker to that supplied reply, post it, and only then resolve the thread.
7. Re-run the enumerator to confirm no substantive thread remains unresolved.
8. If a reviewer applied a repair, treat the resulting head as a new authored candidate and invalidate affected review dimensions.

Do not use raw thread-resolution APIs, resolve performatively, or use pr-responded or
reviewer-persona labels as evidence.

## GitHub boundary

Localized findings and dispositions belong in their inline threads. Cross-cutting
finding classes, material premise changes, or bounded follow-up decisions may update the
PR/issue synthesis. Preserve the human-readable `Disposition:` / `Evidence:` reply and
helper marker before resolution.

Keep subagent/Team topology, raw logs, duplicate findings, temporary reproduction
output, retries, and routine progress runtime-local. Do not post one subagent summary
per finding or resolve merely to make the thread count green.

## Routes

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `$final-challenge`
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `PROOF_WEAKENED` → `$prepare-proof`
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up and continue within the current claim
- `DISPOSITION_INSTRUMENT_FAILURE` / `BLOCKED` / `NOT_PROVEN` → preserve the unresolved finding or missing evidence
