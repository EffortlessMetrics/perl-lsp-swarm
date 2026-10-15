# Perl admission adoption: smallest compatible deployment order

Status: **documentation-only recommendation; activation NOT_PROVEN**.
Observed 2026-10-15, approximately 03:28 UTC. Source base:
`4cc7960e61b685ce3b7e4cb9fe43b929bf900972`. This plan changes no workflow,
policy row, required context, runner allowance, protection, or dispatch. It is a
consumer of the existing owners below, not another admission implementation or
workflow registry. Re-read their live state before implementation or activation.

## Cause and disposition of the two inputs

The [inactive proposal at c2cff4eacb934da8a14ac8b41618bd2263cf5efa](https://github.com/EffortlessMetrics/perl-lsp-swarm/tree/c2cff4eacb934da8a14ac8b41618bd2263cf5efa/proposals/perl-merge-ready-cutover)
contains useful finite-name, identity, cancellation, result, budget and inventory
controls. Its own tests demonstrate those modeled properties, not trusted native
admission. Its explicit label-edge admission, remove/reapply retry and assertion
that no native queue is needed conflict with the refreshed
[#10058 ruling](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10058)
and [#17900 transport contract](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900).
Do not deploy those parts or describe a `merge-ready` label as authority.
Ready remains review availability. A label may be request data only if the trusted
owner validates an actual authorized request; neither its presence nor its edge
establishes that transaction. This plan does not endorse a new label transport.

The [validator companion at 37dae252c3436a91ae22034fdca0835e05b9cc1f](https://github.com/EffortlessMetrics/perl-lsp-swarm/commit/37dae252c3436a91ae22034fdca0835e05b9cc1f)
implements the previously missing finite opt-in in the canonical validator, its
existing tests and guide. It is based on PR #16134 head
`f5e0f96537e8d71f6b592e6e798d073f6ead0e3e`; it is not a main-only patch lineage.
Adopt only its reviewed three-file delta through the current owner, resolving
actual interactions rather than merging its entire ancestry as a shortcut.
No checked-in workflow or policy row opts in merely by landing that support.
The grammar still includes old lifecycle/material-label events: validating its
emission shape does not make those events a permitted admission mechanism.

The correction is therefore to separate safe inactive source support from the
trusted request, producer and protected activation transaction. A RIPR-only active
label cutover is not the first deployment recommendation.

## Refreshed ownership and holds

GitHub connector issue bodies/comments and both PR objects were read before this
documentation edit. CLI GraphQL and ruleset reads returned `Forbidden`; these are
access limits, not failing tests or successful protection observations.

| Live authority | Observed responsibility and adoption boundary |
| --- | --- |
| [#10058](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10058) | Explicit exact-head request after useful proof/review/repair; current target is admitted H, protected custody, exact G qualification, then protected integration. Preserve the max-1 target; no queue/settings activation here. The October 13 ruling supersedes older Ready-custody comments. |
| [#17900](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900) | One thin command and trusted default-branch dispatch transport, H publication and custody read-back. Final command spelling belongs to #8606/#17073. No implemented command is asserted by this plan. |
| [#17903](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17903) | Existing parser/catalog owns full lifecycle and nested-dispatch perimeter plus matched trigger diff; #9151 retains the wider gate/obligation audit. Guard fixtures can precede activation; final trigger removal cannot strand required producers. |
| [PR #16134](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16134) | Open Draft, head `f5e0f96537e8d71f6b592e6e798d073f6ead0e3e`, branch `feat/contabo-rust-standard`. Canonical Rust consumer writer; validator companion is offered in [comment 6182682200](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16134#issuecomment-6182682200). Exact public pin allowance/approval, native admission/producer/custody, full policy budget and protection transition remain pending. No dispatch follows from this plan. |
| [PR #17624](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17624) | Open Ready, head `2c559069f4df9949321d464d1dfe1e93d186d14e`, branch `perf/windows-ci-selection-17485`. Existing classifier/bootstrap/workflow writer retains its scope. Later comments report a Cargo-cwd fixture failure and an isolated #17639 correction; do not infer it is repaired on this head or overwrite its workflow changes. |

Ownership refresh is an observation, not an acknowledged transfer. This task owns
only this adoption document. Shared workflows/helpers stay with their current
writers; compose sequentially or use an explicitly reviewed stack.

## Preserve the five protected obligations

These five names are the source/proposal and owner-reported enforcement baseline,
not a fresh complete effective-protection read. The proposal attributes active
ruleset 16664791, Actions app 15368 and strict=false to its parent; #10058 reports
queue ruleset 17372329 disabled. Classic protection and all applicable rulesets,
bypass actors and app bindings must be freshly read by #10065 before cutover.
An unreadable source leaves the effective union **NOT_PROVEN**; do not assume an
absent or disabled setting has already been changed.

| Existing result identity | Source producer on the observed main |
| --- | --- |
| `Compile All Targets (bit-rot guard)` | `.github/workflows/ci.yml:check-all-targets` |
| `Conflict marker check` | `.github/workflows/ci.yml:conflict-markers` |
| `validate-title` | `.github/workflows/pr-title-check.yml:validate-title` |
| `Perl LSP Rust Small Result` | `.github/workflows/em-ci-routed-rust.yml:rust-small-result` |
| `ripr+ New Gap Gate` | `.github/workflows/ripr.yml:ripr` |

This is an adoption constraint snapshot, not a second producer catalog. Use the
canonical policy/parser and live union for implementation. Keep every obligation
satisfiable throughout staging, including title metadata. Rehearsal identities
are non-required. No dummy pass, ignored-name skip, old-head green or control-run
green can substitute for a selected protected obligation. Any separately proposed
Rust identity transition remains with #16134/#10065 and needs its own qualified
mapping; this document authorizes none.

## Deployment order and acceptance boundaries

1. **Land inactive validation support, if accepted by its owner.** Integrate the
   three-file `37dae2` delta without workflow/policy opt-in. Recheck static defaults,
   finite-name negative controls, duplicate producers, app bindings and reusable
   projection on the actual composition. This is independently useful source
   support and requires no native queue, fleet migration or main-health redesign.
   Revert those three files together if the unused support is rejected.
2. **Stage the actual request and producer interfaces without protected effects.**
   #10057 supplies eligibility; #17900 supplies trusted transport in non-required,
   no-general-custody rehearsal; #17073 adopts its implemented command in both
   provider routes. Keep control revision S, candidate H and integration G distinct.
   The final `scripts/pre-merge-check.sh` already requires green checks and is not
   an entry evaluator. Prove stale/unauthorized requests refuse, dry-run writes
   nothing, repeated requests converge, H publication binds the approved source,
   and successful dispatch without read-back never reports custody.
3. **Stage compatible producer/perimeter source and the exact activation diff.**
   #17903 extends the existing parser/catalog and event fixtures across CI, RIPR,
   routed Rust, auxiliary and reusable/indirect roots. #10063/#17080 map all five
   obligations to honest H-entry and actual G-integration profiles, with selected
   work complete and fail-closed. Neither profile satisfies the other. #10064 and
   the relevant runner owners qualify a reachable selected execution route; an
   unrelated future fleet rollout is not a universal prerequisite. Coordinate
   #16134 and #17624 before shared-file composition. Keep current protected
   producers active; hold incompatible final trigger removal in the cutover
   candidate. No candidate-controlled activation switch or extra required context.
4. **Qualify and activate only through #10065's separately authorized transaction.**
   Revalidate exact predecessor sources, app/context mapping, full protection union,
   native request/check/custody behavior and rollback packet. Hold admissions while
   switching the compatible source/settings generation: writes are not atomic.
   Prove every H-entry obligation is reachable and H-green cannot direct-merge
   around compulsory G proof before releasing admissions. Keep the initial target
   maximum group size one. Native canaries must exercise actual names, app,
   H/S/G/run/attempt association, queue read-back, stale H2/adverse review revocation,
   cancellation, ignored-event isolation and missing/failed selected results.
   Removing old automatic triggers and enabling their admitted replacement belong
   to this same guarded cutover; neither premature removal nor indefinite duplicate
   execution is acceptable. Restore the compatible source/routing/settings
   generation and reconstruct custody from GitHub on rollback, not source alone.
5. **Prove the deployed boundary and adopt it.** #11132 consumes the actual live
   max-1 generation for wait/wake and merge-barrier proof. #17073 documents the
   actual command, explain mode, blocker and repair/re-request path; H2 needs fresh
   affected proof/review and a new explicit request. Observe zero product dispatch
   for open/Ready/reopen/synchronize and docs inapplicability before allocation.
   Product jobs allocated only to print a no-op do not qualify. Missing read-back
   stays NOT_PROVEN/MERGE_BLOCKED. #17075 can consider max-2 only after the live
   max-1 proof, through a new reviewed settings generation.

Steps 1–3 are source preparation. Steps 4–5 require native qualification and
separate activation authority and are **not performed by this assignment**.
Finite-name support alone cannot shorten this sequence into active label admission.

## Main-health and remaining post-merge dispatch

**Main-health is a distinct evidence consumer.** Preserve current refusal semantics
and provenance, including `scripts/ci/main_red_refusal.py` and the routed Rust
probe. Do not make a main-health redesign a prerequisite for inactive validator
support or request rehearsal. Before removing its producer, identify the genuinely
landed-M obligation and arrange an owner-qualified replacement/cadence through the
existing programme. G evidence is not exact landed squash-M evidence; missing main
observations retain the current explicit limitation and cannot be relabeled green.
Do not silently delete unique main/deep/release/platform/cache work as duplicate CI.

**Generated-status publication has a separate dispatch escape path.** At the pinned
main, [post-merge-status.yml's “Raise CI on the generated PR” step](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/4cc7960e61b685ce3b7e4cb9fe43b929bf900972/.github/workflows/post-merge-status.yml#L542)
dispatches `ci.yml`, `em-ci-routed-rust.yml`, `ripr.yml` and
`pr-title-check.yml` on `automation/post-merge-status`; CI receives base/head inputs.
This is qualification of a newly generated PR, not evidence for the landed M that
triggered generation. Deleting main-push triggers alone leaves it reachable.
#17903 must include it in the existing reachability denominator and coordinate
the existing generated-status owner. Keep publication/freshness/title metadata;
replace its product dispatch with the implemented trusted exact-H request only
after proof/review and explicit intent, rather than treating bot publication as
admission. Preserve current required producers until that replacement is reachable.
The broad zero-dispatch claim remains incomplete until this edge is dispositioned.

## Evidence and remaining limits

Fresh local source proof, each executed in a clean detached worktree at its named
input (Python bytecode output is untracked/ignored):

| Exact subject | Command and observed result |
| --- | --- |
| `c2cff4eacb934da8a14ac8b41618bd2263cf5efa` | `python3 -m unittest discover -s proposals/perl-merge-ready-cutover -p 'test_*.py' -v`: 19 passed, 0.234 s; includes all pinned workflow hashes. |
| `37dae252c3436a91ae22034fdca0835e05b9cc1f` | `python3 -m unittest discover -s scripts/ci -p 'test_*enforcement*.py' -v`: 199 passed, 7.409 s, including acquisition, reconciliation, app binding and finite-name refusal controls. |

These passes support source consistency and the modeled controls only; they do
not reconcile the proposal's obsolete label policy or qualify GitHub expression
evaluation, check association, product execution, runner trust or custody. The
validator's bounded parser cannot prove absence of indirectly synthesized names
in arbitrary unlisted expressions. Retain that limit at adoption.

The proposal's 20-minute execution ceiling is not established by fast fixture
tests or configured workflow maxima. Keep its shared remaining-deadline and
publication-reserve controls as design inputs. #16134 separately reports a cold
verifier measurement and a still-unmeasured full 35-minute policy envelope; neither
is a demonstrated ordinary <=20-minute product run. Resolve applicable item budgets
with their owners before qualification; do not silently extend a ceiling, weaken
proof population, transfer synthetic evidence or automatically repeat completed
proof because its budget outcome differed.

This environment's agent preflight passed branch/worktree/conflict/cwd/target-dir/
stash checks but exited 7 because the pre-push hook is unprovisioned. No hook was
bypassed or installed; hook-enforced admission is NOT_PROVEN. No Cargo build,
broad CI, label, PR event, native canary, release, deployment or settings write is
part of this documentation task. Activation waits for the named owners' exact
source/interface acceptance, approved native route and complete protection proof.
