# Coverage and RIPR Enforcement Baseline

> Human-owned baseline for the coverage / proof / enforcement lane.
> This document records the current measurement/control-plane state.
> Refresh local receipts with the commands below before changing gate policy.

## Claim Boundary

- This lane owns repo-wide proof enforcement, not LSP 3.18 behavior.
- LSP 3.18 work consumes this lane only when a coverage or RIPR receipt names an uncovered behavior or repair seam.
- Transitional advisory mode is allowed only while baseline debt is being measured and burned down.
- Final policy target remains blocking: ripr+ zero and Codecov project/patch coverage at or above 95%.

## Baseline Commands

```bash
rtk cargo test --workspace
rtk cargo llvm-cov nextest --workspace --lcov --output-path target/lcov.info
rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt target/receipts/quality/coverage-baseline.json
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check
rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD
rtk cargo xtask quality-gate --mode advisory --codecov codecov.yml
rtk git status --short --branch
rtk git diff --check
rtk bash scripts/storage-doctor
```

Receipt paths under `target/` are generated proof artifacts and are not committed by default.

## Current Policy Snapshot

| Surface | Current state | Enforcement now | Final target |
| --- | --- | --- | --- |
| Codecov project status | `codecov.yml` target `95%`, threshold `2%`, informational | Burn-down signal only | `95%`, threshold `0.25%`, blocking |
| Codecov patch status | `codecov.yml` target `95%`, threshold `0%`; workflow check `Codecov / Patch 95` | Blocking when Codecov runs on PRs | `95%`, threshold `0%` |
| Local parser branch ratchet | `.ci/coverage-baseline.txt`: branch `50.00%`, line `92.11%`, target branch `80.00%`, allowed drop `1.00%` | Enforced by `rtk just coverage-branch-gate` in the coverage lane | Replaced or complemented by repo-wide 95% Codecov policy |
| Coverage proof LCOV | `rtk just coverage-proof-lcov` writes `lcov.info` for Codecov and `quality-gate`, including parser code plus `xtask` proof-rail code, then uploads with `parser,xtask` Codecov flags | Feeds patch coverage and receipt scope | Workspace-scoped LCOV for final project enforcement |
| RIPR PR evidence | `.github/workflows/ripr.yml` runs `CI / ripr+ New Gap Gate`; local `ripr-plus` and `ripr-pr` receipts feed `quality-gate --mode enforce-new-ripr` | New gaps and receipt freshness blocking in CI | Total zero blocking after burn-down |
| RIPR+ badge | `badges/ripr-plus.json` currently reports `unavailable` | Advisory badge only | Receipt-backed pass/fail status |
| Temporary quality exceptions | `policy/quality-gate-exceptions.toml` names `ripr-total-burndown` and `project-coverage-burndown` | Dated transition ledger only | Removed before total RIPR+ and project coverage become blocking |

Live Codecov badge read on 2026-05-26 reported `65%`. Treat that as a point-in-time external signal; the reproducible repo-local receipt is `target/receipts/quality/coverage-baseline.json`.

## Baseline Receipt Fields

`rtk cargo xtask coverage-baseline` records:

- local LCOV branch and line coverage from `target/lcov.info`
- a positive LCOV `LF` line count; empty or non-measuring LCOV snapshots are
  rejected instead of being treated as `100%`
- the top below-target LCOV files in `files_below_target`, including
  representative positive, 1-based uncovered line samples from LCOV `DA`
  records
- the LCOV source-file scope in `coverage_scope`, including detected workspace
  member roots, required roots, and any missing required roots. Final project
  enforcement requires every Cargo workspace member root plus the `xtask` proof
  rail, so parser-only or parser-plus-xtask LCOV cannot satisfy the repo-wide
  coverage gate. LCOV `SF` paths are normalized from absolute Windows or Linux
  runner paths back to repo-relative paths before scope and file-gap guidance
  are computed.
- LCOV `DA` entries with line `0` are rejected before a baseline receipt is
  written, because they cannot name an actionable source line
- at least one valid `files_below_target` row when local line coverage is below
  `95%`; valid rows must include positive `sample_uncovered_lines` so coverage
  burn-down failures name concrete lines to prove, not only a file-level repair
  target. Non-positive samples are removed from aggregate gate output.
