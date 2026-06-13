# Release-Readiness Bundle — 2026-06 campaign (DRAFT — held for maintainer cut, do not tag/publish)

**Date**: 2026-06-13
**main HEAD SHA**: `1a42415864fba829d674922035fa1cdc48db558a`
**Workspace version (Cargo.toml)**: `0.16.0`
**Last tag**: `v0.16.0` at `b6d9f12b` (2026-06-06)
**Commits since v0.16.0 tag**: 50

> This document is a staged release-readiness artifact for maintainer review.
> No tag, no publish, no crates.io action until Steven explicitly approves and dispatches.

---

## Shipping — merged PRs by theme (46 PRs total, 2026-06-11 through 2026-06-13)

Each entry is verified `state: MERGED` via `gh pr view` at the time of this writing.

### DAP reliability (6 PRs)

These fix five protocol-surface bugs that produced wrong or confusing behavior in real debug sessions.

| PR | Description |
|----|-------------|
| [#1219](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1219) | Allocate `variablesReference` for structured evaluate results (#1002) — structured hash/array results previously returned 0 |
| [#1227](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1227) | Protocol-safe empty response for invalid `variablesReference` (#901) — was returning an error; now returns `success: true, variables: []` |
| [#1240](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1240) | Protocol-safe errors for execution control without a live session (#898) — gives a guidance message instead of a bare panic-style error |
| [#1246](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1246) | Validate `frameId` in evaluate requests (#902) — invalid frame IDs now return an honest error |
| [#1337](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1337) | Clear `stack_frames` on resume + degraded-path empty `stackTrace` (#964, #966) |
| [#1364](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1364) | Separate session-presence from signal-delivery in `handle_pause` (#1363) — pause with active session but failed signal now returns accurate error |

### LSP editor features (7 PRs)

| PR | Description |
|----|-------------|
| [#1223](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1223) | Resolve method declarations in hover and goto-definition (#854) |
| [#1290](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1290) | Wire `enableSemanticTokens`/`enableFormatting`; remove dead `enableDiagnostics` config key |
| [#1298](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1298) | Hover: phase-block (`BEGIN`/`END`/`INIT`/`CHECK`/`UNITCHECK`) timing semantics |
| [#1300](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1300) | Symbols: index `our`-vars and Moo/Moose `has`-attributes in document/workspace symbols |
| [#1301](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1301) | Signature help: resolve workspace method signatures for `->method()` calls |
| [#1304](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1304) | Rename/refactor: covers dereference and string-interpolation occurrences (#956) |
| [#1314](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1314) | Formatting: scope range-format preserve-gate to the requested range (#1313) |

### LSP stability / infrastructure (5 PRs)

| PR | Description |
|----|-------------|
| [#1206](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1206) | Port URI file-path acceptance + request-shape guidance (#1328) |
| [#1283](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1283) | Add stdio smoke tests for `codeAction` and `inlineCompletion` (#949) |
| [#1288](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1288) | Char-aware word-boundary check in workspace rename (#956) |
| [#1292](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1292) | Fix semantic-analyzer: parse all `qw`/`q`/`qq` delimiter forms in import extractor |
| [#1306](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1306) | Un-ignore incremental completion test; add prefix-narrowing assertion |

### Parser / AST substrate (6 PRs)

| PR | Description |
|----|-------------|
| [#1294](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1294) | Centralize `qw`/`q`/`qq` delimiter parsing into a shared helper |
| [#1295](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1295) | `NodeKindCategory` + `NodeKindFlags` classification API (#911) |
| [#1296](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1296) | Lock print/filehandle/subscript ambiguity parser fixtures |
| [#1322](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1322) | Reserve `MissingStatement`/`MissingIdentifier`/`MissingBlock` NodeKind variants; add drift-guard tests (#915) |
| [#1333](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1333) | Refactor document-symbols: adopt `NodeKindCategory` drift-guard for declaration filtering |
| [#1345](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1345) | VSCode: extend ReDoS heuristic to catch bounded inner quantifiers (#953) |

### Testing / conformance substrate (3 PRs)

| PR | Description |
|----|-------------|
| [#1321](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1321) | Paired-delimiter conformance matrix (lexer, parser-core, dap) (#1320) |
| [#1324](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1324) | Conformance matrix for balanced-segment consumption (lexer, parser-core) |
| [#1332](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1332) | Update `execute_command` count assertion 16→17 for `perl.explainProviderDecision` |

### CI / tooling rails (8 PRs)

| PR | Description |
|----|-------------|
| [#1260](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1260) | xtask: validate orchestration ledgers (#1257) |
| [#1264](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1264) | xtask: add canonical close-proof verifier (#1257) |
| [#1307](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1307) | CI: fix merge-gate base ref `origin/main` (was `origin/master`) |
| [#1310](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1310) | CI: sweep remaining `origin/master` refs to `origin/main` |
| [#1327](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1327) | xtask: strip inline `#[cfg(test)]` blocks from patch-coverage LCOV (#1326) |
| [#1329](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1329) | CI: bump RIPR_VERSION pin from 0.5.0 to 0.9.0 (#1289) |
| [#1336](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1336) | xtask: handle ripr 0.9.x `grip_class`/`seam.file` format in gate evidence parser |
| [#1349](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1349) | xtask: apply path suppression for unrecognized ripr classifications (#1346) |

### Docs, learnings, and spec systems (11 PRs)

| PR | Description |
|----|-------------|
| [#967](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/967) | Establish Issue Discovery / Bug Scout Desk lane + scout-wave report (#942) |
| [#1318](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1318) | Add autonomous-campaign guardrails to agent defs (#1316) |
| [#1319](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1319) | Add parser contract index (#1317) |
| [#1340](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1340) | Front-load hazard-class invariants into spec system (#1339) |
| [#1344](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1344) | Establish two-layer learnings (portable doctrine + repo-specific LEARNINGS.md) |
| [#1348](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1348) | Add subsystem hazard-default templates (#1347) |
| [#1389](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1389) | Backfill session incidents + Gate-7 scribe behavior (#1388) |
| [#1391](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1391) | Lock in spec system — template + spec-builder workflow + spec-planner routing |
| [#1412](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1412) | Encode meta-orchestration learnings — substrate-model, model-cost framing |
| [#1420](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1420) | Agentic-maintenance field notes — article-grade narrative of 2026-06 campaign |
| [#1433](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1433) | Encode tagged-range-codec band-overflow + type-level-id-space learnings |

---

## DAP smoke evidence

Source: PR [#1434](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1434) — `docs/project/status/dap-smoke-receipt.md` (OPEN, pending merge; the evidence was gathered against `main` @ `88bd66a4`).

**7-scenario verdict (all PASS, 0 gaps):**

| # | Scenario | Result |
|---|----------|--------|
| 1 | Resume clears stack frames | PASS |
| 2 | Degraded `stackTrace` path does not return stale data | PASS |
| 3 | Structured `evaluate` results expand via `variablesReference > 0` | PASS (soft — CI exercises `success: false` path; unit tests cover `is_expandable`) |
| 4 | Invalid `variablesReference` → `success: true, variables: []` | PASS |
| 5 | Invalid `frameId` → honest error | PASS |
| 6 | Execution-control without session → guidance error | PASS |
| 7 | Pause with signal failure + active session → accurate error | PASS |

Test run evidence: `cargo test -p perl-dap` — 1798 passed. Pre-existing non-scenario failures noted in the receipt: 2 evaluate-message-wording mismatches, 1 stack-parser edge case, 11 Windows path canonicalization failures (issue [#1435](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1435)). None touch the 7 protocol-surface scenarios.

Note: PR [#1434](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1434), which commits `dap-smoke-receipt.md` to the repo, is still OPEN at the time of this writing. The committed file will land when #1434 merges.

---

## Quality gates (CI required checks on main)

Three required checks guard branch protection. Status reflects the current HEAD SHA `1a42415864fb`.

| Required check | Description | Status |
|---------------|-------------|--------|
| `Perl LSP Rust Small Result` | Core Rust test suite | All 45 campaign PRs passed CI at merge time |
| `ripr+ New Gap Gate` | RIPR 0.9.0 seam-coverage gate (bumped in #1329) | #1336 and #1349 fixed the 0.9.x parsing regressions |
| `Codecov / Patch 95` | Patch line coverage ≥ 95% | Systemic false-low issue — see decision (c) below |

> Maintainer action: verify all three are green on `1a42415864fb` before any release cut. Labels on merged PRs are snapshots at merge time; a fresh CI run on HEAD is the authoritative signal.

---

## Deferred and not-shipping

These are explicitly excluded from the current release scope. Each is either blocked on a decision, in active pipeline, or held by design.

| Item | State | Reason |
|------|-------|--------|
| [#1430](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1430) `refactor(dap): type-separate variablesReference spaces` (var_ref capstone) | OPEN — `deep-reviewed` only | Band-overflow hazard fix is correct and reviewed, not yet merged. Ships in a follow-up patch once ops merges it. |
| [#1434](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1434) DAP smoke receipt doc | OPEN — no merge labels | Verification artifact; evidence is real but committed file is not on `main` yet. |
| [#1311](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1311) `feat(inlay-hints): parameter-name hints for ->method() calls` | OPEN — `in-review, deep-reviewed` | In review pipeline; not merge-ready. Hold for next release. |
| [#1419](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1419) Draft CHANGELOG `[Unreleased]` block | OPEN | Staged draft; needs version number substitution before merge. |
| [#991](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/991) DAP Debugger Trust: transport framing + lifecycle matrix | OPEN — no review labels | Needs maintainer answer on trust model (#903). Held. |
| [#1297](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1297) Validate NodeKind `safe_for_breakpoint`/`introduces_scope` flags | OPEN issue | Parked pending Phase 7/8 DAP consumption answers. |
| Codex test-coverage cluster (#642, #651, #669, #676, #678, #682, #686) | OPEN — `codex` label | Singleton codex PRs, in normal pipeline. Not release-blocking. |
| Open dep bumps (#1136, #1201, #1252, #1271, #1273, #1274, #1275, #1334) | OPEN — chore/deps | Routine dependency updates. None blocking. |

---

## Known issues filed (post-campaign)

These were discovered during the 2026-06 campaign and filed as follow-up issues. None block the current release.

| Issue | Title | Severity |
|-------|-------|----------|
| [#1431](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1431) | `worktree_allocator` lease-expiry tests are date-bombs (hardcoded 2026-06 dates will expire) | Low — test infra only |
| [#1432](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1432) | Migrate 6 `variablesReference` consumers to `VariableReference` codec (follow-up to #1430) | Medium — correctness improvement, not regression |
| [#1435](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1435) | ~11 `perl-dap` tests fail locally on Windows (path canonicalization); CI is green | Low — Windows-local only, CI unaffected |

---

## Maintainer decisions required

These are the high-leverage items that need a signal from Steven before the release proceeds or the pipeline can adapt.

### (a) Release cut decision — HELD

The `v0.16.0` tag exists (cut 2026-06-06). The campaign merged 45 PRs with user-facing improvements on top of it. The next release would be `v0.16.1` (patch — bug fixes, no new APIs) or `v0.17.0` (minor — new features: signature help, hover phase-blocks, Moo/Moose symbol indexing, method-declaration goto-definition).

The CHANGELOG `[Unreleased]` draft in PR [#1419](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/1419) documents the full user-facing scope for review.

**Decision needed**: Which version number? Should #1430 (var_ref capstone, deep-reviewed but not merged) and #1311 (inlay hints, in review) wait for the release or be included first?

### (b) Strict-up-to-date vs merge-queue policy

High-velocity `main` + `strict-up-to-date` + slow CI serializes what should be parallel work. Observed this campaign: every merge moves all open PRs behind the new HEAD; GitHub's `auto-merge` will not self-update branches; agents must manually rebase each PR in sequence. At 3–5 PRs/hour merge throughput, a queue of 10 PRs can take 3+ hours to clear.

**Options**: (1) Switch to GitHub merge queue — auto-updates branches on queue entry, (2) relax strict-up-to-date to allow N-commits-behind, (3) keep as-is and accept the serialization cost.

### (c) Codecov patch-95 false-low — systemic (issue #1282)

The patch coverage measurement (`coverage_filters = ["workspace-lib"]`) counts only `--lib` profdata. Integration tests in `tests/` run and exercise changed lines but their coverage is not counted toward patch %. PRs whose fixes are exercised exclusively by integration tests show false-low patch coverage and require inline `#[cfg(test)]` workarounds to pass the gate. This was observed on #1430 (var_ref codec) and #1311 (inlay hints) during this campaign.

The issue is a CI measurement problem, not a real coverage gap. The fix is including integration profdata in the patch-coverage LCOV merge (modify `scripts/ci/route-codecov-packs.py` and `xtask/src/tasks/ci_route.rs` per issue [#1282](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1282)). Until fixed, builders must add inline unit tests as a workaround, which inflates lib code and adds maintenance burden.

**Decision needed**: Prioritize #1282 as a near-term CI fix, or continue accepting inline-test workarounds?

### (d) Two flagged pipeline changes awaiting sign-off

These were identified during the campaign but require maintainer approval before becoming mandatory:

1. **xtask hazard-row → test validator**: Agents wrote hazard rows in `acceptance.md` but no automated check enforces that hazard rows map to actual test coverage. A validator in `cargo xtask` would catch spec-test mismatches before build starts.

2. **Mandatory `spec-test-code-match` routing**: Adding a pipeline gate that verifies each spec claim has a corresponding test before the builder is dispatched. This tightens the Red-TDD → Build contract.

Both are process improvements, not correctness fixes. They would affect every future PR in the pipeline.

---

## Explicit non-goal

This document does NOT authorize or initiate a release. No tag, no `crates.io` publish, no VS Code Marketplace upload, no Docker image, and no GitHub Release will be created until Steven explicitly approves and dispatches the release workflow. The `[Unreleased]` block in CHANGELOG (PR #1419) is a staged draft only.

---

*Generated 2026-06-13 by builder agent from primary sources: `git log`, `gh pr list`, `gh pr view`, `gh issue view`. All PR merge states verified at time of writing.*
