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
- Public entrypoint `first_hour_profile::index_first_hour` takes `PacketV2` +
  `TopologyRequirements` + in-memory `FirstHourFacts` and calls `validate_v2`
  itself; no caller-supplied report can bypass canonical checks.
- The complete actual `ValidationReport` is retained in profile output with
  its exact recommendation and permanent `not_proven` ceiling. Window slices
  select cell obligations only; they never replace the full report.
  Human and agent slices repeat shared canonical obligations. Those copies are
  not independent human or agent observations.
- CLI: `xtask/examples/pre_freeze_public_beta_acceptance.rs` with optional
  `--profile-index`. Historical `pre_freeze_public_beta_acceptance.v1`
  separation preserved; `first_ten_minutes.v1` remains the #5902 study contract.
- Wire packet unchanged. No parallel cell IDs, no new JSON producer schema,
  no parsed bindings or ledger protocol, no new status or denominator.
- `profile_digest` is SHA-256 of compact UTF-8 JSON containing the index
  kind, this document normalized to LF, the ordered observer names, and the
  ordered window/canonical-cell partition. It identifies the applied profile
  independently of the supplied product subject. It authenticates no run.
- Output retains declared phase/source/release, artifact inventory, and row
  host/profile/configuration/fixture content digests. These are declarations
  from the validated canonical packet; observed identity and generations
  remain external proof obligations.

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
- References: #6056 adjudication
  <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056#issuecomment-5750256278>
  and Friday platform reconciliation at
  <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768#issuecomment-5925014719>.
- Missing Windows producers/mappings stay `not_proven`; never invented IDs
  or downgraded smoke.
- FULL/UTF-16 and #9380 / #8286 / #8384 / #8657 prerequisites preserved
  without expanding the incremental programme. #16981 / #16991 byte
  observers retain their code and ownership; collector/harness files untouched.

## Evidence obligations and NOT_PROVEN boundary

Genuine first-hour observation has no concrete producer yet. Each of the six
observer/window slices therefore carries an explanatory `missing` checklist
(plain strings, no locator/digest/owner/category) and stays `not_proven`;
`installed_qualification = not_proven` permanently. In-memory
`FirstHourFacts` are non-authoritative and never persisted. Correctly recorded
assistance stays `not_proven` and must name the assistance plus the unmet
public instruction; a bare intervention count is rejected.

Missing checklist names: public instruction entrypoint/version/content
digest; actual human/fresh-agent identity and allowed environment; installed
server/DAP/VSIX archive bytes and provenance; real host and clean
profile/config generation; fixture source/config/trust and
root/document/session generations; verified 5/15/60 windows and
first-useful/first-correct timings; conventional AND dynamic fixture
execution; safe edit/recovery/restart/update/cleanup outcomes; sustained
request/edit/cancel/indexing overlap and shutdown mode; and
assistance/intervention with unmet public instruction.

- PacketV2 has no document/session generation or instruction-digest
  producers. Canonical Subject joins and the `mixed_generation_result`
  counter fail closed, but they do not prove runtime generation binding.
- Declared fixture IDs do not prove conventional/dynamic execution.
- Byte identity declarations do not authenticate bytes, provenance, or DAP.
- Environment digest or copied declaration does not establish bytes or
  provenance. LSP/VSIX leaves do not fill DAP or source-provenance cells.
- Byte leaves cannot substitute DAP identity or authenticated provenance.
- Synthetic or 30-second runs test the validator but never satisfy the
  60-minute cell, even at 3600 in-memory seconds. Safe refusal stays distinct
  from failure; forced kill, timeout, cancelled/skipped, and
  missing/instrument-failed stay non-pass.
- Zero-budget counters and #5902 friction taxonomy reused; no averaging
  hides unsafe edit, cross-root leakage, or other trust breakers.
- Mechanism consistency (declared vs computed recommendation) is enforced by
  the canonical validator; it is not runtime acceptance.

## Falsifiers (focused wrapper regressions)

`pre_freeze_public_beta_acceptance/tests.rs` calls the safe public wrapper
(therefore the actual `validate_v2`) and rejects: wrong artifact/target/
platform binding; wrong Subject; missing/duplicate row/cell and missing
observation/provenance/DAP; lost Linux floor; omitted
source-inspection/private-rescue/hidden-assistance details; guided-expert or
cross-filled observers; synthetic/short run as hour; and stale Ready after
nonzero `unsafe_edit`/`mixed_generation_result`/`cross_root_leakage`. It
proves the exact canonical obligations are retained, deterministic ordering
under valid collection reordering, safe refusal via a real `SafeRefusal`
proposition plus `AcceptedClaim`, and correct `Blocked` projection. A small
module-only test keeps the fixed 5/15/60 partition. Mechanism tests never
claim genuine installed acceptance.
