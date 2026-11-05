#!/usr/bin/env node
/**
 * Type-aware lint canary (PREP-2, #3662).
 *
 * The dangerous failure mode in Oxlint's type-aware backend is
 * NOT "the rule is wrong" — it's "the type-aware engine
 * silently doesn't run and the lint check passes green anyway" (a crash
 * during tsgolint init, an unsupported platform, a version mismatch, or a
 * silent fallback to syntax-only linting would all produce a false green).
 *
 * This script makes that invariant observable and CI-blocking: a green
 * `npm run lint:canary` is proof that type-aware `typescript/no-floating-promises`
 * genuinely executed, not just that the process exited 0.
 *
 * It generates two throwaway fixtures into a fresh OS temp directory at run
 * time (never under `src/`, never shipped in the VSIX, never lint-scoped by
 * `.oxlintrc.json`'s normal `ignorePatterns`):
 *
 *   (a) bad.ts  — a bare, unhandled floating promise. This MUST be flagged
 *       by `typescript/no-floating-promises` (type-aware only — there is no
 *       syntax-only equivalent). If it is NOT flagged, either type-aware
 *       mode silently fell back to syntax-only, or the rule regressed —
 *       either way this script FAILS the job.
 *
 *   (b) good.ts — the same call, but `await`ed. This MUST pass cleanly. If
 *       it is flagged, or if the run errors out for an unrelated reason
 *       (e.g. tsgolint failed to initialize), this script FAILS the job —
 *       a failure to initialize type-aware mode is itself a red result,
 *       never silently ignored.
 *
 * Because case (a) can only be caught by the type-aware engine, and this
 * script requires it to be caught, there is no code path by which a silent
 * fallback to syntax-only linting produces a green result here.
 */

const { spawnSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { createReporter } = require('./reporter');

const reporter = createReporter('lint-canary');

const EXT_ROOT = path.resolve(__dirname, '..');

// Invoke oxlint's own JS entry point (node_modules/oxlint/bin/oxlint, which
// dispatches to the platform-native binding) directly via `node`, rather
// than the npm-generated node_modules/.bin/oxlint(.cmd) shim. This avoids
// two Windows-specific problems: spawnSync EINVALs on a bare .cmd without
// `shell: true`, and `shell: true` string-concatenates arguments instead of
// escaping them (Node's own DEP0190 warning) — a real risk once paths can
// contain spaces (e.g. a user profile directory).
const OXLINT_ENTRY = path.join(EXT_ROOT, 'node_modules', 'oxlint', 'bin', 'oxlint');

const CANARY_TSCONFIG = {
  compilerOptions: {
    target: 'ES2022',
    module: 'commonjs',
    lib: ['ES2022'],
    strict: true,
    skipLibCheck: true,
  },
  include: ['*.ts'],
};

const CANARY_OXLINTRC = {
  plugins: ['typescript'],
  categories: { correctness: 'off' },
  rules: {
    'typescript/no-floating-promises': 'error',
  },
};

const BAD_TS = `// Canary fixture (a): a bare, unhandled floating promise.
// typescript/no-floating-promises (type-aware) MUST flag this.
async function doAsyncThing(): Promise<void> {
  return Promise.resolve();
}

export function callSite(): void {
  doAsyncThing();
}
`;

const GOOD_TS = `// Canary fixture (b): the same call, properly awaited.
// typescript/no-floating-promises MUST NOT flag this.
async function doAsyncThing(): Promise<void> {
  return Promise.resolve();
}

export async function callSite(): Promise<void> {
  await doAsyncThing();
}
`;

/**
 * Runs oxlint against a fixture file in `tmpDir`, using absolute paths for
 * every argument. The process cwd is deliberately kept at EXT_ROOT (not
 * tmpDir): oxlint-tsgolint is resolved via Node module resolution starting
 * from cwd, and a temp directory outside the extension's node_modules tree
 * cannot see it — running from EXT_ROOT with absolute fixture/config paths
 * gets a real type-aware run without polluting the extension's own lint
 * scope with the canary fixtures.
 *
 * @param {string} tmpDir
 * @param {string} file
 * @param {0 | 1} expectedStatus
 * @returns {Array<{code?: string, severity?: string, filename?: string}>}
 */
function runOxlint(tmpDir, file, expectedStatus) {
  const result = spawnSync(
    process.execPath,
    [
      OXLINT_ENTRY,
      path.join(tmpDir, file),
      '--type-aware',
      '--format',
      'json',
      '-c',
      path.join(tmpDir, '.oxlintrc.json'),
      '--tsconfig',
      path.join(tmpDir, 'tsconfig.json'),
    ],
    { cwd: EXT_ROOT, encoding: 'utf8' },
  );
  const detail =
    `exit=${result.status} signal=${result.signal ?? 'none'} error=${result.error?.message ?? 'none'}` +
    `\nstdout:\n${(result.stdout || '').slice(0, 2000)}\nstderr:\n${(result.stderr || '').slice(0, 2000)}`;
  if (
    result.error ||
    result.signal ||
    result.status !== expectedStatus ||
    (result.stderr || '').trim()
  ) {
    throw new Error(
      `${file} did not complete cleanly with the expected exit ${expectedStatus}.\n${detail}`,
    );
  }

  let report;
  try {
    report = JSON.parse(result.stdout || '');
  } catch {
    throw new Error(`${file} did not return JSON diagnostics.\n${detail}`);
  }
  // This canary proves one exact file was analyzed. The warning-budget reader
  // accepts diagnostic arrays without metadata; that is insufficient here.
  if (!report || !Array.isArray(report.diagnostics) || report.number_of_files !== 1) {
    throw new Error(`${file} did not return diagnostics for exactly one analyzed file.\n${detail}`);
  }
  return report.diagnostics;
}

function fail(message) {
  reporter.error(`FAIL: ${message}`);
  reporter.error(
    'This canary could not prove that type-aware analysis completed correctly. ' +
      'Initialization failures, malformed diagnostics, and syntax-only fallback must not pass ' +
      'as a green lint check. See scripts/lint-canary.js for the invariant this proves.',
  );
  process.exitCode = 1;
}

function main() {
  const oxlintVersion = require(
    path.join(EXT_ROOT, 'node_modules', 'oxlint', 'package.json'),
  ).version;
  const tsgolintVersion = require(
    path.join(EXT_ROOT, 'node_modules', 'oxlint-tsgolint', 'package.json'),
  ).version;

  reporter.info(`oxlint@${oxlintVersion}, installed oxlint-tsgolint@${tsgolintVersion}`);
  if (process.env.OXLINT_TSGOLINT_PATH !== undefined) {
    reporter.info(
      `type-aware backend override: ${JSON.stringify(process.env.OXLINT_TSGOLINT_PATH)}`,
    );
  }
  reporter.info(
    'asserting type-aware mode is genuinely engaged (not a silent syntax-only fallback)...',
  );

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-oxlint-canary-'));
  let allPassed = false;

  try {
    fs.writeFileSync(path.join(tmpDir, 'tsconfig.json'), JSON.stringify(CANARY_TSCONFIG, null, 2));
    fs.writeFileSync(path.join(tmpDir, '.oxlintrc.json'), JSON.stringify(CANARY_OXLINTRC, null, 2));
    fs.writeFileSync(path.join(tmpDir, 'bad.ts'), BAD_TS);
    fs.writeFileSync(path.join(tmpDir, 'good.ts'), GOOD_TS);

    // Case (a): bare floating promise MUST be flagged. A clean/zero-exit
    // result here means the engine did not really run type-aware analysis.
    const badDiagnostics = runOxlint(tmpDir, 'bad.ts', 1);
    const [badDiagnostic] = badDiagnostics;
    if (
      badDiagnostics.length !== 1 ||
      badDiagnostic?.code !== 'typescript(no-floating-promises)' ||
      badDiagnostic?.severity !== 'error' ||
      typeof badDiagnostic?.filename !== 'string' ||
      path.resolve(EXT_ROOT, badDiagnostic.filename) !== path.join(tmpDir, 'bad.ts')
    ) {
      throw new Error(
        'case (a) — expected exactly one typescript(no-floating-promises) error for bad.ts.\n' +
          JSON.stringify(badDiagnostics).slice(0, 2000),
      );
    }
    reporter.info(
      'OK  case (a): bad.ts flagged by typescript/no-floating-promises (type-aware engine ran).',
    );

    // Case (b): the awaited form MUST pass. A nonzero/error result here
    // means either a false positive or failed analysis — both are RED.
    const goodDiagnostics = runOxlint(tmpDir, 'good.ts', 0);
    if (goodDiagnostics.length !== 0) {
      throw new Error(
        'case (b) — expected no diagnostics for the properly-awaited good.ts.\n' +
          JSON.stringify(goodDiagnostics).slice(0, 2000),
      );
    }
    reporter.info('OK  case (b): good.ts passes cleanly.');
    allPassed = true;
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }

  if (allPassed) {
    reporter.info('PASS — type-aware typescript/no-floating-promises genuinely executed.');
    process.exitCode = 0;
  }
}

main();
