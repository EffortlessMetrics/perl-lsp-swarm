# First-hour execution profile index (#17038)

Parent release execution: #13768. Installed acceptance authorities: #6056 /
#4346 / #4350. Experience owners: #16668 / #5900 / #5902. This document is a
deterministic index beside the canonical installed authority. It is not a
runner, receipt schema, status model, release controller, or acceptance
denominator. #6056 / #4346 still own acceptance and #13768 still owns the
joined RC result.

## Canonical basis (reused, not copied)

- Authority: `xtask/src/pre_freeze_public_beta_acceptance.rs`,
  `SCHEMA = pre_freeze_public_beta_acceptance.v2`.
- Denominator: exactly `ROWS[3]` by `CELLS[24]`, validated by `validate_v2`.
- Types reused: `PacketV2`, `Subject`, `EvidenceRef`, `JourneyRow`,
  `TopologyRequirements`, `ValidationReport`, `EvidenceRequirement`,
  `Status`, `Recommendation`, `InstalledQualification`.
- CLI: `xtask/examples/pre_freeze_public_beta_acceptance.rs` with optional
  `--profile-index`. Historical `first_ten_minutes.v1` separation preserved.
- Wire packet unchanged. No parallel cell IDs, no new JSON producer schema,
  no parsed bindings or ledger protocol, no new status or denominator.

## Profile windows (observation windows, not latency promises)

Windows index canonical cells; all 24 cells on each of the 3 host rows remain
governed by `validate_v2`. Checkpoints record first-useful/correct timings;
they never promise that all projects index within those times.

### By minute 5: install, boot, first useful answer

Canonical cells: `install_upgrade_identity`, `startup_readiness`,
`diagnostics`, `completion`, `definition`, `result_state_distinctions`.

New human follows the public install route from a clean environment,
identifies the binary and readiness state, opens an ordinary known Perl file,
gets fixture-exact diagnostic/completion/navigation, and distinguishes
indexing, legitimate empty, dynamic limitation, and action-required failure
without repository knowledge. Fresh agent receives only the task, public
entrypoint, fixture, and allowed environment, selects the intended tool,
produces the same expected answer, and records typed blocked/not-proven
rather than guessing.

### By minute 15: ordinary change and correct recovery

Canonical cells: `hover`, `references`, `document_symbols`,
`workspace_symbols`, `unicode_crlf_edit_save_requery`,
`file_create_rename_delete`, `safe_rename_or_refusal`,
`whole_document_formatting`, `native_only_critic`, `doctor_optional_tools`,
`diagnosis_and_identity_repair`.

Both observers edit/save/requery current answers, exercise FULL/UTF-16
Unicode/non-BMP and CRLF/LF, use one retained formatting or
safe-edit/refusal path, recover through one bounded public remedy, and keep
native health without optional Perl::Critic/perltidy.

### By minute 60: sustained ordinary use

Canonical cells: `workspace_multiroot`, `retained_code_action`,
`dap_preview`, `retained_test_entry`,
`installed_contribution_reachability`, `restart_recovery_rollback`,
`shutdown_cleanup`.

Both observers complete a finite multi-file task with request/edit/cancel
cycles and indexing overlap, close/reopen/restart through the documented
route, prove update/repair or truthful reload-required/unsupported, and
finish with correct identity, no stale results, no unsafe edit, no silent
false success, and no owned process residue.

At least conventional and dynamic fixtures remain; framework/test/multiroot/
DAP cells cannot be erased. Human and fresh-agent rows stay distinct; a
guided expert run fills neither. No every-human/every-agent/every-host
denominator is invented where the authority records observed rows only.

## Platform and prerequisites (referenced, not invented)

- #13768 Gate 3 activates #4346's controller-expansion exception.
  `windows_x64_current_stable` retains the full retained journey at the
  recorded host; Linux minimum/current both retained. No new Windows
  two-version matrix, no macOS semantic parity.
- Reference: #6056 adjudication
  <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056#issuecomment-5750256278>
  and `inputs/friday-platform-reconciliation.json` at
  <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768#issuecomment-5925014719>.
- Missing Windows producers/mappings stay `not_proven`; never invented IDs
  or downgraded smoke.
- FULL/UTF-16 and #9380 / #8286 / #8384 / #8657 prerequisites preserved
  without expanding the incremental programme. #16981 / #16991 byte
  observers retain their code and ownership; collector/harness files untouched.

## Evidence obligations and NOT_PROVEN boundary

Genuine first-hour observation has no concrete producer yet. The native
`index_first_hour` function therefore emits six outstanding
`EvidenceRequirement` obligations (`first_hour_observation` /
`new_human|fresh_agent` × `first_5|15|60_minutes`, category
`FirstTenMinutes`, locator `missing:genuine-first-hour-observation:*`) and
keeps `installed_qualification = not_proven` permanently. In-memory
`FirstHourFacts` are non-authoritative and never persisted.

- Environment digest or copied declaration does not establish bytes or
  provenance. LSP/VSIX leaves do not fill DAP or source-provenance cells.
- Byte leaves cannot substitute DAP identity or authenticated provenance.
- Synthetic or 30-second runs test the validator but never satisfy the
  60-minute cell. Safe refusal stays distinct from failure; forced kill,
  timeout, cancelled/skipped, and missing/instrument-failed stay non-pass.
- Zero-budget counters and #5902 friction taxonomy reused; no averaging
  hides unsafe edit, cross-root leakage, or other trust breakers.

## Falsifiers (focused native tests)

`first_hour_profile::tests` rejects: wrong artifact/source binding; missing
or duplicate rows/cells; guided-expert or cross-filled observers; omitted
source-inspection/private-rescue intervention; synthetic/short run as hour;
byte leaf for DAP/provenance; lost Linux floor or canonical denominator;
lost safe-refusal accepted-claim obligation; and any recomputed or averaged
recommendation. Canonical packet/observation/counter/refusal semantics stay
covered by the existing validator tests. Mechanism tests never claim genuine
installed acceptance.
