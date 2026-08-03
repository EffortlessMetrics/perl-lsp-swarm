---
name: address-review-comments
description: Verify and disposition every substantive human, bot, and CI finding through the repository's sanctioned GitHub review helper.
user-invocable: false
---

# Address review comments

Read the current PR head, controlling issue, governing contract, proof, submitted reviews, inline threads, and relevant CI findings.

For each substantive finding choose one supported lowercase class:

```text
fixed
refuted
superseded
follow-up
```

## Procedure

1. Verify the finding against current source and authority; do not patch comments literally.
2. Batch accepted repairs through one writer on the selected candidate.
3. Run affected focused proof.
4. Compose the canonical human reply with `Disposition: <class>` and `Evidence: <claim-bounded evidence summary>` lines, then pass that complete text through `--reply` to `scripts/reviews/disposition` with the PR, thread ID, lowercase class, and required class-specific evidence (`--commit`, `--argument`, `--superseded-by`, or `--issue`).
5. Let the helper append the `<!-- disposition:v1 ... -->` marker to that supplied reply, post it, and only then resolve the thread.
6. If a reviewer applied a repair, treat the resulting head as a new authored candidate and invalidate affected review dimensions.

Do not use raw thread-resolution APIs, resolve performatively, or use pr-responded or reviewer-persona labels as evidence.

Finding verification and CI classification may fan out; the main thread joins dispositions and integrates accepted repairs.

## Routes

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `final-challenge`
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `PROOF_WEAKENED` → `prepare-proof`
- `FOLLOW_UP_ACCEPTED` → create or link the bounded follow-up and continue within the current claim
- `DISPOSITION_INSTRUMENT_FAILURE` / `BLOCKED` / `NOT_PROVEN` → preserve the unresolved finding or missing evidence
