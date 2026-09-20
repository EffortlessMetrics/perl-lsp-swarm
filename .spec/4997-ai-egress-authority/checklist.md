# #4997 - AI egress activation authority checklist

Initial proof was executed on candidate branch `fix/4997-generic-channel-ai-arm`
(worktree `perl-lsp-swarm-4997`, based on `origin/main@ab3cece9d`).

## Current-main reconstruction receipt — 2026-08-23

- [x] Original reviewed implementation commit
      `4e3a89b190e259f1265672d89c77b34690654cb3` reconstructed onto
      `main@1518b76b75e238d1adf1424910760ea5e5f987dc` in PR #12042.
- [x] Cherry-pick conflict denominator was exactly two current-main files:
      `config/mod.rs` and `perllsp_settings_schema_tests.rs`; all other
      implementation files applied mechanically. The three rejected hunks were
      resolved against current anchors without selecting the stale branch copy.
- [x] Review findings were applied before publication: generic-channel comment
      truth, schema transport wording, non-default provider preservation,
      Zed envelope-field observability, exact feature-gated test command,
      hosted `cargo fmt --all` receipt, and VS Code activation documentation.
- [x] Hosted reconstruction run
      `https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/32662424014`
      completed successfully and published candidate
      `c82977abebdf0c8de17fa63004a62c4a87f366ef` only after:
      `cargo fmt --all -- --check`; both focused rs-core authority tests;
      generic-schema tests; Zed settings-behavior tests; the transport hostile
      streaming test; scoped rs-core/server Clippy with `-D warnings`; JSON
      validation; and `git diff --check`.
- [x] Both one-shot workflows and both maintenance scripts were removed before
      the candidate commit. PR #12042 contains only the 17 reviewed product,
      test, specification, fixture, schema, and documentation files.
- [ ] Required repository PR workflows are green on the human-authored receipt
      commit. The workflow-authored candidate produced zero-job
      `action_required` runs by GitHub design; this receipt commit deliberately
      retriggers the ordinary base-branch workflows.

## Production changes

- [x] `AiActivationAuthority` (`Unavailable` | `TrustedUserOperator`) added to
      `AiCompletionConfig`; default fails closed; serde-defaulted
      (`crates/perl-lsp-rs-core/src/config/mod.rs`)
