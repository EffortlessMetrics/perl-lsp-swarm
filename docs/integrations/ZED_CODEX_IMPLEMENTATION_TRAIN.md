# Zed Codex PR-by-PR implementation train

> **Core LSP programme:** #7759  
> **Current public claim:** planned / not proven  
> **Core CI control:** #9483  
> **Non-blocking DAP sidecar:** #9484  
> **External writes:** manual maintainer actions only

This is the execution authority for moving Zed from the current repository-owned candidate to exact-source evidence, upstream publication, official-registry proof, and checked support projection. The DAP rail is recorded here because it uses the same external extension and distribution surface, but it is explicitly **not** a prerequisite for closing the Zed LSP programme.

## Product identities

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp

DAP adapter ID       -> perl-dap
DAP executable       -> perl-dap
```

The identities are not aliases. Every EffortlessMetrics LSP process receipt proves exact `perllsp --stdio`. Every debugger receipt proves exact `perl-dap`. Neither process may satisfy the other surface.

## Train rules

1. One PR owns one semantic increment. Branch from current `main` after required predecessors merge.
2. Existing branches are evidence, not authority. Reconstruct the unique correct increment instead of preserving stale stacked history.
3. Write falsifiers first. A schema, workflow, template, packet, or build is not behavioral evidence.
4. Receipt PRs bind exact host/source/artifact/configuration/instrument identities and retain limitations/currentness.
5. Missing, stale, skipped, cancelled, timed-out, malformed, cross-SHA, or instrument-failed evidence is never pass.
6. Use `Advances #...` until the issue's behavioral acceptance is actually complete; use `Closes #...` only when that PR earns closure.
7. Every PR runs focused proof, formatting, applicable policy checks, Changie where required, and `git diff --check`.
8. External extension, Zed-core, registry, and release mutations are maintainer stop points. Codex prepares packets and stops.
9. Public docs are generated from or checked against #7122. Prose cannot outrun the receipt.
10. Zed DAP remains a separate support row and does not block #7759.

# Core LSP train

## Phase 0 — converge the static substrate

### P00 — #7975 / PR #8023: static convergence landed

The accepted unique Zed increment is now present on current `main`, including the truthful #7898 boundary and the static extension/settings/defaults/managed/packet contracts. The authoritative convergence manifest records `static_substrate_complete_execution_not_proven`; the static contracts do not establish host or public behavior.

**Current Codex frontier:** P01, P02, and C01 may now proceed from the landed P00 substrate. Keep their existing owners and do not treat the landed static substrate as execution evidence.

## Phase 1 — implementation authorities

### P01 — #7980 / PR #8365: public-asset producer

**Depends on:** P00.  
Build the read-only public release asset producer/validator: exact release/target/member identity, safe extraction, matching-host execution, cross-built non-execution, process cleanup, and offline falsifiers. Authority only.

### P02 — #7984 / PR #8369: exact-source host driver

**Depends on:** P00.  
Build the operator-assisted real-Zed prepare/launch/finalize path with exact subject binding, isolated profile, supported Zed surfaces only, exact process inventory, and shared receipt validation. Authority only.

### P03 — #8647: deterministic fixture and expectation contract

**Depends on:** P02.  
Create the shared fixture for activation, `.pod` separation, diagnostics/repair, completion/navigation, edit/refusal, UTF-16, LF/CRLF, custom token cases, freshness, and project settings. Fixture authority only.

### P04 — #7990 / PR #8373: settings behavior authority

**Depends on:** P02 + P03.  
Define the checked `project_only`, `zed_override`, `zed_override_removed`, and `live_edit` experiment and validator. Receipt remains `not_run`.

### P05 — #7992 / PR #8379: default-order authority

**Depends on:** P02 + P03.  
Define the four-row defaults/extension/provider-selection matrix and deterministic publication-order ruling. No ruling is earned until host evidence executes.

### P06 — #8661: public-asset workflow

