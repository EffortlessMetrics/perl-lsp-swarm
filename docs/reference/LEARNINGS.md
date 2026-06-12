# Repo-Specific Learnings Index

**Purpose.** A greppable, keyword-rich index of real incidents that happened in this
repository. Future agents: grep for the exact symbol, error string, PR number, or
hazard class you are investigating. Each entry links to the relevant portable doctrine
pattern in docs/doctrine/ and to the relevant spec contract.

For the portable, repo-agnostic patterns behind these incidents, see docs/doctrine/.
For the cross-implementation contracts these incidents violate or establish, see
docs/reference/PARSER_CONTRACTS.md and docs/agents/SPEC_UPDATE_CHECKLIST.md.

---

## Incident Index

| # | What | Hazard class | PR/issue | Portable pattern |
|---|------|-------------|----------|------------------|
| L-1 | ripr 0.5.0->0.9.0 output-schema rename | Coverage/measurement integrity | #1329, #1336 | hazard-class-invariants.md |
| L-2 | variablesReference base 50_000 collision | ID/reference-space collision | #1219 | hazard-class-invariants.md |
| L-3 | LCOV brace scanner literal-blind | Scanner literal/comment blindness | #1327, #1326 | hazard-class-invariants.md |
| L-4 | pre-existing test asserted defect as expected | Test encodes the bug | #1337 | hazard-class-invariants.md |
| L-5 | #1309 multi-agent tangle, re-created as #1337 | Multi-agent tangle | #1309, #1337 | re-create-over-untangle.md |
| L-6 | Codecov Patch-95 cancelled under concurrent merges | CI cancellation cascade | convergence wave | serialize-merges-and-cancellation.md |
| L-7 | Codecov false-low: --lib profdata only | Coverage/measurement integrity | #1282 | hazard-class-invariants.md |
| L-8 | Shift-left validated: 0-fix deep-review after hazard front-load | Shift-left pattern | #1246, #1340 | shift-left-ladder.md |

---

## L-1: ripr 0.5.0 -> 0.9.0 Output-Schema Rename (#1329, #1336)

**What happened**: ripr 0.9.x renamed per-finding JSON fields compared to 0.5.x:
- classification -> grip_class (values: weakly_exposed -> weakly_gripped)
- probe.file -> seam.file

The xtask gate evidence parser in xtask/src/tasks/ripr_evidence.rs only read the
0.5.x field names. Under 0.9.x, all findings were silently skipped during suppression
matching: suppressed_by_policy stayed 0, path-based suppressions in
policy/ripr-suppressions.toml never fired, producing false-positive ripr+ New Gap
Gate failures on PRs whose gaps were covered by existing policy entries.

The gate was over-strict, not neutered (gross counts still came from ripr own summary
output, which 0.9.x computes correctly in its summary section).

