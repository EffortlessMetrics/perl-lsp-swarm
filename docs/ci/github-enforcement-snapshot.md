# GitHub enforcement snapshot contract

`github_enforcement_snapshot.v1` is the read-only input contract for reconciling checked-in status-context policy with GitHub's complete live enforcement union:

```text
classic branch protection
+ every active branch ruleset targeting the default branch
= observed live enforcement union
```

The model is deliberately offline. It accepts captured observations, consumes the existing `Gate Enforcement Contract` receipt, and emits `MATCH`, `DRIFT`, or `NOT_PROVEN`. It performs no GitHub API call and cannot change branch protection, rulesets, bypass actors, or checked-in policy.

## Input identity

A snapshot binds:

- repository full name and numeric repository ID;
- default branch, exact observed branch SHA, and observation time;
- capture source, permission completeness, and explicit limitations;
- the exact static-contract subject, policy digest, and repository SHA;
- classic-protection instrument state, branch, contexts, and app IDs where available;
- ruleset instrument state and every captured branch ruleset's ID, name, enforcement state, default-branch applicability, bypass actors, contexts, and app IDs.

Inputs are closed and versioned. Unknown fields, malformed IDs, duplicate rulesets, ambiguous timestamps, unsupported states, or cross-subject identity produce `NOT_PROVEN` rather than an empty union.

## Complete verdicts

`MATCH` and `DRIFT` require both enforcement surfaces to be observed with complete permission and no capture limitation. Classic-only or ruleset-only observations are incomplete even when the visible surface is empty.

Only rulesets with `enforcement = active` and `targets_default_branch = true` contribute to the union. Inactive, evaluate-only, disabled, or untargeted rulesets remain in `excluded_rulesets` so their exclusion is reviewable.

A context present in both systems is retained with `source_class = both`; it is not treated as a duplicate error. The reconciler reports exact differences for:

- checked-in required context missing from live enforcement;
- live required context absent from checked-in policy;
- classic/ruleset source mismatch;
- context app-identity mismatch when checked-in policy declares one;
- unsupported required enforcement claims.

`DRIFT` is a proved mismatch. `NOT_PROVEN` is incomplete or stale evidence. Neither applies a correction.

## Commands

```bash
python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  validate snapshot.json

python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  reconcile \
  --snapshot snapshot.json \
  --static-receipt target/receipts/gate-enforcement-contract.json \
  --receipt target/receipts/github-enforcement-union.json

python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  explain target/receipts/github-enforcement-union.json
```

The semantic snapshot digest is deterministic: input ordering of checks, rulesets, and bypass actors does not change the normalized receipt.

## Authority boundary

The static receipt remains the authority for checked-in roles, producer identity, workflow posture, and policy/workflow digests. This model owns only offline normalization and additive-union reconciliation.

Issue #9154 owns the trusted observer that captures both live surfaces and feeds this model. Promotion and correction remain separate human-authorized transactions under #3048. A source PR, a green workflow, or one accessible API never establishes live enforcement by itself.
