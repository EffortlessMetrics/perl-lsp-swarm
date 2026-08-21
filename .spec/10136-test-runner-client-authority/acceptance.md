# Acceptance: #10136 — removed generic test-runner authority

## Required outcome

The Rust LSP server has no accepted generic configuration state capable of
selecting a test executable, command, argv or runner process. The complete inert
`testRunner` block is absent from current schema, reflection and accepted state.

## Stable acceptance rows

### TESTCFG-001 — no executable field

`ServerConfig` and every accepted testing view contain no generic executable,
command, shell or runner-path field derived from LSP/client/workspace/project
payloads.

### TESTCFG-002 — no argv field

`ServerConfig` and every accepted testing view contain no generic argument vector
or free-form command-line fragment derived from those payloads.

### TESTCFG-003 — no inert enablement contract

`testRunner.enabled` is absent from current generic configuration unless a real
canonical testing consumer is introduced under #10898. This issue introduces no
such consumer.

### TESTCFG-004 — no inert timeout contract

`testRunner.timeout` is absent from current generic configuration. A future timeout
is an admitted hard-envelope contribution owned by #10898/#10917, not a raw
process limit stored here.

### TESTCFG-005 — hostile initialization options cannot arm a runner

An `initializationOptions` payload containing all four legacy keys produces no
stored executable, argv, enablement or timeout state and no process planning or
start effect.

### TESTCFG-006 — hostile didChangeConfiguration cannot arm a runner

A `workspace/didChangeConfiguration` payload containing the legacy block produces
no accepted process authority and leaves unrelated accepted configuration
unchanged except for explicitly supplied unrelated fields.

### TESTCFG-007 — configuration response cannot arm a runner

Unscoped and per-root `workspace/configuration` result items cannot populate the
removed state, regardless of slot, root or client-provided labels.

### TESTCFG-008 — removed keys are not reflected

The legacy reflection route exposes none of:

```text
perl.testRunner.enabled
perl.testRunner.testCommand
perl.testRunner.testArgs
perl.testRunner.testTimeout
```

### TESTCFG-009 — schema and current docs agree

The current generic settings schema contains no `testRunner` object or equivalent
command/argv aliases. Generated/current configuration documentation agrees with
the schema.

### TESTCFG-010 — authority catalog agrees

The current configuration-authority catalog contains no `test.enabled`,
`test.command`, `test.args` or `test.timeout_ms` rows and makes no unsupported
`TestRunner` consumer claim.

### TESTCFG-011 — removed input is bounded and non-secret

Where a warning or unsupported/deprecated observation is emitted, it names only
the removed field family and disposition. It does not persist or print command,
argument, path or environment values supplied by the client.

### TESTCFG-012 — unrelated settings survive

Removal does not change accepted behavior for inlay hints, telemetry, native
critic policy, formatting policy, AI settings, next-edit gating or workspace
module-resolution settings.

### TESTCFG-013 — no hidden alias or compatibility storage

No parser, reflection map, compatibility adapter, schema alias, serde field or
sidecar store preserves the removed authority under another spelling.

### TESTCFG-014 — TypeScript test stack remains separate

The VS Code Test Explorer/process implementation remains unchanged by this PR and
is not presented as proof that the Rust generic settings are behavior-backed.

### TESTCFG-015 — recurrence is mechanical

A source/architecture check fails when any removed Rust field, generic JSON key,
legacy reflection alias or authority-catalog row is restored to a current
production surface.

### TESTCFG-016 — exact claim boundary

The completion receipt states:

```text
established:
  generic Rust LSP configuration cannot store or reflect test process authority

not established:
  canonical server testing implementation
  RunnerPlan or ProcessSupervisor correctness
  TypeScript Test Explorer migration
  actual-editor or installed-product testing support
  #6736 exact-process transaction closeout
```

## Required scenario matrix

| Scenario | Expected result |
| --- | --- |
| Legacy block absent | ordinary unrelated settings apply normally |
| `enabled=false` only | unsupported/ignored; no accepted testing state |
| `timeout=0` or maximum integer | unsupported/ignored; no local clamp or process limit |
| arbitrary command | unsupported/ignored; value not retained or logged |
| mixed-type args | unsupported/ignored; values not retained or logged |
| command plus unrelated valid setting | removed block has zero effect; unrelated field applies |
| unscoped configuration response | no special trust or runner authority |
| per-root contradictory legacy blocks | neither root arms or selects a runner |
| repeated identical legacy payload | no accepted-state churn or repeated value-bearing receipt |
| malformed legacy block | no partial mutation and no false success |

## Negative controls

The focused proof must become red when a mutation:

1. restores any `ServerConfig::test_runner_*` field;
2. parses `testRunner.command` or `testRunner.args` into accepted state;
3. restores `testCommand`, `testArgs` or `testTimeout` reflection;
4. restores a generic schema row without a current consumer;
5. lets the authority catalog claim `TestRunner` from declarations alone;
6. retains only `enabled` or `timeout` as inert public state;
7. logs a command or argument canary from hostile input;
8. lets a project/client payload select an executable indirectly through an
   alias, enum string or compatibility object;
9. deletes unrelated configuration behavior to make the legacy tests pass; or
10. claims the TypeScript Test Explorer or a future runner is migrated by this
    removal.

## Completion condition

#10136 is complete only when every current accepted-state, parser, reflection,
catalog, schema and current-documentation row is removed or explicitly historical,
the hostile-input and recurrence controls are discriminating, and no current
behavior-bearing consumer requires the deleted state.
