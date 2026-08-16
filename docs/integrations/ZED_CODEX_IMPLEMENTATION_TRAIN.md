# Zed Codex PR-by-PR implementation train

> **Core LSP programme:** #7759
> **Machine DAG:** `.ci/fixtures/zed-perl-upstream/codex-train.v1.json`
> **Core CI/currentness control:** #9483
> **Non-blocking DAP sidecar:** #9484
> **Current public claim:** planned / not proven
> **External writes:** maintainer stop points only

This is the execution map for moving the existing Zed Perl integration from repository-owned static authority to exact-source evidence, upstream publication, official-registry proof, and checked support projection. It records the DAP sidecar because both surfaces use the existing public `perl` extension, but DAP remains outside the closure invariant for #7759.

## Product identities

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp

DAP adapter ID       -> perl-dap
DAP executable       -> perl-dap
```

These are not aliases. Every EffortlessMetrics LSP receipt must prove exact `perllsp --stdio`. Every debugger receipt must prove exact `perl-dap` using the canonical DAP transport contract. One process, cache family, asset receipt, or support row cannot satisfy the other surface.

## Current execution frontier

The train was reconciled against GitHub on August 16, 2026.

| Stage | Live state | Codex action |
|---|---|---|
| P00 — #7975 / PR #8023 | merged; issue closed | historical substrate only |
| P01 — #7980 / PR #8365 | authority merged; executable evidence not proven | do not rebuild; proceed to P06 |
| P02 — #7984 / PR #8369 | authority merged; executable evidence not proven | do not rebuild; P03 is now unblocked |
| P03 — #8647 | ready | add the deterministic fixture and expectation contract from current `main` |
| P04 — #7990 / PR #8373 | authority merged; executable evidence not proven | do not rebuild; execution is owned by P12 |
| P05 — #7992 / PR #8379 | authority merged; executable evidence not proven | do not rebuild; execution is owned by P13 |
| P06 — #8661 | ready | add the read-only public-asset workflow from current `main` |
| P09 — #9468 | ready | add the fail-closed support projection substrate from current `main` |
| C01 — #9483 | ready | add stable checks, semantic routing, and receipt invalidation from current `main` |
| D01 — #9485 | ready, non-blocking | add the static `perl-dap` adapter authority independently of the LSP train |

The first core wave is therefore:

```text
Codex A -> P03 / #8647
Codex B -> P06 / #8661
Codex C -> P09 / #9468
Codex D -> C01 / #9483
```

The first non-blocking debugger wave is D01 / #9485. DA01 / #9516 and D02 / #9486 stay closed until D01 establishes the exact adapter/configuration/target subject.

PR #8369, PR #8373, and PR #8379 landed their authority increments on `main` directly; the earlier successor plan to reconstruct P04/P05 as stacked seeds is superseded by that merge and is not revived here. Their owning issues #7984, #7990, and #7992 remain open because a merged authority increment is not executable host evidence: P11 owns the exact-source core receipt, P12 owns the settings receipts, and P13 owns the defaults receipts. P04 and P05 therefore depend on the P02 host driver they actually consume, and reach the P03 fixture through P11 at execution time rather than at authority time.

## Train rules

1. One PR owns one semantic increment. Branch from current `main` only after every required predecessor has landed.
2. Existing branches are evidence, not authority. Preserve useful changes, tests, and review findings; do not preserve stale stacked history for its own sake.
3. Write falsifiers first. A schema, template, packet, workflow definition, compilation result, or raw protocol smoke is not host behavior.
4. Bind receipts to exact source, host, artifact, configuration, fixture, instrument, platform, and route identities. Keep limitations and currentness local to each row.
5. Missing, stale, skipped, cancelled, timed-out, malformed, duplicate, cross-run, cross-SHA, or instrument-failed evidence is never pass.
6. Use `Advances #...` until the owning issue's actual acceptance is earned. Use `Closes #...` only when the PR completes that issue rather than merely landing substrate.
7. Run focused tests, formatting, applicable workflow/process/network/file policies, Changie where required, and `git diff --check` in every PR.
8. Upstream extension, Zed-core, official registry, and release mutations are maintainer stop points. Codex prepares internal packets and stops.
9. Public Zed documentation is generated from or checked against #7122. Prose cannot outrun a current receipt.
10. LSP and DAP remain independent evidence rails. DAP never blocks or broadens #7759.

## Reusable Codex instruction