- local coverage-ratchet policy from `.ci/coverage-baseline.txt`
- Codecov project and patch policy from `codecov.yml`
- advisory next actions when local line coverage is below `95%`

`rtk cargo xtask ripr-plus` records:

- repo-wide RIPR seam count from `ripr check --format repo-seams-json`
- an explicit `unresolved` count; the aggregate gate rejects RIPR+ receipts
  that omit the count even when schema and head fields are present
- at least one actionable `sample_seams` row under `top_actionable_files` or
  `top_files` when `unresolved` is greater than zero, so failures can name the
  gap id, file, line, seam, reason, and suggested test instead of only a file
  count
- seam counts grouped by kind
- raw top seam-heavy files in `top_files`
- the highest-count production burn-down candidates in `top_actionable_files`;
  these rows require at least one actionable sample with gap id, positive line,
  seam, reason, and suggested test
- representative `sample_seams` for top file clusters with gap ids, line
  numbers, seam names, reasons, and suggested tests
- archive, generated-looking, or missing-actionable-sample clusters in
  `deferred_files`
- advisory next actions for the highest-count actionable seam clusters

The actionable/deferred split is classification only. It does not suppress or
waive any RIPR+ seam; final `quality-gate --mode enforce` still requires total
RIPR+ unresolved seams to reach zero. A `missing_actionable_sample` deferred
row means the receipt did not have enough gap detail to route a focused repair;
it is not an exception.

`rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD` records:

- diff-scoped RIPR severe gaps from `target/ripr/pr/pr.diff`
- resolved `base_sha` and `head_sha` so stale PR proof fails locally; the
  aggregate gate validates both when the recorded base ref resolves
- changed-file count and severe-gap class counts used by the new-gap gate

`rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD` records:

- resolved `base_sha` and `head_sha` for the review-guidance receipt; stale
  base or head proof is treated like missing guidance
- file, line, seam, gap id, reason, and suggested proof rows when RIPR can
  place guidance on exact seams
- an explicit error receipt when guidance generation times out or fails, so CI
  still has a current proof artifact to summarize

`ripr-plus` remains the repo-wide seam inventory. `quality-gate` copies the diff-scoped severe-gap count into `ripr_plus.new_unresolved` for the aggregate receipt, but the authoritative new-gap receipt is `target/ripr/pr/repo-exposure.json`.

`rtk cargo xtask quality-gate --mode advisory` records:

- schema/kind, freshness, and measured-line status for the RIPR+ and coverage
  receipts
- schema/kind and freshness status for the diff-scoped RIPR PR receipt
- RIPR total and new-gap counts when present
- coverage project and patch values when present; patch coverage can come from
  the coverage receipt or from `--patch-coverage <percent>`
- coverage source scope from the coverage receipt; final enforce mode blocks
  partial or unknown scope so local receipts prove the production crates and
  the proof rail, not only a parser slice
- live Codecov policy parse status in `coverage.codecov_config_status`; pass
  `--codecov codecov.yml` in CI so the evaluated policy file is explicit and
  authoritative over any policy snapshot stored in an older coverage receipt.
  Missing or invalid Codecov policy is reported as a separate repair action
- Codecov PR comment guidance from `codecov.yml`; patch enforcement requires
  comment layout entries for `diff` and `files` plus `require_head = true`
- Codecov project policy from `codecov.yml`; final enforcement requires target
  `95%`, threshold `0.25%`, `if_ci_failed = error`, and no informational mode
- active temporary exceptions from `policy/quality-gate-exceptions.toml`
- next repair actions for missing, stale, or below-target proof
- a markdown summary at `target/receipts/quality/quality-gate.md` for the
  GitHub step summary
- a `Quality Gates` table with pass/fail/external/unknown status, current
  value, target, and blocking posture for RIPR+ zero, new RIPR+ gaps, RIPR+
  receipt freshness, RIPR PR receipt freshness, RIPR review-guidance freshness,
  Codecov patch coverage, Codecov config, Codecov patch policy, Codecov failure
  guidance, Codecov project policy, coverage scope, and Codecov project coverage
