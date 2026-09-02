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
| Fingerprints are deterministic, versioned, path/secret-safe, and bounded to 128 bits; the canonical encoding is deterministic, versioned, path/secret-safe, linear in plan size and non-amplifying, but **not** capped | `canonical_encoding_is_stable_under_construction_order`, `a_meaning_change_moves_the_fingerprint`, `environment_values_never_reach_a_public_identity`, `private_paths_and_bytes_are_redacted_in_debug_output`, `the_canonical_encoding_of_a_fixture_plan_is_locked_to_the_schema_version` |
| Existing callers compile only through an explicitly temporary bounded adapter where unavoidable | `the_legacy_seam_is_contained_and_owned` for the ledger, `no_unrecorded_second_execution_seam_exists_in_the_crate` for the crate; `process::legacy` records what the seam cannot express and that `#1975` owns removal |
| No OS process spawn or product behavior change occurs in this PR | `the_domain_never_reaches_for_an_operating_system_process_api`. Outside the crate this PR changes only the three `.spec/11076-process-domain-contract/` files and the regenerated `docs/policy/NON_RUST_INVENTORY.md`; no product source is touched |

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
| `PrivateBytes` stdin content is fingerprinted into the plan identity while `SecretValue` is excluded, with the asymmetry undocumented and untested | the two privacy tiers are now named and documented on `PrivateBytes` and `StdinPolicy::Bytes`, with guidance to use `SecretValue` for low-entropy secrets | `stdin_content_identifies_a_plan_while_its_bytes_stay_out_of_the_encoding` | — |
| structural scans used a non-recursive `read_dir`, so files moved into submodules in the follow-on lanes would silently stop being covered | — | `rust_sources_under` recurses | — |

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

### Third external round (Devin, `e5b908b`)

Ten findings. Eight repaired; two answered as accepted boundaries.

| Severity | Finding | Disposition |
|---|---|---|
| security | `SHELL_PROGRAMS` omitted `powershell.exe`/`pwsh.exe`, so a Windows shell passed `-Command` under the name it actually has | **Fixed** — executable suffixes are stripped before matching, rather than enumerated, and `ash`/`rbash`/`busybox` added |
| security | case-mismatched denials and the loader gate | **Fixed** — set membership now folds case like detection does; see the note below on the direction |
| correctness | `RefuseStart` accepted `CompletedExit { code: 0 }`, so a start that never ran a child read as an ordinary success | **Fixed** — only refusal-shaped dispositions are accepted; anything else settles as `SupervisorFailed` |
| correctness | a scripted terminal event in the final position could announce an outcome that disagreed with what `wait` returned | **Fixed** — the handle owns the terminal event; any scripted one refuses the script |
| correctness | `Signaled` with failed cleanup, and `CleanupFailed` with completed cleanup, both validated | **Fixed** — both directions of the precedence rule are enforced |
| correctness | `supervisor_failure` claimed cleanup was unnecessary, though a supervisor failure can happen after the child started | **Fixed** — the conservative `NotObserved`/`Unknown` pair |
| correctness | `supervisor_failure` bypassed limitation derivation, so its predicate and its published limitations disagreed | **Fixed** — one `derive_limitations` shared by every assembly path |
| correctness | `ObservationTruncated` could still report observing more than the limit it said stopped it | **Fixed** — exact equality |
| hygiene | `unwrap_or_else` in production conflicts with the repository's ban on `unwrap` forms | **Fixed** — no `unwrap` spelling remains anywhere under `src/process/`, enforced by `the_domain_uses_no_unwrap_spelling_in_production` |
| analysis | retention enforcement remains backend-owned; result assembly cannot see the plan's retention policy | **Accepted boundary** — see below |
| analysis | text scans are incomplete ratchets | **Accepted boundary** — see below |

#### On the case-mismatch direction

The report described this as a bypass: "denying `perl5lib` can still inherit
`PERL5LIB` without acknowledgement". Traced through the code, the actual
behaviour was the opposite — the canonical name stayed in the admitted set, so
the gate *fired* and the plan was refused. That is over-rejection, not a
bypass: a plan that had already denied the vector was still asked to
acknowledge it.

