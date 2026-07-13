# Oxlint warning baseline

This is the ratchet for the expanded Oxlint scope. It covers `src/`,
`src/test/`, `scripts/`, `jest.config.js`, and `rolldown.config.mjs` with
type-aware checking enabled.

The machine-readable inventory is [`oxlint-warning-baseline.json`](./oxlint-warning-baseline.json).

- command: `npm run lint`
- Oxlint: 1.73.0
- oxlint-tsgolint: 0.24.0
- result: 0 errors, 444 warnings
- breakdown by rule: 413 `typescript/no-explicit-any`, 16 `no-console`, 12
  `no-unused-vars`, 3 `typescript/consistent-type-imports`
- breakdown by surface: 428 tests, 16 scripts, 0 production, 0 build-config

`scripts/check-oxlint-warning-budget.js` consumes Oxlint JSON diagnostics and
enforces a non-increasing total, per-rule, and per-surface inventory. New
errors fail immediately; warning cleanup may lower the budget, but increasing
any baseline bucket requires an intentional baseline review.
