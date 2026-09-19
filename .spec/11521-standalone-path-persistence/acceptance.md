# Acceptance: #11521 — standalone PATH persistence and fresh-process resolution

## §Behavior

| Input | Condition | Result |
|---|---|---|
| PATH plan | mutation owner is `installer` and the subject is `confirmed_current` | accepted |
| PATH plan | mutation owner is `installer` and the subject is `unconfirmed`, `superseded`, or `not_proven` | refused: the confirmation gate is a hard predecessor |
| PATH plan | a non-installer owner declares an owned entry | refused: ownership is not laundered |
| PATH plan | scope is `session_only` under an installer or package-manager owner | refused: a current-process change is not durable persistence |
| PATH plan | entry value lies outside the install root | refused |
| PATH plan | path text carries expansion syntax or a PATH list separator | refused |
| Persistence receipt | `installer_persisted` with a durable write that added exactly one owned entry | accepted |
| Persistence receipt | an outcome that wrote nothing claims a mutation or an added entry | refused |
| Persistence receipt | a visible-entry claim observed at `session_only` scope | refused |
| Persistence receipt | `added` or `already_present` with other than exactly one owned entry | refused: duplicate, case, and alias spellings are not idempotent |
| Persistence receipt | observed scope differs from the planned scope | refused: scope is never silently widened |
| Fresh-process observation | a new ambient session performed a command lookup that unambiguously resolved the candidate | accepted as `path_visible_immediately` or `path_visible_after_documented_new_session` per the plan's declared boundary |
| Fresh-process observation | visibility claimed from the current process, an installer child, a mutated current environment, a harness-injected PATH, or an absolute-path invocation | refused |
| Fresh-process observation | the lookup resolved a binary that is not the candidate | must be `wrong_ambient_binary` |
| Fresh-process observation | the lookup was ambiguous | must be `ambiguous_resolution`, with at least two observed candidates |
| Fresh-process observation | observed platform differs from the planned platform, or the shell differs under a shell-specific policy | refused: Windows, WSL, and POSIX evidence is not interchangeable |
| Any document | bound plan digest differs from the current plan | refused: the selection or policy moved |
| Any document | instrument incomplete | only `instrument_failure` or `not_proven` |

## §Contracts

