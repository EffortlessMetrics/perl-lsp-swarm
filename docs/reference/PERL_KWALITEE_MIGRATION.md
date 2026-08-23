# Legacy `perl_kwalitee.v1` migration

> Generated from `crates/perl-kwalitee/legacy_indicator_migrations.toml` and the frozen
> legacy catalog. Do not edit this table independently.

## Contract

- Legacy receipt kind: `perl_kwalitee`
- Legacy schema: `1`
- Historical domain: `mixed_repository_product_release_readiness`
- Status: compatibility-read-only; closed to new indicators
- Replacement: independent release-readiness rails plus the native Rust `perl-kwalitee` analyser

Historical receipts remain readable. They are not `distribution_kwalitee` receipts and
cannot authorize a current release candidate.

## Indicator disposition

| Legacy indicator | Title | Mandatory | Weight | Source | Scope | Destination | Action | Owner | Reproduce |
|---|---|---:|---:|---|---|---|---|---|---|
| `manifest.workspace_member_declared` | perl-kwalitee is a declared workspace member | yes | 3 | `native` | `all` | `retired` | `retire` | #7164 | `cargo metadata --no-deps` |
| `manifest.publish_policy_clean` | publish policy is intentional | yes | 4 | `native` | `all` | `release_governance` / `release_governance.publish_policy_clean` | `transfer` | #7191 | `cargo xtask publish-manifest-check` |
| `license.declared` | crate declares license metadata | yes | 3 | `native` | `all` | `release_governance` / `release_governance.license_declared` | `transfer` | #7191 | `cargo xtask publish-manifest-check` |
| `product_surface.native_only` | first-mile surfaces stay native-only | yes | 15 | `native` | `all` | `native_product` / `native_product.native_only` | `transfer` | #7168 | `cargo xtask check-native-product-surface` |
| `dap.cli_native_only` | shipped perl-dap CLI stays native-only | yes | 7 | `native` | `all` | `native_product` / `native_product.dap_cli_native_only` | `transfer` | #7168 | `cargo test -p perl-kwalitee dap_cli_native_only` |
| `release.native_binaries_present` | release archives contain the native binaries | yes | 7 | `external` | `release_only` | `release_integrity` / `release_integrity.native_binaries_present` | `replace` | #4145 | `cargo xtask release artifact-check --dist <dir>` |
| `release.no_external_tooling` | release archives bundle no external Perl tooling | yes | 8 | `external` | `release_only` | `release_integrity` / `release_integrity.no_external_tooling` | `replace` | #4145 | `cargo xtask release artifact-check --dist <dir>` |
| `release.checksums_valid` | consolidated checksums are present and valid | yes | 5 | `external` | `release_only` | `release_integrity` / `release_integrity.checksums_valid` | `replace` | #4145 | `cargo xtask release artifact-check --dist <dir>` |
| `formatter.native_default` | formatter defaults to the native engine | yes | 10 | `readiness_receipt` | `all` | `native_product` / `native_product.formatter_native_default` | `transfer` | #7168 | `cargo xtask native-tooling readiness` |
| `critic.native_default` | critic defaults to the native engine | yes | 8 | `readiness_receipt` | `all` | `native_product` / `native_product.critic_native_default` | `transfer` | #7168 | `cargo xtask native-tooling readiness` |
| `critic.run_critic_registry_parity` | perl.runCritic matches editor native diagnostics | yes | 7 | `external` | `all` | `engineering_evidence` / `engineering_evidence.critic_registry_parity` | `narrow` | #4791 | `cargo test -p perl-lsp-rs --lib execute_command::tests::run_critic_native_matches_pull_diagnostics_registry -- --exact` |
| `quality.no_new_severe_gaps` | no new severe coverage/ripr regressions | yes | 15 | `quality_gate_receipt` | `all` | `engineering_evidence` / `engineering_evidence.no_new_severe_gaps` | `transfer` | #4791 | `cargo xtask quality-gate` |
| `docs.status_current` | generated status docs are current | yes | 5 | `external` | `all` | `release_governance` / `release_governance.status_current` | `narrow` | #7191 | `cargo xtask update-status --check` |
| `formatter.corpus_idempotent` | native formatter is idempotent + parse-preserving over the corpus | no | 3 | `nightly_receipt` | `nightly_only` | `engineering_evidence` / `engineering_evidence.formatter_corpus_idempotent` | `narrow` | #4791 | `cargo xtask native-format corpus` |
| `critic.no_false_positives` | native critic raises no findings on the clean fixtures | no | 3 | `nightly_receipt` | `nightly_only` | `engineering_evidence` / `engineering_evidence.critic_no_false_positives` | `narrow` | #4791 | `cargo xtask native-critic check --root xtask/tests/fixtures/native-critic/false-positive` |
| `formatter.perltidy_compat_no_external_only` | perltidy compatibility has no external-only gaps | no | 2 | `nightly_receipt` | `nightly_only` | `engineering_evidence` / `engineering_evidence.formatter_perltidy_compat` | `narrow` | #4791 | `cargo xtask native-format perltidy-compat --profile .ci/kwalitee/perltidyrc` |
| `critic.perlcritic_compat_no_external_only` | perlcritic compatibility has no external-only gaps | no | 2 | `nightly_receipt` | `nightly_only` | `engineering_evidence` / `engineering_evidence.critic_perlcritic_compat` | `narrow` | #4791 | `cargo xtask native-tooling perlcritic-compat --profile .ci/kwalitee/perlcriticrc` |

## Interpretation

- `transfer` preserves the proposition while moving it to its correct domain authority.
- `replace` requires stronger candidate- or artifact-bound evidence before the legacy row can retire.
- `narrow` keeps only the bounded proposition actually established by the evidence.
- `retire` removes a bootstrap or ordinary-gate check from release readiness.

No row migrates into the CPANTS-compatible Kwalitee score. The native analyser has its
own catalog, input identity, scoring contract, and conformance fixtures.
