# Zed Codex PR-by-PR implementation train

> **Programme:** #7759
>
> **Current public claim:** planned / not proven
>
> **External writes:** manual maintainer actions only

This document is the execution order for moving the repository-owned Zed work from its current static and harness state to a proven official-registry integration. It separates implementation authority, actual evidence, packet freeze, external publication, public proof, and support projection. Codex must not substitute one stage for another.

## Product identities

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp
```

The IDs are not aliases. Every `perllsp` process receipt must prove exact `perllsp --stdio`; another product, MCP, socket mode, utility mode, or fallback provider cannot satisfy it.

## Current repair gate

PR #8023 is the convergence surface, but its current head predates substantial mainline movement and fails both general CI and the dedicated Zed rustfmt ratchet. The failing dedicated step runs:

```text
rustfmt --edition 2024 --check xtask/tests/zed_*.rs xtask/tests/support/zed_host_compat.rs
```

Do not merge descendants first. Reconstruct or rebase the unique #8023 increment onto current `main`, run `cargo fmt --all`, prove the focused Zed contracts, and ensure the semantic diff contains no unrelated mainline history. Then rebuild each child authority PR from the accepted mainline subject.

## Train rules

1. One PR owns one semantic increment. A controller issue may have multiple explicit phases only where an existing issue already owns both infrastructure and later execution; the PR body must state which phase it advances.
2. Branch from current `main` after required dependencies merge. Do not keep merge-piling sibling or stale train branches merely to preserve history.
3. Write falsifiers first. Every contract PR includes negative controls for the exact false-green states it prevents.
4. A schema, template, driver, workflow, or build is not evidence. Checked templates remain `not_run`.
5. A receipt PR commits exact subjects, normalized results, content digests, bounded redacted artifacts, and limitations. It never edits a failing row into a pass.
6. Use `Advances #...` until the issue's behavioral acceptance is actually complete. Use `Closes #...` only in the evidence or freeze PR that earns closure.
7. Every PR includes a changie fragment, focused tests, `cargo fmt --all -- --check`, applicable file/process/network/workflow policy checks, and `git diff --check`.
8. External extension, Zed-core, registry, or release actions are stop points. Codex prepares packets and then stops.
9. Public documentation is generated or checked from the public support registry. Prose cannot outrun the receipt.
10. DAP remains outside the Zed claim.

## Phase 0 — repair and converge the static substrate

### PR 00 — finish #8023 / #7975

**Goal:** one current-main static substrate.

**Required work:**

- rebuild the unique 16-file #8023 increment on current `main`;
- format every Zed Rust contract test with the pinned Rust 1.95 formatter;
- retain settings, managed-asset, blocked-submission, registry, defaults, and receipt authorities exactly once;
- keep `not_run` and `not_proven` boundaries intact;
- remove stale branch history and any temporary repair machinery;
- rerun the dedicated Zed workflow and required repository checks.

**Merge gate:** green incremental diff; no host/public claim.

## Phase 1 — land implementation authorities

After PR 00 merges, rebuild these PRs onto current `main`. PRs 01 and 02 may proceed in parallel.

### PR 01 — rebuild #8365 / #7980 public-asset producer

**Goal:** read-only public release asset receipt producer and validator.

**Boundary:** authority only; no committed executable receipts.

**Merge gate:** exact release drift handling, safe archive extraction, matching-host process lifecycle, explicit cross-built non-execution, offline falsifiers, green policies.

### PR 02 — rebuild #8369 / #7984 exact-source host driver

**Goal:** operator-assisted real-Zed prepare/launch/finalize path using supported Zed surfaces.

**Boundary:** authority only; exact-source template remains `not_run`.

**Merge gate:** exact subject binding, isolated profile, no internal Zed state surgery, exact process inventory, shared receipt validation.

### PR 03 — #8647 deterministic fixture and expectation contract

**Depends on:** PR 02.

**Goal:** one repository-owned fixture reused by exact-source, settings, defaults, managed, and public journeys.

**Required coverage:** activation families, `.pod` separation, diagnostic/repair, completion and navigation, edit/refusal, UTF-16, LF/CRLF, custom SQL/JSON tokens, freshness, and project settings.

**Boundary:** fixture authority only; no Zed result.

### PR 04 — rebuild #8373 / #7990 settings behavior authority

**Depends on:** PR 02; consume PR 03 fixture/schema identity where appropriate.

**Goal:** checked four-role settings experiment and validator.

**Boundary:** experiment authority only; receipt remains `not_run`.

### PR 05 — rebuild #8379 / #7992 default-order authority

**Depends on:** PR 02; consume PR 03 fixture identity.