> Take the named train stage through `deliver-pr` as the accountable lane root, and record that route on the stage's GitHub subject. Re-fetch current `main`, the owning issue and comments, existing branches and PRs, and every named external read-only subject before editing. Treat stale branches as evidence rather than authority; reconstruct the narrowest current-base increment, write falsifiers first, preserve product and evidence-stage boundaries, run focused and policy checks, and open one PR. Use `Advances` until actual acceptance is earned. Perform no external submission, release, registry mutation, unsupported support promotion, or destructive cleanup.

Each stage is one claim. Enter `deliver-pr` at the earliest absent or stale judgment for that stage, follow its named normal and material backward edges, and return a typed lane result rather than stopping at research, a green check, or a subagent verdict. A stage whose authority PR has merged is not finished: its owning issue stays open until the named execution stage earns the receipt, and the train records that separation as `authority_merged_execution_not_proven`.

# Core LSP train

## Phase 0 — accepted static substrate

### P00 — #7975 / PR #8023: static convergence

**State:** complete; static substrate only.
**Exit already earned:** the coherent Zed candidate, truthful identity boundary, settings/defaults/managed projections, and packet substrates are on `main`. No actual Zed or public-artifact behavior was proven.

### P01 — #7980 / PR #8365: public-asset producer authority

**Depends on:** P00.
**State:** authority merged; #7980 remains open because executable public evidence is separate.
**Accepted increment:** read-only release selection, exact target/member identity, safe extraction, matching-host process authority, cross-build non-execution, and false-green controls.

Do not reopen P01 to execute the matrix. P06 owns the workflow and P10 owns retained current receipts.

## Phase 1 — implementation authorities

### P02 — #7984 / PR #8369: exact-source real-Zed driver

**Depends on:** P00.
**State:** authority merged; #7984 remains open because executable host evidence is separate.

**Accepted increment:** the operator-assisted prepare/launch/finalize authority — exact Zed, extension, WASM, `perllsp`, fixture, profile, settings, process, and instrument identities; supported Zed surfaces only; shared receipt validation; isolated profile; bounded logs; no direct mutation of Zed internal state. The checked observation stays `not_run`.

Do not reopen P02 to run a host. P03 may now branch, and P11 owns the first actual host receipt.

### P03 — #8647: deterministic fixture and expectation contract

**Depends on:** P02.
**State:** ready now.

Create the shared fixture authority for activation, `.pod` separation, diagnostics and repair, completion/navigation, edit or bounded refusal, UTF-16 positions, LF/CRLF, custom token cases, freshness, settings, and wrong-root discriminators.

**Exit:** fixture and expectations are deterministic, content-addressed, and consumable by all host evidence leaves without duplicating semantic expectations.

### P04 — #7990 / PR #8373: settings behavior authority

**Depends on:** P02.
**State:** authority merged; #7990 remains open because the four checked experiments have not run.

**Accepted increment:** the checked `project_only`, `zed_override`, `zed_override_removed`, and `live_edit` experiment and validator, preserving project/user authority, reversible precedence, restart/live boundaries, and secret-safe receipts. Results stay `not_run`.

P12 owns execution and reaches the P03 fixture through P11.

### P05 — #7992 / PR #8379: defaults and provider-order authority

**Depends on:** P02.
**State:** authority merged; #7992 remains open because no row of the matrix has run.

**Accepted increment:** the four-row defaults/extension/provider-selection matrix and its deterministic ruling contract. No publication-order result is earned before P13 executes it.

### P06 — #8661: public-asset matrix workflow

**Depends on:** P01.
**State:** ready now.

Add one pinned, read-only Windows/Linux/macOS workflow that invokes the merged P01 producer, derives matching-host execution from observed runner identity, retains cross-built rows as not executed, uploads one receipt bundle per runner plus an aggregate manifest, preserves `fail-fast: false` and `cancel-in-progress: false`, and has no repository write authority.

**Exit:** workflow authority is merged. A green definition is not executable evidence; P10 owns the run and retained receipts.

### P07 — #8753: managed route and cache-recovery authority

**Depends on:** P01 + P02.
Define managed first-mile, restart, disable/shutdown, exact known-good selection, recovery scenarios, cleanup boundaries, shared validator, bounded host seam, and `not_run` template. Keep explicit/PATH and managed routes independent.

### P08 — #9467: official-registry host-driver authority

**Depends on:** P02 + P03.
Define prepare/launch/finalize authority for:

```text
Journey A: official existing `perl` registry installation + quiet released defaults
Journey B: explicit public `perllsp` selection + managed route
```

Require content-addressed public subjects and reject development, fork, copied package, local, PATH, and prior-cache substitutions. #7912 remains the execution owner.

