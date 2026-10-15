# Perl merge-ready cutover — inactive revised proposal

Status: **NOT_PROVEN for activation or merge enforcement**. Only files in this
proposal directory change. No workflow, validator, policy, hook, protection,
security, credential, deployment, PR, or label is changed or activated.

## Inventory and authority

Source inventory: main `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`, observed
2026-10-15 UTC. `inventory.json` pins all 103 workflow paths and SHA256 hashes,
complete triggers, concurrency, job conditions, dependencies and timeouts;
69 subscribe to pull_request. Tests compare every hash with the pinned Git object.
The source inventory is a snapshot, not a live enforcement assertion.

| Exact required result | Source producer/job |
| --- | --- |
| Perl LSP Rust Small Result | em-ci-routed-rust.yml / rust-small-result |
| ripr+ New Gap Gate | ripr.yml / ripr |
| Compile All Targets (bit-rot guard) | ci.yml / check-all-targets |
| Conflict marker check | ci.yml / conflict-markers |
| validate-title | pr-title-check.yml / validate-title |

The parent supplied a live readback on this revision: active ruleset **16664791**
requires these five names bound to GitHub Actions app **15368**, with strict=false;
queue ruleset **17372329 is disabled**. This is attributed parent evidence, not a
successful local API read. No native queue is needed. `CI Gate (Advisory Aggregate)`
is advisory. Singular [[check]] workflow-shape contracts are distinct from plural
[[checks]] result identities and must not be mistaken for enforcement.

Current CI and routed Rust run on PR opening, synchronize and Ready, main/master
push and merge_group. RIPR additionally schedules cache seeding. ci-nightly and
TOKMD subscribe to labeled alongside opening/push/Ready; sticky label membership
can repeat heavy work. Editor/host proofs and auxiliary path-selected workflows
also need eventual disposition; a ci.yml-only cutover does not meet the broad goal.
The inventory includes their selectors. Security/release workflow changes remain
outside this slice. Small metadata and affected contract checks may stay automatic.

## Corrected contract prerequisite

