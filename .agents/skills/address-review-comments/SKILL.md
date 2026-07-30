---
name: address-review-comments
description: Explicit atomic skill for verifying every substantive human, bot, and CI finding against the current candidate and dispositioning it through the repository's sanctioned GitHub review helper.
---

# Address review comments

Read the current PR head, controlling issue, governing contract, proof, submitted reviews, inline threads, and relevant CI findings.

For each substantive finding choose one supported class:

```text
fixed
refuted
superseded
follow-up
```

## Procedure

1. Verify the finding against current source and authority; do not patch comments literally.
2. Batch accepted repairs through one integrating writer.
3. Run affected focused proof.
4. Use `scripts/reviews/disposition` as the sanctioned reply-and-resolve path. Supply the PR, thread ID, lowercase class, human reply, and class-specific evidence (`--commit`, `--argument`, `--superseded-by`, or `--issue`).
5. Let the helper emit the canonical `Disposition:` / `Evidence:` reply and `<!-- disposition:v1 ... -->` marker before resolving the thread.
6. If the reviewer applied a repair, treat the resulting head as a new authored candidate and invalidate affected review dimensions.

Do not call raw thread-resolution APIs, resolve performatively, or use `pr-responded` or reviewer-persona labels as evidence.

## Orchestration

Finding verification and CI classification may fan out. One writer integrates accepted repairs and the root joins dispositions.

## Routes

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` → `$final-challenge`
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `PROOF_WEAKENED` → `$prepare-proof`
- `FOLLOW_UP_ACCEPTED` → create/link the bounded follow-up and continue this PR within its claim
- `DISPOSITION_INSTRUMENT_FAILURE` / `BLOCKED` / `NOT_PROVEN` → preserve the unresolved finding or missing evidence