- a `PR Summary Guidance` block that tells agents what proof fields to include
  in the PR body: objective, claim boundary, non-goals, RIPR/coverage effect,
  local proof commands, cleanup performed, and what remains
- suggested local proof commands for the active gate mode, including the
  receipt checks and `rtk git diff --check`

`quality-gate --mode enforce-new-ripr` is present as a local and CI contract check. It blocks missing, stale, wrong-schema, wrong-kind, unknown, or non-zero diff-scoped RIPR PR proof. Diff-scoped proof must include `base`, `base_sha`, and `head_sha`; when Git can resolve the recorded base ref, the aggregate gate rejects receipts whose `base_sha` no longer matches. It also blocks a missing, stale, wrong-schema, wrong-kind, missing-count, or non-actionable repo-wide `target/receipts/quality/ripr-plus.json` receipt so the existing total-debt baseline is current and includes actionable sample seam details when total debt is non-zero. Because this transition mode still allows existing repo-wide RIPR+ debt, it also requires a present and valid `policy/quality-gate-exceptions.toml`. `target/ripr/review/comments.json` is also a required RIPR receipt; if it is missing, stale, invalid, or incomplete, the gate emits a `ripr_review_receipt_not_current` blocker with review-guidance verify and receipt commands. When current review guidance is present, the quality-gate receipt and markdown summary include the top changed file, line, seam, gap id, reason, and suggested proof. When new gaps exist but actionable guidance is incomplete, the blocking `new_ripr_gap` action names the guidance status instead of silently omitting the seam details.

When `quality-gate` renders repo-wide RIPR repair actions, it filters
`top_actionable_files` and fallback `top_files` to rows with actionable sample
seams only. Mixed or incomplete raw receipt samples remain available under
`raw_top_files` on final blockers, but they are not shown as repair targets.

`quality-gate --mode enforce-patch-coverage` is present as the transitional coverage gate. It blocks missing, stale, wrong-schema, wrong-kind, zero-line, non-actionable, or malformed local coverage receipts and missing or invalid Codecov policy from `--codecov codecov.yml`. It also blocks Codecov patch policies that are not configured as blocking `95%` target / `0%` threshold. The live Codecov config is read directly by `quality-gate` and is authoritative over any policy snapshot embedded in a coverage receipt; the coverage receipt still carries the measured LCOV values and below-target files. The gate also blocks Codecov comment configuration that omits `diff` or `files` guidance, or that does not require head coverage, because patch failures must tell agents where the proof gap is. Because this transition mode still allows project coverage burn-down, it also requires a present and valid `policy/quality-gate-exceptions.toml`. Patch coverage must be represented explicitly: pass `--patch-coverage <percent>` when the local or CI job knows the numeric value, or pass `--patch-status-source codecov` when the required Codecov patch status is the external blocking source. If neither is present, the gate fails with `patch_coverage_unknown`. When `--patch-coverage` is used, the aggregate receipt records `coverage.patch_source = "cli"` and blocks values below `95%`; when `--patch-status-source codecov` is used, it records `coverage.patch_source = "codecov_status"` instead of pretending a local number was measured. When the local receipt has actionable below-target files, patch failures include those files plus representative uncovered line samples and behavior-test guidance for error paths, boundaries, config parsing, serialization, cancellation, provider decisions, and output contracts. Non-actionable file rows are filtered from the aggregate receipt instead of being rendered as repair targets. When patch coverage is below target but the local LCOV receipt has no actionable below-target project files, the blocker explicitly points agents to the Codecov patch `diff`/`files` report for the changed uncovered lines.

`quality-gate --mode enforce` is the final target mode. It does not become
required until RIPR+ and project coverage burn-down are complete. Its blocking
actions include the same repair contract as the transitional gates: a serialized
`blocking: true` marker, blocker kind, top actionable RIPR files when available,
raw/deferred RIPR evidence, coverage files, representative seam samples, repair
guidance, verify command, and receipt command. In final mode, blocker verify
commands rerun the aggregate `quality-gate --mode enforce --check`; component
refresh commands stay in the receipt field so agents know exactly which proof
artifact to regenerate. It also
requires the Codecov project status policy to be promoted from informational
burn-down to blocking `95%` target with `0.25%` threshold. Final enforce also
fails when the coverage receipt is partial or unknown instead of workspace
scope; that blocker names the missing required workspace member roots so agents
know whether they need full workspace LCOV or only a stale receipt refresh. If a
receipt still claims `workspace` using an older member list, the blocker also
prints the current required roots and the newly missing roots from live
`Cargo.toml`. Once RIPR+ zero and project coverage are at target, the exception
ledger must be removed or emptied instead of carried as stale transition debt.

