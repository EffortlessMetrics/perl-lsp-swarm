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
controls. Its own tests demonstrate modeled properties, not native qualification.
Steven's current requested design, clarified after this plan's first publication,
is a **fresh `merge-ready` labeled edge on an open, non-draft current candidate**.
Ready alone is not admission. A retained label after synchronize, a delayed/stale
event, unrelated label, closed/draft PR or mismatched head/base cannot admit proof.
The edge requests qualification; it does not waive required checks, substantive
review, mergeability, exact-subject checks or other existing protections.

**Correction to the first published plan:** commit `83c2728d6d8a64a4388ed3101d54f2a5b94a7141`
incorrectly said “Do not deploy those parts” about fresh label-edge admission and
substituted trusted-command/compulsory-queue deployment. That was a policy choice,
not just rejection of sticky labels. It followed the October 13
[#10058 ruling](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10058)
and [#17900 transport contract](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900),
which prescribe a separate trusted command and H/custody/G route. Those issue
bodies remain unchanged on refresh. The latest assignment clarification controls
this scoped recommendation; their competing design is recorded for owner
reconciliation, not silently imposed as a new prerequisite. No live issue or
protection has been rewritten. Native queue activation and a new privileged
controller are not prerequisites for the requested label-trigger slice.

The [validator companion at 37dae252c3436a91ae22034fdca0835e05b9cc1f](https://github.com/EffortlessMetrics/perl-lsp-swarm/commit/37dae252c3436a91ae22034fdca0835e05b9cc1f)
implements the previously missing finite opt-in in the canonical validator, its
existing tests and guide. It is based on PR #16134 head
`f5e0f96537e8d71f6b592e6e798d073f6ead0e3e`; it is not a main-only patch lineage.
Adopt only its reviewed three-file delta through the current owner, resolving
actual interactions rather than merging its entire ancestry as a shortcut.
No checked-in workflow or policy row opts in merely by landing that support.
The grammar includes lifecycle invalidations and exact material-label edges.
Emission-shape validation alone does not establish native admission or check
association; that is the remaining qualification boundary.

The smallest source-first order is inactive validator support, then a reviewed
RIPR-only label-edge candidate retaining its proof population, then separately
authorized native qualification and controlled activation. The broader three-root
and nested-dispatch migration follows through existing owners.

## Refreshed ownership and holds (issue designs remain distinct)

GitHub connector issue bodies/comments and both PR objects were read before this
documentation edit. CLI GraphQL and ruleset reads returned `Forbidden`; these are
access limits, not failing tests or successful protection observations.

| Live authority | Observed responsibility and adoption boundary |
| --- | --- |
| [#10058](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10058) | Explicit exact-head request after useful proof/review/repair; current target is admitted H, protected custody, exact G qualification, then protected integration. That is the issue's design, not a new queue prerequisite for this clarified label slice; no queue/settings activation here. The October 13 ruling supersedes older Ready-custody comments. |
| [#17900](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900) | Issue owns a thin command and trusted default-branch transport, H publication and custody read-back. This differs from the clarified fresh-label route; reconcile with its owner before dependent shared-source changes. No new command is asserted or made a prerequisite here. |
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
2. **Prepare the RIPR-only label-edge candidate through its existing writer.**
   Keep the existing command population and five required checks. Admit broad work
   only on the exact fresh `labeled` / `event.label.name == 'merge-ready'` edge,
   first attempt, supported base, open/non-draft PR, with captured repository,
   PR/head/base/ref/label matching live state. Ready/open/reopen/synchronize only
   invalidate or publish cheap honest results; sticky membership does not rerun
   proof. Removal revokes readiness; deliberate removal/reapply requests a new
   attempt. Do not auto-retry stale work or introduce a new command/controller.
   The final `scripts/pre-merge-check.sh` remains the final green-check guard,
   not a prerequisite demanding green CI before starting qualification.
3. **Review emission, concurrency and proof before native activation.** Pair the
   finite name with exact `always() && material` result execution and isolated
   ignored-event concurrency. Missing/skipped/failed/cancelled/timed-out workers or
   instrument failure cannot pass the protected name. Bind event/run/attempt,
   repository/PR, H/base/ref, actual evaluated merge SHA/tree and workflow revision;
   re-read open/draft/label/subject before publication and integration. Candidate
   workflow trust remains an explicit unresolved boundary; neither fixtures nor
   applying a label establishes it. Retain old protected producers until the compatible new
   route is satisfiable. Coordinate #10058/#17900/#17903 and current #16134/#17624
   writers; do not absorb their active workflow work or promote a proposed Rust
   identity. A RIPR-only slice makes no claim that other roots are already gated.
4. **Run separately authorized native canaries, then the controlled RIPR cutover.**
   Preserve the current five names/app bindings and all protections. Obtain the
   complete effective union before activation; unknown policy remains NOT_PROVEN.
   Native canaries must demonstrate fresh-edge admission and refusal of Ready,
   sticky labels, delayed stale events, reruns, wrong subject, draft/closed/retargeted
   PRs and missing/failed workers. Exercise ignored-label pending isolation,
   remove/reapply races, cancellation, actual check name/SHA/app/run/attempt and the
   shared <=20-minute deadline, including publication reserve and termination.
   This plan performs none of those canaries. Existing protection owners retain
   any separately necessary settings transaction; enabling native queue or changing
   protected identities is not authorized or required by this label slice. Hold
   admission during incompatible source transition; rollback restores compatible
   source/result/concurrency semantics and retains existing protection settings.
5. **Adopt the observed slice, then migrate remaining producers with their owners.**
   Publish the actual fresh-edge procedure and limitations only after native proof.
   H changes revoke readiness and need affected proof/review plus a new explicit
   label edge; no automatic broad rerun from membership. Preserve completed valid
   evidence for its exact subject when only base or budget changes, while assessing
   remaining live gates. #17903 owns the remaining CI/routed-Rust/auxiliary/nested
   perimeter using the existing parser/catalog, including generated-status dispatch
   below. Retain unique release/cache/platform obligations. Reconcile the differing
   #17900 command/queue programme through its owner rather than silently switching
   this deployment to it or declaring the broader migration complete.

Steps 1–3 are source preparation. Step 4 requires separate native qualification
and activation authority; no release/deployment is performed by this assignment.
The first active scope remains RIPR only, preserving all five required checks.

## Main-health and remaining post-merge dispatch

**Main-health is a distinct evidence consumer.** Preserve current refusal semantics
and provenance, including `scripts/ci/main_red_refusal.py` and the routed Rust
probe. Do not make a main-health redesign a prerequisite for inactive validator
support or the label-edge candidate. Before removing its producer, identify the genuinely
landed-M obligation and arrange an owner-qualified replacement/cadence through the
existing programme. An evaluated PR merge tree is not exact landed squash-M evidence; missing main
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
retire or gate its product dispatch through the owner-agreed explicit admission
route after proof/review. For this clarified slice, publication must not synthesize
a `merge-ready` edge or treat sticky membership as a request. The actual generated
PR needs its own fresh subject-bound admission. Preserve current required producers
until that replacement is reachable; this document does not implement the edge.
The broad zero-dispatch claim remains incomplete until this edge is dispositioned.

## Evidence and remaining limits

Fresh local source proof, each executed in a clean detached worktree at its named
input (Python bytecode output is untracked/ignored):

| Exact subject | Command and observed result |
| --- | --- |
| `c2cff4eacb934da8a14ac8b41618bd2263cf5efa` | `python3 -m unittest discover -s proposals/perl-merge-ready-cutover -p 'test_*.py' -v`: 19 passed, 0.234 s; includes all pinned workflow hashes. |
| `37dae252c3436a91ae22034fdca0835e05b9cc1f` | `python3 -m unittest discover -s scripts/ci -p 'test_*enforcement*.py' -v`: 199 passed, 7.409 s, including acquisition, reconciliation, app binding and finite-name refusal controls. |

These passes support source consistency and the modeled controls only; they do
not qualify the fresh-label route on GitHub or establish expression
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
