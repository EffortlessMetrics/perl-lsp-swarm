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

1. Verify the finding against current source and authority; do not patch comments literally.
2. Batch accepted repairs through one integrating writer.
3. Run affected focused proof.
4. Compose the canonical human reply with `Disposition: <class>` and `Evidence: <claim-bounded evidence summary>` lines, then pass that complete text through `--reply` to `scripts/reviews/disposition` with the PR, thread ID, lowercase class, and class-specific evidence (`--commit`, `--argument`, `--superseded-by`, or `--issue`).
5. Let the helper append the `<!-- disposition:v1 ... -->` marker to that supplied reply, post it, and only then resolve the thread.
6. If the reviewer applied a repair, treat the resulting head as a new authored candidate and invalidate affected review dimensions.

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