| ID | Contract |
|---|---|
| SPP-C01 | Three documents, three `schema_version` constants; persistence and fresh-process resolution are distinct versioned contracts. |
| SPP-C02 | Installer-owned PATH mutation requires `confirmation_state = confirmed_current`. |
| SPP-C03 | Each `policy` has exactly one `mutation_owner`; the pair cannot disagree. |
| SPP-C04 | Only the `installer` owner declares an owned entry; every other owner declares `entry_kind = none`. |
| SPP-C05 | An owned entry names its location, the single directory it adds, and the marker that makes it exactly removable. |
| SPP-C06 | The owned entry value lies under the install root; PATH is never pointed at a directory the installer does not own. |
| SPP-C07 | `user`, `system_elevated`, `shell_specific`, and `session_only` are distinct; `system_elevated` requires a system root, `shell_specific` names its shell, and `session_only` carries no durable owner. |
| SPP-C08 | The registry is a Windows persistence mechanism; profile lines and `path.d` fragments are POSIX and WSL mechanisms. |
| SPP-C09 | Path text is a literal exact identity: shell, script, registry, and environment expansion syntax is refused, as are traversal, alias, case, and separator variants. |
| SPP-C10 | Complete PATH, profile, registry, and home values never enter a durable document; the conflict reason is a typed field, not free text. |
| SPP-C11 | An outcome that wrote nothing cannot report a mutation or an added entry. |
| SPP-C12 | `installer_persisted` requires a durable write that added the owned entry, under a plan whose owner is the installer. |
| SPP-C13 | `added` or `already_present` requires exactly one canonical owned entry; `absent` requires zero. Repeated install is idempotent. |
| SPP-C14 | A current-process environment edit, and any `session_only` observation, can never support a visible-entry claim. |
| SPP-C15 | A conflict names the exact conflicting state, and only a conflict may report one. |
| SPP-C16 | A visibility claim requires a genuinely new ambient session, an unprepared PATH, a real command lookup, an unambiguous resolution, the candidate's own digest, and a startup that did not fail. |
| SPP-C17 | An absolute-path invocation can never support a visibility claim. |
| SPP-C18 | A resolved binary that is not the candidate is `wrong_ambient_binary`; the selected candidate existing elsewhere does not make the lookup correct. Nothing is rewritten or removed. |
| SPP-C19 | Ambiguity stays visible: an ambiguous lookup reports `ambiguous_resolution` and nothing else. |
| SPP-C20 | The observed candidate set is exact and deterministic: strictly ascending by path, no duplicates, and the resolved winner belongs to it. |
| SPP-C21 | A documented new-session boundary is reported as such, never elided into immediacy; immediacy requires a plan that documents no boundary. |
| SPP-C22 | Observed platform must equal the planned platform, and under a shell-specific policy the observed shell must equal the planned shell. |
| SPP-C23 | Persistence and observation bind their plan by id and by digest of its exact bytes; a moved plan or selection invalidates them. |
| SPP-C24 | An incomplete instrument concludes only `instrument_failure` or `not_proven`; a complete instrument never reports `instrument_failure`. |
| SPP-C25 | A fresh-process observation paired with a persistence receipt must declare which outcome it followed, and cannot claim visibility the receipt contradicts. |
| SPP-C26 | This lane performs no platform mutation, proven against the validator's own source. |
| SPP-C27 | Command lookup is platform-specific: native Windows normalizes an executable extension and matches case-insensitively; POSIX and WSL match the exact file name. |
| SPP-C28 | The resolved winner and its row in the observed candidate set carry one identity; a recorded digest on that row must agree. |
| SPP-C29 | A `logout_login` boundary is strictly stronger than a new shell and is only exercised by a `new_login_session`. |
| SPP-C30 | A current-process environment edit with no durable write cannot certify a visible entry — enforced in the Rust validator **and** the published schema. |

## §Falsifier map

The ten negative controls the issue requires, each bound to the contract rows it
exercises and to the test that proves it.

| # | Issue negative control | Contracts | Proof |
|---|---|---|---|
| 1 | candidate path existence equals PATH success | SPP-C11, SPP-C12 | `a_valid_candidate_does_not_make_persistence_true` |
| 2 | profile or registry write equals fresh-process observation | SPP-C25 | `a_write_that_did_not_happen_cannot_back_a_visibility_claim`, `documents_bound_to_different_plans_do_not_compose` |
| 3 | current-process prepend or absolute path passes | SPP-C14, SPP-C16, SPP-C17 | `a_current_process_prepend_cannot_prove_discoverability`, `an_absolute_path_launch_cannot_prove_discoverability`, `a_harness_prepared_path_cannot_prove_discoverability`, `a_session_only_observation_cannot_claim_a_visible_entry` |
| 4 | wrong ambient binary ignored because the selected candidate exists | SPP-C18 | `a_wrong_ambient_binary_cannot_be_reported_as_success`, `a_resolved_non_candidate_is_wrong_ambient_not_not_found` |
| 5 | Windows evidence satisfies WSL/POSIX, or one shell satisfies all shells | SPP-C08, SPP-C22 | `one_platform_does_not_satisfy_another`, `one_shell_does_not_satisfy_a_shell_specific_policy`, `a_persistence_mechanism_foreign_to_the_platform_is_refused` |
| 6 | duplicate, case, or path alias is added | SPP-C09, SPP-C13, SPP-C20 | `a_duplicate_owned_entry_is_not_idempotent_success`, `the_observed_candidate_set_is_exact_and_deterministic` |
| 7 | user/system scope silently widened | SPP-C07 | `an_observed_scope_cannot_widen_the_planned_scope` |
| 8 | uninstall or rollback deletes unrelated entries or content | SPP-C04, SPP-C05, SPP-C06 | `a_non_installer_owner_cannot_declare_an_owned_entry`, `an_added_entry_requires_a_plan_that_authorized_one`, `an_owned_entry_outside_the_install_root_is_refused`. Execution of removal remains #11470's. |
| 9 | complete PATH, profile, or private values enter durable output | SPP-C09, SPP-C10 | `a_complete_path_value_cannot_enter_a_durable_document`, `a_home_expansion_cannot_enter_a_durable_document`, `shell_expansion_in_path_text_cannot_reach_a_profile` |
| 10 | platform mutation occurs in this PR | SPP-C26 | `this_validator_performs_no_platform_mutation` |

