# Acceptance: #11076

Every row below is proven by a named test in
`crates/perl-subprocess-runtime/tests/process_domain_contract.rs`.

## Issue acceptance

| Acceptance criterion | Proof |
|---|---|
| One versioned `ProcessPlan`/`ProcessEvent`/`ProcessResult` domain exists in the current process crate | `every_shipped_profile_fixture_validates`, `a_scripted_run_streams_in_order_and_then_settles_once` |
| One pure validator makes invalid or unauthorized plans unstartable through the public port | the twelve `*_is_unstartable` / `*_must_be_*` tests; `validation_is_the_only_route_to_a_startable_plan` |
| Terminal process, supervisor, cleanup, truncation, stale, unsupported and not-proven states remain distinct | `a_control_plane_termination_never_becomes_an_ordinary_success`, `precedence_is_total_and_ordered`, `a_nonzero_exit_is_an_executed_result_not_an_instrument_failure`, `a_signal_and_an_unobserved_settlement_stay_distinct_from_success` |
| Exact executable, argv, cwd, environment, budgets, operation, authorization, source/configuration references and claim boundary are retained | `the_supervisor_records_the_exact_plan_it_was_given`, `validation_binds_the_fingerprint_it_validated` |
| Deterministic fake/recording supervisors support cheap, race-free consumer tests | the `FakeSupervisor` tests; `the_fake_supervisor_reads_no_clock_and_spawns_no_thread` pins the determinism mechanism itself |
| Canonical encoding/fingerprints are deterministic, bounded, versioned and path/secret-safe at the public boundary | `canonical_encoding_is_stable_under_construction_order`, `a_meaning_change_moves_the_fingerprint`, `environment_values_never_reach_a_public_identity`, `private_paths_and_bytes_are_redacted_in_debug_output`, `the_canonical_encoding_of_a_fixture_plan_is_locked_to_the_schema_version` |
| Existing callers compile only through an explicitly temporary bounded adapter where unavoidable | `the_legacy_seam_is_contained_and_owned` for the ledger, `no_unrecorded_second_execution_seam_exists_in_the_crate` for the crate; `process::legacy` records what the seam cannot express and that `#1975` owns removal |
| No OS process spawn or product behavior change occurs in this PR | `the_domain_never_reaches_for_an_operating_system_process_api`; no file outside the crate is touched |

## Negative controls

The issue lists twelve mutations the proof must reject.

| # | Mutation | Test that fails |
|---|---|---|
| 1 | a caller constructs a production plan without validation | `validation_is_the_only_route_to_a_startable_plan` |
| 2 | shell text or an ambient executable becomes authority | `a_shell_with_an_inline_command_is_unstartable`, `an_ambient_executable_lookup_is_unstartable` |
| 3 | environment values enter the semantic/public fingerprint | `environment_values_never_reach_a_public_identity` |
| 4 | timeout/cancel/nonzero/signal/cleanup failure collapse | `a_control_plane_termination_never_becomes_an_ordinary_success`, `precedence_is_total_and_ordered` |
| 5 | stdout and stderr identities collapse | `stdout_and_stderr_keep_separate_identities` |
| 6 | an output-limit result claims complete output | `truncated_or_limited_output_never_claims_to_be_complete` |
| 7 | a dropped handle implies successful cleanup | `a_dropped_handle_is_abandoned_work_not_proven_cleanup` |
| 8 | free-form owner/reason strings become policy authority | `a_correlation_identifier_cannot_change_a_policy_outcome` |
| 9 | a domain crate must import OS process APIs to use the port | `the_domain_never_reaches_for_an_operating_system_process_api` |
| 10 | the legacy adapter remains an unrestricted second production path | `no_unrecorded_second_execution_seam_exists_in_the_crate` (the ledger-only test is not sufficient on its own — see the review round below) |
| 11 | schema-bearing meaning changes without version movement | `the_canonical_encoding_of_a_fixture_plan_is_locked_to_the_schema_version` |
| 12 | LSP/DAP/tool-specific semantics leak into the process domain | `the_crate_takes_no_dependencies_that_could_carry_domain_semantics` |

