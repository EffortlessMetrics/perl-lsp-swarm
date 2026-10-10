# Read-only proof reuse assessment

Subjects: helper `7432f164ca08f5802b9f246ded384a6f1066411d`; corrected selection `71a69a477665e98dd11d1be163a4d08cea51ef64`; main `1f973039d812fe080abddc44e461112d7b95a6fa`.

No build, test execution, hosted run, source mutation, push or publication occurred in this assessment. Local evidence extraction only.

## Actual hosted baseline

Git trees for main, PR17507 head12c162c8fe3534f40a61adc88a407d743b77ffe9 and integration4a08fa648f5a69e16bb13c86035ab4c28a5285b3 are identical:069cc53d8819b43b537f7366a30537e4d6b60ef3. Hosted run38057828834 used native Rust1.95.0. Source-tree identity permits reuse of main receipts subject to owning-input checks; it does not make every candidate receipt current.

- PR Smoke job114230000644 actually completed25/25 gates. Its unit_control_plane_bins selector ran only gates,ci_scope,workflow_policy_lint,workflow_trigger_lint in the main xtask binary, plus a separately successful ci_subject integration gate. It did not run unit_routed_full.
- Policy job114230039836 actually passed the compiler_performance_receipt filter and entire postfix-capability-closure bin test harness. Those source modules, compiler contracts and embedded status documents remain unchanged at both candidates. compiler_performance_receipt's committed-source control reads its own unchanged module. These bounded obligations can reuse the hosted receipt. Production ledger validators and doc tests are separate commands and cannot substitute for unrelated unit populations.
- Compile All Targets job114230000653 records a successful LSP JSONRPC dependency-probe step. Main workflow specifies cargo test -p xtask --test lsp_jsonrpc_dependency_probe --locked -- --nocapture. Probe source, model inputs and registry versions are unchanged at both candidates. This is actual successful step-level evidence for the unchanged probe selector. Its raw job log could not be fetched (Transport closed); do not claim an independently inspected two-pass summary or native negative diagnostics from that log.
- UX job114230000565 actually linked perllsp with cargo build -p perllsp --bin perllsp and completed product tests. Product source, root manifest and registry dependency versions are unchanged. This discharges the bounded historical product-link behavior; it is not the literal locked preparation command and provides no verified local executable/handoff for PERL_LSP_BIN.
- Support job114230039732 actually passed its multi-package library selector including the old hygiene library. New helper/selection library receipts supersede that old population; it does not cover the new library merely because the package name is the same.
- Actual main library/bin Clippy receipts remain usable for unchanged owning inputs under the separately reviewed affected-lint reconciliation. No fresh canonical candidate command is claimed.

## Local and selection receipts

Keep these populations separate:

- Helper7432: fresh155 library tests; prior448 affected xtask tests across ten filtered/exact executions; corrected two machine-path fixture tests; strict all-target helper Clippy. 448 includes partial main-bin filters and five integration targets, not448 harnesses.
- Selectionce99: actual213 library+432 binary+73 integration=718 tests across all ten hygiene harnesses, using the agent profile. Corrected71 receipt5e7a7ba9c0223c0e409bf1f6854d4e38756741a8 binds the fixture-only correction through unchanged controlled inputs and expanded-token/runtime-string identity; no718 rerun. This discharges that bounded hygiene behavior population under the accepted semantic reuse; it is not a fresh default-dev canonical full command.
- Corrected71 fresh15 real-caller parity and nonempty10/10 commit gate receipt qualify changed selection adapters; negative conflict still rejects. These are separate from exact ci_subject/ci_scope integration selectors.
- The 277 default eligible lib/bin/integration harnesses are inventory: hygiene10+xtask267. Never subtract test counts155/448/718 from277. Eight xtask examples are outside that runtime harness count although Cargo may compile examples.
- Helper448 cannot be projected wholesale onto71: relocated ci_scope tests, changed ci_subject/ci_scope adapters, changed scope fixture and changed root workflow/policy inputs require a per-test source/input bridge. Unchanged source alone is insufficient for tests reading the real repository root. See independent audit addendum when available.

## Exact residual

No receipt was found executing literal:
cargo build -p perllsp --locked &&
env PERL_LSP_BIN="$PWD/target/debug/perllsp" cargo test --locked --tests -p perl-ci-hygiene -p xtask

For a literal full-gate claim, retain:
1. Exact locked product preparation or an explicitly reconciled equivalent native artifact receipt; private output/source/profile/tool/config identity and admitted PERL_LSP_BIN handoff. Historical hosted link does not provide local runtime bytes.
2. Exact remaining xtask harness/test-selector coverage from the267 inventory, after recording bounded hosted/local subsets and source/runtime-input equivalence. Full xtask library, other binary/integration populations and real-root-sensitive filters cannot inherit a blanket pass.
3. Six lock_partition compiler fixtures and disallowed_fields compiler fixture: no hosted main execution receipt located for these seven names. Production check-lint-policy success does not run them. Parent-owned b5955c0cbb73f4a7da7312ef4393ba51a4366192 prepares the ten nested rows; do not duplicate ownership. JSONRPC step can support unchanged behavior, but its main raw commands differ from the prepared admission-consumer implementation and cannot substitute for candidate admission/capacity/closure proof.
4. Parent-owned six dynamic dispatch groups/transitive shell/exact selection/aggregate artifact capacity inventory. Do not infer complete reachability from the ten known nested rows.
5. Installed/prepush/runtime/platform obligations remain separate. Native Windows and actual full-hook execution remain NOT_PROVEN.

Hosted artifact metadata was readable, but signed downloads were blocked403, including the exact Library transfer helper attempt. No raw artifact zip or product executable was materialized. Excerpts and successful job metadata are preserved in proof-reuse-hosted-excerpts.json; existing residual-target-metadata.json and remaining-selected-target-map.json hold the inventory.

Assessment incremental compiler/storage cost: zero compiler runs and no target allocation. Existing measured qualification costs remain in preserved receipts. No40GiB guard change or bypass.

## Independent read-only input audit

Do not reuse the following helper448 rows at71 without a source/input bridge:
- workflow_policy_lint::shipped_workflows_enumerate_xtask_cli_wiring; runner_mismatch_skipped_for_stale_ref_in_multi_job_workflow; shipped_tree_has_no_undeclared_or_orphaned_self_hosted_capacity; shipped_profiles_are_current_today. All read changed ci.yml directly or through enumeration of all shipped workflows.
- gates::plan_gates_nightly_tier_never_selects_a_commit_tier_gate; plan_gates_all_tier_selects_the_commit_tier_gate; plan_pr_fast_gates_falls_back_broadly_when_explicit_base_does_not_resolve. Planner/owner inputs changed even where the fail-closed branch precedes metadata.
- gates::agent_receipt_builder_preserves_scope_status_and_plan_contract; static_gate_plan_threads_staged_tree_oid_into_agent_receipt; static_gate_plan_leaves_staged_tree_oid_none_when_not_staged. Assertions can be invariant, but current Git identity/root inputs differ.
- ci_subject integration::ci_workflow_routes_scope_gate_contract_and_windows_cache_inputs_through_subject reads the real changed workflow; the other four integration tests are hermetic but still need changed CLI binding reconciliation.

The14 workspace-doctor inventory contract/falsifier fixture-root tests retain exact inputs helper→71: support.rs changed main→helper, not helper→71. Gate tests depending only on unchanged .ci/gate-policy.yaml retain that file input. These are bounded row-level reuses; no whole-filter count or full-bin pass follows.