**Depends on:** P01.  
Add the pinned read-only Windows/Linux/macOS matrix invoking P01 and uploading exact per-runner receipts. Workflow definition is not execution evidence.

### P07 — #8753: managed-route/cache-recovery authority

**Depends on:** P01 + P02.  
Define managed first-mile/restart/disable behavior, known-good recovery scenarios, shared validator, bounded host seam, and `not_run` template.

### P08 — #9467: official-registry host-driver authority

**Depends on:** P02 + P03.  
Define prepare/launch/finalize authority for:

```text
Journey A: official existing `perl` registry installation + quiet released defaults
Journey B: explicit public `perllsp` selection + managed route
```

Require content-addressed published subjects and reject development/fork/local/PATH/prior-cache substitutions. #7912 remains the later execution owner.

### P09 — #9468: fail-closed public-support projection substrate

**Depends on:** P00 + current #7122 substrate.  
Add the importer/generator authority that can later consume #7912 while rendering only planned/not-proven state beforehand. Reject exact-source-as-public, cross-platform promotion, managed/PATH collapse, unobserved methods, `.pod` as Perl, DAP leakage, stale receipts, and generated-doc drift. #8000 remains the later application owner.

### C01 — #9483: stable Zed checks and subject-aware evidence invalidation

**Depends on:** P00.  
Replace whole-workflow absent-green semantics with canonical semantic-subject selection, current-run selection receipts, stable final contexts, exact receipt invalidation, and fail-closed aggregation. Preserve `cancel-in-progress: false` for expensive host work. This control plane proves no Zed behavior itself.

C01 may land while P01–P09 are being rebuilt. P10/P11 and the public receipt cannot execute as current programme evidence until C01 is available.

## Phase 2 — execute internal evidence

### P10 — #8678: public-asset matrix receipts

**Depends on:** P01 + P06 + C01.  
Run the real matrix, validate every target, and commit immutable per-target plus aggregate receipts. Narrow any unsupported target explicitly rather than manufacturing a pass.

### P11 — #8695: exact-source core Zed receipt

**Depends on:** P02 + P03 + C01.  
Run one named current-stable Zed host against the exact development extension and exact `perllsp`; retain activation, core journey, currentness, `.pod` separation, shutdown, process, and limitation evidence.

### P12 — #8714: settings behavior receipts

**Depends on:** P04 + P11.  
Execute all four settings roles on one common host subject and prove typed consumption, reversible precedence, no binary leakage, and actual live/restart behavior.

### P13 — #8733: defaults/order receipts

**Depends on:** P05 + P11.  
Execute the full defaults/provider matrix and derive exactly one validated publication-order result:

```text
zed_defaults_first_safe
extension_first_required
coordinated_release_required
```

### P14 — #8772: managed-route and recovery receipts

**Depends on:** P07 + P10 + P11.  
From an empty managed state, prove exact public artifact selection, core journey, restart reuse, disable/shutdown, and every required recovery row. Explicit/PATH/prior-cache/other-provider substitutions are forbidden for the managed row.

### P15 — #7907: exact-source evidence fan-in

**Depends on:** P10–P14.  
Assemble content-addressed child receipts into one validated exact-source authority/submission input. Preserve legitimate route/role differences and reject partial, stale, mismatched, or public-overclaimed children.

## Phase 3 — freeze external packets

### P16 — #7909: freeze the upstream Perl-extension packet

**Depends on:** P15.  
Refresh `tree-sitter-perl/zed-perl`, reconstruct the smallest current-base LSP diff, bind version/API/license/evidence/digests, clear only resolved blockers, and emit copy-ready PR material.

### P17 — #7908: freeze the Zed-core defaults packet

**Depends on:** P13.  
Refresh `zed-industries/zed`, apply the evidence-derived publication order, retain Perl Navigator as default and alternatives dormant unless the ruling says otherwise, and emit copy-ready material.

### M01 — maintainer external submission

**Depends on:** P16 + P17.  
The maintainer manually submits extension/defaults work in the validated order. Codex performs no external branch/PR/merge/release mutation.

