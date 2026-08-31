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

`scripts/ci/observe_github_enforcement.py` (#9154) owns the least-privileged capture path and freshness policy. It provides classic and ruleset response digests, raw ref-name conditions, and a `trusted_default_branch`, `operator`, or `connector` source without redefining target or union semantics. See [Capturing an observation](#capturing-an-observation) below.

Promotion and correction remain separate human-authorized transactions under #3048. A source PR, a green workflow, one accessible API surface, or a fixture cannot establish live enforcement by itself.

## Reconciliation authority

`reconcile` requires a third, closed offline input that independently states the expected repository full name, numeric repository ID, default branch, evaluation time, maximum observation age, and future-clock-skew allowance. Snapshot self-report cannot authenticate itself. Missing authority, repository mismatch, stale observation, or implausibly future observation yields `NOT_PROVEN`.

## Capturing an observation

`scripts/ci/observe_github_enforcement.py` is the bounded observer. It issues GET requests only, retains no credential material, and has no settings-mutation path. It asserts no verdict: it reports which surfaces it read, hashes the exact bytes, and leaves union, targeting, and `MATCH` / `DRIFT` / `NOT_PROVEN` to the reconciler.

```bash
python3 scripts/ci/validate_gate_enforcement_contract.py \
  --root . \
  --receipt target/receipts/gate-enforcement-contract.json

GITHUB_TOKEN=... python3 scripts/ci/observe_github_enforcement.py capture \
  --repository EffortlessMetrics/perl-lsp-swarm \
  --branch main \
  --source operator \
  --static-receipt target/receipts/gate-enforcement-contract.json \
  --snapshot target/receipts/github-enforcement-observation.json \
  --authority target/receipts/github-enforcement-authority.json \
  --authority-repository-id 1244101844 \
  --capture-bundle target/receipts/github-enforcement-capture.json

python3 scripts/ci/reconcile_github_enforcement_snapshot.py \
  reconcile \
  --snapshot target/receipts/github-enforcement-observation.json \
  --static-receipt target/receipts/gate-enforcement-contract.json \
  --authority target/receipts/github-enforcement-authority.json \
  --receipt target/receipts/github-enforcement-union.json
```

The authority is written from what the caller **declared** — `--repository`, `--branch`, and `--authority-repository-id` — never from the observation, because an authority derived from the snapshot would let the snapshot authenticate itself. `--authority` without `--authority-repository-id` is refused. A declaration that disagrees with what was observed is the reconciler's to report, not the observer's to reconcile away.

Requests never follow redirects: the default opener would forward the bearer token to whatever host a `3xx` names, and these are idempotent `GET`s against a fixed API root. A redirect is surfaced as its own status and read as an unreadable surface.

Branch and repository names are percent-encoded per path segment, so a branch containing characters reserved in a URL cannot address a different endpoint.

`capture` exits `0` on a complete observation, `2` when a surface could not be read to a definitive answer, and `1` when no bindable snapshot exists at all. A `--capture-bundle` records the exact response bytes so `assemble` can re-derive the identical snapshot offline — that is the `connector` shape, and it is what makes the capture reviewable after the fact.

### Execution shapes

The observer supports the least-privileged shape that can actually read both surfaces:

```text
trusted_default_branch  repository-owned default-branch job
operator                a maintainer running the command directly
connector               a capture bundle imported as a typed observation
```

The two surfaces do not cost the same access. Observed against this repository with an ordinary repository-scoped token:

```text
GET /repos/{owner}/{repo}/rulesets                        readable
GET /repos/{owner}/{repo}/rulesets/{id}                   readable
GET /repos/{owner}/{repo}/branches/{branch}/protection     403 — administration read
```

A complete union needs both, so the ruleset surface alone cannot carry a verdict. No repository workflow is wired to this observer, because widening a candidate PR's permissions to obtain administration read is exactly the supply-chain hazard the policy train exists to avoid. **The hosted result is therefore `NOT_PROVEN` by construction until an explicitly managed read-only credential with administration read is provisioned.** The operator and connector shapes are usable today and produce the same contract; run them with a credential that carries administration read to reach `MATCH` or `DRIFT`.

### Limitations are a closed vocabulary

Raw host errors, response bodies, and credential material never reach a snapshot. A surface that was not read to a definitive answer emits one of:

```text
classic_branch_protection_forbidden
classic_branch_protection_unreadable
ruleset_list_forbidden
ruleset_list_unreadable
ruleset_detail_forbidden:<ruleset id>
ruleset_detail_unreadable:<ruleset id>
ruleset_detail_unrepresentable:<ruleset id>
ruleset_list_incomplete:<ruleset id>
ruleset_list_truncated
```

The ruleset listing is requested at the maximum page size and is never followed across pages: one bounded request per surface keeps the response digest single-valued. A repository that outgrows one page emits `ruleset_list_truncated`, so an observed subset of rulesets can never present as a complete union.

An unreadable surface never becomes an empty surface. A surface that was not observed carries no rows and no digest, and any limitation downgrades observation permission below `complete`, so a permission failure reaches the reconciler as incomplete evidence rather than as proof that no enforcement exists. `permission` describes access completeness only: a branch with no classic protection at all is a *complete* observation of a `missing` instrument, and what that means for the verdict stays with the reconciler.

## Source-specific bindings

The static contract supplies independent optional `classic_app_id` and `ruleset_integration_id` values. Producer identity supplies neither. Classic observations remain `{context, app_id}`; ruleset observations remain paired `{ruleset_id, context, integration_id}`. When a binding is declared, every contributing observation for that source must match it. When it is absent, the observed value remains receipt-visible but creates no inferred binding verdict.