## §Hazards

- **Confirmation bypass.** A superseded candidate reaching PATH mutation would
  make a stale binary the one users resolve. SPP-C02 is a hard predecessor, not
  a warning.
- **Manufactured freshness.** The cheapest way to make a PATH proof green is to
  prepend the install directory in the observing process. SPP-C14 and SPP-C16
  refuse every spelling of it, and SPP-C26 proves this lane cannot perform one.
- **Silent destruction.** Rewriting or removing a competing installation to make
  the lookup succeed. SPP-C18 keeps the wrong ambient binary visible and
  reported instead.
- **Scope creep.** Writing a system profile under a user-scope plan. SPP-C07 and
  the observed-vs-planned scope check refuse it.
- **Injection through path text.** A path that reaches a profile, a script, or
  the registry unescaped. SPP-C09 treats path text as a literal exact identity.
- **Receipt leakage.** A complete PATH or profile value in a durable receipt.
  SPP-C10 bounds every field and types the conflict reason.
- **Vocabulary escape.** An unmodelled outcome word or an extra field silently
  accepted. Every document is `deny_unknown_fields` with closed enums in both
  the Rust types and the published schemas.

## §Schema limitations

JSON Schema 2020-12 cannot relate two string fields, so one contract row is a
**declared consumer obligation** rather than a published law:

| Row | Why | Who enforces it |
|---|---|---|
| SPP-C06 (owned entry lies under the install root) | requires prefix containment between `path_policy.owned_entry.entry_value` and `environment.install_root.path` | the Rust validator; a schema-only consumer **must** perform the check itself |

This is stated in the schema's own `entry_value` description with a
`NOT SCHEMA-ENFORCED` marker, and pinned by
`test_install_root_containment_is_a_declared_consumer_obligation`, which asserts
both that the schema still accepts `plan_invalid_entry_outside_root.json` and
that the marker is present. If a future schema dialect can express containment,
that test fails and the fixture moves into `INVALID_PLAN_FIXTURES`. Every other
contract row is carried by both the Rust validator and the published schemas.

## §API-Shape

New, additive only. No existing public surface changes.

- `schemas/standalone_path_plan.v1.schema.json`
- `schemas/standalone_path_persistence.v1.schema.json`
- `schemas/standalone_fresh_process.v1.schema.json`
- `xtask/examples/standalone_path_persistence.rs` — `validate_plan`,
  `validate_persistence`, `validate_persistence_against_plan`,
  `validate_fresh_process`, `validate_fresh_process_against_plan`,
  `validate_fresh_process_against_persistence`, and the closed vocabularies.
- CLI: `cargo run -p xtask --example standalone_path_persistence -- --plan P [--persistence R] [--fresh-process O] [--print-canonical]`

## §Test-Grid

| Layer | Command | Count |
|---|---|---|
| Checked validator battery | `cargo test -p xtask --example standalone_path_persistence --locked` | 52 |
| Schema-only parity harness | `python -m unittest scripts.ci.test_standalone_path_contract_schemas` | 42 |
| Fixtures | `fixtures/experience/install_path_persistence/` | 27 (19 positive, 8 committed-invalid) |

The battery includes `every_invalid_fixture_is_valid_once_its_one_violation_is_repaired`,
which proves each committed invalid fixture is refused for the law it names and
not for an unrelated reason.

## §Blast-Radius

Additive. No existing crate, task, gate, or receipt consumes these documents
yet, so nothing on `origin/main` changes behavior. The hosted `Standalone
contract tests` workflow gains two steps, gated on the new paths. `xtask`
gains one example target. The rollback boundary is the file set: deleting the
three schemas, the example, the fixture directory, the harness, and the two
workflow steps returns the tree to current behavior.
