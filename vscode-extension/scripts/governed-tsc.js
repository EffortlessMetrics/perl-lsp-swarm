#!/usr/bin/env node
'use strict';

/**
 * The single governed TypeScript execution seam (#9842).
 *
 * Before this wrapper, the npm script surface reached `tsc` directly from
 * nine places (`typecheck`, the four other `typecheck:*` configs,
 * `compile:test`, `test:integration`, `test:published`, and a bare
 * `watch:types`). Only the aggregate `typecheck:all` prefixed the
 * compiler-authority gate, so any one of those commands invoked whatever
 * `tsc` resolved to that day without first proving it was the pinned
 * TypeScript 7 authority — the exact bypass `check-typescript-authority.js`
 * exists to make impossible.
 *
 * Every public TypeScript execution in `package.json` now routes through this
 * wrapper. It refuses to compile unless the landed authority gate is green
 * against the real tree, then executes the pinned package's own `bin/tsc`
 * entry point — resolved through the same `declaredTscBin` the authority gate
 * verifies — so the compiler that runs is the compiler that was proven. A
 * second compiler-version checker is deliberately NOT implemented here: the
 * authority logic lives in exactly one module and this seam consumes it.
 *
 * The check runs before every invocation, including inside aggregates like
 * `typecheck:all` and `build`. That is the point: governance that only holds
 * when a human remembers the right prefix is the bypass this seam removes.
 */

const path = require('node:path');
const { spawn } = require('node:child_process');
const { createReporter } = require('./reporter');
const { checkTypeScriptAuthority, declaredTscBin } = require('./check-typescript-authority');

/** The npm script this file is invoked as, for repair guidance. */
const WRAPPER_INVOCATION = 'node scripts/governed-tsc.js';

/**
 * @typedef {{code: number | null, signal: string | null, error?: string}} ExitResult
 *   `error` is set when the child could not be launched at all; such a result
 *   has no exit code and must be treated as failure, never as a hang.
 */

/**
 * Runs the authority gate, then executes the pinned `tsc` with `args`.
 *
 * Dependency injection keeps the two load-bearing behaviors provable without
 * mutating the real tree: `authorityCheck` stands in for the gate, and
 * `spawnChild` for the process launch. Both default to the real ones, and a
 * plain (already-resolved) object is accepted wherever a promise is.
 *
 * @param {{
 *   extensionRoot: string,
 *   args: string[],
 *   reporter: {info: (message: string) => void, error: (message: string) => void},
 *   authorityCheck?: (extensionRoot: string) => {ok: boolean, failures: string[], facts: string[]},
 *   spawnChild?: (command: string, argv: string[]) => Promise<ExitResult> | ExitResult,
 * }} input
 * @returns {Promise<{code: number, spawned: boolean, authorityFailures: string[]}>}
 *   Resolves with the process exit code. Never rejects: a red gate or a
 *   launch failure is a result, not an exception, so callers and tests can
 *   assert on the code rather than catch.
 */
