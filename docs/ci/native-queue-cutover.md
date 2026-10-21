# Native merge-queue cutover

## Selected target, not an activation receipt

Steven's October 21, 2026 ruling selects native queue-time integration for Perl
LSP. It supersedes the earlier no-queue mechanism in #17900, not the trust,
review, source identity, evidence completeness or release boundaries.

Current request authority: [#17900](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900#issuecomment-6308924192).
Existing parent: #10058 / #3390. Eligibility: #10057. Provider route: #17073.
Event perimeter: #17903. Candidate/base: #10063. Trusted tooling: #15001.
Desired settings and native activation: #17080 / #10065. Keep #16134, #17959
and #17494 with their existing source owners. Do not create another controller,
planner, gate catalog or workflow parser.

This document is source-stage guidance. It does not enable the dormant ruleset,
change a workflow trigger, publish a protected result, or establish a native
queue run. Existing protection and producer behavior stay authoritative until
the coordinated replacement is qualified and read back.

## Inspected state — October 21

Source basis: `383099c3a3a22d90467588c2d5d0261bab040fb2`.

- `.github/workflows/em-ci-routed-rust.yml` already accepts PR, `merge_group`
  and main/master push events. Queue support is not the same as queue-only cost.
  Its per-SHA main push grouping deliberately preserves each landed full run;
  retirement must replace that proof purpose, not merely delete a trigger.
- Active main ruleset **16664791** requires five contexts, all bound to GitHub
  Actions integration **15368**. Required review-thread resolution remains active;
  zero required approvals is not permission to omit substantive review.
- Ruleset **17372329**, named `merge-queue`, is **disabled**. Its stored settings
  are SQUASH, build concurrency 3, merge minimum 1/maximum 3, three-minute minimum
  wait, ALLGREEN and 60-minute check timeout. These are observed dormant settings,
  not the selected initial canary configuration or a measured runtime target.
- The branch summary reports no classic required contexts. That does not remove
  the active ruleset obligations or prove the entire enforcement union complete.

| Required context | Queue migration obligation |
|---|---|
| `Perl LSP Rust Small Result` | Preserve product/subject/producer proof and main-red behavior; move its expensive work to G only after enforced queue-only landing |
| `ripr+ New Gap Gate` | Bind the selected evidence and result to the actual group composition; PR-head evidence is not an automatic G substitute |
| `Compile All Targets (bit-rot guard)` | Preserve the obligation; integrate its expensive execution into G rather than accidentally duplicating full compilation |
| `Conflict marker check` | Run a bounded real check on the tested G; missing/skipped is not a substitute |
| `validate-title` | Supply exact constituent metadata proof on G without privileged candidate execution; #18255 owns the confirmed missing event producer |

The complete live union must be refreshed by the cutover owner. Checked-in
`.ci/policies/required-checks.toml`, observed rulesets and actual check producers
must agree; do not infer live truth from any single inventory.

## Concrete activation blocker: title metadata

At the inspected source, `pr-title-check.yml` listens only to
`pull_request_target` and manual dispatch. Its no-PR branch lists open PRs whose
source branch matches the dispatched ref and returns successful `nothing to
validate` when the list is empty. A native queue ref is not that PR source branch.

Do not add `merge_group` to this workflow without changing that consumer. It
could turn missing group membership into success, and its current PR-comment
permissions must not leak into candidate-controlled queue work. [#18255](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/18255)
requires complete native constituent identity, the existing title policy,
read-only metadata execution, wrong/missing-member falsifiers and actual G check
publication. A queue hang has not been observed here; the missing producer is a
source-established activation blocker.

## Target lifecycle and subjects

```text
cheap mutable development and Draft publication
-> focused proof + substantive review + joined repairs
-> exact-H product/scope judgment and conditional authorization to land
-> enqueue through the installed native adapter
-> one applicable expensive qualification transaction on G
-> protected native squash
-> cheap landed M reconciliation
```

H is the reviewed head; P is a historical PR integration; G is the exact native
queue composition; M is the actual landed commit/tree. Keep base, ordered
constituents, trusted control/tool/policy identities and run/attempt distinct.
The existing subject classes in the [receipt protocol](merge-ready-protocol.md)
remain useful; do not introduce a second vocabulary or promote old H/P evidence
to G merely because one PR's source is unchanged.

Enqueue authorizes automatic landing if current required conditions pass. Finish
product/scope judgment and expected repairs before enqueue. A material new finding
at unchanged H withdraws eligibility; a push requires affected review/proof and
new admission. Reuse unaffected evidence with its original identity. The same
owner handles ejection and final readback without a status-polling child.

GitHub couples required checks on PRs and merge groups. The steady-state target
is real cheap PR admission plus complete applicable G qualification, not zero
PR-side checks. Never let that cheaper admission authorize direct landing.
Metadata and conflict checks remain real checks; no unconditional-success shim,
skipped-required-job shortcut or context removal is part of the design.

## Safe deployment sequence

1. **Inventory and reconcile:** current workflows, live enforcement union and
   application bindings, every required context's event producer, active runs,
   trusted control/tooling and cache writers. Use existing #9151/#4787 authorities.
2. **Prepare the consumers:** exact G/membership/checkout binding; complete
   selected obligations; cheap PR admission; current review withdrawal; explicit
   inapplicability; native exit/reporting separation; #18255 metadata repair.
   A workflow that mentions `merge_group` is not sufficient execution evidence.
3. **Rehearse without replacing authority:** use non-required rehearsal identities
   and existing proof producers. Cover ordinary Rust, docs-only, dependency,
   policy/workflow and failed-composition cases. Keep all five current protected
   obligations satisfiable under the installed route.
4. **Bounded bootstrap:** after source qualification, the authorized #10065 owner
   enables and reads back queue enforcement while full PR qualification remains.
   Start with build concurrency 1, one-entry landing, SQUASH and ALLGREEN. Do not
   blindly enable the old stored 3/3 settings. Record temporary duplicate cost.
5. **Native proof:** retain actual enqueue/member identities, G checkout and all
   protected results, successful protected M, and corresponding tree evidence.
   Native failure/ejection and repair observations must retain their original runs.
6. **Single-expensive-run cutover:** only after queue-only landing is demonstrated,
   convert protected PR contexts to genuine cheap admission and retire duplicate
   broad ordinary-PR/main-push/callable-child routes. Retain cheap M reconciliation,
   distinct incident/deep/release work and the positive docs-only path.
7. **Acceptance:** measure first-G pass rate, invalidation/retry cost, queue age,
   cold/warm correctness, active runtime and cost per accepted change. Normally
   <=20 minutes applies to the complete active transaction, not a single child.

Merge min/max settings group landings, not builds. Concurrency 1 is a canary
setting; at a 35-minute runtime its no-failure ceiling is about 1.7 entries/hour.
Tune concurrency only from evidence rather than treating serialization as free.

For rollback, pause new admissions and reconcile in-flight groups. Restore and
verify the full protected PR qualification route before removing queue-only
landing enforcement. Restore a coherent producer/settings/cache generation.
No window may permit cheap admission alone to authorize direct merge.

## Main health and failure controls

The existing main-red refusal consumes eight advisory shard results. Those
shards can therefore become required dependencies when main is red. Its current
canonical-ci.yml comparison can prevent a workflow-changing repair from supplying
comparable proof. Preserve the evidence boundary but prove a designated repair
path before claiming queue operation works during an incident; do not bypass the
guard or require the very repair to be on main before it can qualify.

One incident integration owner coordinates repair/revert; parallel diagnosis may
continue. Ordinary admission pauses without turning every worker into a poller.
Queue freshness does not solve undeclared fixture dependencies, unreliable tests,
insufficient local memory/storage, or swallowed native failures.

Required negative cases include: semantically incompatible independently green
PRs; old P on a changed base; head movement; late adverse review; absent metadata
producer; empty/never-executed/wrong-app/wrong-subject/cross-attempt results;
predecessor ejection; cold missing fixture dependency; duplicate/lost request;
main-red repair; G/M mismatch; and rollback during active work. Source fixtures
and a green wrapper do not establish native deployment.

## Instruction and distribution boundaries

Reconcile the loaded AGENTS/CLAUDE roots, both provider finish/review procedures,
request helper, main-health guidance and current receipt protocol through their
existing owners. Older no-queue text is superseded as the target, not permission
to pretend a consumer has already migrated. Keep history and original receipts.

`perl-lsp-swarm` is the development/proof lane. Do not copy this rollout directly
into the public `perl-lsp` distribution/contribution lane, rename repositories,
move organizations, change marketplace continuity, or publish a release as a
side effect. Those require their own qualified synchronization or release step.

Primary contracts: [GitHub merge queues](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
and [merge-group event semantics](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#merge_group).
