# Checklist: #10301 formatter property/fuzz harness

Base pin: `origin/main@e6e956461534b2566c735696f289e0915e2cb189` (2026-08-27).
Composition sibling: #10302 bench spec
(`.spec/10302-formatter-production-pipeline-bench/`) — disjoint file set.

Red-first receipts-of-record (2026-08-27, pure reads on the base pin):

- Zero formatter fuzz surface: `fuzz/Cargo.toml` declares 20 targets
  (substitution_parsing, builtin_functions, lsp_cancellation_registry,
  unicode_positions, utf16_roundtrip, lsp_navigation, heredoc_parsing,
  quote_operators, declaration_parsing, parser_integration,
  structured_perl_programs, module_surface, symbol_query_ranking,
  semantic_model, incremental_edit_sequences, lexer_tokenization,
  config_surfaces, dap_eval_validator, dap_stack_parser, regex_validation,
  pod_extraction) and no `perl-lsp-perltidy` dependency.
- Zero property generator in the crate:
  `crates/perl-lsp-perltidy/Cargo.toml` dev-dependencies are exactly
  perl-tdd-support, serde_json, tempfile.
- Safety rests on goldens + one equivalence test:
  `tests/fixtures/native_formatter/*` plus
  `tests/edit_application_equivalence_tests.rs`.
- Parse gate makes random bytes useless as subjects:
  `src/native/implementation.rs` lines ~56-96 (`validate_clean_parse`,
  refusal codes above).
- Independent applicator available for the oracle side:
  `src/native/edit_application.rs::apply_edits_exact`.
- proptest precedent: root `Cargo.toml:364`;
  `crates/perl-lexer/tests/lexer_robustness_tests.proptest-regressions`.

Planned surface:

- [ ] `crates/perl-lsp-perltidy/tests/support/formatter_property_harness/`:
      generators/mutators over admitted safe-subset families, bounded case
      construction, seed/schema/receipt types, invariant checker consuming
      only `format_*_typed` + `apply_edits_exact`, dormant disposition
      registry for gated invariants
- [ ] `crates/perl-lsp-perltidy/tests/formatter_property_harness_tests.rs`:
      FPH-001..FPH-009 incl. mutation controls
- [ ] `crates/perl-lsp-perltidy/Cargo.toml`: additive
      `proptest.workspace = true` dev-dependency only
- [ ] `fuzz/Cargo.toml`: add `perl-lsp-perltidy` path dep +
      `[[bin]] name = "perl_tidy_formatter"`
- [ ] `fuzz/fuzz_targets/perl_tidy_formatter.rs`: structured mutation front
      end calling the shared invariant core; never executes Perl
- [ ] One minimized committed regression demonstrating the crash/property →
      focused-fixture pipeline end to end

Proof commands:

```bash
cargo fmt -p perl-lsp-perltidy -- --check
cargo clippy -p perl-lsp-perltidy --all-targets --locked -- -D warnings
cargo test -p perl-lsp-perltidy --all-targets --locked
cargo test -p perl-lsp-perltidy --test formatter_property_harness_tests -- --test-threads=1
```

Open residuals (owned by upstream issues, not silently dropped):

- [ ] Cancellation/budget interruption property converts from FPH-008
      dormancy when #7140 lands checkpoint inputs
- [ ] Structural preservation beyond parse success (#8146) and trivia/
      opaque hashing (#7101/#7104/#7111/#7120) convert their dormancies
- [ ] Scheduled/release tier budgets and #7147/#9749 receipt consumption