async function runGovernedTsc(input) {
  const authorityCheck = input.authorityCheck ?? checkTypeScriptAuthority;
  const spawnChild = input.spawnChild ?? spawnPinnedTsc;

  if (input.args.length === 0) {
    input.reporter.error(
      `${WRAPPER_INVOCATION} requires the tsc arguments to forward ` +
        '(for example: node scripts/governed-tsc.js --noEmit -p ./tsconfig.json). ' +
        'A bare tsc with no arguments would compile an unintended project.',
    );
    return { code: 2, spawned: false, authorityFailures: [] };
  }

  // Fail closed: the gate runs to completion before any compilation starts,
  // and a red result names the drift instead of running "the tsc that
  // happened to resolve".
  const authority = authorityCheck(input.extensionRoot);
  if (!authority.ok) {
    for (const failure of authority.failures) {
      input.reporter.error(`FAIL: ${failure}`);
    }
    input.reporter.error(
      'Governed tsc refused to compile: the compiler-authority gate is red, so the ' +
        'TypeScript that would run is not the pinned repository authority. Repair the ' +
        'drift above (commonly `npm ci`, or restoring the pinned typescript devDependency), ' +
        'then confirm with `npm run typecheck:authority`.',
    );
    return { code: 1, spawned: false, authorityFailures: authority.failures };
  }

  // Resolve the compiler through the pinned package's own `bin.tsc`
  // declaration — the same resolution the authority gate just verified end to
  // end (shim -> package -> executing binary). Spawning `tsc` by name would
  // re-introduce the PATH resolution the gate cannot vouch for.
  const declared = declaredTscBin(path.join(input.extensionRoot, 'node_modules', 'typescript'));
  if ('reason' in declared) {
    input.reporter.error(
      `FAIL: the pinned TypeScript package cannot be executed (${declared.reason}) — ` +
        'run `npm ci` and retry `npm run typecheck:authority`.',
    );
    return { code: 1, spawned: false, authorityFailures: [] };
  }

  const result = await spawnChild(process.execPath, [declared.binPath, ...input.args]);
  if (result.error !== undefined) {
    input.reporter.error(
      `FAIL: the pinned TypeScript compiler could not be launched (${result.error}) — ` +
        'run `npm ci` and retry `npm run typecheck:authority`.',
    );
  }
  // A child killed by a signal or never launched (code null) must not read as
  // success to npm.
  return {
    code: result.code === null ? 1 : result.code,
    spawned: true,
    authorityFailures: [],
  };
}

/**
 * Spawns the pinned compiler with inherited stdio and resolves exactly once —
 * on its exit, or on a launch failure.
 *
 * `spawn` (not `spawnSync`) keeps watch mode usable: `watch:types` forwards
 * incremental output, and SIGINT/SIGTERM are forwarded so Ctrl+C stops the
 * child tsc rather than orphaning it under npm.
 *
 * A failed launch (`error`, e.g. ENOENT) never emits `exit`; settling only on
 * `exit` would leave the promise pending forever and the npm script hanging.
 * Both events settle the same promise through one cleanup path, first wins.
 *
 * @param {string} command
 * @param {string[]} argv
 * @returns {Promise<ExitResult>}
 */
function spawnPinnedTsc(command, argv) {
  return new Promise((resolve) => {
    const child = spawn(command, argv, { stdio: 'inherit' });
    let settled = false;
    /**
     * @param {NodeJS.Signals} signal
     */
    const forward = (signal) => {
      child.kill(signal);
    };
    const settle = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      process.removeListener('SIGINT', forward);
      process.removeListener('SIGTERM', forward);
      resolve(result);
    };
    process.once('SIGINT', forward);
    process.once('SIGTERM', forward);
    child.once('error', (error) => {
      // No exit event follows a launch failure; the null code maps to a
      // nonzero exit in runGovernedTsc so npm never sees a hang or success.
      settle({ code: null, signal: null, error: error.message });
    });
    child.once('exit', (code, signal) => {
      settle({ code, signal });
    });
  });
}

function main() {
  const reporter = createReporter('governed-tsc');
  runGovernedTsc({
    extensionRoot: path.resolve(__dirname, '..'),
    args: process.argv.slice(2),
    reporter,
  })
    .then((result) => {
      process.exitCode = result.code;
    })
    .catch((error) => {
      // runGovernedTsc resolves rather than throwing, so this guards only a
      // genuinely unexpected failure (for example a spawn crash) — which must
      // still exit red rather than hang the npm script.
      reporter.error(
        `unexpected failure: ${error instanceof Error ? error.message : String(error)}`,
      );
      process.exitCode = 1;
    });
}

if (require.main === module) {
  main();
}

module.exports = { runGovernedTsc, spawnPinnedTsc, WRAPPER_INVOCATION };