The underlying inconsistency was real and is fixed: detection folded ASCII case
while set membership did not. Both fold now, which matches Windows semantics
and removes the incoherence in either direction.

#### Accepted boundaries

**Retention enforcement is backend-owned.** True. `ProcessResult::new` receives
components, not the validated plan, so it cannot check retained bytes against
`RetentionPolicy`. Passing the plan into result assembly would couple the two
and expand this PR's surface; the policy is carried on the plan for the backend
that applies it, and enforcing it is part of #11085's receipt work. Recorded
here rather than left implicit.

**Text scans are incomplete ratchets.** Also true, and already treated that way:
they were hardened after a reviewer defeated three of them, and they are
defence in depth, not the primary guarantee. The load-bearing guarantees are
type-level — private fields on `ValidatedProcessPlan`, closed enums, a fallible
result constructor — plus Cargo itself for the dependency claim. A scan that
can be evaded by an alias or a macro is worth having and is not worth
mistaking for a proof.

### Fourth external round (Devin, `fcb0bfc`)

Two findings, both real, both about the same failure mode this packet exists to
prevent: an event stream and a result that disagree.

| Finding | Repair | Mutation replay |
|---|---|---|
| `RetentionTruncated` was bounded on one side only, so evidence could retain *fewer* bytes than the stop point it named, or claim truncation with nothing beyond the limit to truncate | retention must equal its stop point exactly, and observation must exceed it — otherwise nothing was truncated | KILLED |
| refusing a scripted terminal event returned `None` without settling the run, so the next call emitted the *elected* terminal event — announcing a success while `wait` reported a supervisor failure | the rejection settles the stream on the same supervisor failure `wait` reports | KILLED |

The second is worth recording precisely: the guard added in the previous round
was correct about *what* to refuse and wrong about *how*. Leaving the stream
open reintroduced, one call later, exactly the divergence the guard existed to
prevent. Refusing a thing without settling it is not refusing it.

### Fifth external round (Devin, `01e447f`) — and a correction

One finding: `ProcessResult::new` still derived limitations inline while
`supervisor_failure` used the `derive_limitations` helper. Two implementations.

**This packet previously claimed otherwise, and that claim was wrong.** The
third-round entry above says "one `derive_limitations` shared by every assembly
path", and the commit that introduced the helper said the same. In fact the
edit replacing the inline derivation in `ProcessResult::new` silently did not
apply — the text it searched for had been reflowed by `rustfmt` — so the helper
was added and adopted by one caller only.

Nothing behaved incorrectly: the two copies agreed, so every behavioural test
passed and the duplication was invisible. That is exactly why the claim
survived. A green suite confirmed the behaviour and said nothing about the
structural property being asserted.

Both are now fixed: `ProcessResult::new` calls the helper, and
`limitation_derivation_has_exactly_one_implementation` fails if any constructor
derives limitations outside it. Confirmed by replaying the duplication.

The general lesson is recorded because it recurred: three separate edits in
this PR silently failed to apply after `rustfmt` reflowed their target text,
and in this case the failure was then asserted as done. An edit whose effect is
not verified is not a change.

### Sixth external round (Devin, `2dd567b`) — and a second correction

One new finding, plus one older finding that turned out never to have been
fixed despite being reported as fixed.

| Finding | Disposition | Mutation replay |
|---|---|---|
| a rejected script settles its stream with a terminal event, but the fallback result defaulted `events_emitted` to zero — the consumer receives events and is then told none were emitted | **Fixed** — `supervisor_failure` takes `WorkMetadata`, and every fake result path reports the ledger's count | KILLED |
| `ObservationTruncated` still admitted observing *past* the limit it named | **Fixed** — exact equality, as the second-round entry above already claimed | KILLED |

**Second correction.** The second-round entry above states that observation
truncation was fixed to exact equality. It was not: that edit silently failed
to apply — `rustfmt` had reflowed its target text — and the code still read
`observed_bytes() < limit_bytes` four rounds later. The control written at the
time only exercised observing *fewer* bytes than the stop point, so it passed
against the unfixed code and the claim went unchallenged.

