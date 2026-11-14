#!/usr/bin/env bash
# The one producer for the post-compile required merge-surface commands.
# Run after the ordinary CI setup and `just check-all-targets`. `just pr-fast`
# deliberately does not call this complete, measured-duration proof.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# An early refusal must never leave an earlier green receipt looking current.
mkdir -p target/receipts
rm -f target/receipts/required-merge-surface.json

if [[ -z "${RUNNER_TEMP:-}" || ! -d "${RUNNER_TEMP:-}" ]]; then
  echo '::error::required-merge-surface NOT_PROVEN: RUNNER_TEMP must name an existing artifact directory' >&2
  exit 80
fi
if [[ -n "${REQUIRED_MERGE_SURFACE_ONLY:-}" ]]; then
  echo '::error::required-merge-surface NOT_PROVEN: filtered runs are not a complete admission proof' >&2
  exit 81
fi
for tool in bash cargo just git python3 jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "::error::required-merge-surface NOT_PROVEN: required tool $tool is unavailable" >&2
    exit 82
  fi
done
. scripts/lib/cargo-toolchain-guard.sh && cargo_toolchain_guard
if ! git ls-files --error-unmatch -- scripts/ci/required-merge-surface.sh >/dev/null 2>&1; then
  echo '::error::required-merge-surface NOT_PROVEN: producer is not tracked at the evaluated commit' >&2
  exit 83
fi
if ! git diff --quiet HEAD -- .; then
  echo '::error::required-merge-surface NOT_PROVEN: tracked-tree modifications invalidate exact-head proof' >&2
  exit 84
fi
if [[ -n "$(git status --porcelain --untracked-files=normal)" ]]; then
  echo '::error::required-merge-surface NOT_PROVEN: non-ignored untracked files invalidate exact-head proof' >&2
  exit 86
fi
# Command 22 reads every .rs file in this directory, including files ignored
# by **/temp_*.rs. CI build artifacts remain allowed; proof inputs may not vary.
python3 - <<'PY'
from pathlib import Path
import subprocess

expected = set(subprocess.check_output([
    'git', 'ls-files', '--', 'xtask/src/tasks/standalone_vectors/*.rs',
], text=True).splitlines())
actual = {
    path.as_posix()
    for path in Path('xtask/src/tasks/standalone_vectors').glob('*.rs')
}
if actual != expected:
    difference = ', '.join(sorted(actual ^ expected))
    raise SystemExit(
        '::error::required-merge-surface NOT_PROVEN: '
        f'standalone-vector inputs differ from HEAD: {difference}'
    )
