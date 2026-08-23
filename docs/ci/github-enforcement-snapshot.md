# GitHub enforcement snapshot contract

`github_enforcement_snapshot.v1` is the offline input contract for reconciling checked-in status-context policy with GitHub's complete enforcement union for the default branch:

```text
classic branch protection
+ every active branch ruleset proven to target the default branch
= observed live enforcement union
```

The model performs no GitHub API call and has no settings-mutation path. A later trusted observer captures API responses, hashes the exact evidence, and feeds this contract. The reconciler owns normalization, target applicability, union construction, and `MATCH` / `DRIFT` / `NOT_PROVEN`; the observer does not reinterpret those semantics.

## Input identity

A snapshot binds:

- repository full name and numeric repository ID;
- default branch, exact observed branch SHA, and normalized UTC observation time;
- capture source, permission completeness, and explicit limitations;
- the exact static-contract subject, exact-source attestation digest, policy digest, and repository SHA;
- classic-protection instrument state, response digest, branch, strictness, contexts, and app IDs;
- ruleset-list response digest;
- every captured branch ruleset's ID, name, source, enforcement state, detail-response digest, ref-name conditions, bypass actors, required-check settings, contexts, and app IDs.

The accepted observation sources are:

```text
trusted_default_branch
operator
connector
fixture
```

`fixture` exists for deterministic falsifiers but cannot produce a live `MATCH`; it returns `NOT_PROVEN` with `non_live_observation_source`. The other sources still require complete permission, both observed surfaces, exact subject identity, and no capture limitation.

Observed surfaces require response digests. A missing, unreadable, or errored surface must not carry stale status-check or ruleset rows. Contradictory state and payload are invalid input, not partial truth.

## Ruleset targeting is P2 authority

The snapshot retains the ruleset's `conditions.ref_name.include` and `exclude` selectors. The observer does **not** supply a `targets_default_branch` boolean.

P2 derives one targeting state against `refs/heads/<default_branch>`:

```text
TARGETED
NOT_TARGETED
NOT_PROVEN
```

The current closed evaluator proves GitHub's special selectors `~DEFAULT_BRANCH` and `~ALL`, plus exact branch refs. An active ruleset containing a wildcard selector that this bounded evaluator cannot prove is `NOT_PROVEN`; it is not silently excluded or counted. This keeps P3 from acquiring a second target-matching implementation.

Only rulesets with:

```text
enforcement = active
targeting.status = TARGETED
```

contribute status checks to the union. Evaluate/disabled rulesets and proven untargeted rulesets remain in `excluded_rulesets` with the exclusion reason. Ambiguous active targeting is a receipt limitation.

## Complete verdicts

`MATCH` and `DRIFT` require:

- classic branch protection observed;
- rulesets observed;
- complete observation permission;
- a live-capable observation source;
- no capture limitation;
- exact static subject, policy digest, repository SHA, and branch SHA;
- proven applicability for every active ruleset that could affect the default branch.

`DRIFT` is a proved mismatch. `NOT_PROVEN` is incomplete, stale, contradictory, fixture-only, or semantically unresolved evidence. Neither applies a correction.

The reconciler reports exact differences for:

- checked-in required context absent from live enforcement;
- live-required context absent from checked-in policy;
- a checked-in advisory/informational context that is live-required;
- classic/ruleset source mismatch;
- app-identity mismatch for each declared enforcement source;
- unsupported checked-in enforcement claims.

Producer identity supplies no enforcement binding. The static contract may declare `classic_app_id` and `ruleset_integration_id` independently; an absent binding remains unconstrained rather than being synthesized from a repository-owned job.

## Receipt evidence

The result retains more than the flat context set:

- classic, ruleset-list, and per-ruleset response digests;
- the normalized ruleset inventory, including target conditions and derived targeting state;
- bypass actors and required-status-check settings;
- each live context's source bindings, app IDs, and contributing ruleset IDs;
- excluded rulesets and their reason;
- exact limitations or differences.

A context present in both systems has `source_class = both`; it is not a duplicate error. App IDs remain associated with each source rather than being compared only against a union-wide set.

## Commands

```bash
python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  validate snapshot.json

python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  reconcile \
  --snapshot snapshot.json \
  --static-receipt target/receipts/gate-enforcement-contract.json \
  --authority reconciliation-authority.json \
  --receipt target/receipts/github-enforcement-union.json

python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  explain target/receipts/github-enforcement-union.json
```

Malformed JSON supplied to `reconcile --receipt` still writes a typed `NOT_PROVEN` receipt.

The semantic snapshot digest is deterministic. Ordering of checks, rulesets, selectors, bypass actors, and equivalent UTC timestamp representations does not change the normalized subject.

## Authority boundary

The Gate Enforcement Contract remains authority for checked-in roles, producer identity, workflow posture, event reachability, and policy/workflow digests. This model consumes that receipt; it does not parse workflows again.

Issue #9154 owns the least-privileged capture path and freshness policy. It can now provide classic and ruleset response digests, raw ref-name conditions, and a `trusted_default_branch`, `operator`, or `connector` source without redefining target or union semantics.

Promotion and correction remain separate human-authorized transactions under #3048. A source PR, a green workflow, one accessible API surface, or a fixture cannot establish live enforcement by itself.

## Reconciliation authority

`reconcile` requires a third, closed offline input that independently states the expected repository full name, numeric repository ID, default branch, evaluation time, maximum observation age, and future-clock-skew allowance. Snapshot self-report cannot authenticate itself. Missing authority, repository mismatch, stale observation, or implausibly future observation yields `NOT_PROVEN`.

## Source-specific bindings

The static contract supplies independent optional `classic_app_id` and `ruleset_integration_id` values. Producer identity supplies neither. Classic observations remain `{context, app_id}`; ruleset observations remain paired `{ruleset_id, context, integration_id}`. When a binding is declared, every contributing observation for that source must match it. When it is absent, the observed value remains receipt-visible but creates no inferred binding verdict.