That is the same failure as the `derive_limitations` correction recorded below
it, with the same two causes: an unverified edit, and a control that covered
one side of a two-sided invariant. Both are now closed, and the new control
covers the side that was missing.

The regression in the first row is worth noting for a different reason: it was
introduced by the previous round's own repair. Making the rejection settle the
stream was correct, and it made the previously-accurate zero event count wrong.
A fix that changes what a component does can invalidate a neighbouring
component's assumptions, which is why the full proof is re-run after each
round rather than only the affected test.

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

### Seventh external round (Devin, `3376e5c`) — a documented limitation was the wrong answer

One finding: `TruncationState` could not describe a channel that reached both
its observation and its retention bound, so such a run had to claim one of the
two had been complete.

I had found the same gap myself, in the independent final challenge run one
commit earlier, and **recorded it as a documented limitation deferred to issue
`#11085`**. That was the wrong call, and the reviewer was right to press it.

The reasoning I used was "no backend exists yet, so widening the type now is
speculative design." What that missed is that the deliverable of this PR *is*
the contract. A contract that cannot express an ordinary outcome is defective
as a contract, whatever exists downstream of it. And the outcome is ordinary:
`CaptureBudget` carries two independent limits, and the crate's own
`CaptureBudget::observe_only(n)` constructor sets `retain_limit_bytes: 0` with
`observe_limit_bytes: n` — a child that writes past `n` reaches both bounds.
That is not an exotic future case; it is the shipped convenience constructor.

Documenting a hole is honest, but honesty about a defect is not a substitute
for fixing one that is cheap to fix. It was also inconsistent with how this
same PR handled `StreamChunkEvidence.channel`, which was *removed* rather than
validated on the explicit grounds that unrepresentable beats checked.

`TruncationState` is now a struct carrying two independent optional bounds
rather than a choice between them, with `complete()`,
`observation_truncated()`, `retention_truncated()`, and
`observation_and_retention_truncated()` constructors. `check_stream` validates
each bound separately: an observation bound must equal the observed count, a
retention bound must equal the retained length and be exceeded by the observed
count, and an *absent* retention bound now asserts that every observed byte was
kept — which is the check that had been missing entirely.

| Finding | Disposition | Mutation replay |
|---|---|---|
| a channel reaching both capture bounds has no truthful state | **Fixed** — two independent facts, each with its exact bound | KILLED |

Two of this packet's own fixtures turned out to be instances of the reported
bug: `truncated_or_limited_output_never_claims_to_be_complete` and
`observation_truncation_must_match_its_stop_point_exactly` both declared
`ObservationTruncated { limit_bytes: 1024 }` while retaining 8 and 4 bytes
respectively. Under the old model that was silently accepted, because nothing
constrained retention when observation was the named bound. Under the new one
both are rejected until they state the retention bound they actually had. The
controls had been writing the defect they existed to catch — which is the
strongest evidence available that the gap was real and reachable, and not a
theoretical objection.

New control `a_channel_that_reaches_both_bounds_can_say_so` asserts the dual
state round-trips and that neither bound can be hidden. Mutation-verified:
restoring the old permissiveness — dropping the "absent retention bound means
everything observed was kept" check — kills exactly that control and no other.

### Eighth external round (Devin, `8550c02`) — including a round-10 regression

Five findings: four bugs, all real and all repaired, plus one analysis note that
turned out to be half-fixable.

| Severity | Finding | Disposition | Mutation replay |
|---|---|---|---|
| correctness | observation-truncated evidence skipped the fingerprint check, so it could publish the identity of content it never held | **Fixed** — the check is gated on retention being unbounded, not on completeness | KILLED |
| correctness | pre-start dispositions accepted child output, an observed cleanup, and a terminated process group | **Fixed** — `PreStartOutcomeCarriesChildEvidence` | KILLED |
| security | NUL in the resolved executable path and the cwd, and `=` or an empty string as an environment variable name, all passed validation and could only fail at the syscall | **Fixed** — refused in the validator with typed reasons | KILLED (×2) |
| correctness | the fake emitted the *elected* terminal event and only afterwards discovered the result could not be assembled, so the announced outcome disagreed with `wait` | **Fixed** — assembly is attempted before anything is announced | KILLED |
| analysis | chunk offsets, discontinuities, and totals are unchecked | **Half fixed** — see below | KILLED |