Two further controls guard the proof itself: `a_shell_without_an_inline_command_is_not_refused_as_a_shell` stops the shell rule degrading into a program-name blocklist, and `an_unscripted_start_attempt_settles_as_not_proven` stops an unconfigured fake greening consumer tests.

## Mutation evidence

Each control was confirmed to discriminate by breaking the implementation and
observing exactly that control fail:

| Mutation applied to production code | Result |
|---|---|
| `TerminalDisposition::elect` drops the deadline branch | `a_control_plane_termination_never_becomes_an_ordinary_success` fails |
| `EnvironmentProjection::encode` also encodes addition values | `environment_values_never_reach_a_public_identity` fails |
| `ProcessResult::claims_complete_output` returns `true` | `truncated_or_limited_output_never_claims_to_be_complete` fails |
| `FakeHandle::drop` always reports `SettledBeforeDrop` | `a_dropped_handle_is_abandoned_work_not_proven_cleanup` fails |
| a public `ValidatedProcessPlan::trust_me` constructor is added | `validation_is_the_only_route_to_a_startable_plan` fails |
| `use std::process::Command` is added to `port.rs` | `the_domain_never_reaches_for_an_operating_system_process_api` fails |

## Adversarial review round (post-publication)

Two independent reviewers challenged the candidate on separate lenses. Seven
findings were confirmed and repaired; each repair carries its own control.

### Correctness lens

| Finding | Repair | Control |
|---|---|---|
| `AmbientInheritance::InheritExceptDenied` admits every ambient code-loading variable without ever tripping the acknowledgement gate, for 3 of 4 profiles — the permissive policy was the one place the gate never fired | `admitted_code_loading_variables` now counts ambient admission, not only named admission | `ambient_inheritance_cannot_smuggle_a_code_loading_variable`, `ambient_inheritance_is_startable_once_the_risk_is_faced` |
| `StdinPolicy::Streamed` is validated and shipped as a fixture, but `ProcessHandle` had no operation to drive stdin | `write_stdin`/`close_stdin` added to the port with a closed `StdinWriteOutcome`; the fake records what it accepted | `a_streamed_stdin_plan_can_actually_be_driven_through_the_port`, `a_plan_without_a_streamed_channel_refuses_stdin_rather_than_dropping_it`, `stdin_writes_after_settlement_are_refused` |
| `CleanupDisposition::Failed` produced `Limitation::CleanupNotObserved`, reporting a *known* failure as unknown confidence | `Limitation::CleanupFailed` added and emitted separately | `an_observed_cleanup_failure_is_not_reported_as_never_observed` |
| `MissingAuthorizationEvidence` rendered "is missing" for evidence that was supplied but of unknown freshness | message corrected | covered by `authorization_must_be_present_current_and_sufficient` |
| the shell rule missed long-form inline-command spellings (`--command=...`) | prefix spellings added | `a_long_form_inline_command_is_still_a_shell_invocation` |

### Proof-quality lens

The reviewer empirically defeated three structural controls while the defect
they guard remained present and compiling. All three are repaired and the
reviewer's own mutations were replayed against them.

