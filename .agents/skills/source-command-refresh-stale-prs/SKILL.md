---
name: "source-command-refresh-stale-prs"
description: "Deprecated compatibility wrapper for auditing apparently stale PR checks without mass-mutating conflict-free candidates"
---

# Refresh stale PRs — compatibility wrapper

This migrated command is retained temporarily for compatibility. Its historical fire-fix cascade procedure is superseded by candidate-and-claim currentness.

**Do not mass-update, rebase, or merge `main` into PR branches merely because checks are red, a branch is behind, or several merges landed.** This repository squash-merges; ancestry distance alone does not stale candidate evidence.

## Procedure

1. Enumerate the selected failing PRs and resolve each exact head, failed check, draft state, mergeability, and current review subject.
2. Classify the failure:
   - candidate-caused product/test failure;
   - stale or cancelled workflow result for the same head;
   - base/instrument failure already corrected on `main`;
   - actual merge conflict;
   - material same-semantic-seam interaction;
   - missing/partial evidence (`NOT_PROVEN`).
3. Prefer a same-head check rerun when the candidate is unchanged and only the workflow/instrument result is stale.
4. Mutate the candidate only when:
   - an actual conflict must be resolved;
   - a material same-semantic-seam change changes the integration result;
   - current branch protection, rulesets, or merge queue requires integration evidence; or
   - the integration result cannot otherwise be interpreted reliably.
5. Any candidate mutation creates a new head and requires affected proof plus fresh candidate-level formal review.
6. Report each PR independently. Do not use lifecycle labels as evidence and do not merge from this compatibility wrapper.

## Results

- `SAME_HEAD_RERUN` — rerun the stale/cancelled instrument without branch mutation.
- `CANDIDATE_REPAIR` — candidate-caused failure needs the normal build/repair flow.
- `CONFLICT_OR_INTERACTION` — one integrating writer resolves the exact seam, then re-proves/reviews.
- `CURRENT_EVIDENCE` — no action; existing candidate evidence remains current.
- `NOT_PROVEN` — preserve the exact missing or contradictory evidence.

This wrapper will leave active discovery when the provider-native PR-convergence flow completes its cutover.