`policy/quality-gate-exceptions.toml` is the burn-down exception ledger for the
two non-final rails:

- `ripr-total-burndown`: existing repo-wide RIPR+ debt while new gaps are
  already blocking.
- `project-coverage-burndown`: project coverage below the final `95%` target
  while patch coverage is already blocking.

These entries are intentionally dated with `review_after`, `expires`, and
removal criteria. They document why advisory burn-down is still allowed; they do
not suppress `quality-gate --mode enforce` blockers. `quality-gate` requires
these two named entries plus their metric, aggregate-gate, and policy evidence
paths, so transition debt cannot be hidden by renaming or dropping the exception
while the gate still runs in partial-enforcement mode. In final enforce mode,
active entries become a blocker because the burn-down exception must be retired.
While entries remain, the ledger `status` must be `active`; a `retired` or
inactive ledger with live entries is invalid because it hides active transition
debt.
Dates must use
`YYYY-MM-DD`, and the aggregate gate rejects exception entries where
`review_after` is earlier than the ledger `updated` date or `expires` is earlier
than `review_after`. Entries whose `review_after` date is already past must be
re-justified with fresh evidence and dates; expired entries also invalidate the
transition gate instead of becoming silent waivers.

### Durable Policy Contract

The transition policy is configurable only where the proof rail needs explicit,
dated burn-down mechanics:

- coverage target and project threshold values while project coverage is still
  below target;
- excluded generated or legacy files, when the exclusion reason is documented;
- suppressed RIPR gaps, allowed advisory classes, and temporary burn-down
  exceptions with owner, evidence, review date, expiry date, and removal
  criteria.

The following are not configurable without a policy PR that changes this
contract directly:

- RIPR+ zero as the final target;
- Codecov patch coverage enforcement;
- Codecov project coverage enforcement after burn-down;
- the RIPR, coverage, and aggregate `quality-gate` receipt requirements.

## Initial Distance To Target

| Target | Current measured rail | Gap |
| --- | --- | --- |
| Patch coverage `>=95%` | Codecov patch policy is `95%` with `0%` threshold; PR check name is `Codecov / Patch 95` | Keep blocking while improving failure summaries |
| Project coverage `>=95%` | Live badge point-in-time value was `65%`; Codecov project policy is informational during burn-down; local LCOV receipt must be regenerated for the branch under review | Burn down high-risk uncovered behavior before enforcing |
| New RIPR+ gaps `0` | `quality-gate --mode enforce-new-ripr` blocks non-zero diff-scoped severe gaps and stale RIPR+ receipts locally and in the unfiltered `.github/workflows/ripr.yml` PR workflow | Keep check required while existing debt burns down |
| Total RIPR+ unresolved `0` | Local `ripr-plus` receipt on `e735c7057a3ab11d956cba1ce14737d3f72443fc` reported `133298` unresolved seams | Burn down or explicitly classify existing seam clusters, then enforce zero |

## RIPR+ Baseline Snapshot

Generated locally on 2026-05-26:

```bash
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json
```

Receipt summary:

- head: `e735c7057a3ab11d956cba1ce14737d3f72443fc`
- unresolved seams: `133298`
- receipt path: `target/receipts/quality/ripr-plus.json` (generated; not committed)

Top seam kinds:

| Kind | Count |
| --- | ---: |
| `call_presence` | 64065 |
| `return_value` | 17906 |
| `field_construction` | 16618 |
| `predicate_boundary` | 15980 |
| `match_arm` | 13698 |
| `side_effect` | 3773 |
| `error_variant` | 1258 |

Top seam-heavy files:

