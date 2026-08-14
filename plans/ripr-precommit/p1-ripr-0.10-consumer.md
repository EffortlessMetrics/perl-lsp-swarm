# P1 — RIPR 0.10 consumer migration

Issue: #9113  
Parent: #9112  
Branch: `agent/ripr-010-consumer`  
Base at creation: `4d95b37d9eb70efe9165d3a077a23b2a38a4eaa3`

## End goal

Move every reviewed perl-lsp-swarm RIPR consumer from 0.9.0 to the published 0.10.0 release while preserving the existing PR-CI enforcement claim exactly, and lock the consumer against another silent output-schema drift. This PR is deliberately version-only: it establishes a trustworthy 0.10 baseline before any staged-tree/precommit behavior is introduced. The dedicated `.github/workflows/ripr.yml` lane remains required throughout this PR. A later PR owns changing proof placement.

## Codex implementation order

1. Inventory every current RIPR version authority and runtime install surface. At minimum inspect `.github/workflows/ripr.yml`, `.github/workflows/badge-endpoints.yml`, `docs/ci/ripr.md`, `xtask/tests/badge_ripr_version_contract.rs`, `xtask/src/tasks/ripr_evidence.rs`, and the workflow-contract tests around the new-gap gate.
2. Diff real `ripr 0.9.0 check --format json` and `ripr 0.10.0 check --format json` outputs through deterministic fixtures. Do not infer schema compatibility from version numbers or hand-authored JSON alone.
3. Update all reviewed execution surfaces to exactly 0.10.0. Keep the existing workflow-driven version authority for this PR; P5/#9117 owns moving that authority when the workflow is retired.
4. Harden the consumer parser only where real 0.10 output requires it. Required fields that disappear or become malformed must produce an instrument/contract failure, never an implicit zero-gap result.
5. Exercise suppression matching with positive and negative controls using real 0.10 output. Confirm path/classification suppressions still select only the intended finding and an unsuppressed control finding remains visible to the existing gate.
6. Update docs and fixtures that intentionally name the reviewed producer release. Do not rewrite historical release notes or benchmark names where `0.9.0` refers to perl-lsp itself rather than RIPR.
7. Run the repository-prescribed formatting, focused xtask tests, workflow policy checks, and affected Rust checks before requesting review.

## Required real-output cases

- one unsuppressed actionable finding;
- one path-suppressed finding;
- one classification-suppressed finding;
- no findings from a genuinely analyzed diff;
- changed test/evidence surface;
- deleted/base-side finding reproduction currently contained by `HeadLineExtents`;
- malformed/truncated JSON negative control.

## Files likely to change

```text
.github/workflows/ripr.yml
.github/workflows/badge-endpoints.yml
docs/ci/ripr.md
xtask/tests/badge_ripr_version_contract.rs
xtask/tests/ripr_new_gap_gate_workflow.rs        # only if version assertions live here
xtask/src/tasks/ripr_evidence.rs                 # only if real 0.10 schema requires it
focused fixtures/goldens for real producer output
this plan file / Changie fragment if repository policy requires one
```

## Guardrails

- No staged-tree sandbox or precommit RIPR logic in P1.
- No removal of `.github/workflows/ripr.yml`.
- No change to `ripr+ New Gap Gate` required-check semantics.
- No removal of `HeadLineExtents` or current suppressions.
- No 0.11 work.
- No generic external-tool registry.
- Do not mass-replace every textual `0.9.0`; distinguish RIPR-version references from perl-lsp release/history references.

## Acceptance before merge

- all reviewed RIPR runtime consumers execute 0.10.0;
- real 0.10 output is parsed by the repository consumer in tests;
- missing/malformed required fields cannot become clean;
- suppression-positive and unsuppressed-negative controls pass;
- badge and PR RIPR workflows cannot drift to different versions;
- the existing required new-gap gate retains teeth;
- the PR contains no precommit-placement change.

## Suggested review map

Review version changes first, then real-output fixtures, then parser changes, then workflow-contract tests. Any parser broadening should be justified by a captured 0.10 producer shape. Treat a permissive fallback without a real producer example as a regression risk.
