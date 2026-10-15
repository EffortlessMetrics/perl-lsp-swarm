# Perl native merge-ready cutover — inactive proposal

Status: fixture-tested design, **NOT_PROVEN for activation or merge enforcement**.
No file under `.github/`, `.ci/`, hooks, credentials, or settings is changed.
This directory is a review artifact; neither its model nor its inventory is a
production policy source. No new service, controller, queue, or receipt platform.

## Observation and scope

Source: `EffortlessMetrics/perl-lsp-swarm` main
`4cc7960e61b685ce3b7e4cb9fe43b929bf900972`, observed 2026-10-15 UTC.
`inventory.json` records all 103 workflow definitions, their byte hashes, complete
triggers, concurrency, job conditions, dependencies and declared timeouts. Of these,
69 subscribe to `pull_request`. These are source facts, not live settings assertions.
The intended change is one explicit `merge-ready` label edge after focused local
proof and substantive review; opening, pushing and marking Ready do no broad work.
The label requests qualification; it does not assert that qualification passed.

The source-required result inventory (all GitHub Actions integration ID 15368):

| Exact result identity | Producer / job | Source policy role |
| --- | --- | --- |
| Perl LSP Rust Small Result | em-ci-routed-rust.yml / rust-small-result | required |
| ripr+ New Gap Gate | ripr.yml / ripr | required |
| Compile All Targets (bit-rot guard) | ci.yml / check-all-targets | required, conditional |
| Conflict marker check | ci.yml / conflict-markers | required, conditional |
| validate-title | pr-title-check.yml / validate-title | required, metadata only |

`CI Gate (Advisory Aggregate)` is advisory. The singular `[[check]]` workflow-shape
entries in required-checks.toml are not the plural `[[checks]]` result inventory.
Do not promote shape-contract names, skipped jobs, or advisory aggregates into proof.

Broad owners currently include:

| Workflow | Current broad admission | Cutover disposition |
| --- | --- | --- |
| ci.yml | opened, synchronize, reopened, Ready, draft conversion; main/master push; merge_group; dispatch | label edge for PR qualification; no broad push or native queue |
| em-ci-routed-rust.yml | opened, synchronize, reopened, Ready; main/master push; merge_group; dispatch | same PR admission, preserve exact result name |
| ripr.yml | opened, synchronize, reopened, Ready; main/master push; merge_group; schedule; dispatch | same PR admission; separate scheduled cache seeding from qualification |
| ci-nightly.yml | PR opening/push/Ready/label with sticky per-job label selection; nightly; manual | explicit heavy labels only on their matching label edge, not on every metadata event or push |
| tokmd.yml | opening/push/Ready/label | explicit cockpit label edge only; advisory |
| ux-regression-gate.yml and editor/host proofs | path-selected automatic PR work | carry relevant affected proof in the admitted qualification or explicit manual/scheduled support proof |
| workflow-policy / workflow-trigger-lint / gate-enforcement-contract | source contracts require old trigger shape, often merge_group | update contracts and fixtures with cutover; do not add exemptions to conceal contradictory policy |

Full inventory matters: changing only ci.yml leaves routed Rust, RIPR and auxiliary
build workflows running early. `inventory.json` contains every path selector,
including editor, protocol, security and release-related contracts. Do not modify
security or release workflows under this proposal. Their owner must determine which
are cheap affected checks and which broad work needs a separately authorized change.
Metadata checks and small scoped contract checks can remain automatic. A literal
zero-work-on-push policy is not claimed; the target is zero **broad** work.

## Smallest native design

Keep existing workflow owners, worker commands and GitHub App bindings. Change
admission and result propagation in place rather than introducing a dispatch bot.
Use read-only `pull_request`, never execute candidate contents under a privileged
`pull_request_target` token. Keep the title workflow unchanged.

1. Subscribe the three broad PR workflows to opened, synchronize, reopened,
   ready_for_review, converted_to_draft, labeled, unlabeled and edited. Omit the
   broad push and merge_group qualification triggers. Preserve explicitly separate
   schedule/manual maintenance only where it cannot emit qualification identities.
   Remove automatic broad dispatches by post-merge consumers too: a trigger-only
   change otherwise moves the same cost to another event.