| Defeated control | How it was defeated | Repair | Mutation replay |
|---|---|---|---|
| `validation_is_the_only_route_to_a_startable_plan` | a bypass constructor whose signature `rustfmt` wraps across lines slipped past a line-oriented scan | scan whitespace-collapsed, comment-stripped text, and additionally count `ValidatedProcessPlan` *constructions*, which no signature spelling can avoid | KILLED |
| `the_legacy_seam_is_contained_and_owned` | circular: it checked the containment ledger against itself, so a brand-new unfenced `pub fn` execution seam elsewhere in the crate was invisible | new `no_unrecorded_second_execution_seam_exists_in_the_crate` scans every crate source for functions producing `SubprocessOutput` and requires each to be declared | KILLED |
| `the_crate_takes_no_dependencies_that_could_carry_domain_semantics` | `[dependencies.log]` dotted-table syntax walked straight past a `line == "[dependencies]"` check | table headers are parsed, and any dotted or `target.*` dependency table fails | KILLED |
| acceptance claimed the fake "spawns no thread and reads no clock" with nothing asserting it | — | new `the_fake_supervisor_reads_no_clock_and_spawns_no_thread` | — |
| `PrivateBytes` stdin content is fingerprinted into the plan identity while `SecretValue` is excluded, with the asymmetry undocumented and untested | the two privacy tiers are now named and documented on `PrivateBytes` and `StdinPolicy::Bytes`, with guidance to use `SecretValue` for low-entropy secrets | `stdin_content_identifies_a_plan_while_its_bytes_stay_out_of_the_encoding` |
| structural scans used a non-recursive `read_dir`, so files moved into submodules in the follow-on lanes would silently stop being covered | `rust_sources_under` recurses | — |

One methodological note worth recording: two mutation runs first reported
SURVIVED and were false negatives of the harness, not weak controls — a
malformed patch in one case, and `--locked` refusing to build a newly added
dependency before any test could run in the other. Both were re-run correctly
and both controls killed their mutation. A mutation that "survives" without
the test binary having been built is not evidence of anything.

### External bot review lens (Devin)

Nine findings, all verified against the code and all repaired. The first was a
genuine panic in production code, introduced by the previous repair commit.

| Severity | Finding | Repair | Control | Mutation replay |
|---|---|---|---|---|
| critical | `arg[..prefix.len()]` panics when the byte index lands inside a multi-byte character, so an accented argument terminates the caller — in a crate that forbids production panics | `arg.get(..)` | `a_multibyte_argument_cannot_crash_the_validator` | KILLED |
| security | a plan labelling `/bin/sh` as "perl" kept unrestricted shell execution, because the rule keyed on the caller-supplied logical name | the resolved path's file name is checked too | `a_shell_renamed_by_its_plan_is_still_a_shell` | KILLED |
| security | authorization evidence with an empty reference validated, so a plan could claim authority naming no decision a backend can verify | blank references refused | `authorization_must_identify_a_decision` | — |
| security | `is_code_loading_variable` compared case-sensitively, so `Perl5Lib` evaded the acknowledgement gate | case-insensitive, the fail-safe direction | `a_mixed_case_loader_variable_does_not_evade_the_gate` | KILLED |
| correctness | durations encoded as milliseconds, so two plans with different sub-millisecond timing shared one identity — breaking the injectivity the canonical encoding exists for | seconds and nanoseconds encoded separately | `submillisecond_policy_differences_change_the_plan_identity` | KILLED |
| correctness | `PrivatePath::fingerprint` hashed `to_string_lossy`, which is not injective for non-UTF-8 Unix paths, so two different executables shared a public identity | platform-native bytes with a platform tag | `distinct_non_utf8_paths_keep_distinct_identities` | KILLED |
| correctness | a *removed* loader variable still counted as admitted, so a plan that eliminated the risk was still forced to acknowledge it | removed names excluded | `a_removed_loader_variable_is_not_admitted` | — |
| correctness | `ProcessResult::new` accepted swapped channels and evidence claiming a completeness it lacked | `ProcessResult::new` is fallible and validates stream coherence | `a_result_cannot_carry_swapped_or_incoherent_stream_evidence` | KILLED |
| correctness | `CompletedExit { code: 0 }` could be paired with `CleanupDisposition::Failed`, so `is_ordinary_success` returned true for a run whose cleanup failed — bypassing the precedence rule by constructing the result directly | that pairing is refused | `a_completed_exit_cannot_be_paired_with_a_failed_cleanup` | KILLED |

The locked canonical fingerprint moved to `1ec851f73284fbebad7abfd4c5662ac8`
because the duration and path encodings legitimately changed. That is the
locked-fingerprint control working: the encoding cannot change quietly.

