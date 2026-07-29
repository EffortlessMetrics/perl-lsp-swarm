---
description: Deprecated compatibility command for auditing apparently stale PR checks without mass-mutating conflict-free candidates
---

# Refresh stale PRs — compatibility wrapper

This historical command is retained temporarily for compatibility. Its old fire-fix cascade procedure is superseded by candidate-and-claim currentness.

Do not mass-update, rebase, or merge `main` into PR branches merely because checks are red, a branch is behind, or several merges landed. This repository squash-merges; ancestry distance alone does not stale candidate evidence.

## Procedure

1. Enumerate the selected failing PRs and resolve each exact head, failed check, draft state, mergeability, and current review subject.
2. Classify each failure as candidate-caused, stale/cancelled same-head instrumentation, corrected base/instrument failure, actual conflict, material same-semantic-seam interaction, or `NOT_PROVEN`.
3. Prefer a same-head workflow/check rerun when the candidate is unchanged and only the instrument result is stale.
4. Mutate a candidate only for an actual conflict, material same-seam interaction, when current GitHub branch protection, rulesets, merge queue, or required checks require integration evidence, or when the integration result is otherwise uninterpretable.
5. Any candidate mutation requires affected proof and fresh candidate-level formal review.
6. Report each PR independently. Do not use lifecycle labels as evidence and do not merge from this compatibility command.

## Results

- `SAME_HEAD_RERUN` — rerun the stale/cancelled instrument without branch mutation.
- `CANDIDATE_REPAIR` — candidate-caused failure needs the normal build/repair flow.
- `CONFLICT_OR_INTERACTION` — one integrating writer resolves the exact seam, then re-proves/reviews.
- `CURRENT_EVIDENCE` — no action; existing evidence remains current.
- `NOT_PROVEN` — preserve the exact missing or contradictory evidence.

This command leaves active discovery when Claude's provider-native PR-convergence flow completes its cutover.