PY
subject_sha="$(git rev-parse HEAD)"
if [[ ! "$subject_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo '::error::required-merge-surface NOT_PROVEN: subject is not an exact commit' >&2
  exit 85
fi

# Reject a filtered or edited original command even if its marker survives.
# New contract commands may be appended after the immutable existing prefix.
python3 - <<'PY'
import pathlib
import re
import subprocess
import yaml

baseline = subprocess.check_output([
    'git', 'show', '032468df9b9912ab847e5f5243b7f4897b8465b9:.github/workflows/ci.yml'
]).decode('utf-8')
steps = yaml.safe_load(baseline)['jobs']['check-all-targets']['steps']
original = [step for step in steps if step.get('name', '').endswith('(required merge surface)')]
text = pathlib.Path('scripts/ci/required-merge-surface.sh').read_text(encoding='utf-8')
markers = list(re.finditer(r'^# required merge surface command \d+: (.*)$', text, re.M))
actual = []
for index, marker in enumerate(markers):
    segment = text[marker.end() + 1:markers[index + 1].start() if index + 1 < len(markers) else len(text)]
    commands = '\n'.join(
        line for line in segment.splitlines() if not line.startswith('_step_index=')
    ).strip()
    actual.append((marker.group(1), commands))
expected = [(step['name'], step['run'].strip()) for step in original]
if actual[:len(expected)] != expected:
    position = next(
        (index for index, pair in enumerate(actual)
         if index >= len(expected) or pair != expected[index]),
        'missing original command',
    )
    raise SystemExit(
        f'::error::required-merge-surface NOT_PROVEN: original command missing or edited at {position}'
    )
PY
mkdir -p target/receipts
_step_index=0

_finish() {
  rc=$?
  trap - EXIT
  python3 - "$rc" "$_step_index" "$subject_sha" <<'PY'
import datetime
import hashlib
import json
import pathlib
import re
import subprocess
import sys

rc = int(sys.argv[1])
attempted = int(sys.argv[2])
head = sys.argv[3]
root = pathlib.Path.cwd()
source = pathlib.Path('scripts/ci/required-merge-surface.sh')
text = source.read_text(encoding='utf-8')
names = re.findall(r'^# required merge surface command \d+: (.*)$', text, re.M)
current_head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
clean = (
    subprocess.run(['git', 'diff', '--quiet', 'HEAD', '--', '.']).returncode == 0
    and subprocess.check_output(
        ['git', 'status', '--porcelain', '--untracked-files=normal'], text=True
    ) == ''
)
expected = set(subprocess.check_output([
    'git', 'ls-files', '--', 'xtask/src/tasks/standalone_vectors/*.rs',
], text=True).splitlines())
actual = {
    path.as_posix()
    for path in pathlib.Path('xtask/src/tasks/standalone_vectors').glob('*.rs')
}
proof_inputs_match = actual == expected
passed = rc == 0 and attempted == len(names) and head == current_head and clean and proof_inputs_match
rc = 0 if passed else 1
packet = {
    'schema_version': 1,
    'kind': 'required_merge_surface_admission',
    'result': 'pass' if passed else 'not_proven',
    'result_reason': 'all required commands and proof identity verified' if passed else 'missing, failing, filtered, or modified evidence',
    'head': head,
    'head_at_receipt': current_head,
    'tracked_tree_clean': clean,
    'script_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
    'commands_attempted': attempted,
    'commands_required': len(names),
    'command_names': names,
    'exit_code': rc,
    'recorded_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
}
out = root / 'target/receipts/required-merge-surface.json'
out.parent.mkdir(parents=True, exist_ok=True)
temporary = out.with_suffix('.tmp')
temporary.write_text(json.dumps(packet, indent=2) + '\n', encoding='utf-8')
temporary.replace(out)
print(f'required-merge-surface: {packet["result"]}; commands_attempted={attempted}; required={len(names)}; head={head}')
sys.exit(rc)
PY
  rc=$?
  exit "$rc"
}
trap _finish EXIT

# required merge surface command 1: PR-fast watchdog contract (required merge surface)
_step_index=$((_step_index + 1))
set -o pipefail
cargo test -p xtask --test gates_1469_pr_required_static_checks --locked -- \
  --test-threads=1 --color never | tee "$RUNNER_TEMP/pr-fast-watchdog-contract.log"
# Reject zero matches, ignored/filtered tests, and missing target controls.
grep -Eq '^test result: ok\. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in ' \
  "$RUNNER_TEMP/pr-fast-watchdog-contract.log"
for test_name in \
  ci_workflow_runs_unit_routed_full_in_pr_smoke \
  pr_fast_watchdog_accepts_direct_and_heredoc_runners \
  pr_fast_watchdog_rejects_untimed_or_status_losing_runners; do
  grep -Fx "test $test_name ... ok" "$RUNNER_TEMP/pr-fast-watchdog-contract.log"
done

# required merge surface command 2: Routed test preparation contract (required merge surface)
_step_index=$((_step_index + 1))
set -o pipefail
cargo test -p xtask --test routed_test_preparation_contract --locked -- \
  --test-threads=1 --color never | tee "$RUNNER_TEMP/routed-test-preparation-contract.log"
grep -Eq '^test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in ' \
  "$RUNNER_TEMP/routed-test-preparation-contract.log"
for test_name in \
  preparation_preserves_runtime_command_and_budget \
  preparation_precedes_watchdog_and_has_separate_evidence; do
  grep -Fx "test $test_name ... ok" "$RUNNER_TEMP/routed-test-preparation-contract.log"
done

# required merge surface command 3: Legacy parity library authority (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p perl-release-readiness --test legacy_parity --locked -- --nocapture

# required merge surface command 4: Legacy parity CLI authority (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test perl_kwalitee_parity --locked -- --nocapture

# required merge surface command 5: Emacs docs contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test emacs_docs_contract --locked -- --nocapture

# required merge surface command 6: Direct command guidance contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test direct_command_guidance --locked -- --nocapture

# required merge surface command 7: Package formatter caller contracts (required merge surface)
_step_index=$((_step_index + 1))
set -o pipefail
cargo test -p xtask --bin xtask --locked -- --exact \
  tasks::check::tests::check_task_run_routes_fmt_through_package_formatter \
  tasks::check::tests::check_task_run_preserves_all_check_order \
  tasks::ci::runner::tests::ci_runner_fmt_check_routes_to_package_formatter \
  tasks::ci_doctor::tests::ci_doctor_fmt_drift_routes_to_package_formatter \
  --nocapture | tee "$RUNNER_TEMP/package-formatter-callers.log"
# libtest succeeds with zero matches; require all four contracts to run.
grep -Eq '^test result: ok\. 4 passed; 0 failed; 0 ignored; 0 measured;' \
  "$RUNNER_TEMP/package-formatter-callers.log"

# required merge surface command 8: CI cache inventory contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin xtask tasks::ci_cache_inventory --locked -- --nocapture

# required merge surface command 9: CI cache inventory manifest drift gate (required merge surface)
_step_index=$((_step_index + 1))
cargo run --locked --package xtask -- ci-cache-inventory --check

# required merge surface command 10: Release-asset identity contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test publish_extension_release_identity --locked -- --nocapture

# required merge surface command 11: Publish-provenance contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test post_publish_smoke_version_provenance --locked -- --nocapture

# required merge surface command 12: Semantic facade consumer contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test parser_semantic_facade_consumers --locked -- --nocapture

# required merge surface command 13: Workspace/index facade consumer contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test parser_workspace_index_facade_consumers --locked -- --nocapture

# required merge surface command 14: Index lifecycle ownership ledger (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test workspace_index_lifecycle_ledger --locked -- --nocapture

# required merge surface command 15: TDD facade consumer contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test parser_tdd_facade_consumers --locked -- --nocapture

# required merge surface command 16: Gate-policy budget witness contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test issue_4657_ci_workflow_contract --locked -- --nocapture

# required merge surface command 17: ripr-liveness dedup paginate contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test issue_16133_ripr_liveness_dedup_paginate --locked -- --nocapture

# required merge surface command 18: ripr gap-gate workflow contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test ripr_new_gap_gate_workflow --locked -- --nocapture

# required merge surface command 19: ripr version badge contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test badge_ripr_version_contract --locked -- --nocapture

# required merge surface command 20: ripr gap CLI policy contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test quality_gate_ripr_new_gap_cli_policy --locked -- --nocapture

# required merge surface command 21: ripr seam-proof observation contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test ripr_seam_proof_contributor_observation --locked -- --nocapture

# required merge surface command 22: Standalone semantic conformance corpus contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test standalone_vectors_cli --locked -- --nocapture

# required merge surface command 23: LSP JSON-RPC dependency probe (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test lsp_jsonrpc_dependency_probe --locked -- --nocapture

# required merge surface command 24: Non-Rust file policy contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test file_policy --locked -- --nocapture

# required merge surface command 25: Dependabot dependency-name contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test dependabot_cargo_dependency_names --locked -- --nocapture

# required merge surface command 26: Dependabot Cargo commit-message contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test dependabot_cargo_commit_message --locked -- --nocapture

# required merge surface command 27: Changelog fragment gate contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin xtask --locked -- --nocapture changelog changie

# required merge surface command 28: PR-subject materializer unit tests (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin xtask --locked ci_subject_materializer::tests -- --nocapture

# required merge surface command 29: File-policy preapproval unit tests (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin xtask --locked file_policy::tests::preapproved -- --nocapture

# required merge surface command 30: Workflow policy CLI wiring (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin xtask --locked -- workflow_policy_lint

# required merge surface command 31: Dependabot GitHub Actions and npm commit-message contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test dependabot_gha_npm_commit_message --locked -- --nocapture

# required merge surface command 32: Post-merge generated-PR dispatch contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test post_merge_status_regen_tests --locked -- --nocapture

# required merge surface command 33: Lane-history generated-PR dispatch contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test lane_history_ci_dispatch_contract --locked -- --nocapture

# required merge surface command 34: Freshness-gate cfg-binding contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test gate_policy_profile_tests --locked -- freshness_gate_commands_satisfy_every_named_suite_cfg --nocapture

# required merge surface command 35: Product-topology staged contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin product-topology --locked -- --nocapture
cargo test -p xtask --test product_topology_cli --locked -- --nocapture

# required merge surface command 36: Dead-code API ledger contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --bin xtask --locked -- --nocapture dead_code_api_ledger
cargo test -p perl-parser --test dead_code_api_compat --locked -- --nocapture

# required merge surface command 37: Required-context mirror contract (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test workflow_draft_snapshot_supersession --locked -- --nocapture

# required merge surface command 38: Activation inventory suite (required merge surface)
_step_index=$((_step_index + 1))
cargo test -p xtask --test activation --test activation_check --locked -- --nocapture

# required merge surface command 39: Formal verification inventory and refusal controls (required merge surface)
_step_index=$((_step_index + 1))
set -o pipefail
cargo test -p xtask --bin formal-verification-claims --locked -- \
  --test-threads=1 --color never | tee "$RUNNER_TEMP/formal-verification-inventory-tests.log"
# This canonical producer runs on Linux, including the Unix symlink control.
# Keep the exact denominator and named controls synchronized with the suite.
grep -Eq '^test result: ok\. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in ' \
  "$RUNNER_TEMP/formal-verification-inventory-tests.log"
for test_name in \
  registry_is_valid_at_committed_head \
  paths_reject_ambiguous_platform_and_pathspec_forms \
  bound_requires_a_positive_number_not_protocol_digits \
  claim_ids_have_closed_grammar \
  snapshot_reads_blobs_not_modified_working_files \
  dirty_and_untracked_inputs_refuse_commit_attribution \
  changed_head_refuses_mixed_identity \
  committed_symlink_is_not_an_evidence_source \
  source_markers_cannot_be_empty_or_stale \
  mutated_required_gate_markers_are_stale \
  removed_gate_with_surviving_comment_is_rejected \
  advisory_wrong_command_and_duplicate_gates_are_rejected \
  commented_or_optional_workflow_steps_are_rejected \
  forged_rollout_and_empty_runners_are_rejected \
  unknown_schema_fields_and_enum_values_fail_closed \
  reports_preserve_identity_limits_and_not_evaluated_result \
  native_proof_cannot_be_promoted_by_metadata \
  git_selectors_cannot_redirect_snapshot_or_fixture_mutations \
  containing_job_admission_cannot_be_disabled_or_replaced \
  required_gate_markers_bind_active_rows \
  removed_actual_unsafe_gate_with_retained_marker_is_rejected \
  inventory_surface_binds_declared_controls; do
  grep -Fx "test tests::$test_name ... ok" "$RUNNER_TEMP/formal-verification-inventory-tests.log"
done
cargo run -p xtask --bin formal-verification-claims --locked -- --check

# required merge surface command 40: Kani structured output refusal controls (required merge surface)
_step_index=$((_step_index + 1))
set -o pipefail
cargo test -p xtask --bin formal-kani-output --locked -- \
  --test-threads=1 --color never | tee "$RUNNER_TEMP/formal-kani-output-tests.log"
grep -Eq '^test result: ok\. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in ' \
  "$RUNNER_TEMP/formal-kani-output-tests.log"
for test_name in \
  complete_positive_report_is_not_a_proof_receipt \
  duplicate_json_keys_cannot_overwrite_native_failure \
  malformed_empty_oversized_and_trailing_json_are_rejected \
  tool_version_target_and_schema_mismatches_are_rejected \
  unknown_or_null_used_tool_identity_is_not_accepted \
  every_harness_array_rejects_missing_duplicate_and_foreign_rows \
  arrays_join_by_harness_identity_not_position \
  summary_counts_cannot_hide_missing_or_failed_work \
  zero_work_and_vacuous_expectations_are_rejected \
  each_property_count_is_recomputed_including_cover_statuses \
  missing_foreign_and_duplicate_covers_are_rejected \
  unsatisfied_and_unreachable_covers_defeat_aggregate_success \
  unreachable_required_assertion_is_not_vacuously_positive \
  ordinary_unreachable_safety_check_is_preserved_separately \
  wrong_status_vocabularies_and_cover_classifications_are_rejected \
  should_panic_automatic_and_loop_contract_semantics_are_not_reinterpreted \
  native_assertion_failure_is_reported_but_never_admitted \
  undetermined_unknown_and_solver_errors_never_become_positive \
  error_details_cannot_contradict_the_native_result \
  missing_null_fractional_and_negative_property_counts_are_rejected \
  duplicate_out_of_range_and_zero_check_ids_are_rejected \
  assertion_identity_cannot_be_missing_duplicated_or_reclassified \
  source_identity_requires_the_exact_emitted_path \
  coverage_external_solver_and_unknown_semantic_fields_fail_closed \
  expectation_files_have_closed_schema_and_no_duplicate_keys \
  unknown_metadata_changes_the_retained_raw_identity \
  no_cover_counterexample_control_requires_its_named_assertion \
  unrelated_failure_does_not_satisfy_a_negative_control \
  timeout_or_crash_is_not_a_successful_negative_control \
  inspection_binds_the_expectation_not_just_the_supplied_export \
  unwind_and_unsupported_failures_are_not_production_counterexamples \
  a_new_property_category_requires_explicit_profile_review \
  a_negative_control_cannot_hide_an_unsatisfied_required_cover \
  object_bit_configuration_cannot_be_omitted_or_changed; do
  grep -Fx "test formal_kani::tests::$test_name ... ok" "$RUNNER_TEMP/formal-kani-output-tests.log"
done
