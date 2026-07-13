# VS Code extension test runner decision

Status: retain Jest as the unit-test authority until a parity harness shows a
material operational improvement from another runner.

## Evidence captured

The probe ran from the current extension tree on 2026-07-13 with Node
`v26.5.0` and npm `10.8.2` (the repository's npm authority). The current Jest
pipeline completed these checks:

| Check                   | Result                             | Wall time |
| ----------------------- | ---------------------------------- | --------: |
| `npm run compile:test`  | pass                               |    2.05 s |
| `npm run test:ci`       | pass: 732 tests, 1 documented skip |   16.52 s |
| `npm run typecheck:all` | pass                               |    7.17 s |
| `npm run lint`          | pass: 446/446 warning budget       |    2.46 s |

The Jest result includes the existing post-completion downloader warning on
Windows (`ENOENT` from an asynchronous test fixture after the suite exits).
The process still exits successfully; this probe does not claim that warning
is resolved.

The repository has no `vitest` dependency or configuration. A bounded probe
using `npx --yes vitest@latest` resolved Vitest `4.1.10` and exited in 4.36 s,
but it was not a comparable test run: Vitest discovered both `src/test/**/*.ts`
and the emitted `out-test/**/*.js`, then failed 71 suites because the current
contract depends on Jest globals, `@jest/globals`, the Jest `vscode` module
mapper, and Jest-specific mocks. The probe therefore establishes integration
work, not a performance result.

## Decision

Keep Jest as the canonical unit runner. The current compile-ahead design is
already decoupled from `ts-jest`, works with TypeScript 7, preserves source
maps, and has a green full-suite proof. Introducing Vitest now would add a
runner migration, mock-environment conversion, discovery rules, and coverage
parity work without evidence of a material benefit.

This decision does not prohibit a future comparison. A migration proposal must
first provide a separate parity harness that:

1. runs the same unit-test surface without mixing source and emitted files;
2. preserves VS Code module mocks, Jest-style fake timers, module mocks, and
   source-mapped failures;
3. compares clean-install size, full-suite and focused-test time, watch
   latency, coverage parity, Windows behavior, failure output, and open-handle
   detection; and
4. records the commands, versions, environment, and limitations before
   recommending a switch.

Until that evidence exists, Jest remains the lower-risk and reproducible
authority. This record does not make a startup-performance claim and does not
change runtime extension behavior.