**The first finding was mine, from the round immediately before.** Round 10
made `retained == observed` an enforced invariant whenever retention is
unbounded, which newly made the fingerprint check *meaningful* for
observation-truncated evidence. I wrote the count half of that and gated the
fingerprint half on `is_complete()` — both bounds absent — which excludes the
very case round 10 had just made checkable. My own comment on the line stated
the correct rule ("when those are the whole of it") while the code below it
implemented a narrower one.

That is the third repair in this PR to introduce a defect: round 3's repair
introduced the production panic, round 4's made `events_emitted` false, and
round 10's left this fingerprint hole. The pattern is consistent enough to be
worth naming: a change that *strengthens* an invariant creates new checkable
consequences, and the repair is not finished until those are checked too.

**`UnsupportedBackend` in one of this packet's own fixtures was an instance of
the second finding**, paired with `b"partial"` stdout and
`TreeDisposition::GroupTerminated`. That is now the third fixture found writing
the defect it was meant to guard against. The fixture is split: causes that can
follow a started child keep their partial output, and the pre-start causes get
evidence a never-started run could actually carry.

**On the analysis note.** "Chunk coherence is backend-owned" is only half true,
and round 10's lesson was not to reach for a documented limitation before
checking whether the contract can just enforce the thing. `StreamChunkEvidence`
carries `offset` and `byte_count`, so per-channel continuity is entirely within
`EventLedger`'s reach: a chunk's offset must equal what that channel has already
admitted. `EventLedger` now enforces exactly that, per channel, with
`ChunkOffsetDiscontinuous`. An offset that skips ahead hides bytes and one that
goes backward double-counts them, and either way the events no longer reassemble
into what the run produced — the field existed to make reassembly possible and
was previously decorative.

What genuinely remains backend-owned is the cross-object half: the ledger never
sees the `ProcessResult`, so it cannot check that the chunk totals agree with
the result's `observed_bytes`. That one is recorded as a boundary because it is
actually outside this object's reach, not because fixing it would be
inconvenient.

### Ninth external round (Devin, `8538b86`) — a security bypass and a second live-branch regression

Five findings: four repaired, one answered as a standing claim boundary.

| Severity | Finding | Disposition | Mutation replay |
|---|---|---|---|
| security | bundled short-option clusters (`bash -lc`, `sh -ic`) bypassed the inline-command gate entirely | **Fixed** — a single-dash all-letters cluster containing `c`/`C` is an inline command | KILLED |
| correctness | scanning every argv position refused valid plans: in `sh script.sh -c` the flag belongs to the script | **Fixed** — the scan stops at `--` or the first operand, with a multi-call exception | KILLED (×2) |
| correctness | contradictory cancellation evidence became terminal truth | **Fixed** — election fails closed to `NotProven` | KILLED |
| correctness | a rejected chunk left the event stream open, so a later poll could announce success | **Fixed** — the rejection settles the run | KILLED |
| security | `Fingerprint` is unkeyed FNV-1a | **Standing boundary** — see below | — |

**The bypass was the serious one.** `bash -lc 'curl … | sh'` is an ordinary
idiom, and the gate compared whole argv tokens against `-c`, so every bundled
spelling walked straight through the boundary #11076 exists to enforce. The
detector now recognises three forms — exact flag, same-token prefix, and short
cluster — and counts `C` alongside `c`, because over-refusing a `noclobber`
cluster costs a caller one explicit plan while under-refusing one hands a shell
a command string.

**Fixing the over-rejection nearly opened a second bypass.** The obvious rule —
stop scanning at the first operand — breaks on multi-call binaries, where
`busybox sh -c 'cmd'` puts the shell's own name in the operand slot and would
have ended the scan before `-c`. The scan therefore continues past an operand
that is itself a shell, and stops at one that is not (`busybox ls -c` is `ls`'s
business). Both directions are controlled.

