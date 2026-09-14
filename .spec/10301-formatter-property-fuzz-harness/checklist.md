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
- Transitional proof applicator exists at
  `src/native/edit_application.rs::apply_edits_exact`, but `src/native.rs`
  marks this #8048 seam production-unwired and #10239/#10242-owned. It is a
  research witness, not the future harness oracle or product authority.
- proptest precedent: root `Cargo.toml:364`;
  `crates/perl-lexer/tests/lexer_robustness_tests.proptest-regressions`.

These receipts are historical evidence for the spec. Before implementation,
rerun every source/count/API receipt against that implementation branch's merge
base and update this bundle if the seam changed; a mismatch or unavailable
source is `NOT_PROVEN`, not permission to code from the old line reference.

Implementation gate: do not start the formatter-bound checker until #10237 and
#10239 land the canonical byte-native source/request/result/edit-plan API and
the applicable #7138 strict plan/application authority is reachable. Reopen
this plan at each merged issue and revalidate the exact API. #7140,
#7101/#7104/#7111/#7120, and #8146 are also required before the live mandatory
profile can aggregate to `pass`; before then it must exit non-zero as
`not_proven`.

Planned surface:

- [ ] `crates/perl-lsp-perltidy/tests/support/formatter_property_harness/`:
      one test-only module owning fallible `check_case`, the admitted-family
      registry, structured strategies, seed/profile/schema/receipt types, and
      mandatory dispositions. It consumes the post-#10237/#10239 byte-native
      formatter result/edit-plan API and owns a test-only strict applicator
      that shares no production mapper, clamping, constructor, or applicator;
      it exposes no public product feature or second checker. The exact allowed
      inventory is `mod.rs`, `generator.rs`, `checker.rs`, `strict_apply.rs`,
      `profile.rs`, and `receipt.rs`; any additional file is a policy failure
- [ ] `crates/perl-lsp-perltidy/tests/formatter_property_harness_tests.rs`:
      programmatic `FPH_SEED` parsing, pinned `RngAlgorithm::ChaCha` with
      domain-separated 32-byte seed derivation, one deterministic exemplar per
      admitted family, exactly 64 novel focused cases on one test thread, and
      FPH-001..FPH-010 including mutation controls. Its `fph_policy_pins` test
      recursively rejects any file outside the exact allowed support inventory,
      scans all crate `src/**/*.rs` and `tests/**/*.rs` for unique harness
      ownership/checker markers, and proves one checker, one producer, one
      validator, no
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
      corpus; its schema/version and digest are present in every profile receipt.
      The producer replays each entry exactly once per profile, disables
      implicit persistence replay in novel runners, and atomically persists a
      newly minimized failure through one fallible canonical writer
- [ ] Three profiles are DATA in the harness: focused = every-family exemplar +
      persisted corpus once + 64 novel cases/1,024 shrink iterations; scheduled
      = exemplars + persisted corpus once + 16 fixed seeds × 256 novel cases/
      4,096 shrink iterations; release = exemplars + persisted corpus once +
      64 fixed seeds × 256 novel cases/
      4,096 shrink iterations, bound to the supplied candidate/profile/schema.
      Invalid profile/seed/candidate input is a typed failure. Missing, timeout,
      stale-schema, or instrument failure is `NOT_PROVEN`
- [ ] Normalized schema `formatter_property_harness.v1` writes atomically to
      `target/formatter-property-harness/<profile>/receipt.json`, with stable
      row order, canonical corpus schema/digest, exact locked proptest package/
      lock digest, RNG algorithm, seed-derivation domain, strategy fingerprint,
      and separate exemplar/persisted/novel/total counts and ordered digests;
      no timestamps/absolute paths. Scheduled/release require caller-supplied
      `FPH_CANDIDATE_SHA`; consumers validate that binding
- [ ] Every mandatory row and the aggregate use `pass | fail | not_proven`.
      Aggregate precedence is fail, then not_proven, then pass. Any non-pass
      writes the receipt and makes the producer test non-zero. The independent
      validator checks schema, candidate, profile, aggregate, counts, and
      digests and rejects timeout/missing/crash/stale/instrument evidence
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
  --test formatter_property_harness_tests --locked \
  run_formatter_property_profile -- --exact --test-threads=1
FPH_PROFILE=scheduled FPH_CANDIDATE_SHA=<40-hex-sha> \
  cargo test -p perl-lsp-perltidy --test formatter_property_harness_tests \
  --locked run_formatter_property_profile -- --exact --test-threads=1
FPH_PROFILE=release FPH_CANDIDATE_SHA=<40-hex-sha> \
  cargo test -p perl-lsp-perltidy --test formatter_property_harness_tests \
  --locked run_formatter_property_profile -- --exact --test-threads=1
FPH_RECEIPT_PATH=target/formatter-property-harness/<profile>/receipt.json \
FPH_EXPECTED_PROFILE=<profile> FPH_EXPECTED_CANDIDATE_SHA=<40-hex-sha> \
  cargo test -p perl-lsp-perltidy --test formatter_property_harness_tests \
  --locked validate_formatter_property_receipt -- --exact --test-threads=1
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
- [ ] #10237/#10239 and applicable #7138 merge, then this spec's API/source
      receipts are revalidated before implementation
