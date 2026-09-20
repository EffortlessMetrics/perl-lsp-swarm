# Context: #10136 — remove LSP-client test-runner process authority

## Problem

The generic LSP configuration model currently exposes an inert but dangerous
process-authority contract:

```text
testRunner.enabled
testRunner.command
testRunner.args
testRunner.timeout
```

Those keys are accepted from the same undifferentiated JSON object used by
`initializationOptions`, `workspace/didChangeConfiguration`, and
`workspace/configuration` results. `ServerConfig` then stores the values as:

```text
test_runner_enabled
test_runner_command
test_runner_args
test_runner_timeout
```

Current-main inventory found no behavior-bearing server process planner or runner
that consumes those fields. They survive in configuration parsing/defaults,
legacy configuration reflection, the configuration-authority catalog, schema and
tests. The public contract is therefore false today and is a latent arbitrary
process-authority path for the next testing cutover.

This issue removes that authority before #10898 connects accepted testing
configuration to the canonical test-runner lifecycle.

## Governing ruling

Client, workspace and project configuration do not select an executable, shell,
argv, cwd, environment or runner command. A future testing configuration surface
may reduce or bound an already server-owned `RunnerPlan`; it may not construct the
plan from generic strings.

The complete current `testRunner` block is retired. `enabled` and `timeout` are
removed with `command` and `args` because no current behavior-bearing consumer
establishes their semantics. #10898 may reintroduce only the fields it can bind to
one accepted configuration generation and one real canonical run vertical.

## Current live-surface inventory

### Rust accepted-state surface

`crates/perl-lsp-rs-core/src/config/mod.rs` currently owns:

- four public `ServerConfig` fields;
- four defaults;
- generic JSON parsing of `testRunner`;
- wrong-type warning treatment; and
- tests that currently pin assignment of arbitrary command and argument values.

Disposition: remove the fields, defaults and parser branch. Replace the positive
assignment test with a hostile-input negative control proving the removed block
cannot alter accepted server state.

### Runtime reflection surface

`crates/perl-lsp-rs/src/runtime/workspace.rs` currently reflects legacy names:

```text
perl.testRunner.enabled
perl.testRunner.testCommand
perl.testRunner.testArgs
perl.testRunner.testTimeout
```

Disposition: remove every reflection arm. Removed keys are unsupported, not
aliases for hidden accepted state.

### Declarative authority surface

`crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs` currently names
`test.enabled`, `test.command`, `test.args`, and `test.timeout_ms`, including a
claimed `TestRunner` consumer and executable sensitivity for command/args.

Disposition: remove those rows. A declaration without a located first-effect
consumer is not retained as future authority.

### Schema and documentation surface

`schemas/perllsp-settings.schema.json` and generated/reference documentation
currently advertise `testRunner` as supported configuration.

Disposition: remove the object and regenerate or update every current projection.
Historical release notes and forensics remain historical when clearly scoped; they
must not remain a current configuration instruction.

### Test and fuzz surface

Current tests reference the four fields in:

- `crates/perl-lsp-rs-core/tests/perllsp_settings_schema_tests.rs`;
- `crates/perl-lsp-rs-core/tests/wave_final_absorption_tests.rs`;
- `crates/perl-lsp-rs/tests/lsp_smoke_e2e.rs`; and
- inline configuration tests in `config/mod.rs`.

The configuration fuzz target may continue to submit removed keys as hostile or
legacy input, but the result must be no accepted process authority.

Disposition: replace value/default assertions with absence, rejection/ignore and
recurrence controls. Do not delete hostile-input coverage merely because the keys
are no longer supported.

## Authority laws

1. No generic LSP payload can store executable, argv, cwd, environment, shell or
   runner-selection state.
2. Removed `testRunner` input cannot be reflected as effective configuration.
3. A compatibility path cannot preserve the same authority under `testCommand`,
   `testArgs` or another spelling.
4. Configuration catalogs and schemas describe current behavior; an inert field
   does not remain public for possible future use.
5. Future testing configuration may only reduce or bound a server-owned plan
   through the accepted configuration programme.
6. Unknown or removed input is a typed unsupported/deprecated observation where
   the current transport supports that distinction; it is never accepted state.
7. Warnings and receipts retain field identity and disposition only. They do not
   retain untrusted command or argument values.
8. The TypeScript Test Explorer/process stack is a separate product surface. This
   issue neither rewrites it nor uses its existence to justify the Rust server
   fields.

## Implementation boundary

### Required removal

```text
ServerConfig.test_runner_*
ServerConfig defaults for those fields
ServerConfig::update_from_value testRunner mutation
legacy workspace/configuration reflection aliases
configuration-authority test.* rows
current generic schema/documentation rows
positive tests that treat arbitrary client strings as supported behavior
```

### Required replacement evidence

- hostile `testRunner` input produces no stored process authority;
- current configuration reflection does not expose removed keys;
- current schema rejects or does not define the block;
- architecture/source inventory fails if the removed field or aliases return;
- unrelated inlay, formatter, critic, AI, workspace and telemetry settings retain
  their existing behavior;
- the TypeScript test stack is unchanged and explicitly outside the claim.

## Strongest counter-read

The strongest case for retaining the fields is that they are currently inert and
could later feed a trusted runner implementation. That is the wrong sequencing.
An inert public field advertises behavior that does not exist and gives later code
a convenient mutable string surface to consume. Preserving it silently decides the
future process-authority model before `RunnerPlan`, accepted configuration and hard
envelopes have established their own contracts.

The safer and smaller action is removal now. A later reviewed runner kind or
bounded timeout request can be added when one real consumer proves its first
effect.

## Dependencies and hand-off

This issue is ready independently of the full configuration-generation train. It
precedes the behavior-bearing testing consumer cutover:

```text
#10136 remove false generic process authority
+ #10857 accepted configuration store/change set
+ canonical RunnerPlan / ProcessSupervisor owners
→ #10898 accepted testing configuration consumer cutover
→ #7066 exact-process fan-in
→ #6736 controller reconciliation
```

A valid result closes #10136 and advances #10898/#6736. It does not claim the
server has a complete testing service.

## Rollback and stop conditions

Rollback must not restore generic command or argv authority. If a current-main
rebase reveals a real production consumer, stop and classify it by exact executable
and plan owner before editing. Retain only server-owned constants or a canonical
plan seam; do not preserve raw client strings.

Stop if completion requires designing test discovery, TAP semantics, runner
selection, process supervision, extension Test Explorer behavior, DAP test
execution or installed-client support. Those remain with their existing owners.

## Scope boundary

In scope: Rust server configuration state, generic schema/current docs, legacy
reflection, authority catalog, focused tests, recurrence checks and this spec
bundle.

Out of scope: a new test runner, `RunnerPlan` design, subprocess algorithms,
TypeScript Test Explorer changes, actual-editor proof, installed-product claims,
release publication and the broader #6736 transaction implementation.
