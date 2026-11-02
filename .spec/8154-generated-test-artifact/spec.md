# Generated test artifact contract (#8154)

## Status

Accepted for the pure-generation slice in `perl-tdd-support`.

## Contract

Generated Perl tests are classified by `GeneratedTestReadiness`:

- `Runnable` — oracle-backed or structural facts (e.g. module load) with no placeholder assertion.
- `DraftNeedsOracle` — invocation or behavior not established; emitted Perl uses `TODO` + `fail('not implemented: …')`.
- `DraftNeedsSetup` — integration/setup/call/verification not performed; same non-passing Perl form.
- `Unsupported` — framework/test-kind combination has no renderer; no empty runnable source.

`TestCase` carries `readiness` plus legacy `is_todo` (true for every non-`Runnable` row).

## Non-goals (this slice)

Execution, TAP parsing, editor application, and TDD cycle Green policy (#4972).