| File | Count |
| --- | ---: |
| `archive/crates/tree-sitter-perl-rs/src/pure_rust_parser.rs` | 2261 |
| `crates/perl-parser-pest/src/pure_rust_parser.rs` | 2261 |
| `crates/perl-semantic-analyzer/src/analysis/symbol.rs` | 2138 |
| `crates/perl-workspace/src/workspace/workspace_index.rs` | 2067 |
| `crates/perl-lexer/src/lib.rs` | 1848 |
| `crates/perl-ci-hygiene/src/main.rs` | 1572 |
| `archive/crates/perl-ts-heredoc-parser/src/perl_lexer.rs` | 1448 |
| `crates/perl-parser-core/src/hir/lower.rs` | 1416 |
| `crates/perl-semantic-analyzer/src/analysis/type_inference.rs` | 1376 |
| `crates/perl-lsp-rs-core/src/tooling/perl_critic/native.rs` | 1337 |

The receipt keeps that raw list as evidence, then separates burn-down guidance
into actionable and deferred clusters. Archived files are deferred so the next
repair PRs start with maintained production crates; they still remain part of
the total RIPR+ unresolved count until the final zero gate is satisfied.

## Existing Checks

| Check | Command or workflow | Required vs advisory |
| --- | --- | --- |
| Local parser branch coverage | `rtk just coverage-branch-gate` | Enforced inside coverage lane |
| Codecov status | `codecov.yml` + Codecov upload from `.github/workflows/ci-nightly.yml` | Patch required at `95%`; project informational during burn-down |
| Coverage proof LCOV | `rtk just coverage-proof-lcov` | Generates `lcov.info` for Codecov and the coverage receipt with parser plus proof-rail scope |
| Coverage CI receipt | Local equivalent: `rtk cargo xtask coverage-baseline --lcov lcov.info`; CI runs the same xtask subcommand after proof LCOV generation | Uploaded as `target/receipts/quality/coverage-baseline.json` proof |
| Coverage quality gate | Local equivalent: `rtk cargo xtask quality-gate --mode enforce-patch-coverage --codecov codecov.yml --patch-status-source codecov`; CI runs the same xtask subcommand | Required coverage receipt plus live `codecov.yml` policy proof; patch percentage is explicitly delegated to the required Codecov status; required proof artifacts are checked before upload |
| RIPR PR evidence | `.github/workflows/ripr.yml` runs on every PR, without path filters | Required for new-gap proof |
| RIPR proof artifacts | `.github/workflows/ripr.yml` checks diff evidence, review guidance, impacted evidence, RIPR+ receipt, and quality-gate receipts before upload | Required for new-gap proof |
| RIPR+ baseline receipt | `rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json` | Required freshness for new-gap proof; total zero still burn-down |
| RIPR+ check receipt | `rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check` | Required freshness for new-gap proof; total zero still burn-down |
| RIPR PR receipt | `rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD --check` | Required by new-gap proof |
| Coverage baseline receipt | `rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt target/receipts/quality/coverage-baseline.json` | Advisory |
| Aggregated quality gate | `rtk cargo xtask quality-gate --mode advisory --codecov codecov.yml` | Advisory; markdown summary includes PR proof guidance and exact local commands |
| Temporary exceptions ledger | `policy/quality-gate-exceptions.toml` | Advisory transition ledger; no final-gate waiver |

## Known Baseline Gaps

- Full workspace LCOV generation did not complete in the local Windows checkout after 20 minutes, so `target/receipts/quality/coverage-baseline.json` could not be produced from the requested workspace command in this pass.
- The narrower parser LCOV attempt ran `163` parser tests successfully, but report export failed on Windows with `os error 206` because the generated `llvm-cov export` command line was too long.
- `rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json` generated the current repo-wide RIPR+ receipt; `rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD --check` validates diff-scoped new-gap proof once generated; `rtk cargo xtask coverage-baseline --check` becomes meaningful once `target/lcov.info` exists.

## Next Split

1. Burn down existing RIPR+ seam clusters with focused tests and receipts, starting with `top_actionable_files` from the RIPR+ receipt rather than generated or archived surfaces.
2. Raise project coverage to `95%`, then enforce project status.
3. If maintainers want a PR comment instead of only workflow step summaries,
   wire `target/receipts/quality/*quality-gate.md` into the existing PR summary
   posting path.
