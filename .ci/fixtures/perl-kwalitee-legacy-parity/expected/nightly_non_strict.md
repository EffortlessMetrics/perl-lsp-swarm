# Perl Kwalitee Report

Verdict: WARN

Score: 90/100

Profile: nightly

Commit: unknown

Generated: 2026-08-14T00:00:00Z

## Mandatory indicators

| Indicator | Status | Evidence |
|---|---|---|
| manifest.workspace_member_declared | pass | `Cargo.toml [workspace].members` |
| manifest.publish_policy_clean | pass | `crates/perl-kwalitee/Cargo.toml [package].publish`<br>`Cargo.toml [workspace.metadata.publish].allow` |
| license.declared | pass | `crates/perl-kwalitee/Cargo.toml [package].license` |
| product_surface.native_only | pass | `cargo xtask check-native-product-surface` |
| dap.cli_native_only | pass | `perl-dap main.rs::cli_help_has_no_bridge_product_surface` |
| release.native_binaries_present | not_applicable | `release archives are only evaluated under the release profile` |
| release.no_external_tooling | not_applicable | `release archives are only evaluated under the release profile` |
| release.checksums_valid | not_applicable | `release archives are only evaluated under the release profile` |
| formatter.native_default | pass | `<fixture-root>/target/receipts/native-tooling/readiness.json`<br>`native-default engine` |
| critic.native_default | warn | `<fixture-root>/target/receipts/native-tooling/readiness.json`<br>`native default` |
| critic.run_critic_registry_parity | unverified | `no external result supplied for critic.run_critic_registry_parity` |
| quality.no_new_severe_gaps | pass | `<fixture-root>/target/receipts/quality/quality-gate.json`<br>`pass` |
| docs.status_current | unverified | `no external result supplied for docs.status_current` |

### Remediation

- **critic.native_default** (warn): Run `cargo xtask native-tooling readiness` and confirm the critic native-default criterion is ready.
- **critic.run_critic_registry_parity** (unverified): Run `cargo test -p perl-lsp-rs --lib execute_command::tests::run_critic_native_matches_pull_diagnostics_registry` and resolve the parity failure.
- **docs.status_current** (unverified): Run `cargo xtask update-status --check`; regenerate with `--write` if drift is reported.

## Advisory indicators

| Indicator | Status | Evidence |
|---|---|---|
| formatter.corpus_idempotent | pass | `<fixture-root>/target/receipts/format/native-format-corpus.json`<br>`passed=true over 42 files` |
| critic.no_false_positives | warn | `<fixture-root>/target/receipts/native-tooling/native-critic-false-positive.json`<br>`findings=1 suppressed=0 parse_errors=0` |
| formatter.perltidy_compat_no_external_only | pass | `<fixture-root>/target/receipts/format/native-format-perltidy-compat.json`<br>`external_only_count=0` |
| critic.perlcritic_compat_no_external_only | unverified | `cargo xtask native-tooling perlcritic-compat` |

### Remediation

- **critic.no_false_positives** (warn): Eliminate findings/parse errors the native critic raises on known-clean code.
- **critic.perlcritic_compat_no_external_only** (unverified): Provide the receipt (run `cargo xtask native-tooling perlcritic-compat`). Close or re-classify the external-only perlcritic rules.