### P09 — #9468: fail-closed public-support projection substrate

**Depends on:** P00 + current #7122 substrate.
**State:** ready now.

Extend the generic client-support model only as required to represent Zed without flattening stage, route, platform, provider, activation, method, settings, recovery, limitation, or currentness identity. Add a future #7912 importer and deterministic generated-doc check while retaining a current `planned` / `not_proven` seed.

Reject exact-source-as-public, managed/PATH collapse, cross-platform promotion, unobserved method inheritance, `.pod` as Perl, DAP leakage, another provider satisfying `perllsp`, stale receipts, and second-run generation drift.

### C01 — #9483: stable checks and subject-aware invalidation

**Depends on:** P00.
**State:** ready now.

Replace whole-workflow absent-green semantics with canonical semantic-subject selection, current-run selection receipts, stable final contexts, exact affected-receipt invalidation, and fail-closed aggregation. Preserve the repository router rather than creating a Zed-only change parser. Preserve no-cancel-in-progress behavior for expensive host work.

The finalizer must reject selected lanes that are skipped, cancelled, timed out, missing, malformed, duplicated, cross-run, cross-SHA, stale, or substituted across evidence stages. `not_applicable` may close an unselected current run; it cannot refresh evidence or support.

**Exit:** P10/P11 may produce current programme evidence under stable check identities. C01 itself proves no Zed behavior.

## Phase 2 — execute internal evidence

### P10 — #8678: executable public `perllsp` asset receipts

**Depends on:** P01 + P06 + C01.
Run the real matrix against current public bytes, validate every target, and retain immutable per-target plus aggregate receipts. Matching-host process execution and cross-built non-execution remain distinct. Narrow unsupported targets instead of manufacturing pass.

### P11 — #8695: exact-source core Zed receipt

**Depends on:** P02 + P03 + C01.
Run one named current-stable Zed host against the exact development extension and exact `perllsp`. Retain activation, `.pod` separation, core language journey, process identity, generation freshness, shutdown, limitations, and currentness.

### P12 — #8714: settings behavior receipts

**Depends on:** P04 + P11.
Execute all four settings roles on one common host subject and prove typed consumption, reversible precedence, no secret/binary leakage, and actual live/restart behavior.

### P13 — #8733: defaults and publication-order receipts

**Depends on:** P05 + P11.
Execute the complete defaults/provider matrix and derive exactly one validated result:

```text
zed_defaults_first_safe
extension_first_required
coordinated_release_required
```

### P14 — #8772: managed-route and recovery receipts

**Depends on:** P07 + P10 + P11.
From an empty managed state, prove exact public artifact selection, core journey, restart reuse, disable/shutdown, known-good preservation, and every required failure row. Explicit, PATH, prior-cache, or another-provider substitution is forbidden for the managed row.

### P15 — #7907: exact-source evidence fan-in

**Depends on:** P10 + P11 + P12 + P13 + P14.
Assemble content-addressed child receipts into one exact-source submission authority. Preserve legitimate route and role differences; reject partial, stale, mismatched, substituted, or public-overclaimed children.

## Phase 3 — freeze and submit external packets

### P16 — #7909: upstream Perl-extension packet freeze

**Depends on:** P15.
Refresh `tree-sitter-perl/zed-perl`, reconstruct the smallest current-base LSP diff, bind current API/version/license/evidence/digests, clear only resolved blockers, and emit copy-ready PR material.

### P17 — #7908: Zed-core defaults packet freeze

**Depends on:** P13.
Refresh `zed-industries/zed`, apply the evidence-derived publication order, preserve dormant alternatives unless the ruling says otherwise, and emit copy-ready material.

### M01 — maintainer extension/defaults submission

**Depends on:** P16 + P17.
The maintainer submits external extension and defaults work in the validated order. Codex performs no external branch, PR, merge, registry, or release mutation.

### U01 — merged upstream subject acceptance

**Depends on:** M01.
Accept the actual merged `tree-sitter-perl/zed-perl` subject only when the registry packet and submission contract provide a non-empty changed commit, version, and branch, with branch reachability and manifest-version equality validated. A submitted or blocked packet is not acceptance.

### P18 — #7910: official existing-`perl` registry packet freeze

**Depends on:** M01 + U01.
Refresh `zed-industries/extensions`, update only the existing `perl` identity/version/ref to the accepted upstream subject, and emit copy-ready registry material. No registry write occurs here.

### M02 — maintainer registry submission and released defaults

