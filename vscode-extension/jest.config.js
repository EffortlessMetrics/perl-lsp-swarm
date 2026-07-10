/**
 * Jest configuration — runs the extension's unit suite against the JS emitted by
 * `tsc -p tsconfig.test.json` (into out-test/), with NO transformer.
 *
 * Previously this config used `preset: 'ts-jest'`, which transpiled each .ts on
 * the fly through the TypeScript compiler API. ts-jest pins a `typescript` peer
 * of `>=4.3 <7`, so that coupling blocked upgrading the extension to TypeScript
 * 7. Compiling ahead of time with tsc and running the emitted CommonJS directly
 * removes the ts-jest dependency entirely — the same tests, discovery, mocks,
 * coverage, and source-mapped failure locations, now decoupled from the compiler
 * version. No Babel / SWC / esbuild is introduced: `transform: {}` disables all
 * transformation and Jest executes the tsc output as-is.
 *
 * Source maps: tsconfig.test.json emits `inlineSourceMap` + `inlineSources`, so
 * stack traces and coverage map back to the original .ts (verified: a failing
 * assertion reports `src/test/<file>.ts:line:col`).
 *
 * Coverage: the v8 provider is used because it needs no transform step and honors
 * the emitted inline source maps to attribute coverage to the .ts sources.
 *
 * Integration (@vscode/test-electron) and published-smoke suites are Mocha and
 * are excluded from tsconfig.test.json, so they are never emitted into out-test
 * and never picked up here — they run via their own tsconfig + runner.
 *
 * @type {import('jest').Config}
 */
module.exports = {
  testEnvironment: 'node',
  roots: ['<rootDir>/out-test/test'],
  testMatch: ['**/*.test.js'],
  moduleNameMapper: {
    '^vscode$': '<rootDir>/out-test/test/__mocks__/vscode.js',
  },
  transform: {},
  coverageProvider: 'v8',
};
