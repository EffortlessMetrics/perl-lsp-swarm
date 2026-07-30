---
name: "source-command-refresh-stale-prs"
description: "Deprecated compatibility wrapper for auditing selected stale PR checks without mass-mutating conflict-free candidates"
---

# Refresh stale PRs — compatibility wrapper

This migrated command is retained temporarily for compatibility. Its historical fire-fix cascade procedure is superseded by candidate-and-claim currentness.

**Do not mass-update, rebase, or merge `main` into PR branches merely because checks are red, a branch is behind, or several merges landed.** This repository squash-merges; ancestry distance alone does not stale candidate evidence.

## Procedure

For each PR explicitly selected by the caller:

1. Resolve its exact head, failed check, draft state, mergeability, and current review subject.
2. Classify the observed result:
   - candidate-caused product/test failure;
   - stale or cancelled workflow result for the same head;
   - base/instrument failure already corrected on `main`;
   - actual Git conflict;
   - failed required merge-group/synthetic integration proof;
   - missing or partial evidence (`NOT_PROVEN`).
3. Prefer a same-head check rerun when the candidate is unchanged and only the workflow/instrument result is stale.
4. Leave the branch alone when it is merely behind and still conflict-free.
5. When an actual conflict or integration failure exists, comment on the owning issue/PR and let that lane resolve its own candidate.
6. Any candidate mutation creates a new head and requires affected proof plus fresh candidate-level formal review.
7. Report each PR independently. Do not inspect sibling implementation details, infer overlap from files, use lifecycle labels as evidence, or merge from this compatibility wrapper.

## Results

- `SAME_HEAD_RERUN` — rerun the stale/cancelled instrument without branch mutation.
- `CANDIDATE_REPAIR` — candidate-caused failure needs the normal build/repair flow.
- `LANE_CONFLICT_REPAIR` — the owning lane handles an actual Git or integration failure.
- `CURRENT_EVIDENCE` — no action; existing candidate evidence remains current.
- `NOT_PROVEN` — preserve the exact missing or contradictory evidence.

This wrapper will leave active discovery when the provider-native PR-convergence flow completes its cutover.