**Depends on:** P18.
The maintainer submits the registry update and waits for the required defaults and extension state to be present in a released supported Zed build. Merged-but-unreleased state cannot satisfy public proof.

## Phase 4 — public proof and support projection

### P19 — #7912: official-registry public receipt

**Depends on:** P08 + P10 + P13 + P14 + C01 + M02.
Run a clean ordinary registry installation. Journey A proves distribution and default behavior. Journey B proves explicit public `perllsp` managed behavior. Bind registry, upstream, defaults, release, binary, platform, profile, fixture, settings, driver, method, activation, cache, shutdown, limitation, and currentness cells.

### P20 — #8000: apply public evidence to #7122 and generated docs

**Depends on:** P09 + P19.
Run the checked projection and promote only exact cells earned by P19. Preserve three provider identities, managed/PATH separation, `.pod` separation, unsupported/not-proven cells, platform boundaries, and no DAP claim. A second generation run must be clean.

### P21 — #7759: core programme closeout

**Depends on:** P20.
Reconcile the programme ledger and current support map, mark superseded branches and packets historical, remove stale current-facing instructions, verify every core child state, retain invalidation and unsupported boundaries, and close #7759. DAP may remain open.

## Core dependency graph

```text
P00 #7975/#8023
  ├─ P01 #7980/#8365 ── P06 #8661 ──────────────┐
  ├─ P02 #7984/#8369 ── P03 #8647 ──────────────┤
  │      ├─ P04 #7990 ───────────────────────────┤
  │      ├─ P05 #7992 ───────────────────────────┤
  │      ├─ P07 #8753                            │
  │      └─ P08 #9467                            │
  ├─ P09 #9468                                   │
  └─ C01 #9483 ──────────────────────────────────┤
                                                  ├─ P10 #8678
                                                  └─ P11 #8695
                                                        ├─ P12 #8714
                                                        ├─ P13 #8733
                                                        └─ P14 #8772
                                                               │
P10 + P11 + P12 + P13 + P14 ─────────────── P15 #7907
                                                   ├─ P16 #7909 ─┐
P13 ───────────────────────────────────────────────└─ P17 #7908 ─┴─ M01
                                                                     │
                                                                     U01
                                                                     │
                                                                     P18 #7910 ─ M02
                                                                                   │
P08 + P10 + P13 + P14 + C01 + M02 ─────────────────────────────── P19 #7912
                                                                                   │
P09 ────────────────────────────────────────────────────────────── P20 #8000
                                                                                   │
                                                                           P21 #7759
```

Machine-checked fan-in includes `P01 -> P07`, `P07 -> P14`, `P11 -> P12`, `P11 -> P13`, `P11 -> P14`, `M01 -> U01`, and `U01 -> P18`.

# Non-blocking Zed DAP sidecar

Controller: #9484. None of these stages is part of `public_support_requires` for #7759.

### D01 — #9485: static `perl-dap` adapter authority

**Depends on core:** P00.
**State:** ready now.

Add the smallest current Zed extension API manifest/schema/callback/configuration/binary-resolution authority for exact `perl-dap`. Derive managed target/member identity from canonical release topology; preserve LSP/DAP IDs, executable routes, and cache families; write identity-collapse and wrong-product falsifiers first; keep all behavioral templates `not_run`.

### DA01 — #9516: executable public `perl-dap` asset receipts

**Depends on sidecar:** D01.
Extend or reuse canonical read-only public-asset machinery for `product = perl-dap` without weakening the `perllsp` discriminator. Execute matching-host public adapter binaries, retain cross-built non-execution, prove exact release/target/archive/member/process identity and known-good preservation, and emit per-target plus aggregate receipts.

#7980/#8678 are `perllsp`-only. They cannot satisfy DA01.

### D02 — #9486: exact-source real-Zed DAP receipt

**Depends on:** D01 + core P02/P03.
Run a real pinned Zed development-extension session with exact candidate `perl-dap`: initialize, supported launch or attach, breakpoint request and observed stop, exact frame/source identity, bounded scopes/variables, bounded continue/step transition, termination/disconnect, and adapter/debuggee cleanup.

DA01 is not required for an explicit/PATH exact-source route. D01 is.

### D03 — #9490: upstream DAP extension packet freeze

**Depends on:** D01 + D02.
Refresh `tree-sitter-perl/zed-perl`, reconstruct the smallest current-base DAP patch, bind exact-source evidence and digests, preserve every LSP identity, and emit copy-ready PR material.

### DM01 — maintainer DAP extension submission