That rule also broke Windows shells on its first attempt: `/C` is not
dash-led, so an "options have ended" test placed before the flag test returned
early and let `cmd.exe /C` validate. `a_shell_with_an_inline_command_is_unstartable`
caught it immediately — the flag test now runs first. A pre-existing control
catching a regression introduced while fixing something else is the case for
keeping controls that look redundant.

**The fourth finding was a second regression of the same shape as round 11's.**
`FakeHandle::next_event` returned `None` on any ledger admission error without
settling. That was harmless while both error variants were unreachable on that
path — my own adversarial review had traced exactly that and dispositioned it
"not practically reachable," correctly, *for the code as it then stood*. Adding
`ChunkOffsetDiscontinuous` in round 11 made the branch live and I did not
revisit the judgement. The lesson is narrow and worth keeping: **a reachability
disposition is scoped to the code it was made against, and adding a variant to
an error enum invalidates every prior argument about matching on it.**

#### On the fingerprint

Standing claim boundary, restated rather than newly decided. `Fingerprint` is
unkeyed FNV-1a and an adversary can construct collisions, so two distinct paths
or plans could share a public identity. That is documented at the type, in the
module header, and in this packet, and the type is explicitly never an
authenticator.

Applying round 10's test — *does the contract fail to express something
ordinary?* — the answer is no. Change detection and canonical identity, the
only properties claimed, hold. Adversarial collision resistance was never
claimed, #11076 does not require it, and the crate's zero-dependency posture is
itself an asserted invariant. Whoever needs integrity against an adversary
(#11085's receipts) must layer a keyed or cryptographic digest over this, and
the boundary says so. This is the second security-flagged item accepted rather
than fixed, so: the first (self-authorization) was refused because #11076
assigns that policy elsewhere; this one because the property was never claimed
and cannot be added without breaking a stated invariant of the crate.

`EventLedger::observed_bytes` was also added this round — the accessor deferred
last round, riding along with a push that had to happen anyway, so a backend
can perform the totals join without recomputing from events.

### Tenth external round (Devin, `3ba4a5b`)

Three findings repaired, one answered with evidence.

| Severity | Finding | Disposition | Mutation replay |
|---|---|---|---|
| correctness | a completed exit or signal could carry `CleanupDisposition::NotRequired`, whose contract says nothing started | **Fixed** — `SettledChildCarriesNoChildCleanup` | KILLED |
| correctness | the fake inferred "the child started" from the poll count | **Fixed** — a `Started` event actually admitted | KILLED, after the first control failed to discriminate |
| security | `CODE_LOADING_VARIABLES` omitted `PYTHONSTARTUP`, `RUBYLIB`, `NODE_PATH` | **Fixed** — added, with the list's nature documented | KILLED |
| analysis | which public-API gates apply | **Answered with evidence** — see the PR thread | — |

The first is the **inverse of a rule I wrote two rounds earlier** and should have
written at the same time. Round 11 established that a disposition asserting no
child started cannot carry child evidence; the mirror — that a disposition
proving the child *ran* cannot carry evidence saying none started — was left
open. Adding a rule in one direction is a reason to check the other.

The second exposed the cost of an incomplete reconciliation. Setting
`started_before_cancellation` from real start state left the scripted
settlement saying `Exited`, so round 12's contradiction check elected
`NotProven` for an ordinary pre-start cancellation; reconciling the settlement
alone then left a *completed cleanup* beside a child that never ran, which
round 11's check refused. All of the child evidence — settlement, cleanup,
tree, and both streams — has to move together. Two of my own earlier controls
caught the intermediate states, which is the argument for keeping them.

**A control that did not discriminate.** The first version of
`cancelling_before_the_child_starts_is_a_pre_start_cancellation` cancelled
before any poll, where `admitted_count() > 0` and the real start state are both
false — so reverting to the poll count changed nothing and the mutation
*survived*. The control now scripts a run whose first event is a chunk rather
than `Started`, making the two measures diverge, and the mutation is killed. A
control that passes against the defect it names is worth no more than no
control, and only the replay distinguishes them.

#### On the loader-variable list

`RUBYOPT` was listed without `RUBYLIB`, `PYTHONPATH` without `PYTHONSTARTUP`,
`NODE_OPTIONS` without `NODE_PATH` — each omission the direct analogue of a
name already present, so this was an inconsistency in the list's own logic
rather than an unbounded request. Added.

The list can never be complete, and that is now stated at the constant: a name
absent from it is *unrecognised*, not proven safe. The list is a floor over the
known vectors; the actual boundary is `AmbientInheritance`, and a plan wanting
a guarantee uses `DenyAll` or `AllowListedOnly` rather than relying on this
list to have anticipated every runtime.

### Eleventh external round (Devin, `c9c27b5`) — the reconciliation was the wrong fix

One finding, and it was a regression from the round before it.

| Finding | Disposition | Mutation replay |
|---|---|---|
| pre-start cancellation cleared stream evidence a consumer had already been handed, so the result reported zero observed bytes against a delivered chunk event | **Fixed** — by refusing the premature event, not by reconciling it | KILLED |

Round 13 reconciled a pre-start cancellation by emptying the scripted stream
evidence. That is correct when nothing was emitted and wrong when something
was: a script can emit a nonzero chunk before `Started`, and a consumer that
polled it then held an event the result went on to deny. My own fixture used a
**zero**-byte chunk, which is exactly why it did not catch this.

The report offered two repairs — reject child output before `Started`, or
preserve the already-emitted evidence. Only the first is consistent with a rule
this contract already has. Round 11 established that an outcome asserting no
child started cannot carry output bytes, so preserving the evidence would
produce a result that same rule refuses. The event and the result would then
disagree about which of the two contradictions to report, which is the failure
mode this domain exists to prevent.

So `EventLedger::admit` now refuses `StdoutBytes`/`StderrBytes` before a
`Started` event with `ChildOutputBeforeStart`. Output requires a child; the
result-level rule and the event-level rule are the same statement one step
apart, and the earlier one is the better place to say it.

That also makes round 13's clearing safe rather than merely correct-in-practice:
a consumer can only hold a chunk if `Started` was admitted, in which case
cancellation sees a started child and clears nothing. The scenario is now
unreachable rather than reconciled.

Two existing controls had to change, both because they scripted sequences the
new rule refuses. `a_chunk_must_continue_from_what_its_channel_already_saw` now
admits `Started` before its chunks. `cancelling_before_the_child_starts_is_a_pre_start_cancellation`
needed a different way to make the poll count and the real start state diverge,
since its chunk-before-`Started` fixture is no longer admissible; it uses a
`TerminationPhase` instead, which is coherent before a start and keeps the
control discriminating.

Sixth self-inflicted regression, and the third in a row where the *shape* was
the same: a repair that reconciles a contradiction after the fact, where
refusing to create it would have been simpler and safer. The pattern is now
explicit in this packet because it kept recurring: **when two facts contradict,
prefer making one unrepresentable over teaching a later stage to paper over
it.**

### Twelfth external round (Devin, `868c4a0`)

Two findings, both repaired, both the same rule as round 11's extended to
neighbouring fields.

| Finding | Disposition | Mutation replay |
|---|---|---|
| `EventLedger` accepted a second `Started`, and accepted exit/signal/running-cancellation terminals with no start | **Fixed** — `ChildStartedTwice`, `ChildSettlementBeforeStart` | KILLED (×2) |
| `OutputLimitExceeded` accepted two streams that reached no bound | **Fixed** — `OutputLimitWithoutATruncatedStream` | KILLED |

Both are the round-11 statement applied one field over: *the child's own
account requires a child*. Round 11 said it about result-level output, round 14
about output events, and this round about terminal causes and start events. The
pre-start dispositions stay admissible without a start, because those are
precisely the outcomes of a run that never began — a control asserts that, so
the rule cannot degrade into "no terminal without a start."

**Two more of this packet's fixtures encoded the defect they sat beside.**
`truncated_or_limited_output_never_claims_to_be_complete` and
`only_a_settled_child_can_establish_complete_output` both built
`OutputLimitExceeded` results from fully complete streams — the exact
contradiction the new rule refuses. That is the fourth and fifth fixture in
this PR found writing a defect. The recurring cause is the same each time: a
fixture is written to exercise *one* property and its other fields are filled
in with whatever constructs, so it silently asserts things nobody checked.

## Standing pattern across the review rounds

Recorded once rather than repeated per round, because the same three shapes
account for most of the findings here:

1. **A rule stated for one field, not its neighbours.** Output, then terminal
   causes, then start events; the pre-start rule, then its inverse. Adding a
   coherence rule is a reason to ask which adjacent field admits the same
   contradiction.
2. **Reconciling a contradiction instead of refusing it.** Three consecutive
   regressions came from repairing evidence after the fact where refusing to
   admit it was simpler and safer.
3. **Fixtures asserting more than they test.** Five fixtures encoded the very
   defect their neighbourhood was meant to guard, because their incidental
   fields were never scrutinised.

### Thirteenth external round (Devin, `1d07ba1`)

| Finding | Disposition | Mutation replay |
|---|---|---|
| signals, group reaping, and limit events were admissible before a start | **Fixed** — and the whole pre-start rule moved into one predicate | KILLED (×2) |
| the acceptance row claims a *bounded* canonical encoding that nothing proved | **Claim narrowed and then proven** — see below | — |

The first finding is the third time the same rule was found half-applied, so
this round the rule stopped being a per-field check. `ProcessEventKind::pre_start_violation`
now answers, in one place, whether an event presupposes a running child;
`admit` consults it once. Adding a variant to the enum forces a decision in
that match rather than silently defaulting to admissible, which is what let
output, then terminal causes, then termination phases each be found separately.

The exception set matters as much as the rule and is controlled: `Started`
itself, a cancellation *request* (requesting is not acting on a child), and a
deadline elapsing (a run can miss its deadline before it ever spawns) all stay
legal pre-start.

## On the boundedness claim

The acceptance row said the canonical encoding is "bounded" and none of the
tests beside it proved that word — they prove determinism, meaning-change
detection, secret-safety, and schema locking. The report offered the right
alternative: either bound it, or stop presenting the claim as proven.

I did not add length caps. The limits that actually bite — `ARG_MAX`, the
environment block size — are the platform's, differ per target, and are
enforced at spawn by the backend that knows which platform it is on. A number
invented here would be policy this domain does not own and would refuse plans a
real system accepts. Bounding *untrusted input* belongs to whoever accepts it,
which is not this type.

What is true, and is now stated at `canonical_bytes` and tested by
`the_fingerprint_is_fixed_size_however_large_the_plan`:

- the **fingerprint** is bounded — 128 bits for any plan;
- the **encoding** is linear in the plan's own size and performs no
  amplification, so it cannot turn a small plan into a large buffer;
- the **bytes are not capped**, and the control asserts they grow with the
  plan, so nobody reads the row as promising otherwise.

That is a narrower claim than the row implied, which is the point.

### Fourteenth external round (Devin, `6b1c7fa`) — and the first deferral

Three findings: two repaired, one deferred to #14556 on a stated test.

| Finding | Disposition | Mutation replay |
|---|---|---|
| the supervisor-failure fallback replaced observed stream evidence with empty streams | **Fixed** — the observed count is kept and the content identity withheld | KILLED |
| `validate_retention` proves less than its presence implies | **Claim narrowed** — documented at the variant and the validator | — |
| the fake does not cross-validate scripted events against scripted control state | **Deferred** — #14556 | — |

**The first is the same defect as round 14, one path over.** Round 14 stopped a
pre-start cancellation from erasing emitted output; the supervisor-failure
fallback erased it the same way, because `supervisor_failure` hard-coded empty
streams. A consumer holding a chunk event was told zero bytes were observed.

Fixing it exposed a genuine gap in the evidence model rather than a slip.
`StreamEvidence` required a `ContentFingerprint`, so there was no way to say
*"I read N bytes and cannot identify them"* — which is exactly what a
supervisor that failed mid-stream knows, and what this fake knows, since a
scripted chunk carries a count and no bytes. Reporting zero was a false
negative; reporting a fingerprint of nothing would have been a false positive.

Applying round 10's test — *does the contract fail to express something
ordinary?* — the answer was yes, so the type changed:
`observed_fingerprint` is now `Option<ContentFingerprint>`, `None` meaning the
count is known and the identity is not. `StreamEvidence::observed_but_unidentified`
builds that shape, and `supervisor_failure` takes stream evidence as a
parameter instead of assuming emptiness.

### On the retention check

`validate_retention` refuses `IncludeRetainedOutput` when the plan carries
private values. That is all it can do — validation runs before any child
exists, so nothing there can know what a child will write. But the check's
presence reads as though publishing retained output had been cleared, when it
has only been cleared of *the caller's own* secrets.

No new refusal: the domain cannot inspect output it never sees, and inventing a
rule about output content would be policy it does not own. Instead the variant
now says plainly that choosing it is an owner assertion about unseen content,
that passing validation means the plan's inputs are clean and not that the
output will be, and that whoever publishes the projection owns reviewing it.

### The deferral, and the test used

This is the first finding sent to a follow-up rather than fixed here, so the
test is recorded rather than left to look like fatigue.

The domain rules this PR enforces are statements about what a *result* or an
*event stream* may claim. The deferred finding is a **test double validating
its own input against a second field of that same input**: `elect` is driven by
`ControlState`, not by the event list, so a script that disagrees with itself
is a malformed fixture rather than a domain state a real backend reaches. It is
also open-ended in a way the domain rules are not — it needs a per-event
mapping to control flags that #11076 does not specify.

Two findings this round *did* meet the bar and were fixed: one finished a rule
left half-done, the other corrected a claim. That is the line, and it was set
before this round's findings arrived, not after seeing them.

### Fifteenth external round (Devin, `0195714`) — three defects from one commit

All three findings are consequences of the previous round's single commit.

| Finding | Disposition | Mutation replay |
|---|---|---|
| `supervisor_failure` accepted public stream evidence without running `check_stream`, and derived limitations from `None` rather than the streams it stored | **Fixed** — incoherent evidence is dropped, limitations derive from what is stored | KILLED |
| a missing fingerprint was accepted even when retention was unbounded, so a result could claim complete output with no content identity | **Fixed** — `AvailableContentLacksIdentity` | KILLED |
| `OutputLimitExceeded` and `CleanupFailed` were admissible before a start | **Fixed** — the terminal match is now exhaustive | KILLED |

The first two are the cost of the round-17 change, and they are the same
mistake in two directions. Widening `supervisor_failure` to take stream
evidence turned an infallible constructor into an unchecked publication
channel: *being unable to fail is not a licence to publish evidence nothing
checked*. It now drops incoherent evidence rather than propagating it, and
derives limitations from the streams it actually stores.

Making `observed_fingerprint` optional opened the matching hole. `None` was
meant to say "the supervisor could not establish the identity", but nothing
confined it to that case. When retention is unbounded the retained bytes are
the whole of what was observed, so the identity is available and omitting it
would let `claims_complete_output` be true with no identity at all. `None` is
now legal only where the content was genuinely dropped. `observed_but_unidentified(_, 0)`
was corrected in the same pass: reading nothing is not reading something
unidentifiable, so zero bytes publish the identity of emptiness.

**An escape hatch introduced for one honest case has to be confined to that
case in the same change, or it becomes a hole everywhere else.**

The third is the fourth time the same rule shipped half-applied, one round
after it was consolidated into a single predicate specifically to stop that.
Consolidating was not enough because the terminal arm still asked "is this one
of the kinds I thought of?" rather than classifying every variant. It is now an
exhaustive match that lists what is *allowed* before a start, so a new terminal
variant fails to compile until someone decides which side it is on. That is the
first version of this rule that cannot silently regress.

## Note on regression rate

Nine of the findings across these rounds were defects this PR's own repairs
introduced, three of them from one commit. That rate is itself evidence: the
domain's cross-field surface is large enough that a local change routinely has
non-local consequences, and the controls — not the reasoning — are what caught
them. Every fix here is mutation-replayed for that reason, including the ones
that looked obvious.