2. Cheap admission jobs distinguish material events from unrelated metadata events.
   Broad workers run only on `labeled` where **event.label.name == merge-ready**,
   attempt 1, non-draft, accepted base, and a live head/base/label read matches the
   snapshot. Never admit from `contains(pull_request.labels, 'merge-ready')` alone.
   Synchronize with the label still attached invalidates proof but does not build.
   Remove and reapply the label to explicitly qualify the new candidate.
3. Move the four protected broad result names onto small result jobs in the same
   owning workflows; rename former protected workers. On every material event,
   these result jobs use `if: always()` and execute a fail-closed verdict step.
   A missing admission, draft, retarget, stale snapshot, non-success dependency,
   missing receipt or instrument failure returns failure. Require actual `success`
   from the complete selected worker set; no `skipped`, `neutral`, empty output,
   cancellation, timeout or `continue-on-error` can become qualification proof.
   An `always()` verdict step must explicitly reject `cancelled()` for its own run,
   even if the label was removed and reapplied before its final live read. Bind the
   event PR/run/attempt to the expected tuple as well as comparing receipt fields.
   Scoped no-ops need a successful explicit scope evaluator and recorded reason;
   never infer an exemption from absence. Preserve the five result names/app IDs;
   this design does not require changing the known required contexts.
4. Unrelated labeled/unlabeled events must not emit even a **skipped check with a
   protected name**. Use an event-dependent job name: protected name for material
   events, `Ignored metadata / <name>` otherwise. Apply it to every protected result
   job, not just the final aggregate. Ignore title-only edits here; title validation
   retains its own owner. Edited base targets are material invalidations.
5. Material events share a per-workflow/per-PR concurrency domain with cancellation
   enabled; this cancels superseded qualification on push, draft conversion, retarget
   or label removal. Irrelevant events get a unique per-run domain. Different
   workflows never share a group. Merely setting cancel-in-progress:false does not
   protect a pending qualification from replacement by an unrelated label run.
6. Qualification records repository, PR number, head SHA, base SHA, actual evaluated
   merge SHA and tree, workflow definition SHA, run ID and attempt. Checkout the
   immutable event merge SHA with credentials unpersisted; verify its parents are
   precisely the captured base and head and record `git rev-parse HEAD HEAD^{tree}`.
   Live API or ref failures fail closed, with their exact errors. Run workers and
   result jobs against this same tuple; no lookup for the latest same-named green
   check and no checkout of a moving branch/ref after admission. A workflow SHA can
   differ from candidate SHA: bind each explicitly rather than silently assuming
   they match. Check live head/base/label again before success. Label/push races
   therefore cannot turn an old run into current proof.
7. Immediately before authorized integration, re-read current required contexts,
   App identities, reviews, findings, head, base and mergeability. Check results on
   the actual evaluated source (head versus synthetic merge is explicit), not on
   whichever SHA happened to be listed first. Use expected-head merge protection.
   Conflict or changed combined tree needs affected requalification. Unrelated main
   movement alone does not justify a broad replay: carry forward only evidence whose
   subject and unaffected scope can be established under current repository policy.
   GitHub reads plus merge are not atomic. No native merge queue is introduced.

`expressions.txt` gives the event/name/concurrency expressions to port into owners'
workflows; `model.py` and fixtures exercise their intended contract. This is not a
ready-to-copy workflow replacement and does not duplicate worker implementations.

## Budget and existing dependencies

Current declared maxima include CI 75m, routed Rust 75m and RIPR 135m. A trigger
change cannot demonstrate a 20-minute qualification. Reuse the CI-selection owners'
affected-lane work, restore existing warm caches, avoid duplicate formatter/build
execution, and run selected workers in parallel once. Keep broad nightly, mutation,
coverage and platform-support matrices out of ordinary qualification; do not drop a
currently required proof obligation to make the clock green. Bound each worker by
the remaining qualification budget and reserve time for result publication.

Measure wall time from native run creation to all required results completed,
including runner wait, routing, cache restore, builds and result jobs. Record warm
and cold behavior and actual selected jobs separately. A terminal result later than
1200 seconds is not a within-budget qualification. Local fixture elapsed time is
**not** CI performance evidence. Queue delay can leave the result pending beyond
20m; a timeout setting cannot force runners to become available. Ordinary one-run
success and the <=20m SLO remain NOT_PROVEN until a real authorized canary measures
them. Do not turn this limitation into automatic retries or a new scheduler.

