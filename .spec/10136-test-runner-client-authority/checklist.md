# Checklist: #10136 — test-runner client authority removal

## Preparation

- [x] Current open PRs searched; no existing implementation owner found.
- [x] Current Rust field, parser, reflection, catalog, schema and test surfaces inventoried.
- [x] No behavior-bearing server process-planning consumer found for the four fields.
- [x] Authority decision recorded: remove the complete generic block now.
- [ ] Rebase the implementation branch on current `main` before final review.

## Production removal

- [ ] Remove `ServerConfig::test_runner_enabled`.
- [ ] Remove `ServerConfig::test_runner_command`.
- [ ] Remove `ServerConfig::test_runner_args`.
- [ ] Remove `ServerConfig::test_runner_timeout`.
- [ ] Remove their default values.
- [ ] Remove the `testRunner` mutation branch from `ServerConfig::update_from_value`.
- [ ] Remove `testRunner` wrong-type handling that implies the block remains supported.
- [ ] Remove all legacy `perl.testRunner.*` reflection arms.
- [ ] Remove the four `test.*` configuration-authority rows.
- [ ] Remove the claimed generic `TestRunner` consumer when no other current row requires it.
- [ ] Remove the current generic schema object and aliases.
- [ ] Regenerate or update all current configuration documentation projections.
- [ ] Add a changelog fragment describing removal of an inert unsafe configuration surface.

## Proof changes

- [ ] Replace the inline positive assignment test with hostile legacy-input no-effect proof.
- [ ] Update schema tests to require `testRunner` absence.
- [ ] Update default/absorption/smoke tests so they do not pin deleted fields.
- [ ] Add reflection absence proof for all four legacy names.
- [ ] Add authority-catalog absence proof.
- [ ] Add source/architecture recurrence proof for fields, aliases and catalog rows.
- [ ] Keep legacy keys in fuzz/adversarial input where useful; assert no accepted authority.
- [ ] Add a mixed payload proving unrelated settings still apply.
- [ ] Add command/argument canaries and prove they do not enter durable output.

## Review lanes

### Process-authority review

- [ ] Start at every remaining executable/argv/cwd/env construction seam.
- [ ] Confirm none reads generic LSP `testRunner` state or aliases.
- [ ] Confirm no compatibility enum or string reconstructs arbitrary command authority.

### Configuration-boundary review

- [ ] Inspect initialization options, didChangeConfiguration and configuration-response paths.
- [ ] Confirm removed input cannot become accepted state through any channel.
- [ ] Confirm unscoped response position does not imply trusted process authority.

### Consumer-denominator review

- [ ] Search every `test_runner_*`, `testRunner`, `testCommand`, `testArgs` and `testTimeout` occurrence.
- [ ] Classify remaining occurrences as historical, adversarial fixture or exact separate TypeScript surface.
- [ ] Fail review on an unclassified current occurrence.

### Compatibility and documentation review

- [ ] Current schema and current docs agree on absence.
- [ ] Historical release notes remain explicitly historical.
- [ ] TypeScript Test Explorer settings are not silently changed or claimed by this PR.

### Scope and claim review

- [ ] No `RunnerPlan`, test discovery, TAP, ProcessSupervisor or DAP behavior is implemented here.
- [ ] No installed/editor testing support is claimed.
- [ ] PR relation closes #10136 only after all acceptance rows are satisfied; otherwise it advances the issue.

## Verification

Discover final exact test filters from the edited tree. Expected minimum:

```bash
cargo fmt --all -- --check
cargo test -p perl-lsp-rs-core --all-targets --locked config
cargo test -p perl-lsp-rs-core --all-targets --locked perllsp_settings_schema
cargo test -p perl-lsp-rs --all-targets --locked configuration
cargo test -p perl-lsp-rs --all-targets --locked smoke
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs --all-targets --locked -- -D warnings
cargo xtask check-architecture
cargo xtask check-test-wiring
cargo xtask docs-check
git diff --check
```

Additional required checks:

```text
search current production tree for test_runner_
search current production tree for testRunner/testCommand/testArgs/testTimeout
run schema/document generation twice and require no second diff
run hostile command/args canary proof
```

A command selecting zero tests, missing tool, unavailable runner or failed
instrument is `NOT_PROVEN`, not green.

## Completion receipt

- [ ] Exact base and head SHA recorded.
- [ ] Removed current surfaces listed.
- [ ] Remaining historical/adversarial/TypeScript occurrences dispositioned.
- [ ] Positive and negative controls listed with results.
- [ ] Established and explicitly-not-established claims recorded.
- [ ] #10898 and #6736 named as downstream owners rather than implied complete.