**Goal:** checked four-row defaults/extension matrix, provider-selection cases, and derived publication-order validator.

**Boundary:** matrix authority only; ruling remains unresolved until host evidence.

### PR 06 — #8661 public-asset workflow

**Depends on:** PR 01.

**Goal:** pinned, read-only Windows/Linux/macOS matrix that invokes the asset producer and uploads exact per-runner receipts.

**Boundary:** workflow infrastructure only. A workflow definition is not an executed receipt.

### PR 07 — #8753 managed-route and cache-recovery authority

**Depends on:** PR 01 and PR 02.

**Seed:** inspect `zed-managed-route-7994`, but reconstruct only correct unique work on current `main`.

**Goal:** managed first-mile/restart/disable contract, nine known-good recovery scenarios, shared validator, bounded driver seam, and `not_run` template.

**Boundary:** authority only; exact-source managed evidence remains absent.

### PR 08 — #7912 official-registry host driver phase

**Depends on:** PR 02 and PR 03.

**Goal:** operator-assisted prepare/launch/finalize authority for:

```text
Journey A: official `perl` registry installation + quiet released defaults
Journey B: explicit public `perllsp` selection + managed route
```

It must require a content-addressed `published` public subject and reject development/local/fork/PATH/prior-cache substitutions.

**Boundary:** infrastructure only; public template remains `not_run` and #7912 stays open.

### PR 09 — #8000 fail-closed public support projection substrate

**Depends on:** PR 00 and the current #7122 registry substrate.

**Goal:** generator/validator that can consume a future passing #7912 receipt and project exact Zed cells into #7122 and generated docs.

Before public evidence exists, it must render only planned/not-proven state and reject exact-source-as-public, cross-platform promotion, managed/PATH collapse, unobserved method inheritance, `.pod` as Perl, DAP, stale receipts, and generated-doc drift.

**Boundary:** projection authority only; #8000 stays open.

## Phase 2 — execute and freeze internal evidence

Authority PRs must be merged before their evidence PRs begin. Receipt PRs branch from current `main`, not authority feature branches.

### PR 10 — #8678 public asset matrix receipts

**Depends on:** PR 01 and PR 06.

Run the matrix, fetch artifacts, validate every row, and commit immutable per-target receipts plus one aggregate. Close #7980 only if every managed target passes its exact boundary or the target claim is narrowed explicitly.

### PR 11 — #8695 exact-source core Zed receipt

**Depends on:** PR 02 and PR 03.

Run one named current-stable Zed host against the exact development extension and exact `perllsp`. Commit required core journey, activation, process, shutdown, and `.pod` separation evidence. Close #7984 only at the stated boundary.

### PR 12 — #8714 settings behavior receipts

**Depends on:** PR 04 and PR 11.

Execute `project_only`, `zed_override`, `zed_override_removed`, and `live_edit` roles for one common exact host subject. Prove typed consumption, reversible precedence, no binary leakage, and actual live/restart behavior. Close #7990 without promoting the full Zed row.

### PR 13 — #8733 defaults compatibility receipts

**Depends on:** PR 05 and PR 11.

Execute all four defaults/extension rows plus provider selection/failure cases. Derive exactly one:

```text
zed_defaults_first_safe
extension_first_required
coordinated_release_required
```

Project the ruling into the blocked packets. Close #7992 only when the final combination is quiet and the ruling validates.

### PR 14 — #8772 managed-route and recovery receipts

**Depends on:** PR 07, PR 10, and PR 11.

From an empty managed state with explicit/PATH/prior-cache/other-provider routes absent, prove exact public asset selection, core journey, restart reuse, disable/shutdown, and every required known-good recovery row. Close #7994 at the exact-source managed boundary only.

### PR 15 — #7907 exact-source submission authority

**Depends on:** PRs 10–14.

Assemble content-addressed child receipts into one validated authority and deterministic submission input. Preserve legitimate route/role differences; reject unexplained subject drift, partial children, and public overclaim. Close #7907 only at the exact-source boundary.

## Phase 3 — freeze external submission packets

These remain repository PRs. They create no external branches or PRs.

### PR 16 — #7909 freeze the current tree-sitter-perl extension packet

**Depends on:** PR 15.

Refresh `tree-sitter-perl/zed-perl` immediately before freeze. Reconstruct the smallest current-base diff, select the valid next version/API/license/grammar subject, bind evidence and digests, remove every blocked marker, and emit copy-ready PR material. Close #7909 when the packet is ready for the maintainer's manual submission.

### PR 17 — #7908 freeze the current Zed-core defaults packet

**Depends on:** PR 13.

