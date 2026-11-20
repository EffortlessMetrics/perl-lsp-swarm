# Native merge-queue cutover

## Selected target, not an activation receipt

Steven's October 21, 2026 ruling selects native queue-time integration for Perl
LSP. The October 22 clarification requires **no CI on Draft, Ready or ordinary
publication/update events**, including cheap PR workflows. Useful local tests
and substantive review finish before enqueue; CI qualifies the actual group.
This supersedes the earlier no-queue mechanism, not trust, review, source
identity, evidence completeness or separate release authority.

Current request authority: [#17900](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900#issuecomment-6338138191).
Existing parent: #10058 / #3390. Eligibility: #10057. Provider route: #17073.
Event perimeter: #17903. Candidate/base: #10063. Trusted tooling: #15001.
Desired settings and native activation: #17080 / #10065. Preserve #16134,
#17959 and #17494's existing implementation ownership. Do not create another
controller, planner, gate catalog or workflow parser.

The reusable source contract is now on
[DemoSwarm main](https://github.com/EffortlessMetrics/demo-swarm/blob/c7a965684783c9713efa8194c9312b1d316142aa/docs/reference/native-queue-qualification.md).
That source landing does not activate this repository or prove harness adoption.
This document changes no workflow, check, setting, runner or release. Preserve
the actual installed protected route until its replacement is qualified.

## Inspected source and settings

Historical source basis: `383099c3a3a22d90467588c2d5d0261bab040fb2`, October 21.
The dormant queue ruleset was read again on October 23 and remained disabled.

- `em-ci-routed-rust.yml` accepts PR, `merge_group` and main/master push events.
  Queue support is not queue-only execution. Its per-SHA main grouping deliberately
  preserves each landed full run; retirement must replace the proof purpose.
- Main ruleset **16664791** requires five GitHub Actions contexts, bound to
  application **15368**, with required review-thread resolution. Zero required
  approvals does not eliminate substantive review.
- Queue ruleset **17372329** is disabled. Stored settings: SQUASH, build
  concurrency 3, merge minimum 1/maximum 3, three-minute minimum wait, ALLGREEN,
  60-minute check timeout. These are neither accepted canary settings nor measured
  performance. Refresh the whole enforcement union at apply time.
- No classic required context in a branch summary does not remove active ruleset
  obligations. Desired policy files, observed enforcement and actual producers
  must agree; none alone establishes the complete live contract.

| Required context | Queue migration obligation |
|---|---|
| `Perl LSP Rust Small Result` | Preserve product/subject/producer proof and the main-red repair path; allocate expensive work to G only after safe cutover |
| `ripr+ New Gap Gate` | Evaluate actual group changes and accepted evidence; raw H proof is not automatically G proof |
| `Compile All Targets (bit-rot guard)` | Preserve the obligation within G qualification without duplicate full compilation |
| `Conflict marker check` | Perform the real bounded check on G; missing/skipped is not verification |
| `validate-title` | Establish constituent metadata coverage or reviewed all-entry admission equivalence; preserve the trusted producer |

## Title metadata: continue the existing candidate

[#18255](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/18255)
owns queue metadata acceptance. Existing [#16017](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16017),
branch `fix/15351-title-exact-binding`, owns the title workflow, exact dispatch
caller and contract tests. Do not start a second title writer.

The October 21 main producer had no merge-group event. Its manual branch lookup
could return successful `nothing to validate` for an unresolved queue ref.
#16017 replaces branch discovery with explicit PR/head input and final reread;
that source repair is not a G producer or native activation receipt. Its stricter
issue-reference/placeholder policy belongs to #15351, not an implicit policy
change under #18255's preserve-policy transport scope.

On October 23, exact-head replay of #16017 found that removing `skip-title-check`
between initial and final reads still emitted an exemption receipt. Successor
`3f0ae35f99fe58ee50b9710aff756f9854da6351` revalidates that particular exemption,
with seven regression cases and a guard-disabled negative control. This is a
validation-time repair on the existing candidate, not ongoing post-publication
invalidation, complete native qualification or a landed change.

Do not merely add `merge_group` to a PR-shaped workflow. Missing membership must
not take a no-PR success fallback, and candidate content must not execute with
metadata/publisher authority. Existing source checks and native evidence remain
separate acceptance stages.

## Exact subjects and the unresolved membership contract

H is the reviewed candidate head; P is a historical PR integration; G is the
native queue composition; M is the actual landed commit/tree. Reuse the existing
[receipt subject classes](merge-ready-protocol.md). Bind actual checkout/tree,
base, trusted control/tool/policy revision, selected proof and run/attempt.
A green H/P result cannot impersonate a later G merely because H is unchanged.

The public merge-group payload supplies G/base identities, not an immutable
constituent list. PR associations, ref parsing, commit timestamps and current
queue prefixes do not establish complete event-time membership. The
[October 22 owner reconciliation](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/18255#issuecomment-6338272679)
records that limitation. Do not publish guessed membership as authenticated proof.

The existing owners must qualify either authoritative membership or an explicit
all-entry admission-equivalence contract. Equivalence must cover every native
entry path, exact H, expected producer/policy, mutable title/label state, review
withdrawal and bypass treatment. A matching status name is insufficient.
A controlled singleton canary can establish only its observed B/G/H composition
when other admissions and in-flight groups are actually excluded. Build
concurrency 1 and maximum merge 1 alone do not prove singleton membership.

## Steady-state lifecycle

```text
local implementation and focused proof; Draft publication starts no CI
-> substantive review and joined repairs
-> exact-H product/scope judgment and conditional merge authorization
-> explicit admission publication and native enqueue
-> one applicable qualification transaction on G
-> protected native squash when current required conditions hold
-> cheap M reconciliation
```

Enqueue authorizes automatic landing if qualification succeeds. Complete expected
repairs and material dispositions beforehand; no additional routine approval
after green or competing direct merge is required. A new finding at unchanged H
withdraws eligibility without falsifying old test results. A pushed successor
requires affected review/proof and fresh admission. Retain the useful owner
without a polling child.

GitHub requires contexts before enqueue and on G. In the final design, PR-side
contexts come from an explicit qualified admission publisher, **not automatic
PR-event CI**. The producer must validate actual evidence/review and preserve
application binding. A worker assertion, unconditional success or skipped job
is not admission. Never let cheaper admission authorize a direct merge.

## Deployment order under #10065

1. Inventory current source, complete enforcement union, all five event producers,
   trusted tooling, cache writers and active work using existing authorities.
2. Repair G consumers and explicit admission at their current owners. Cover
   actual checkout, metadata/membership or equivalence, selected obligations,
   late findings, positive inapplicability and native failure distinctions.
3. Rehearse under non-required identities while retaining the installed full
   proof. Source fixtures do not claim actual G execution or settings activation.
4. Enable and read back queue enforcement for a bounded bootstrap. Full legacy
   PR qualification supplies required entry contexts; current exact-H review and
   conditional merge authorization still precede enqueue. Start with concurrency
   1, one-entry landing, SQUASH and ALLGREEN, without mistaking settings for
   membership proof. Record temporary duplicate costs and actual native outcomes.
5. After queue-only landing and all protected G producers are demonstrated,
   replace full PR qualification with the qualified explicit admission publisher.
   Remove ordinary PR workflows, including cheap wrappers, and duplicate broad
   main-push/callable-child work coherently. Preserve cheap landed reconciliation
   and distinct incident, deep and release obligations.
6. Prove zero workflow starts for ordinary Draft/Ready/update events. Retain actual
   entry/G/result/M evidence and the correct landed-prefix tree reconciliation.
   Observe first-G pass rate, invalidation/retry costs, queue age and accepted-change
   cost before claiming economics or installed adoption.

Merge limits group landings, not builds. Concurrency 1 is a bootstrap setting,
not a permanent throughput ceiling. Normally <=20 minutes applies to the complete
active qualification, including setup and finalization. Queue delay and all
failed/retried/local/review costs remain separately visible. No speed claim is
established by changing a trigger, raising a timeout or dropping an obligation.

Rollback pauses new admissions and reconciles active groups. Restore and verify
full protected PR qualification **before** removing queue-only enforcement.
Restore a coherent producer/settings/cache generation; never expose a window
where admission alone permits direct landing.

## Main health and discriminating acceptance

The existing main-red refusal consumes eight advisory shards, making their
results conditionally load-bearing. Its canonical-ci.yml comparison can prevent
a workflow-changing repair from providing comparable proof. Qualify a designated
repair path without a blanket override or circular already-repaired-main
prerequisite. One incident integration owner coordinates repair/revert; parallel
diagnosis and independent useful work can continue.

Retain cases for incompatible independently green PRs; stale P; moved H; adverse
review or title/label movement without H movement; missing metadata producer;
empty/never-executed/wrong-app/SHA/attempt results; predecessor removal and
recomposition; undeclared cold fixture dependencies; duplicate/lost requests;
main-red repair; G/M mismatch; and rollback during active work. Distinguish source
replays, native observations and final activation. A valid diagnostic failure
receipt is not successful product proof.

## Scope boundaries

Reconcile loaded roots, provider finish/review procedures, request helpers and
health guidance through their existing owners. Historical no-queue instructions
are superseded as the target, not permission to assume installation. Preserve
original evidence and the current enforcement path during transition.

Development/proof remains in `perl-lsp-swarm`. Public `perl-lsp` distribution,
repository/organization changes, marketplace update continuity, release tagging
and publication retain their own qualified synchronization/authorization. None
is a side effect of this rollout.

Primary contracts: [GitHub merge queues](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
and [merge-group events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#merge_group).
