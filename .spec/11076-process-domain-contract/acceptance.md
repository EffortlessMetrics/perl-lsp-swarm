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
| Deterministic fake/recording supervisors support cheap, race-free consumer tests | the `FakeSupervisor` tests; the fake spawns no thread and reads no clock |
| Canonical encoding/fingerprints are deterministic, bounded, versioned and path/secret-safe at the public boundary | `canonical_encoding_is_stable_under_construction_order`, `a_meaning_change_moves_the_fingerprint`, `environment_values_never_reach_a_public_identity`, `private_paths_and_bytes_are_redacted_in_debug_output`, `the_canonical_encoding_of_a_fixture_plan_is_locked_to_the_schema_version` |
| Existing callers compile only through an explicitly temporary bounded adapter where unavoidable | `the_legacy_seam_is_contained_and_owned`; `process::legacy` records what the seam cannot express and that `#1975` owns removal |
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
| 10 | the legacy adapter remains an unrestricted second production path | `the_legacy_seam_is_contained_and_owned` |
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
