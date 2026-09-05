# Acceptance: #12823 checkpointed corpus warm lane

Each row binds one stable proposition to its discriminating executable proof.
Proof lives in `xtask/tests/corpus_ratchet_checkpoint_policy.rs` (structural
YAML policy pins, house pattern of `ux_regression_gate_workflow_policy.rs`) and
`xtask/src/tasks/cpan_corpus.rs` unit tests.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| CRW-001 | A dedicated schedule-gated `corpus-warm-full` job exists and never runs on `pull_request` | `warm_job_is_schedule_gated_and_never_pull_request` | offline |
| CRW-002 | The install step always executes (restore-hit no longer skips it), so the cpanm frontier advances monotonically even over warm caches | `install_step_is_unconditional_in_warm_job` (mutation control: reintroducing the `cache-hit != 'true'` skip turns this red) | offline |
| CRW-003 | Install carries an explicit wall-clock budget strictly below the observed preemption envelope, leaving setup+save headroom inside a 30-minute job ceiling | `install_budget_pins_below_preemption_envelope` — asserts `--time-budget-minutes 12`, `timeout-minutes: 30`; any raise above 15 or above 12→envelope margin turns it red | offline |
| CRW-004 | Completion is machine-readable: the install emits a final `CPAN_CORPUS_INSTALL_COMPLETE=true/false` marker, captured into `outputs.complete` | `completeness_marker_is_captured_into_outputs` + xtask `complete_marker_reflects_batch_plan_outcome` | offline |
| CRW-005 | Canonical corpus cache saves exactly when the corpus completed; otherwise a rolling per-run checkpoint key banks consistent partial state, and neither save is skipped by restore hits | `canonical_save_gated_on_completion_and_checkpoint_save_exists_ungated_on_hit` | offline |
| CRW-006 | No assertion anywhere is weakened: on completion the gate chain runs the identical sweep → ratchet → enforce commands; no configured timeout of any job increases vs base pin | `gate_chain_commands_are_byte_preserved` + `no_timeout_minutes_exceeds_base_pin_maxima` (mutation controls: editing either command string or raising any `timeout-minutes` turns these red) | offline |
| CRW-007 | `corpus-ratchet-full` only enforces when `needs.corpus-warm-full.outputs.complete == 'true'`; during convergence it skips neutrally instead of falsely reporting failure from platform kills | `ratchet_job_gates_on_warm_completion_output` | offline |
| CRW-008 | Budget expiry stops batch planning cleanly: with an exhausted budget at most zero further batches start, state stays consistent, and the run reports incomplete rather than failing | `expired_budget_attempts_no_further_batches_and_reports_incomplete`; `per_batch_timeout_caps_at_fixed_batch_ceiling_and_remaining_budget` | offline |
| CRW-009 | Unlimited budget absence preserves today's behavior byte-for-byte (default config carries no deadline; plan completes all batches) | `unbudgeted_install_matches_legacy_behavior` | offline |
| CRW-010 | Red-first receipts-of-record: legacy full lane pinned by its exact failure strings (`outcome == 'success'` save gate) as mutation-control fixtures that the new shape forbids | embedded constants in policy test + live run receipts (runs 32946491767, 32825363442, 32705081617 …) cited in checklist.md | offline + receipts |

## Mutation controls (must stay red if reintroduced)

- Restore of cache skipping the install in the warm job → CRW-002
- Removing/lifting `--time-budget-minutes` beyond the envelope → CRW-003
- Saving canonical key without the completion marker, or deleting the rolling
  checkpoint save → CRW-005
- Any edit to the gate-chain command strings or baseline/manifest scope list →
  CRW-006
- Ratchet job running unconditionally again (dropping `needs.…complete`) →
  CRW-007
- Increasing any `timeout-minutes` in this workflow beyond base maxima
  (bounded 30 / full-lane legacy 120 removed downward-only) → CRW-006

## Non-proof residuals (named, not silently dropped)

- SIGTERM provenance remains externally unidentified (issue direction 2,
  diagnostics part): the design sidesteps provenance by ending before the
  envelope; runner-debug investigation stays open on the issue.
- Cache-volume economics: rolling checkpoint keys accumulate until LRU eviction
  under the repo-wide 10 GB limit. Mitigated by daily-key immutability
  (same-day re-saves are harmless no-ops) and restore-prefix reuse; a periodic
  prune/retention design stays open if eviction pressure ever displaces other
  lanes' caches.
