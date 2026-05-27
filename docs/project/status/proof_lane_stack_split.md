# Proof Lane Stack Split Plan

> Human-owned branch note for `port/source-pr-9572-swarm-port`.
> Date: 2026-05-27.
> Scope: coverage / RIPR / proof enforcement only.

## Claim Boundary

- This lane owns repo-wide proof enforcement: RIPR+ receipts, coverage
  receipts, patch coverage, project coverage, temporary exceptions, and
  actionable failure output.
- This lane does not own LSP 3.18 protocol behavior, protocol crate extraction,
  release work, parser behavior changes, or unrelated status generation.
- The current dirty stack must not merge as one PR. Split it into reviewable
  slices and land them in dependency order.

## Current Dirty Stack

The branch currently mixes workflow policy, Codecov policy, quality-gate CLI
implementation, exception policy, PR summary guidance, generated/status drift,
and unrelated UX/status/test-wiring changes. Treat this document as the PR 0
inventory for splitting that stack.

When a file appears in more than one slice, split by hunk. If a hunk cannot be
separated cleanly, land the smaller enabling slice first and keep later behavior
disabled or fixture-only until its own PR.

## Landing Order

| Slice | Title | Primary files / hunks | Objective | Claim boundary | Focused proof |
| --- | --- | --- | --- | --- | --- |
| PR 0 | `docs(quality): inventory proof-lane stack split (#8197)` | `docs/project/status/proof_lane_stack_split.md`; baseline/claim-boundary/navigation hunks in `docs/project/status/coverage_and_ripr_enforcement.md`; proof-lane navigation links in `docs/project/status/index.md` and `docs/project/CURRENT_STATUS.md`; `xtask/tests/proof_lane_stack_split_policy.rs` | Record the dirty stack inventory, classify proof-lane slices, and make the coverage/RIPR baseline discoverable from status navigation. | Inventory/navigation plus a split-plan policy test. No CI enforcement, no quality-gate behavior, no LSP 3.18 behavior. | `rtk cargo test -p xtask --test proof_lane_stack_split_policy --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 1 | `ci(ripr): run new-gap gate when draft PR becomes ready (#8197)` | ready-for-review/no-path-filter hunks in `.github/workflows/ripr.yml`; routing hunks in `docs/ci/ripr.md`; `xtask/tests/ripr_new_gap_gate_workflow.rs` | Run the RIPR proof workflow on every PR when it becomes ready for review, including docs/policy/workflow-only PRs. | Workflow routing and receipt presence only. No RIPR analyzer changes, no total-zero enforcement, no blocking quality-gate wiring, no coverage policy. | `rtk cargo test -p xtask --test ripr_new_gap_gate_workflow --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 2 | `docs(coverage): align Codecov rollout with proof lane (#8197)` | `codecov.yml`; policy-target/comment hunks in `.ci/README-coverage.md`, `docs/ci/codecov-rollout.md`, and `docs/how-to/COVERAGE.md`; `xtask/tests/codecov_patch_gate_policy.rs` | Make patch coverage the documented front-door policy at `95%` / `0%`, keep project coverage transitional, and move the baseline proof path to `target/receipts/quality/coverage-baseline.json`. | Codecov policy/docs/test contract only. No CI workflow enforcement, no quality-gate CLI contract, and no final project enforcement. | `rtk cargo test -p xtask --test codecov_patch_gate_policy --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 3 | `xtask(quality): prove patch coverage gate at CLI boundary (#8197)` | `xtask/src/main.rs`; `xtask/src/tasks/mod.rs`; `xtask/src/tasks/quality_gate.rs`; `xtask/src/tasks/quality_baseline.rs`; `xtask/tests/quality_gate_patch_coverage_cli_policy.rs`; initial shared helpers in `xtask/tests/quality_gate_cli_support/mod.rs`; local quality-gate guidance hunks in `docs/how-to/COVERAGE.md` | Prove `quality-gate --mode enforce-patch-coverage`, JSON and markdown receipts, `--check`, stale/missing coverage receipt failure, below-target patch failure, and actionable file/line/test guidance. | CLI proof only. No CI wiring and no project coverage final enforcement. | `rtk cargo test -p xtask --test quality_gate_patch_coverage_cli_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 4 | `xtask(quality): prove RIPR new-gap gate at CLI boundary (#8197)` | RIPR sections of `xtask/src/tasks/quality_gate.rs`; `xtask/src/tasks/ripr_evidence.rs`; `xtask/tests/quality_gate_ripr_new_gap_cli_policy.rs`; RIPR helper additions in `xtask/tests/quality_gate_cli_support/mod.rs` | Prove `quality-gate --mode enforce-new-ripr`, missing/stale RIPR+ receipt failure, missing/stale diff-scoped PR receipt failure, missing/stale review guidance failure, and gap id/file/line/seam guidance. | New-gap and receipt-freshness enforcement only. Existing total RIPR+ debt remains transitional. | `rtk cargo test -p xtask --test quality_gate_ripr_new_gap_cli_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 5 | `policy(quality): encode temporary quality-gate exceptions (#8197)` | `policy/quality-gate-exceptions.toml`; exception sections of `xtask/src/tasks/quality_gate.rs`; `xtask/tests/quality_gate_exception_policy.rs`; exception sections of `docs/project/status/coverage_and_ripr_enforcement.md` | Encode burn-down exceptions with owner, reason, target, evidence, review date, expiry, and removal criteria. | Exceptions document transition debt only. They are not permanent bypasses and final enforce fails while active. | `rtk cargo test -p xtask --test quality_gate_exception_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 6 | `xtask(quality): prove final quality-gate enforcement contract (#8197)` | final-enforce sections of `xtask/src/tasks/quality_gate.rs`; `xtask/tests/quality_gate_final_enforce_cli_policy.rs`; final-enforce helper additions in `xtask/tests/quality_gate_cli_support/mod.rs`; final target sections of `docs/project/status/coverage_and_ripr_enforcement.md` | Prove the future `quality-gate --mode enforce` fixture contract: RIPR+ total zero, new gaps zero, patch/project coverage at target, workspace scope, blocking Codecov policies, fresh receipts, and no active exceptions. | Future final-state CLI contract only. It can land before live burn-down reaches target because it uses fixture receipts. | `rtk cargo test -p xtask --test quality_gate_final_enforce_cli_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 7 | `ci(quality): show actionable proof failures in PR summary (#8197)` | `.github/PULL_REQUEST_TEMPLATE.md`; markdown-summary sections of `xtask/src/tasks/quality_gate.rs`; `xtask/tests/quality_pr_summary_policy.rs`; reused summary fixtures in `xtask/tests/quality_gate_cli_support/mod.rs`; verification-guidance hunks in `docs/VERIFICATION.md`; summary-only hunks in workflows if already separated | Put RIPR/coverage status, receipt freshness, exception status, top gaps/files, and exact local commands in PR-facing guidance. | Presentation only. No new enforcement. | `rtk cargo test -p xtask --test quality_pr_summary_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |
| PR 8 | `ci(quality): enforce new ripr gaps and patch coverage (#8197)` | enforcement hunks in `.github/workflows/ripr.yml`; enforcement hunks in `.github/workflows/ci-nightly.yml`; artifact checks/uploads; `justfile` coverage-proof target; CI-lane policy entries in `policy/ci-lanes.toml` and `policy/ci-lane-whitelist.toml`; blocking-policy hunks in `docs/ci/ripr.md`; workflow/coverage-quality-gate hunks in `.ci/README-coverage.md`, `docs/ci/codecov-rollout.md`, and `docs/how-to/COVERAGE.md`; `docs/ci/test-evidence-lanes.md`; `xtask/tests/quality_ci_wiring_policy.rs` | Wire the first blocking CI gate: new RIPR gaps, stale/missing RIPR receipts, patch coverage below `95%`, and stale/missing coverage receipts fail PRs. | First blocking CI step only. Total RIPR+ zero and project coverage `95%` remain temporary exceptions. | `rtk cargo test -p xtask --test quality_ci_wiring_policy --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor` |

## Quarantine Until Reclassified

Do not include these files in proof-enforcement PRs without a separate
objective and proof command:

- `.perl-lsp/goals/active.toml`
- `crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json`
- `crates/perl-lsp-ux-tests/tests/ux_scenario_46_receiver_real_workspace_quality.rs`
- `docs/project/metrics/RATCHET.md`
- `docs/project/status/SUPPORT_TIERS.md`
- `docs/project/status/provider_confidence_matrix.md`
- `docs/project/status/real_perl_editor_trust_v1.md`
- `docs/project/status/receiver_facts.md`
- `docs/reference/STABILITY.md`
- `plans/real-perl-editor-trust/implementation-plan.md`
- `xtask/src/tasks/check_test_wiring.rs`
- `xtask/src/tasks/cpan_corpus.rs`
- `xtask/src/tasks/update_status/mod.rs`
- `xtask/src/tasks/update_status/parser.rs`
- `xtask/src/tasks/update_status/parser/tests.rs`
- `xtask/src/tasks/update_status/parser/metrics.rs`
- `xtask/src/tasks/update_status/subsystem.rs`
- `xtask/src/tasks/worktree_allocator.rs`
- `xtask/tests/wave1_perl_module_collapse_tests.rs`
- `xtask/tests/wave_g1a_collapse_tests.rs`

Most of this quarantine set looks like status-generation, real-workspace UX,
parser/status refactoring, CPAN corpus, or wave-collapse work. It may be valid
work, but it is not part of the coverage/RIPR proof-enforcement control plane
unless a later slice names the exact proof obligation.

## Dirty File Classification

Every file currently shown by `rtk git status --short --branch` is assigned
below. This table is the split ledger; use it when cherry-picking hunks onto
clean slice branches.

| Path | Slice | Reason |
| --- | --- | --- |
| `.ci/README-coverage.md` | PR 2 / PR 8 | Coverage policy commands belong to PR 2; proof-LCOV workflow command belongs to PR 8. |
| `.github/PULL_REQUEST_TEMPLATE.md` | PR 7 | PR body quality-proof guidance. |
| `.github/workflows/ci-nightly.yml` | PR 8 | Coverage workflow enforcement and proof-artifact wiring. |
| `.github/workflows/ripr.yml` | PR 1 / PR 8 | Ready-for-review routing belongs to PR 1; blocking gate/artifact enforcement belongs to PR 8. |
| `.perl-lsp/goals/active.toml` | Quarantine | Active-goal metadata, not proof-gate control plane. |
| `codecov.yml` | PR 2 | Patch/project coverage policy shape. |
| `crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json` | Quarantine | UX fixture/status work. |
| `crates/perl-lsp-ux-tests/tests/ux_scenario_46_receiver_real_workspace_quality.rs` | Quarantine | Receiver-quality UX receipt work. |
| `docs/VERIFICATION.md` | PR 7 | User-facing proof/receipt command guidance. |
| `docs/ci/codecov-rollout.md` | PR 2 / PR 8 | Codecov policy rollout belongs to PR 2; workflow and coverage-quality-gate wiring docs belong to PR 8. |
| `docs/ci/ripr.md` | PR 1 / PR 8 | Ready-for-review routing docs belong to PR 1; blocking quality-gate posture belongs to PR 8. |
| `docs/ci/test-evidence-lanes.md` | PR 8 | CI evidence-lane blocking/advisory classification. |
| `docs/how-to/COVERAGE.md` | PR 2 / PR 3 / PR 8 | Policy targets belong to PR 2; local quality-gate CLI guidance belongs to PR 3; workflow/CI wiring guidance belongs to PR 8. |
| `docs/project/CURRENT_STATUS.md` | PR 0 | Navigation link to coverage/RIPR enforcement baseline. |
| `docs/project/metrics/RATCHET.md` | Quarantine | Metrics baseline/status drift unrelated to proof-gate split. |
| `docs/project/status/SUPPORT_TIERS.md` | Quarantine | Receiver/support-tier status work. |
| `docs/project/status/coverage_and_ripr_enforcement.md` | PR 0 / PR 5 / PR 6 | Baseline/navigation in PR 0; exception contract in PR 5; final-target contract in PR 6. |
| `docs/project/status/index.md` | PR 0 | Navigation link to coverage/RIPR enforcement baseline. |
| `docs/project/status/proof_lane_stack_split.md` | PR 0 | This inventory and split plan. |
| `docs/project/status/provider_confidence_matrix.md` | Quarantine | Receiver/provider-confidence status work. |
| `docs/project/status/real_perl_editor_trust_v1.md` | Quarantine | Receiver/real-workspace UX status work. |
| `docs/project/status/receiver_facts.md` | Quarantine | Receiver-fact status work. |
| `docs/reference/STABILITY.md` | Quarantine | Release/version stability docs. |
| `justfile` | PR 8 | Coverage proof LCOV recipe used by CI wiring. |
| `plans/real-perl-editor-trust/implementation-plan.md` | Quarantine | Real Perl editor trust plan work. |
| `policy/ci-lane-whitelist.toml` | PR 8 | CI lane policy for blocking RIPR/coverage gates. |
| `policy/ci-lanes.toml` | PR 8 | CI lane policy for blocking RIPR/coverage gates. |
| `policy/quality-gate-exceptions.toml` | PR 5 | Temporary exception ledger. |
| `xtask/src/main.rs` | PR 3 / PR 4 / PR 6 | `quality-gate` and baseline CLI wiring; split by mode/argument hunks. |
| `xtask/src/tasks/check_test_wiring.rs` | Quarantine | Test-wiring helper changes outside proof-gate scope. |
| `xtask/src/tasks/cpan_corpus.rs` | Quarantine | CPAN/status support work. |
| `xtask/src/tasks/mod.rs` | PR 3 | `quality_gate` and `quality_baseline` module registration. |
| `xtask/src/tasks/quality_baseline.rs` | PR 3 / PR 4 | Coverage baseline receipt producer belongs to PR 3; RIPR+ baseline receipt producer belongs to PR 4. |
| `xtask/src/tasks/quality_gate.rs` | PR 3 / PR 4 / PR 5 / PR 6 / PR 7 | Aggregate gate implementation; split by patch, RIPR, exception, final enforce, and markdown summary hunks. |
| `xtask/src/tasks/ripr_evidence.rs` | PR 4 | RIPR receipt/guidance helpers used by new-gap proof. |
| `xtask/src/tasks/update_status/mod.rs` | Quarantine | Status-generator refactor. |
| `xtask/src/tasks/update_status/parser.rs` | Quarantine | Parser status refactor. |
| `xtask/src/tasks/update_status/parser/metrics.rs` | Quarantine | Parser metrics extraction. |
| `xtask/src/tasks/update_status/parser/tests.rs` | Quarantine | Parser status tests. |
| `xtask/src/tasks/update_status/subsystem.rs` | Quarantine | Status subsystem refactor. |
| `xtask/src/tasks/worktree_allocator.rs` | Quarantine | Worktree allocator changes unrelated to proof-gate scope. |
| `xtask/tests/codecov_patch_gate_policy.rs` | PR 2 | Codecov config/docs contract test. |
| `xtask/tests/quality_ci_wiring_policy.rs` | PR 8 | Workflow, artifact, `justfile`, and CI policy contract test. |
| `xtask/tests/quality_gate_cli_support/mod.rs` | PR 3 / PR 4 / PR 6 / PR 7 | Shared quality-gate CLI fixtures; introduce only the helpers each slice needs. |
| `xtask/tests/quality_gate_exception_policy.rs` | PR 5 | Temporary exception policy test. |
| `xtask/tests/quality_gate_final_enforce_cli_policy.rs` | PR 6 | Final enforce CLI contract test. |
| `xtask/tests/quality_gate_patch_coverage_cli_policy.rs` | PR 3 | Patch coverage CLI contract test. |
| `xtask/tests/quality_gate_ripr_new_gap_cli_policy.rs` | PR 4 | RIPR new-gap CLI contract test. |
| `xtask/tests/quality_pr_summary_policy.rs` | PR 7 | PR template and quality-gate summary guidance test. |
| `xtask/tests/proof_lane_stack_split_policy.rs` | PR 0 | Split-plan inventory guard test. |
| `xtask/tests/ripr_new_gap_gate_workflow.rs` | PR 1 | Ready-for-review RIPR workflow/docs contract test. |
| `xtask/tests/wave1_perl_module_collapse_tests.rs` | Quarantine | Wave-collapse tests unrelated to proof-gate split. |
| `xtask/tests/wave_g1a_collapse_tests.rs` | Quarantine | Wave-collapse tests unrelated to proof-gate split. |

## Coverage/RIPR Status Doc Split

`docs/project/status/coverage_and_ripr_enforcement.md` is a shared proof-lane
status artifact. Split it by hunk so PR 0 can land the baseline without
silently carrying temporary-exception or final-enforcement semantics.

| Status-doc hunk | Slice | Include | Exclude |
| --- | --- | --- | --- |
| Baseline and navigation | PR 0 | Claim boundary, measurement-only baseline commands, current policy snapshot, generated receipt paths under `target/receipts/quality/*`, existing check inventory, known baseline gaps, and the next split. | Temporary exception ledger mechanics, final `quality-gate --mode enforce` blockers, Codecov project coverage promotion, and any live claim that RIPR+ zero or project coverage `95%` has already been reached. |
| Temporary burn-down exceptions | PR 5 | `policy/quality-gate-exceptions.toml`, `ripr-total-burndown`, `project-coverage-burndown`, owner/evidence/review/expiry/removal criteria, due/expired failure rules, and the configurable transition controls in `### Durable Policy Contract`. | Final enforcement promotion, CI workflow blocking, removal of active exceptions, and unrelated suppression or allowlist churn. |
| Future final target | PR 6 | Fixture-backed final `quality-gate --mode enforce` contract, RIPR+ zero, new RIPR gaps zero, Codecov patch coverage enforcement, Codecov project coverage enforcement after burn-down, workspace coverage scope, fresh receipts, and no active temporary exceptions. | CI promotion to blocking, burn-down test fills, PR summary presentation, and any statement that the live repo has already reached the final target. |

PR 0 may make the baseline discoverable, but it must remain measurement only:
no LSP 3.18 behavior, no protocol extraction, no Codecov project promotion, and
no final `quality-gate --mode enforce` requirement.

## Quality-Gate Helper Split

`xtask/tests/quality_gate_cli_support/mod.rs` is a shared fixture module in the
dirty stack, but it should not land all at once. When extracting slices, add
only the helper rows needed by that slice and by slices that have already
landed.

| Helper group | First slice | Later reuse | Purpose |
| --- | --- | --- | --- |
| `repo_root`, `current_head`, `next_action`, `next_actions_contain`, `assert_failure_stderr_points_to_receipt_and_summary`, `assert_blocking_actions_have_repair_contract` | PR 3 | PR 4 / PR 6 / PR 7 | Shared CLI assertion foundation for all quality-gate integration tests. |
| `patch_quality_gate_command`, `patch_quality_gate_command_with_cli_patch`, `write_coverage_receipt`, `write_stale_coverage_receipt`, `write_patch_gap_coverage_receipt`, `actionable_codecov_comment`, `write_exception_policy` | PR 3 | PR 4 / PR 6 / PR 7 | Patch coverage gate fixtures and the minimal exception fixture needed to exercise the CLI. |
| `new_ripr_quality_gate_command`, `write_ripr_plus_receipt`, `write_stale_ripr_plus_receipt`, `write_ripr_pr_receipt`, `write_stale_ripr_pr_receipt`, `write_review_guidance_receipt`, `write_empty_review_guidance_receipt`, `write_stale_review_guidance_receipt` | PR 4 | PR 6 | RIPR new-gap, stale receipt, and review-guidance fixtures. |
| `write_actionable_ripr_plus_receipt`, `final_quality_gate_command`, `write_workspace_coverage_receipt`, `write_project_gap_workspace_coverage_receipt`, `write_patch_gap_workspace_coverage_receipt`, `write_workspace_coverage_receipt_with_values`, `coverage_gap_files`, `write_final_codecov_config`, `write_advisory_project_codecov_config`, `workspace_coverage_scope`, `required_coverage_roots`, `normalize_member_root`, `final_codecov_project_status` | PR 6 | none | Final enforce fixtures for RIPR total zero, workspace coverage, project coverage, and final Codecov policy. |

## Xtask CLI Entry Split

`xtask/src/main.rs` also needs hunk-level extraction. Keep the CLI surface in
lockstep with the implementation slices so early PRs do not expose modes whose
implementation still belongs to a later slice.

| CLI surface | First slice | Later reuse | Purpose |
| --- | --- | --- | --- |
| `Commands::CoverageBaseline`, `Commands::QualityGate`, `QualityGateCliMode::Advisory`, `QualityGateCliMode::EnforcePatchCoverage`, `QualityGatePatchStatusSource`, `Commands::CoverageBaseline` dispatch, `Commands::QualityGate` dispatch, and `QualityGateCliMode` conversion for advisory/patch modes | PR 3 | PR 4 / PR 5 / PR 6 / PR 7 / PR 8 | Introduce the local quality-gate front door and patch coverage mode with receipt/summary/check wiring. |
| `Commands::RiprPlus`, `Commands::RiprPlus` dispatch, `QualityGateCliMode::EnforceNewRipr`, and `QualityGateCliMode` conversion for `enforce-new-ripr` | PR 4 | PR 6 / PR 7 / PR 8 | Add repo-wide RIPR+ baseline receipt generation and the new-gap gate mode. |
| `QualityGateCliMode::Enforce` and `QualityGateCliMode` conversion for full `enforce` | PR 6 | PR 7 / final CI promotion | Expose the post-burn-down final gate only after fixture tests prove total RIPR+ zero, project coverage, workspace scope, blocking Codecov project policy, and no active exceptions. |

Do not expose `--mode enforce` in PR 3 or PR 4. Do not wire `ripr-plus` as a
side effect of the patch coverage CLI slice. If a later test needs a mode
variant before the implementation slice lands, keep it as fixture-only inside
that test slice rather than exposing the production CLI.

## Quality-Baseline Implementation Split

`xtask/src/tasks/quality_baseline.rs` is a shared measurement module, but the
coverage and RIPR producers should still land in separate slices.

| Implementation group | First slice | Later reuse | Purpose |
| --- | --- | --- | --- |
| `QUALITY_RECEIPT_SCHEMA_VERSION`, `LOCAL_COMMAND_PREFIX`, `write_or_check_receipt`, `display_path`, `command_arg`, `git_head`, shared check/receipt tests | PR 3 | PR 4 / PR 6 / PR 8 | Common receipt I/O, command rendering, and freshness-check behavior used by both measurement receipts. |
| `CoverageCounters`, `CoverageScope`, `CoverageFileRow`, `CoverageBaselineReceipt`, `coverage_baseline`, `coverage_baseline_receipt`, `coverage_baseline_command`, `parse_lcov`, `coverage_scope`, `required_coverage_roots`, `workspace_root`, `normalize_member_root`, `lcov_source_paths`, `coverage_roots`, `normalize_coverage_path`, `repo_relative_coverage_path`, `coverage_files_below_target`, `flush_coverage_file`, `parse_lcov_count`, `parse_lcov_da`, `percent`, `parse_key_value_file`, `read_codecov_config`, coverage unit tests | PR 3 | PR 6 / PR 8 | Coverage baseline receipt, LCOV parsing, workspace scope, Codecov policy snapshot, and uncovered-file guidance. |
| `CountRow`, `RiprSeamSample`, `RiprFileCluster`, `DeferredCountRow`, `RiprPlusReceipt`, `ripr_plus`, `ripr_plus_receipt`, `ripr_seam_cluster_action`, `ripr_plus_command`, `run_ripr_repo_seams`, `top_counts`, `ripr_count_field`, `top_file_clusters`, `classified_file_counts`, `file_clusters`, `ripr_seam_path`, `ripr_seam_kind`, `normalize_ripr_path`, `ripr_seam_sample`, `ripr_seam_sample_is_actionable`, `first_string`, `first_u64`, `deferred_ripr_file_reason`, RIPR unit tests | PR 4 | PR 6 / PR 8 | Repo-wide RIPR+ baseline receipt, actionable seam samples, deferred-file classification, and RIPR receipt commands. |

Do not include RIPR+ receipt code in PR 3. PR 3 can expose shared helpers and
coverage receipt generation; PR 4 adds `ripr-plus` and the RIPR seam parsing
surface.

## Quality-Gate Implementation Split

`xtask/src/tasks/quality_gate.rs` is the main implementation risk in the dirty
stack. Split it by behavior group, not by line number alone:

| Implementation group | First slice | Later reuse | Purpose |
| --- | --- | --- | --- |
| `QualityGateMode`, `PatchStatusSource`, `QualityGateConfig`, `QualityGateReceipt`, `QualityGateCommandState`, `run`, `quality_gate_receipt`, `quality_gate_command_state`, `write_or_check_quality_gate_receipt`, `write_or_check_text`, `read_receipt`, `quality_gate_command`, `local_command` | PR 3 | PR 4 / PR 5 / PR 6 / PR 7 | Stable CLI/receipt shell shared by every quality-gate mode. |
| `CoverageGateState`, `CodecovStatusPolicy`, `CodecovCommentPolicy`, `validate_patch_inputs`, `coverage_state`, `coverage_receipt_state`, `apply_codecov_config_fallback`, `codecov_status_policy*`, `codecov_comment_policy*`, `coverage_receipt_status`, `coverage_measurement_violation`, `has_coverage_file_guidance`, `coverage_file_guidance_is_valid`, `coverage_file_guidance_prefix`, `actionable_coverage_file_guidance`, `positive_sample_uncovered_lines`, `patch_coverage_policy_blockers`, `coverage_receipt_verify_command`, `patch_policy_is_blocking`, `patch_policy_is_enforcing`, `patch_coverage_value_blockers`, `patch_coverage_unknown_action`, `patch_coverage_below_target_action`, `coverage_baseline_command` | PR 3 | PR 6 / PR 7 | Patch coverage receipt freshness, Codecov patch policy, and actionable uncovered-file guidance. |
| `RiprGateState`, `RiprPrGateState`, `ReviewGuidanceState`, `ripr_state`, `ripr_pr_state`, `review_guidance_state`, `ripr_plus_receipt_status`, `ripr_plus_measurement_violation`, `has_ripr_plus_actionable_guidance`, `ripr_plus_file_guidance_is_actionable`, `ripr_plus_sample_seam_is_actionable`, `diff_receipt_status`, `review_guidance_items`, `review_guidance_declares_items`, `review_guidance_item_is_actionable`, `review_guidance_item`, `ripr_guidance_files`, `actionable_ripr_guidance_prefix`, `actionable_ripr_file_guidance`, `new_ripr_gap_action`, `ripr_pr_verify_command`, `ripr_review_verify_command`, `ripr_review_guidance_gap_action`, `ripr_review_guidance_verify_command`, `ripr_plus_command`, `ripr_pr_command`, `ripr_review_command` | PR 4 | PR 6 / PR 7 | RIPR+ receipt freshness, diff-scoped new-gap enforcement, and review-guidance repair actions. |
| `QualityGateExceptionState`, `QualityGateException`, `QualityGateExceptionFile`, `RequiredQualityException`, `REQUIRED_QUALITY_EXCEPTIONS`, `exception_state`, `exception_warnings`, `required_exception_warnings`, `exception_entry_warnings`, `exception_date_warning`, `parse_policy_date`, `current_policy_date`, `format_policy_date`, `exception_actions`, `final_exception_blockers`, `exception_policy_blockers` | PR 5 | PR 6 / PR 7 | Temporary exception ledger parsing, due/expired warnings, and final-enforce exception blockers. |
| `coverage_scope_value`, `unknown_coverage_scope`, `coverage_scope_blockers`, `coverage_scope_is_workspace`, `CoverageScopeContract`, `coverage_scope_contract`, `string_array_value`, `project_coverage_policy_blockers`, `project_policy_is_final`, `codecov_config_blockers`, `codecov_comment_policy_blockers`, `codecov_comment_is_actionable`, `live_codecov_policy_is_available`, `patch_status_is_external`, final-mode branches in `advisory_actions`, `blockers`, `advisory_*_is_useful`, `mark_blocking_actions` | PR 6 | PR 7 | Future final-state enforcement for RIPR total zero, workspace coverage scope, project coverage, blocking project policy, and no active exceptions. |
| `render_quality_gate_markdown`, `render_pr_summary_guidance`, `render_claim_boundary`, `render_quality_gate_matrix`, `local_proof_commands`, `quality_gate_row`, `format_action`, `format_warning`, `format_sample_seams`, `format_sample_uncovered_lines`, `md_cell`, `format_scope`, status/current helpers used only by markdown presentation | PR 7 | PR 8 summary wiring only | Human/PR-facing summary rendering. No new enforcement semantics. |

When extracting, PR 3 should introduce only the shared shell and patch coverage
rows. PR 4 adds the RIPR row. PR 5 adds exception parsing and transition-debt
checks. PR 6 adds final-enforce blockers. PR 7 adds markdown presentation.

## Split Mechanics

- Start each slice from a clean branch, then apply only the hunks listed for
  that slice.
- Keep `xtask/src/tasks/quality_gate.rs` split by behavior group: patch
  coverage, RIPR new-gap/freshness, exceptions, final enforce, and presentation.
- Keep the quality-gate CLI integration tests physically split:
  `quality_gate_patch_coverage_cli_policy.rs` for PR 3,
  `quality_gate_ripr_new_gap_cli_policy.rs` for PR 4, and
  `quality_gate_final_enforce_cli_policy.rs` for PR 6.
- Keep workflow routing separate from CLI behavior unless the workflow test
  requires an already-landed CLI command.
- Keep `xtask/tests/ripr_new_gap_gate_workflow.rs` limited to the PR 1
  workflow and RIPR-doc contract. CI policy-ledger assertions belong with PR 8.
- Keep `docs/ci/ripr.md` split by hunk: ready-for-review/no-path-filter
  routing belongs with PR 1; blocking quality-gate semantics, required
  artifacts, and final promotion language belong with PR 8.
- Keep `xtask/tests/codecov_patch_gate_policy.rs` limited to PR 2 Codecov
  config and policy docs until the CLI and CI wiring slices. Local
  `quality-gate --mode enforce-patch-coverage` behavior and file/line guidance
  belong with PR 3; workflow, `justfile`, coverage-quality-gate receipt, and CI
  policy-ledger assertions belong with PR 8.
- Use `xtask/tests/quality_ci_wiring_policy.rs` for PR 8 workflow, artifact,
  `justfile`, CI-lane ledger, and evidence-lane assertions.
- Keep `xtask/tests/quality_gate_cli_support/mod.rs` split by the
  Quality-Gate Helper Split table. Do not include final-enforce helper rows in
  PR 3 or PR 4 just because the broad dirty stack currently has them.
- Keep `xtask/src/main.rs` split by the Xtask CLI Entry Split table. Do not
  expose final `--mode enforce` before PR 6.
- Keep `xtask/src/tasks/quality_baseline.rs` split by the Quality-Baseline
  Implementation Split table. Do not include RIPR+ receipt generation in PR 3.
- Keep `xtask/src/tasks/quality_gate.rs` split by the Quality-Gate
  Implementation Split table. Do not introduce final-mode blockers or PR
  summary rendering in PR 3/PR 4.
- Keep generated receipts under `target/receipts/quality/*` uncommitted unless
  repository policy changes explicitly.
- Every slice must state objective, claim boundary, non-goals, proof commands,
  cleanup performed, and remaining advisory burn-down debt.

## PR Handoff Contract

Every extracted slice must fill the PR template's `Quality Proof` block with
evidence, not intent. Use this exact checklist before opening or updating the
slice PR:

- Lane: check `coverage / proof / enforcement`.
- Objective: one sentence matching the slice objective in the landing-order
  table.
- Claim boundary: state the proof surface this PR owns and the enforcement
  state it does or does not change.
- Non-goals: explicitly name no LSP 3.18 behavior, no protocol extraction, no
  release work, and no unrelated cleanup unless the slice explicitly owns one of
  those items.
- RIPR/coverage effect: state whether the slice is measurement-only,
  CLI-contract-only, presentation-only, or blocking; include before/after counts
  only when receipts prove them.
- Local proof commands and pass/fail results: paste the focused command set from
  the landing-order table with actual outcomes, including any `quality-gate`
  receipt command when the slice emits one.
- Cleanup performed: report `rtk git status --short --branch`, `rtk git diff
  --check`, `rtk bash scripts/storage-doctor`, and removal of any repo-local
  `target/` or `xtask/target/` output created by validation.
- What remains: name advisory burn-down debt such as `ripr-total-burndown` or
  `project-coverage-burndown`, or write `N/A` only when there is no remaining
  transition debt for that slice.

Do not open a slice PR with a generic "CI failed/passed" proof note. The body
must tell the next agent exactly what was proven, what was not proven, and what
command to run locally.

## Slice Extraction Checklist

Use this checklist when moving from the broad dirty stack onto clean slice
branches. Each slice starts from a clean integration base and applies only the
named hunks. Do not use `git stash`; it is shared across worktrees.

| Slice | Include | Exclude | Proof before handoff |
| --- | --- | --- | --- |
| PR 0 | Split inventory docs, coverage/RIPR baseline navigation, `proof_lane_stack_split_policy.rs`. | All workflow enforcement, `quality-gate` implementation, Codecov config changes, and quarantined receiver/status/parser work. | `rtk cargo test -p xtask --test proof_lane_stack_split_policy --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 1 | `.github/workflows/ripr.yml` ready-for-review/no-path-filter routing hunks, routing-only `docs/ci/ripr.md` hunks, `ripr_new_gap_gate_workflow.rs`. | `quality-gate --mode enforce-new-ripr`, blocking artifact checks, `quality-gate.md` step-summary upload, CI policy ledgers, and Codecov changes. | `rtk cargo test -p xtask --test ripr_new_gap_gate_workflow --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 2 | `codecov.yml` patch/project policy shape, Codecov policy docs, `codecov_patch_gate_policy.rs`. | `.github/workflows/ci-nightly.yml`, `justfile`, quality-gate CLI behavior, coverage-quality-gate receipts, and CI lane ledgers. | `rtk cargo test -p xtask --test codecov_patch_gate_policy --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 3 | `coverage-baseline`, `quality-gate --mode enforce-patch-coverage`, patch coverage helpers/tests, patch sections of `quality_gate.rs`, and local coverage CLI guidance. | `ripr-plus`, `enforce-new-ripr`, final `--mode enforce`, exception policy, PR summary presentation, and CI workflow wiring. | `rtk cargo test -p xtask --test quality_gate_patch_coverage_cli_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 4 | `ripr-plus`, `quality-gate --mode enforce-new-ripr`, RIPR receipt/review guidance helpers/tests, RIPR sections of `quality_gate.rs`, RIPR+ sections of `quality_baseline.rs`, `ripr_evidence.rs`. | Patch coverage implementation already landed in PR 3, final total-zero enforcement, temporary exception policy, PR summary presentation, and CI workflow wiring. | `rtk cargo test -p xtask --test quality_gate_ripr_new_gap_cli_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 5 | `policy/quality-gate-exceptions.toml`, exception parsing/checking sections of `quality_gate.rs`, exception contract tests, exception sections of `coverage_and_ripr_enforcement.md`. | Final enforcement promotion, CI workflow blocking, Codecov project enforcement, and unrelated suppression/allowlist churn. | `rtk cargo test -p xtask --test quality_gate_exception_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 6 | Final `quality-gate --mode enforce`, final-enforce fixtures/tests, workspace coverage scope, project coverage policy blockers, total RIPR+ zero blockers, final target docs. | CI promotion to blocking, PR summary presentation, burn-down test fills, and live claim that the repo has reached final target. | `rtk cargo test -p xtask --test quality_gate_final_enforce_cli_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 7 | PR template quality-proof block, quality-gate markdown summary rendering, PR summary guidance tests, verification docs. | New enforcement semantics, workflow hard-fail wiring, Codecov project promotion, and burn-down test fills. | `rtk cargo test -p xtask --test quality_pr_summary_policy --profile agent --locked`; `rtk cargo test -p xtask quality_gate --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |
| PR 8 | First blocking CI wiring for new RIPR gaps and patch coverage: workflows, required artifact checks/uploads, `justfile`, CI lane policy ledgers, evidence-lane docs, `quality_ci_wiring_policy.rs`. | Total RIPR+ zero, project coverage blocking, removal of burn-down exceptions, LSP behavior, and unrelated status/receiver/parser work. | `rtk cargo test -p xtask --test quality_ci_wiring_policy --profile agent --locked`; `rtk cargo fmt -p xtask --check`; `rtk git diff --check`; `rtk bash scripts/storage-doctor`. |

Before opening a slice PR, re-run `rtk git status --short --branch` and confirm
only files assigned to that slice are dirty. If a dirty file is listed as
`Quarantine`, leave it behind and open a separate objective before touching it.

## Per-Slice Cleanup

Run the focused validation for the slice, then clean repo-local build output
created by the validation:

```bash
rtk cargo fmt -p xtask --check
rtk git diff --check
rtk bash scripts/storage-doctor
```

If cargo tests create repo-local `target/` or `xtask/target/` directories, remove
only those directories after verifying the resolved paths are inside the
workspace, then rerun `rtk bash scripts/storage-doctor`.
