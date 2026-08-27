# Checklist: #10301 formatter property/fuzz harness

Base pin: `origin/main@e6e956461534b2566c735696f289e0915e2cb189` (2026-08-27).
Composition sibling: #10302 bench spec
(`.spec/10302-formatter-production-pipeline-bench/`) — disjoint file set.

Red-first receipts-of-record (2026-08-27, pure reads on the base pin):

- Zero formatter fuzz surface: `fuzz/Cargo.toml` declares 21 targets
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

These receipts are historical evidence for the spec. Before implementation,
rerun every source/count/API receipt against that implementation branch's merge
base and update this bundle if the seam changed; a mismatch or unavailable
source is `NOT_PROVEN`, not permission to code from the old line reference.

Planned surface:

- [ ] `crates/perl-lsp-perltidy/tests/support/formatter_property_harness/`:
      one test-only module owning fallible `check_case`, the admitted-family
      registry, structured strategies, seed/profile/schema/receipt types, and
      dormant dispositions. It consumes canonical product APIs but exposes no
      public product feature or second checker
- [ ] `crates/perl-lsp-perltidy/tests/formatter_property_harness_tests.rs`:
      programmatic `FPH_SEED` parsing, one deterministic exemplar per admitted
      family, exactly 64 generated focused cases on one test thread, and
      FPH-001..FPH-010 including mutation controls. Its `fph_policy_pins` test
      reads the complete harness/test surface and proves one checker, no
      subprocess/Perl execution or external oracle, and no producer-side
      expected-byte derivation. The same test rejects `unwrap`, `expect`,
      panic/assert/todo/unimplemented/unreachable/debug macros, unchecked
      indexing/slicing, and `unsafe`; repository panic/unsafe and cargo-allow
      no-new ratchets provide independent backstops
- [ ] `crates/perl-lsp-perltidy/Cargo.toml`: add
      `proptest.workspace = true` as a dev-dependency; update `Cargo.lock` only
      if Cargo changes the existing locked graph
- [ ] `tests/formatter_property_harness_tests.proptest-regressions`: checked-in
      minimized persistence entries; every discovered entry is paired with a
      readable named Rust regression test and normalized receipt identity. The
      ordered admitted-family exemplars plus these entries are the canonical
      corpus; its schema/version and digest are present in every profile receipt
- [ ] Three profiles are DATA in the harness: focused = every-family exemplar +
      64 generated cases/1,024 shrink iterations; scheduled = 16 fixed seeds ×
      256 cases/4,096 shrink iterations; release = 64 fixed seeds × 256 cases/
      4,096 shrink iterations, bound to the supplied candidate/profile/schema.
      Invalid profile/seed/candidate input is a typed failure. Missing, timeout,
      stale-schema, or instrument failure is `NOT_PROVEN`
- [ ] Normalized schema `formatter_property_harness.v1` writes atomically to
      `target/formatter-property-harness/<profile>/receipt.json`, with stable
      row order, canonical corpus schema/digest, and no timestamps/absolute
      paths. Release rows require the caller-supplied exact candidate;
      consumers validate that binding
- [ ] Ordinary PR routing selects only focused proof for formatter/harness
      changes. Scheduled/release consumers own their later workflow wiring and
      normalized artifact upload; before enabling either, record a measured LEM
      projection and routing rationale. This claim changes no workflow

Proof commands:

```bash
cargo fmt -p perl-lsp-perltidy -- --check
cargo clippy -p perl-lsp-perltidy --all-targets --locked -- -D warnings
cargo test -p perl-lsp-perltidy --all-targets --locked
cargo test -p perl-lsp-perltidy \
  --test formatter_property_harness_tests --locked -- --test-threads=1
FPH_PROFILE=focused FPH_SEED=10301 cargo test -p perl-lsp-perltidy \
  --test formatter_property_harness_tests --locked -- --test-threads=1
cargo xtask ci-hygiene check-unwraps-prod
cargo xtask ci-hygiene check-panic-test
cargo xtask ci-hygiene check-unsafe-prod
cargo-allow check --mode no-new
```

Open residuals (owned by upstream issues, not silently dropped):

- [ ] Cancellation/budget interruption property converts from FPH-008
      dormancy when #7140 lands checkpoint inputs
- [ ] Structural preservation beyond parse success (#8146) and trivia/
      opaque hashing (#7101/#7104/#7111/#7120) convert their dormancies
- [ ] Scheduled/release workflow wiring and #7147/#9749 receipt consumption