**Depends on:** D03.
The maintainer submits and lands the external extension change. Semantic review changes invalidate the tested adapter/configuration subject and return to D01/D02.

### DU01 — merged and released DAP subject acceptance

**Depends on:** DM01.
**Acceptance source:** `.ci/fixtures/zed-perl-upstream/registry/manifest.toml`.

Accept only the actual changed upstream commit/version/branch with branch reachability, manifest-version equality, and released-build containment proven. Submission or merge metadata alone is insufficient.

DU01 is evaluated as a predicate over the named acceptance manifest, not as a prose promise. A DAP subject is accepted only when every one of these holds in that document:

```text
extension.new_commit                        non-empty and != extension.current_commit
extension.new_version                       non-empty and != extension.current_version
extension.upstream_branch_containing_commit non-empty
zed_defaults.released_build                 non-empty
validation.submodule_commit_branch_reachable  true
validation.manifest_version_matches           true
validation.released_build_contains_commit     true
```

The released-build identity is subject-bound: a named non-empty build is required, and `validation.released_build_contains_commit` must tie that build to the accepted `extension.new_commit`. A non-empty build with unproven containment, or proven containment with no named build, is not acceptance.

This is exactly where DU01 and the LSP-side U01 differ. A subject that has merged upstream but has not shipped in a released build satisfies U01 and must still fail DU01. The train test drives both predicates over the same manifest and requires that difference to hold, so neither acceptance can silently collapse into the other.

### D04 — #9491: official existing-`perl` registry packet freeze

**Depends on:** DU01.
Refresh `zed-industries/extensions`, update only the existing `perl` entry to the accepted DAP-capable version/ref, bind current packet evidence, and emit copy-ready material.

### DM02 — maintainer DAP registry submission

**Depends on:** D04.
The maintainer submits the registry change and waits until the DAP-capable existing `perl` extension is ordinarily installable.

### D05 — #9487: official-registry managed real-Zed DAP receipt

**Depends on:** DA01 + D02 + DM02 + core C01.
From a clean profile with no development extension, explicit override, PATH substitution, or prior managed DAP cache, install the ordinary official `perl` extension, prove the exact DA01 public `perl-dap` asset/member/process, and repeat the bounded real debug journey.

Exact-source D02 cannot satisfy D05. Public adapter process evidence DA01 cannot satisfy real Zed behavior.

### D06 — #9489: separate debugger support projection

**Depends on:** D05 + core P09.
Project only exact D02/D05-earned debugger cells into #7122 and generated Zed docs. Keep LSP/DAP rows, exact-source/public stages, managed/PATH routes, platforms, launch/attach states, limitations, and unsupported/not-proven cells independent.

### D07 — #9484: debugger programme closeout

**Depends on:** D06.
Reconcile sidecar state, mark superseded packets historical, preserve currentness and limitations, and close only the debugger programme. D07 never becomes a retroactive prerequisite for #7759.

## DAP sidecar graph

```text
core P02 #7984/#8369 ─┐
core P03 #8647 ───────┤
D01 #9485 ────────────┤
  │                   │
  ├─ DA01 #9516 public perl-dap asset receipts ──────────┐
  │                   │                                  │
  └───────────────────┴─ D02 #9486 exact-source Zed DAP  │
                             │                           │
                             D03 #9490 ─ DM01            │
                             │                           │
                             DU01                        │
                             │                           │
                             D04 #9491 ─ DM02            │
                                                         │
core C01 #9483 ──────────────────────────────────────────┤
DA01 + D02 + DM02 + C01 ───────────────── D05 #9487 ─────┘
                                              │
core P09 #9468 ───────────────────────── D06 #9489
                                              │
                                          D07 #9484
```

D02 is gated by the core exact-source host authority as well as the sidecar: it cannot start before `P02` and `P03`, because a real Zed DAP session needs the exact-source host driver and the deterministic fixture. These are hard dependencies, not advisory ordering.

Machine-checked sidecar fan-in includes `D01 -> DA01`, `D01 -> D02`, `P02 -> D02`, `P03 -> D02`, `DA01 -> D05`, `D02 -> D05`, `DM02 -> D05`, `C01 -> D05`, `D05 -> D06`, and `P09 -> D06`.

## Stop points

Codex stops before each of these external actions:

1. M01 — submit extension/defaults changes.
2. M02 — submit the official registry change.
3. DM01 — submit the DAP extension change.
4. DM02 — submit the DAP registry change.

A prepared packet is not an accepted public subject. A merged subject is not a released subject. A released artifact is not host behavior. A host result is not a support projection until the owning importer validates and applies it.