### U01 — merged upstream subject acceptance

**Depends on:** M01.
This is an explicit acceptance stage, not metadata attached to P18. It accepts the
actual merged `tree-sitter-perl/zed-perl` subject only when the registry packet and
[registry submission contract](ZED_REGISTRY_SUBMISSION.md) provide a non-empty
branch-reachable commit, manifest version, and upstream branch, with matching
validation. A blocked packet remains parseable evidence, but it cannot complete U01.

### P18 — #7910: freeze the official existing-`perl` registry packet

**Depends on:** M01 + U01.
Refresh `zed-industries/extensions`, update only the existing `perl` identity/version/ref, bind merged upstream identity, and emit copy-ready registry material. P18 is not accepted from M01 alone: U01 must independently accept the authoritative registry packet at `.ci/fixtures/zed-perl-upstream/registry/manifest.toml` and [the registry submission contract](ZED_REGISTRY_SUBMISSION.md). That packet must contain a non-empty merged `tree-sitter-perl/zed-perl` commit, manifest version, and upstream branch, with branch reachability and manifest/version equality validated. The new commit and version must differ from the captured registry subject. This remains a planned/not-proven packet boundary; it performs no registry write.

### M02 — maintainer registry submission + released defaults

**Depends on:** P18.  
The maintainer manually submits the registry update and waits until the required defaults exist in a released Zed build. Merged-but-unreleased state cannot satisfy public proof.

## Phase 4 — official public proof and projection

### P19 — #7912: official-registry public receipt

**Depends on:** P08 + P10 + P13 + P14 + C01 + M02.  
Run a clean official-registry installation. Journey A proves distribution/default behavior; Journey B proves explicit public `perllsp` managed behavior. Bind registry/upstream/defaults/release/binary/platform/profile/fixture/settings/driver/method/activation/cache/shutdown/limitation cells.

### P20 — #8000: apply public evidence to #7122 and generated docs

**Depends on:** P09 + P19.  
Run the checked projection and promote only exact cells earned by P19. Preserve three provider identities, managed/PATH separation, `.pod` separation, unsupported/not-proven cells, and **no DAP claim**. A second generation run must be clean.

### P21 — #7759: core LSP programme closeout

**Depends on:** P20.  
Reconcile the programme ledger/current support map, mark superseded branches/packets historical, remove stale current-facing instructions, verify every core child state, retain invalidation/unsupported rules, and close #7759. DAP may still be open.

## Core merge graph

```text
P00 #7975/#8023
  ├─ P01 #7980/#8365 ── P06 #8661 ─┐
  ├─ P02 #7984/#8369 ── P03 #8647 ├─ C01 #9483 ─┐
  │      ├─ P04 #7990 ──────────────┤             ├─ P10 #8678
  │      ├─ P05 #7992 ──────────────┤             └─ P11 #8695
  │      ├─ P07 #8753               │                    ├─ P12 #8714
  │      └─ P08 #9467               │                    ├─ P13 #8733
  └─ P09 #9468                      │                    └─ P14 #8772
                                                           │
P10 + P11 + P12 + P13 + P14 ── P15 #7907
                                  ├─ P16 #7909 ─┐
P13 ──────────────────────────────└─ P17 #7908 ─┴─ M01
                                                   │
                                                   U01 (merged upstream acceptance)
                                                   │
                                                   P18 #7910 ─ M02
                                                                │
P08 + P10 + P13 + P14 + C01 + M02 ─────────────── P19 #7912
                                                                │
P09 ────────────────────────────────────────────── P20 #8000
                                                                │
                                                        P21 #7759
```

The prose graph retains the fixture's required fan-in edges, including `P01 -> P07`,
`P07 -> P14`, `P11 -> P12`, `P11 -> P13`, `P11 -> P14`, `M01 -> U01`, and
`U01 -> P18`. These edges are
dependency constraints, not ownership transfers: P07 remains the managed-route
authority, P11 remains the exact-source receipt owner, and P14 remains the
managed-route/recovery receipt owner.