**Why it happened**: The version bump PR (#1329) merged without diffing the tool output
schema between versions. The schema break was silent -- no parse error, just silently-empty
suppression matches.

**The fix** (PR #1336): ripr_pr_summary_counts now reads both classification (0.5.x)
and grip_class (0.9.x) via .or_else. ripr_finding_path tries finding[probe][file]
(0.5.x) then finding[seam][file] (0.9.x). Two new unit tests prove suppression fires
correctly under 0.9.x AND that the gate retains teeth for unsuppressed gaps.

**The checklist item**: Before bumping any tool version, diff the tool OUTPUT SCHEMA
(the JSON/text it emits) between old and new versions. A behavioral change in the tool
does not require code changes; a schema change in the tool output ALWAYS requires
code changes in every consumer of that output.

**Greppable keywords**: ripr_evidence.rs, grip_class, seam.file, weakly_gripped,
suppressed_by_policy, RIPR_VERSION, ripr 0.9.0, ripr 0.5.0, ripr_pr_summary_counts,
ripr_finding_path

**Portable pattern**: docs/doctrine/hazard-class-invariants.md (Class 6: Coverage/
Measurement Integrity)

---

## L-2: variablesReference Base 50_000 ID Collision (#1219)

**What happened**: PR #1219 allocated a new variablesReference range starting at
50_000 for expandable DAP evaluate results. Existing scope references used the formula
frame_id * 10 + scope_type. For a frame_id of 5_000, the scope ref is exactly
50_000 -- a direct collision. The debugger would fetch the wrong container when the
client tried to expand the evaluate result.

**Why it happened**: The new range was chosen without documenting or checking existing
range allocations. The collision is latent (requires a specific frame_id value) and
not caught by happy-path tests using small frame IDs.

**The fix**: Bump the base to 1_000_000 (provably beyond the frame_id*10+scope_type
range for any realistic frame count). Name the constant; document the rationale.

**The checklist item**: Any change that introduces a new pool of numeric IDs must
prove disjointness from all existing ranges. The proof must be a named constant with
a documented rationale. An adversarial test asserts IDs from different pools never
collide.

**Greppable keywords**: variablesReference, variables_reference, frame_id*10,
scope_type, EvaluateResult, VariableCacheKind, allocate_evaluate_result_ref,
50_000, 1_000_000, dap_evaluate_comprehensive_tests

**Portable pattern**: docs/doctrine/hazard-class-invariants.md (Class 1: ID/Reference-
Space Collision); docs/doctrine/shift-left-ladder.md

---

## L-3: LCOV Brace Scanner Literal-Blind (#1327, #1326)

**What happened**: The patch-coverage LCOV post-processor in xtask/src/tasks/
quality_baseline.rs used cfg_test_line_numbers to scan source text for
#[cfg(test)]-gated scopes by counting brace depth. The scanner did not skip braces
inside string literals, character literals, or block comments. A source file containing
a brace character inside a literal within a cfg(test) block could cause the scanner
to miscalculate the block boundary and exclude production lines from LCOV.

**Why it happened**: The scanner was correct for the common case (no braces in
literals). The adversarial case was not tested.

**The fix** (PR #1327): The brace scanner was replaced with a state machine that
tracks whether the current character is inside a string literal, character literal,
or block comment. Braces in those states are ignored.

**The checklist item**: Any scanner that counts or matches delimiter characters must
be tested with inputs where the delimiter appears exclusively inside a string literal,
a character literal, and a block comment.

**Greppable keywords**: cfg_test_line_numbers, strip_cfg_test_lines, LcovSummary,
quality_baseline.rs, #[cfg(test)], brace scanner, LCOV filter, DA:, line_hit,
line_found, #1326, #1327, literal-blind

**Portable pattern**: docs/doctrine/hazard-class-invariants.md (Class 4: Scanner
Literal/Comment Blindness; Class 6: Coverage/Measurement Integrity)

---

## L-4: Pre-Existing Test Asserted Defect as Expected Behavior (#1337)

**What happened**: PR #1337 fixed the stale-stack-frames bug (#964): stack frames
were never cleared when the debugger resumed. A pre-existing test
test_stack_trace_uses_recent_output_when_available asserted frames.len() >= 2 --
testing the now-removed snapshot-buffer parsing behavior that was exactly the bug.
The fix changed frames.len() to 0 in the degraded path, causing the old test to
fail with the correct code in place.

**Why it happened**: The test was written to characterize existing behavior, not correct
behavior. When the existing behavior IS the bug, the test encodes the bug. This is
common when a bug produces a plausible-looking output (e.g., "returns 1 frame" when
the correct answer is "returns 0 frames").

**The fix**: Update the test to assert the correct post-fix behavior: frames.len() == 0.
Mark in the commit message: "was testing the bug."

**The checklist item**: When a bug fix changes a test expected value, read the old
assertion and articulate what it was testing. If the old assertion tested incorrect
behavior, mark the test as "was testing the bug."

**Greppable keywords**: test_stack_trace_uses_recent_output_when_available,
test_stack_trace_returns_empty_without_live_session, stack_frames_stale_resume_tests,
frames.len(), snapshot_buffer, handle_stack_trace, frames.rs, #964, #933,
test-encodes-bug

**Portable pattern**: docs/doctrine/hazard-class-invariants.md (Class 5: Test Encodes
the Bug); docs/doctrine/shift-left-ladder.md

---

## L-5: Multi-Agent Branch Tangle -- #1309 Re-Created as #1337

**What happened**: Branch claude/admiring-volta-uotucs (PR #1309) accreted commits
from multiple agents over multiple rounds: a DAP stack-frame clear (fix for #964),
an xtask ripr evidence parser fix (for the 0.9.x format change, L-1 above), an
extracted gate-parser fix, and a stray xtask command. The ripr+ gate failed due to
the schema mismatch, which caused suppression entries to be added that then collided
with parallel PRs editing policy/ripr-suppressions.toml.

The correct fix: extract the xtask fix into PR #1336 (one concern, touching only
xtask/src/tasks/ripr_evidence.rs) and re-create the DAP fix as PR #1337 (from spec,
touching only DAP files and one policy entry).

**Why it happened**: Multiple agents worked on the same branch without a clean handoff.
Each agent added what it needed; no agent owned the cumulative diff.

**The fix**: Re-create each concern as a standalone PR from its own spec. Close the
tangled PR with pointers to the replacements.

**Greppable keywords**: claude/admiring-volta-uotucs, #1309, #1337, #1336, #1216,
#1325, multi-agent tangle, ripr-suppression collision, ripr-suppress-dap-stack-frame-lifecycle

**Portable pattern**: docs/doctrine/re-create-over-untangle.md;
docs/doctrine/serialize-merges-and-cancellation.md

---

## L-6: Codecov Patch-95 Cancelled Under Concurrent Merges (Convergence Wave)

**What happened**: During the 2026-06 convergence campaign, multiple PRs were rebased
and their update-branches pushed within short windows of each other. The Codecov upload
step (which runs after the main CI job) requires several minutes to complete. Concurrent
pushes caused the CI runner to cancel the still-running Codecov upload for one PR when
another push triggered a new run. Codecov recorded a failure for the cancelled run even
though the coverage data was correct and the local quality gate had passed.

The most-affected PR was #1206, whose Codecov step was cancelled repeatedly by
concurrent activity on adjacent PRs.

**Why it happened**: The Codecov upload step is long-running and does not hold a CI slot
after the main build completes. Concurrent pushes trigger CI run cancellation at the
runner level, killing the still-running upload step.

**The fix**: Serialize merges: one CI cycle completes before the next merge is attempted.

**Greppable keywords**: Codecov upload cancelled, Codecov Patch 95 failure, #1206,
INPUT_TOKEN, concurrent rebase, merge queue cancellation, upload-coverage step,
coverage-proof-routed

**Portable pattern**: docs/doctrine/serialize-merges-and-cancellation.md

---

## L-7: Codecov False-Low: --lib Profdata Only (#1282)

**What happened**: The Codecov / Patch 95 gate ran cargo llvm-cov with
coverage_filters = ["workspace-lib"], which counts only --lib profdata. Integration
tests in crates/*/tests/ run and exercise changed lines, but their coverage profdata
was not included in the patch-coverage measurement. PRs whose fixes were genuinely
covered by integration tests showed false-low patch coverage (< 95%) even though the
lines were hit dozens of times by integration suites. Affected: #1223 (method-decl
hover), #1238 (workspace rename) -- both had 10+ LCOV hits but measured under 95%.

A companion failure mode (same session, different root cause): PR #1321 (test-only
conformance matrix) failed Codecov / Patch 95 because the gate measured coverage of
the TEST LINES the PR added -- a dead branch inside an inline #[cfg(test)] block.
This is addressed by #1327 (L-3 above).

**The fix**: Use inline #[cfg(test)] lib tests (counted by --lib profdata) to cover
changed lines. Do NOT use LCOV_EXCL_* padding. See also #1263 and #1282 for the
longer-term fix (include --tests profdata in patch-coverage LCOV merge).

**Greppable keywords**: workspace-lib, --lib profdata, patch coverage, LCOV,
coverage_filters, integration test undercounted, Codecov Patch 95, #1282, #1263,
coverage-proof-routed, quality_baseline.rs, cargo llvm-cov, false-low coverage

**Portable pattern**: docs/doctrine/hazard-class-invariants.md (Class 6: Coverage/
Measurement Integrity)

---

## L-8: Shift-Left Validated -- 0-Fix Deep-Review After Hazard Front-Load (#1246, #1340)

**What happened**: PR #1340 added hazard-class invariants to the spec system (six classes
in docs/agents/SPEC_UPDATE_CHECKLIST.md section 8, plus instructions to spec-planner,
red-tdd, and architecture-reviewer to enumerate and test applicable classes before the
builder starts).

The first PR built under this regime was #1246 (DAP frameId validation). The spec
included explicit acceptance rows for bounds, protocol-safety, and stale-after-resume.
Red-tdd wrote adversarial tests for each. Deep-review (sonnet) found zero correctness
gaps: "found no correctness gaps beyond what the tests already covered; shift-left was
effective."

**Contrast with same session, pre-invariant PRs**: Deep-review DISCOVERED real bugs on:
- #1219 (ID/ref-space collision, L-2): fix required.
- #1337 (test-encodes-bug, L-4): fix required.
- #1327 (scanner literal-blind, L-3): fix required.

**Why it worked**: Front-loading hazard invariants into the spec means the builder tests
are designed to catch adversarial cases before implementation, not just verify the happy
path. Deep-review becomes a confirmation net rather than a discovery pass.

**Greppable keywords**: shift-left, hazard-class invariants, SPEC_UPDATE_CHECKLIST.md,
#1246, #1340, 0-fix deep-review, adversarial tests, acceptance.md,
test_evaluate_with_out_of_range_frameid_no_panic,
test_evaluate_stopped_session_frame_not_found_returns_error,
test_evaluate_stale_frameid_after_resume_rejected

**Portable pattern**: docs/doctrine/shift-left-ladder.md;
docs/doctrine/hazard-class-invariants.md

---

## Cross-References

**From docs/reference/PARSER_CONTRACTS.md**: See L-3 (scanner literal/comment blindness)
when a new scanner consumer is added to the contract index. New scanners must pass the
adversarial literal-blind test pattern before being added as a canonical consumer.

**From docs/agents/SPEC_UPDATE_CHECKLIST.md section 8**: The six hazard classes in
section 8 correspond to the six classes in docs/doctrine/hazard-class-invariants.md.
For repo-specific incidents that motivate each class, see this document:
- Class 1 (ID/ref collision) -> L-2
- Class 3 (Protocol-safety) -> L-2, L-8
- Class 4 (Scanner blindness) -> L-3
- Class 5 (Test encodes bug) -> L-4
- Class 6 (Coverage integrity) -> L-1, L-7