Current main-red refusal/baseline consumers depend on main-push evidence. Inspect
post-merge-status.yml, scripts/pre-merge-check.sh, routed Rust result aggregation,
merge-ready receipts and their exact-tree consumers before removing push admission.
Keep their refusal semantics: the owner must establish how existing exact landed
evidence or scoped baseline proof satisfies them within the budget. Do not silently
substitute a PR merge SHA for a landed squash commit, weaken a red baseline, or
claim the main baseline solved. This dependency is an activation blocker here.

## Ownership and limitations

Native git refs found, fetched read-only, and left unchanged:

- codex/late-ci-ready-qualification-20261002 at
  `22ff6470e0babaa79fab5971ac19f37e8bcb994e` (tip references #9603).
- perf/windows-ci-selection-17485 at
  `2c559069f4df9949321d464d1dfe1e93d186d14e`.
- recovery/perl-ci-selection-17485* family (multiple recovery/evidence candidates).

These establish existing candidate lines, not a live person's writer ownership.
Coordinate this companion design with those owners through their issue/PR before
activation. Live ownership could not be verified: `gh issue list` and `gh pr list`
both returned `Post "https://api.github.com/graphql": Forbidden`. REST probes:

```
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/branches/main/protection": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/rules/branches/main": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues/9603": Forbidden
Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues?state=open&per_page=100": Forbidden
```

No root cause inferred. Live enforcement union, review ownership and durable issue
coordination remain unverified. Source policy is explicitly not a live gate readback.
Existing candidate-controlled workflow trust is also not repaired by dynamic names
or app bindings: an adversarial workflow edit can forge its own result. Trusted
enforcement/ruleset work stays with its existing owner; no security-setting change
is authorized or included here. Dynamic-name/check association behavior needs a
native canary before activation; local fixtures cannot establish hosted behavior.

The isolated branch is for this proposal only. Native writer-admission has source
but no installed warm xtask executable in this environment; no Cargo build was run
for a documentation/model proposal. Mechanical checks established clean original
worktree, pinned base, unused new local branch, one proposal writer and 30GiB free.
The existing work checkout and all owner branches are unchanged. This does not claim
a successful canonical writer-admission run. Hook configuration was inspected:
no core.hooksPath override, only sample hooks installed in the original common dir;
tracked hooks/pre-push normally selects the fast gate. No hooks were changed,
installed or bypassed. Source trigger inspection found no push workflow selecting
the proposal branch; publication is limited to that branch, with no PR or label
event. This does not establish behavior of undisclosed external integrations.

Local proof command:

```
python3 -m unittest discover -s proposals/perl-merge-ready-cutover -p 'test_*.py' -v
git diff --check
```

Twelve tests pass, including 17 event fixtures and byte verification against the
103 pinned workflow Git objects. Independent source review confirmed the complete
inventory, required-result ownership and proposed native semantics. Review found
the model did not initially bind its event PR/run to its expected subject; that gap
is repaired with negative fixtures. Explicit own-run cancellation rejection and
malformed exact-SHA rejection were added during challenge. These are model/design
findings and repairs, not claims about activated workflow behavior.

## Alternatives and integration requirements

- Only add `if: label` to existing required jobs: rejected, skipped-required false
  green and sticky-label broad rebuilds.
- Put unrelated labels in the same concurrency group: rejected, pending replacement
  still happens with cancel-in-progress:false.
- Add a new required aggregate/context: possible later, but changes protections and
  the result contract. Preserve current identities in this proposal.
- Privileged label-controller + dispatch: extra write permission, another run and
  different check/source association. Larger than event-dependent result names;
  not needed for this design and not implemented.
- Native merge queue, receipt database or new CI service: outside requested scope.

Before activation, existing owners must reconcile lane scope and baseline consumers;
obtain live enforcement/ownership readback; port admission/result changes and affected
trigger-lint contracts together; independently review the exact implementation; run
an explicitly authorized canary covering skipped worker, unrelated-label pending
replacement, remove/reapply, new head, retarget, fork and check SHA/app association;
and measure ordinary warm/cold qualification against the 1200-second wall budget.
No current evidence supports activating the change or editing protections now.

Primary native semantics:
[skipped required jobs report success](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions),
[concurrency and pending replacement](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency),
[PR event merge SHA](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request).