The initial proposal incorrectly treated dynamic result names as compatible with
current canonical enforcement. Parent live review identified the same rejected
approach in [#9603 comment 5964825594](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9603#issuecomment-5964825594).
Source confirms the blocker in scripts/ci/validate_gate_enforcement_contract.py:
_read_job discards dynamic name text; _producer_index indexes only static names;
entry validation emits **job_name_not_static**, **mapped_producer_not_indexed** and
**duplicate_emitted_context**. This is not a routine trigger-lint exemption.

The revised first deliverable is an owner-reviewed, narrow canonical contract
extension, **before any workflow cutover**. `conditional_contract.py` models a
closed shape named `pr-material-required-name-v1` for only
`.github/workflows/ripr.yml: ripr`. It is an inactive grammar/contract oracle, not
a wrapper around the canonical validator and not a replacement policy source.

A production extension would need to:

1. Version the policy/schema to explicitly declare the emission contract on its
   mapped context row; reject unknown contracts and undeclared dynamic names.
2. Retain raw direct job-name text and accept only the exact finite expression
   shape in conditional_contract.py: one fixed protected name on material PR
   events, one fixed unprotected ignored name otherwise. Arbitrary expressions,
   user-supplied names, inversions, sticky-label predicates and additional dynamic
   producers remain invalid. Do not broadly enable dynamic-name evaluation.
3. Index the two possible names and their disjoint event domains, preserving one
   producer per protected name and rejecting fallback collisions with any required
   context. Replace the three static-index findings only for this fully validated
   shape; retain all other canonical role, source, event, applicability, app-binding,
   path-filter, continue-on-error and producer checks. No suppression list.
   Applicability must become explicit on the declared emission domain: material
   events always execute their required result; outside that domain the required
   name is absent. The current whole-job conditional classification cannot simply
   be relabeled always-or-scoped-noop to pass an unchanged applicability check.
4. Couple the name predicate to the result job's exact `always() && material`
   condition. Material events must execute the protected result; irrelevant events
   may skip only the ignored name. Reject conditional failure suppression and
   required event path filters. Couple concurrency to the same event partition,
   with unique per-run ignored groups and per-workflow/per-PR material groups.
5. Run adversarial canonical fixtures, preserving the current static-name control,
   demonstrating unrecognized dynamic names still fail. Tests here reproduce the
   actual current validator rejection and test the proposed closed-shape recognizer;
   they do **not** claim the canonical extension is already implemented or accepted.

`expressions.txt` contains expanded expressions generated from the modeled contract;
there is no undefined M variable to port. Hosted expression/name/SHA/app association
still needs an authorized canary. If maintainers reject this narrow schema change,
return to the owners rather than exempting the old design. A privileged dispatch
controller adds permissions and different check/source association; it is a larger,
separately reviewed alternative, not an implemented escape hatch.

## Smallest staged native cutover

After the canonical contract is accepted, start with **ripr.yml only**, retaining
its existing command population. Its required aggregate already owns the name.
Change admission/result semantics, not proof scope. Full three-workflow migration
is subsequent owner work; a RIPR-only stage does not claim the whole goal complete.

- Broad PR admission is only the explicit labeled edge where event.label.name is
  merge-ready, attempt 1, PR open, non-draft, supported base ref, and captured
  head/base/label agree with live state. Label membership alone never admits work.
  Opening, synchronize and Ready run cheap invalidation/results, not broad work.
  Retained labels after a push do not rerun qualification; explicit removal/reapply
  requests a new attempt. No automatic retries or native merge queue.
- Subscribe material invalidations (opened, synchronize, reopened, Ready, draft
  conversion, exact merge-ready add/remove, base edits). Title-only edits and
  unrelated labels use only ignored names and unique run groups. The protected
  result executes with always() on material events and fails for missing admission,
  absent/failed/skipped/neutral/cancelled/timeout worker or instrumentation failure.
  An always() result explicitly rejects cancellation of its own run, including a
  rapid label remove/reapply race. Scope no-ops require a successful scope evaluator;
  absence never means success. Preserve names/app bindings and failure propagation.
- Material groups cancel obsolete work. Unrelated label runs cannot cancel running
  qualification or replace its pending slot, even where cancel-in-progress:false
  would otherwise replace pending work. Native event execution order is not
  guaranteed: a delayed old material event can cancel newer qualification. This is
  an availability failure to test and repair, never a reason to accept stale proof.
- Bind repository, PR, event run/attempt, head SHA, base SHA, actual evaluated merge
  SHA/tree and workflow definition SHA. Checkout immutable merge SHA without
  persisting credentials, verify exact base/head parents and record HEAD/tree.
  Candidate SHA and workflow SHA may differ and are recorded separately. Never
  borrow the latest same-named green check or checkout a moving ref after admission.
- Before publishing readiness and immediately before integration, read live PR
  **open state, draft state, base ref, head/base SHA and label**, current enforcement,
  reviews/findings and mergeability. A closed, draft or unsupported-target PR is not
  ready even with unchanged H/B/label. API failures preserve exact errors and yield
  no readiness assertion. Expected-head merge is still needed; reads plus merge are
  not atomic. Candidate-controlled workflow trust is not repaired by this proposal.

## Evidence, runtime budget and current readiness are separate

`model.accepts` validates completed proof for its immutable subject.
`model.readiness` assesses current integration eligibility; it does not erase valid
completed proof. If base advances, preserve unaffected completed evidence and return
BASE_ADVANCED_REASSESS for the changed combined-tree seam. Do not force a broad replay
or label completed evidence false solely because main moved. Strict=false in the
parent readback supports avoiding an invented always-rebase gate; conflicts and
actual combined-tree interactions still require affected proof under repository policy.

Steven's execution ceiling remains **20 minutes**, not permission for longer runs.
Pass each command the remaining shared deadline (including routing, cache restore,
builds and publication reserve), terminate unfinished work at the deadline and
report unfinished proof as NOT_PROVEN. Do not lower the required command population
to make the clock green. Current declared maxima (CI/routed Rust 75m, RIPR 135m) do
not establish compliance; runtime repair and measured canaries are still required.

Record budget outcome separately. Otherwise valid completed evidence observed after
1200 seconds remains valid evidence with **BUDGET_EXCEEDED**. Repair runtime; do not
automatically repeat completed work, relabel it false or extend the execution budget.
Native runner wait may exceed the wall deadline without executing code; pending is
not proof. Fixtures test budget boundaries and remaining execution time; their local
runtime is not a CI performance measurement. Ordinary one-run <=20m remains NOT_PROVEN.

Do not block this narrow RIPR admission/validator slice on main-health redesign.
Retain existing main-health/refusal semantics and exact-source provenance. Do not
substitute synthetic merge evidence for an exact landed squash SHA. A later real
indirect path is `.github/workflows/post-merge-status.yml:576`, which dispatches
ci.yml, em-ci-routed-rust.yml, ripr.yml and pr-title-check.yml after generated status
publication; its CI dispatch supplies base/head inputs. Disposition this path with
its owner during the broader migration so deleting a push trigger does not silently
move broad work to dispatch. This proposal neither changes nor weakens that path.

## Current owners, history and limits

Parent-authoritative current migration coordination: [#17940](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17940)
points to **#10058 / #17900 / #17903**. **PR #16134** separately replaces routed Rust
with Contabo work; coordinate its current writer before executable edits to that
workflow. **#17624** is another overlapping owner surface to reconcile. This slice
makes no executable workflow edits and claims no handoff acknowledgment.

`codex/late-ci-ready-qualification-20261002` at 22ff6470 and withdrawn PR #17077 are
**historical**, not active owners. perf/windows-ci-selection-17485 and recovery
branches were inspected as source leads, not allocation authority. All remain
unchanged. Current ownership/ruleset facts above are supplied by parent live review;
local API attempts still fail, with no inferred root cause:

```
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/branches/main/protection": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/rules/branches/main": Forbidden
Post "https://api.github.com/graphql": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues/9603": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues/comments/5964825594": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues/17940": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues?state=open&per_page=100": Forbidden
```

## Proof and remaining integration

```
python3 -m unittest discover -s proposals/perl-merge-ready-cutover -p 'test_*.py' -v
git diff --check
```

Tests cover canonical static control/dynamic rejection; narrow name grammar,
producer/fallback collisions, event partition and concurrency; skipped workers;
every immutable identity axis; live unchanged-H/B/label close/draft/retarget;
base advancement retaining evidence; budget overrun retaining evidence; and all
103 source hashes. They do not execute hosted workflows or implement the canonical
schema extension. The initial review missed the static-name blocker; this revision
records and fixture-tests that correction rather than carrying forward its clean
verdict as evidence for the revised contract.

Next: canonical validator owner review/implementation first, then independent exact
implementation review, RIPR-only inactive candidate with unchanged command population,
and explicitly authorized native canaries for ignored labels/pending replacement,
remove/reapply, delayed event order, close/draft/retarget/forks, exact check SHA/app
association and measured deadline termination. Coordinate #10058/#17900/#17903 and
#16134/#17624 before dependent edits. No activation or protection edits here.

Normal Git branch publication is scoped to the proposal branch. Source inspection
found no push selector matching it. No PR or label event is created. Installed
hooks remain unchanged; no hook is bypassed. No warm xtask executable was available
for canonical writer-admission; this is not claimed as a successful admission run.

Primary native semantics:
[skipped required jobs report success](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions),
[concurrency and pending replacement](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency),
[PR event merge SHA](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request).