- [x] `admit_trusted_user_operator_activation()` is the sole trusted writer;
      takes no client-derived arguments (#4997 doc contract)
- [x] `update_from_value` rejects generic `aiCompletion.enabled`, `provider`,
      `model`, and `streaming.enabled` arrivals with key-naming warnings;
      accepted state preserved; envelope fields unchanged
- [x] `refresh_ai_backend` requires effective enabled AND accepted trusted
      activation (`crates/perl-lsp-rs/src/runtime/mod.rs`)
- [x] Catalog arm/select rows moved to `CompiledDefault + TrustedUserSettings`;
      derived rows document project reduction; new `ai.activation_authority`
      row satisfies the completeness gate
      (`configuration_authority/catalog.rs`)
- [x] Recurrence gates extended: `ai_arm_and_select_rows_admit_only_trusted_operator_sources`
      and the restricted-client-channel list
- [x] Schema transports emptied with pending-trusted-adapter descriptions for
      the four authority keys (`schemas/perllsp-settings.schema.json`)
- [x] Test-only API admits authority explicitly as the future adapter stand-in
      (`runtime/test_api.rs::test_configure_ai_completion`)
- [x] docs/reference/AI_COMPLETION.md channel/field tables, known-gap block,
      and troubleshooting rewritten to landed truth
- [x] Zed settings-behavior probe moved to the generically-settable envelope
      field (`streaming.updateDebounceMs`); contract, template receipt, and
      integration doc aligned

## Proof

- [x] `cargo test -p perl-lsp-rs-core --lib configuration_authority` — 11 passed
- [x] `cargo test -p perl-lsp-rs-core --test perllsp_settings_schema_tests` — 5 passed
- [x] rs-core config regressions: `generic_channel_ai_activation_shapes_fail_closed_across_clients`,
      `hostile_and_malformed_traffic_preserves_accepted_trusted_ai_state`,
      `provider_and_model_selection_cannot_exceed_activation_authority`,
      `trusted_operator_admission_arms_eligibility_generic_traffic_cannot`,
      opt-out pair, project-TOML pair — passed
- [x] Runtime: `generic_client_channels_cannot_arm_or_select_ai_backend`
      (construction oracle with usable endpoint + credential),
      positive control `refresh_ai_backend_installs_connector_auth_backend`,
      hostile project pair — passed (`--lib ai` filter, 134 passed)
- [x] `cargo test -p perl-lsp-rs --test lsp_streaming_completion_tests`
      (default features) — 8 passed, including two transport-level hostile
      enable/disable regressions
- [x] `cargo test -p perl-lsp-rs --features expose_lsp_test_api --test
      lsp_streaming_completion_tests` — 23 passed, including armed progress
      contract and session rotation moved behind the trusted test API
- [x] `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --features expose_lsp_test_api --test lsp_ai_inline_completion_tests -- --test-threads=2` — 14 passed
- [x] `cargo test -p perl-lsp-rs --test lsp_inline_completion_stream_bdd_workflows` — 3 passed
- [x] `cargo test -p xtask --test zed_settings_behavior` — 8 passed
- [x] Per-package `cargo fmt -p <pkg> -- --check` for `perl-lsp-rs-core`, `perl-lsp-rs`, and `xtask` — passed on the original Windows review host
- [x] `cargo fmt --all -- --check` — passed in hosted Linux review-repair validation; the earlier Windows exit 206 was not treated as proof
- [x] `cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs --lib --locked -- -D warnings` — clean
- [x] clippy on touched integration targets
      (`lsp_streaming_completion_tests`, `lsp_inline_completion_stream_bdd_workflows`,
      `perllsp_settings_schema_tests`) with `-D warnings` — clean after fixing
      two doc-comment placement errors this slice introduced
- [x] `cargo test -p perl-lsp-rs-core --all-targets --locked` — all green
      (3246 lib + every integration target, 0 failed)
- [x] `cargo test -p perl-lsp-rs --lib --all-targets --locked`: lib 1641 green;
      full-suite failures triaged against a pristine `origin/main@ab3cece9d`
      baseline worktree — identical deterministic failure counts on main for
      `lsp_completion_tests` (59/2), `workspace_resolution_tests` (17/1),
      `workstream_e_trust_anchor_tests` (5/2), `cli_smoke` (Windows stderr
      text), and `lsp_batteries_e2e_workflow_test` (Windows EOF-newline);
      remaining full-suite-only failures (`cross_file_goto_definition`,
      `lsp_bdd_workflows`, `lsp_behavioral`, doctor/module-resolution lib
      tests, `lsp_completion_ux_bdd`) pass in isolation in this tree —
      parallel-load flakiness of perl-subprocess probes on the constrained
      box. Zero candidate regressions.
- [x] Snapshot targets (`lsp_features_snapshot_test`, `lsp_pull_diag_snap`)
      produce byte-identical `.snap.new` drift on a pristine
      `origin/main@ab3cece9d` baseline worktree in this environment;
      pre-existing platform drift, no candidate files staged
- [x] Feature-gated reruns: streaming suite (23 passed) and AI inline suite
      (14 passed) under `--features expose_lsp_test_api`

## Residuals (owned elsewhere)

- Accepted-generation/session identity and raw-consumer cutover: #10909.
- Typed observation transport / client adapters: #10817 (#10807/#10813).
- Destination, redirect, response-bound hardening: #5004/#4955.
- Consent UX for legitimate activation: #5049.
- Envelope fields (`timeoutMs`, limits, `fallback`, `localModelMode`,
  debounce) remain generic-settable by design; volume-envelope hardening is
  #5004 territory.