# Non-blocking Zed DAP sidecar

Controller: #9484. This rail may start when its dependencies are available, but **none of D01–D07 is part of `public_support_requires` for #7759**.

### D01 — #9485: static `perl-dap` debug-adapter authority

**Depends on core:** P00 + P01.  
Add the exact Zed debug-adapter manifest/schema/API/binary-resolution contract for `perl-dap`, preserving every LSP identity. Static authority only.

### D02 — #9486: exact-source real-Zed DAP receipt

**Depends on:** D01; reuse P02/P03 host/fixture mechanics where appropriate.  
Run a real pinned Zed development-extension session: initialize, launch, breakpoint, observed stop, exact frame/source identity, bounded scopes/variables, continue/step, termination, and adapter/debuggee cleanup.

### D03 — #9490: freeze the DAP upstream extension packet

**Depends on:** D01 + D02.  
Refresh `tree-sitter-perl/zed-perl`, bind the smallest current-base DAP patch and exact-source evidence, and emit copy-ready material.

### DM01 — maintainer DAP extension submission

**Depends on:** D03.  
Maintainer manually submits/lands the DAP extension change. Any semantic review change returns to D01/D02.

### D04 — #9491: freeze the official registry packet

**Depends on:** DM01 and actual merged/released extension identity.  
Refresh `zed-industries/extensions`, update only the existing `perl` entry to the exact merged DAP extension version, and emit copy-ready material.

### DM02 — maintainer DAP registry submission

**Depends on:** D04.  
Maintainer manually submits the registry update and waits until the DAP-capable extension is ordinarily installable.

### D05 — #9487: official-registry managed DAP receipt

**Depends on:** D01 + D02 + DM02 + core C01 routing.  
From a clean profile and empty managed DAP cache, install the official `perl` extension, prove exact public `perl-dap` asset/member/process identity, and repeat the bounded real debug journey.

### D06 — #9489: separate DAP support projection

**Depends on:** D05 + generic #7122 projection substrate.  
Promote only exact debugger cells, keeping static/exact-source/public stages, managed/PATH routes, platforms, LSP/DAP identities, and unsupported cells distinct.

### D07 — #9484: DAP programme closeout

**Depends on:** D06.  
Reconcile the sidecar and close only the debugger programme. This never reopens or retroactively gates a completed #7759 LSP closeout unless a shared-product regression independently invalidates LSP evidence.

## DAP sidecar graph

```text
P00 + P01 ─ D01 #9485 ─ D02 #9486 ─ D03 #9490 ─ DM01
                                                   │
                                                   D04 #9491 ─ DM02
                                                                  │
C01 ──────────────────────────────────────────────── D05 #9487
                                                                  │
P09/#7122 substrate ───────────────────────────────── D06 #9489
                                                                  │
                                                           D07 #9484
```

# Codex execution prompt

Use this with the named train car:

```text
Implement only train stage <ID> for issue #<ISSUE> in EffortlessMetrics/perl-lsp-swarm. Re-fetch current main, the owning issue/comments, existing PRs/branches, and every named external read-only subject before planning. Preserve product identity and evidence-stage boundaries. Treat stale branches as evidence, reconstruct the narrowest current-base increment, write falsifying tests first, run focused and repository policy checks, add required Changie/generated artifacts, review the final diff against non-goals, and open a draft PR. Say Advances unless the issue's acceptance is actually complete. Do not perform external submission, release, registry mutation, support promotion beyond the owned receipt, or destructive cleanup.
```

# Core closure invariant

#7759 is complete only when the following are simultaneously true:

```text
truthful current docs
coherent current-main extension candidate
stable subject-aware Zed CI/final checks
executable public asset evidence
real exact-source Zed LSP evidence
settings/defaults rulings
managed cache/recovery evidence
copy-ready current upstream/default/registry packets
manual external publication complete
official-registry clean public LSP receipt
exact #7122/documentation projection
unsupported/not-proven cells still visible
```

DAP is intentionally absent from that invariant and remains governed by #9484.