Refresh the exact current `zed-industries/zed` defaults subject, apply the validated publication order, retain Perl Navigator as default and both alternatives dormant, bind patch/tests/digests, and emit copy-ready PR material. Close #7908 when ready for manual submission.

## Manual stop point A

The maintainer manually submits the extension and Zed-core defaults changes in the validated order. Codex must not create branches, issues, PRs, merges, or releases in external repositories.

If upstream changes the implementation, version, target, defaults, or order, return to the narrow affected authority/evidence/freeze stage. Do not absorb external review changes by prose.

### PR 18 — #7910 freeze the official existing-`perl` registry update

**Entry gate:** exact extension upstream commit/version has merged; the validated defaults order is still current.

Refresh `zed-industries/extensions`, update only the existing `perl` entry/submodule identity and matching version, bind merged commit and checks, and emit copy-ready registry material. Close #7910 when ready for manual submission.

## Manual stop point B

The maintainer manually submits the registry update and waits for the required Zed defaults to appear in a released Zed build. No local candidate or merged-but-unreleased default can satisfy public execution.

## Phase 4 — official public proof and projection

### PR 19 — #7912 official-registry public receipt

**Depends on:** PR 08, PR 10, PR 13, PR 14, PR 18, accepted external extension/registry publication, and a released Zed build containing the required defaults.

Run a clean official-registry installation. Journey A proves distribution/default behavior. Journey B proves explicit public `perllsp` managed behavior. Commit exact registry, upstream extension, defaults, release asset, binary, platform, profile, fixture, settings, driver, method, activation, cache/recovery, shutdown, and limitation cells. Close #7912 only when the public-stage validator passes.

### PR 20 — #8000 apply the public receipt to #7122 and generated docs

**Depends on:** PR 09 and PR 19.

Run the checked projection. Promote only exact public cells earned by #7912, retain unsupported/not-proven rows, keep managed and PATH routes separate, preserve three provider identities, `.pod` separation, and no DAP claim. A second generation run must produce no diff. Close #8000.

### PR 21 — #7759 programme closeout

**Depends on:** PR 20 and coherent external/public subjects.

Update the programme ledger and current support map; mark superseded PRs/packets/branches as historical; remove stale current-facing instructions; verify every child state; retain unsupported and invalidation rules; and close #7759. Branch deletion and other destructive cleanup remain an explicit maintainer action after merged history and receipts are confirmed.

## Merge graph

```text
PR00 #8023 convergence
  ├─ PR01 #8365 asset producer
  │    └─ PR06 #8661 asset workflow
  │         └─ PR10 #8678 asset receipts
  └─ PR02 #8369 exact-source driver
       ├─ PR03 #8647 fixture
       │    ├─ PR04 #8373 settings authority ── PR12 #8714 settings receipts
       │    ├─ PR05 #8379 order authority ───── PR13 #8733 order receipts
       │    └─ PR08 #7912 public driver
       └─ PR07 #8753 managed authority ──────── PR14 #8772 managed receipts

PR10 + PR11 + PR12 + PR13 + PR14
  └─ PR15 #7907 exact-source authority
       ├─ PR16 #7909 extension packet
       └─ PR17 #7908 defaults packet
             └─ MANUAL EXTERNAL SUBMISSION
                  └─ PR18 #7910 registry packet
                       └─ MANUAL REGISTRY SUBMISSION + RELEASED DEFAULTS
                            └─ PR19 #7912 public receipt
                                 └─ PR20 #8000 support projection
                                      └─ PR21 #7759 closeout

PR09 #8000 projection substrate may merge early but cannot promote anything before PR19.
```

## Codex execution prompt for each PR

Use this patch with the specific issue:

```text
Work only the named PR stage and its issue. Re-fetch current main and every named external/read-only subject before planning. Preserve product identity and evidence-stage boundaries. Inspect existing branches as evidence, not authority; reconstruct on the narrowest current base. Write falsifying tests first, implement the smallest coherent increment, run focused and policy checks, add a changie fragment, review the final diff against the issue's non-goals, and open a draft PR. Say Advances unless the issue's behavioral acceptance is actually complete. Perform no external submission, release, registry mutation, or destructive cleanup.
```

## Programme closure invariant

The programme is complete only when all of these are simultaneously true:

```text
truthful current docs
coherent mainline candidate
executable public asset evidence
real exact-source Zed evidence
settings and defaults rulings
managed cache/recovery evidence
copy-ready current upstream/default/registry packets
manual external publication complete
official-registry clean public receipt
exact support-registry/documentation projection
unsupported/not-proven cells still visible
```

Anything less remains planned, partial, blocked, or not proven at the exact missing stage.