A second methodological note: the panic control first reported its mutation as
surviving, and the fault was in the *fixture*, not the control —
`is_shell_invocation` short-circuits on a non-shell executable, so the argument
scan never ran. A control whose fixture cannot reach the code under test proves
nothing, and the replay is what exposed it.

### Second external round (Devin, `1e53643`)

Eleven findings. Nine repaired; two refuted with reasoning.

| Severity | Finding | Disposition |
|---|---|---|
| security | NUL bytes in environment names and values bypassed validation, so a plan passed authorization and then forced every OS backend to refuse the spawn | **Fixed** — refused in the validator with a typed reason |
| security | `validate_authorization` ignored `scheme_version`, so evidence from any unknown scheme was accepted | **Fixed** — `SUPPORTED_AUTHORIZATION_SCHEME` is checked; the reference stays opaque, its scheme does not |
| security | "callers can self-authorize execution" | **Refuted** — see below |
| correctness | `claims_complete_output` accepted supervisor failures and unproven outcomes, and `OutputIncomplete` did not track the predicate | **Fixed** — only a child that settled on its own terms can establish completeness, and the limitation is derived from the same rule |
| correctness | `check_stream` validated only the `Complete` variant, so truncated evidence could contradict the very limit it said stopped it | **Fixed** — per-variant invariants |
| correctness | blank root references and blank environment projection identifiers validated | **Fixed** — `BlankOpaqueIdentity` |
| correctness | `StreamChunkEvidence.channel` duplicated the `StdoutBytes`/`StderrBytes` variant and could disagree with it | **Fixed** — the field is removed, making the mismatch unrepresentable rather than merely rejected |
| analysis | the fake merged every run's stdin into one buffer | **Fixed** — `stdin_written_for(run_id)` and `stdin_writes()` |
| analysis | a malformed script's swallowed events read as a clean end of stream | **Fixed** — settles as `SupervisorFailed` |
| analysis | `Limitation::LinuxOneShotProfileOnly` was never attached by anything | **Fixed** — removed; a variant nothing derives is a false promise, and #11085 can add it when receipts carry the profile |
| info | `allocate_run_id` saturates into duplicate ids | **Refuted** — see below |

#### Refutations

**"Callers can self-authorize execution."** True as stated, and deliberate. #11076
requires an *opaque, versioned authorization evidence reference* and forbids
this domain from inventing execution-authorization policy, which #1753 owns:
"Use an opaque/versioned authorization evidence reference until that contract
is current." The domain records the decision it was handed and refuses stale,
unknown-freshness, unproven, blank, and now unknown-scheme evidence; it cannot
verify the decision without importing the authority that makes it, which is
precisely the widening the packet prohibits. `AuthorizationStrength` is a
closed enum rather than caller text for the same reason. The residual risk is
recorded in the claim boundary, not hidden.

**"Run allocation saturates into duplicates."** Reachable only after 2^64
start attempts against one `FakeSupervisor` instance in one test process. At
one allocation per nanosecond that is roughly 584 years. Adding code for it
would be scaffolding, not safety.

## Terminal precedence

Fixed and total, highest first:

```text
supervisor failure
output-limit exceeded
deadline reached
cancellation (running, else before start)
cleanup failure
child signalled
child exited
not proven
```

Control-plane causes outrank the child's own settlement, which is what stops a
timeout or cancellation becoming an ordinary success when the child exits zero
during cleanup. Cleanup failure sits below the causes that describe *why* the
run ended and is additionally always recorded in `ProcessResult::cleanup`, so
electing another cause never discards it.

## Explicit non-claims

- `Fingerprint` is FNV-1a 128: canonical identity and change detection only.
  Not collision-resistant against an adversary, never an authenticator.
- Two plans differing only in a secret environment value share a semantic
  fingerprint. Deliberate: a fingerprint of a low-entropy secret is a guessable
  secret.
- `EvidenceClass::Fake` results prove that a consumer handles a disposition.
  They never stand in for evidence that a real process behaves that way.
- Nothing here claims sandboxing, isolation, or hermeticity;
  `Limitation::NoIsolationClaimed` is attached to every result.
