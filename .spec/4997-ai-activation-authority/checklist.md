# Checklist: #4997 — remote AI activation authority

## Preparation

- [x] Current P0 issue and latest accepted ruling reconciled.
- [x] Open PR and branch search found no implementation owner.
- [x] Implementation base pinned to `cf145b234a9bba19a165653acfeede71aea08bbe`.
- [x] Generic parser, project reducer, authority catalog, backend construction,
      request tests, schema, VS Code scope, and current AI docs inventoried.
- [ ] Rebase on current `main` before final review if the branch moves materially.

## Production cutover

- [ ] Remove generic `aiCompletion.enabled -> user_enabled` assignment.
- [ ] Remove generic provider assignment.
- [ ] Remove generic model assignment.
- [ ] Preserve non-arm/select siblings without treating them as trusted activation.
- [ ] Preserve project opt-out and ignore project enablement.
- [ ] Correct `ai.user_enabled` catalog sources.
- [ ] Correct `ai.provider` catalog sources.
- [ ] Correct `ai.model` catalog sources.
- [ ] Correct derived effective-enabled sources.
- [ ] Remove enabled/provider/model from the generic settings schema.
- [ ] Keep endpoint and credential-routing fields excluded from generic schema/parser.
- [ ] Update current AI configuration documentation.
- [ ] Add a security changelog fragment with the temporary feature-availability boundary.

## Shift-left proof

- [ ] Generic enabled=true leaves AI disabled.
- [ ] Generic enabled=false cannot overwrite trusted test activation.
- [ ] Generic provider/model leave prior values unchanged.
- [ ] Mixed hostile payload cannot compose activation.
- [ ] One admitted non-arm/select sibling still updates, preventing a whole-block no-op shortcut.
- [ ] Project false disables trusted test activation.
- [ ] Project true does not enable.
- [ ] Initialization-options hostile payload produces no backend.
- [ ] didChangeConfiguration hostile payload produces no backend.
- [ ] Configuration-response hostile payload produces no backend.
- [ ] Explicit invoked request after hostile payload calls counting backend zero times.
- [ ] Trusted test enable plus the same backend calls once.
- [ ] Schema and catalog source assertions match production behavior.
- [ ] Warning output contains no hostile value canaries.
- [ ] Recurrence test rejects parser, catalog, schema, and current-doc drift.

## Review lanes

### Threat-model review

- [ ] Start with valid-looking provider/model and a preconfigured transport subject.
- [ ] Challenge backend construction and first invoked request separately.
- [ ] Confirm empty endpoint or missing credential is not the only reason the test passes.

### Provenance review

- [ ] Initialization options are not treated as trusted user settings.
- [ ] Global/unscoped configuration array position is not authority.
- [ ] Client name, machine-scoped storage, or payload-provided scope cannot strengthen provenance.
- [ ] Project input remains reducer-only.

### Consumer and first-effect review

- [ ] Inventory every `user_enabled` assignment outside tests.
- [ ] Inventory every provider/model assignment outside defaults/trusted paths.
- [ ] Inventory every `refresh_ai_backend` caller and backend read.
- [ ] Assert a hostile payload cannot leave a callable backend.
- [ ] Assert explicit invoked request backend counter remains zero.

### Compatibility and product review

- [ ] VS Code machine toggles are described as awaiting trusted server transport.
- [ ] Deterministic completion remains available.
- [ ] Existing test-only AI behavior remains green.
- [ ] No endpoint/credential configuration is moved back into project/generic settings.

### Scope/claim review

- [ ] No accepted-generation store or first-egress lifecycle is invented here.
- [ ] No provider, prompt, ranking, streaming-presentation, consent, or installer work enters.
- [ ] #10909 remains the owner of stale backend/session/request lifecycle.

## Verification

Discover final exact filters after editing. Expected minimum:

```bash
cargo fmt --all -- --check
cargo test -p perl-lsp-rs-core --all-targets --locked ai_completion
cargo test -p perl-lsp-rs-core --test perllsp_settings_schema_tests --locked
cargo test -p perl-lsp-rs --features expose_lsp_test_api \
  --test lsp_ai_inline_completion_tests --locked
cargo test -p perl-lsp-rs --all-targets --locked configuration
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs \
  --all-targets --all-features --locked -- -D warnings
cargo xtask check-architecture
cargo xtask check-test-wiring
cargo xtask docs-check
git diff --check
```

A zero-test filter, missing binary, timeout, skipped exact request, or instrument
failure is `NOT_PROVEN`.

## Completion receipt

- [ ] Exact base/head SHAs recorded.
- [ ] Parser, catalog, schema, runtime, docs, and test rows listed.
- [ ] Backend construction and call counters reported.
- [ ] Established and explicitly-not-established claims recorded.
- [ ] #10909/#6736 named as downstream owners rather than implied complete.
