#!/usr/bin/env node
/**
 * Type-aware lint canary (PREP-2, #3662).
 *
 * Oxlint's type-aware backend (oxlint-tsgolint) is alpha. The dangerous
 * failure mode is NOT "the rule is wrong" — it's "the type-aware engine
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
 * The rule this canary exists to prove. Oxlint's JSON reporter emits it in
 * `typescript(<rule>)` form; the human-readable reporter emits the bare
 * `typescript/no-floating-promises` form. Matching the structured code is what
 * makes a diagnostic attributable to this exact rule.
 */
const EXPECTED_RULE_CODE = 'typescript(no-floating-promises)';

/**
 * Runs oxlint against a fixture file in `tmpDir`, using absolute paths for
 * every argument. The process cwd is deliberately kept at EXT_ROOT (not
 * tmpDir): oxlint-tsgolint is resolved via Node module resolution starting
 * from cwd, and a temp directory outside the extension's node_modules tree
 * cannot see it — running from EXT_ROOT with absolute fixture/config paths
 * gets a real type-aware run without polluting the extension's own lint
 * scope with the canary fixtures.
 *
 * `--format json` is required: the human-readable reporter is free-form text,
 * so a rule name can appear in it for reasons unrelated to a diagnostic being
 * reported for the fixture under test (a backend initialization error, a
 * diagnostic for an unrelated file). The JSON reporter makes each diagnostic a
 * structured record carrying its own rule code and file, which is the only
 * shape from which "this rule fired on this file" can be established.
 *
 * Spawn errors and signals are preserved rather than dropped: a run that could
 * not execute at all is not a clean run, and must not be indistinguishable
 * from one that executed and found nothing.
 *
 * @returns {{status: number|null, stdout: string, stderr: string, signal: string|null, error: Error|null}}
 */
function runOxlint(tmpDir, file) {
  const result = spawnSync(
    process.execPath,
    [
      OXLINT_ENTRY,
      path.join(tmpDir, file),
      '--type-aware',
      '-c',
      path.join(tmpDir, '.oxlintrc.json'),
      '--tsconfig',
      path.join(tmpDir, 'tsconfig.json'),
      '--format',
      'json',
    ],
    { cwd: EXT_ROOT, encoding: 'utf8' },
  );
  return {
    status: result.status,
    stdout: result.stdout || '',
    stderr: result.stderr || '',
    signal: result.signal || null,
    error: result.error || null,
  };
}

/**
 * Parses oxlint's JSON reporter output into its diagnostic records.
 *
 * Returns a failure reason instead of throwing, because malformed output is
 * itself a red condition the canary must report rather than crash on: a
 * malformed report means the tool did not produce a trustworthy answer, and
 * "the tool printed something unparseable" must never be read as "the tool
 * reported no problems".
 *
 * @param {string} stdout
 * @returns {{diagnostics: Array<{code?: string, filename?: string, severity?: string}>, parseError: string|null}}
 */
function parseOxlintJson(stdout) {
  const trimmed = (stdout || '').trim();
  if (trimmed.length === 0) {
    return { diagnostics: [], parseError: 'oxlint produced no output to parse' };
  }
  let parsed;
  try {
    parsed = JSON.parse(trimmed);
  } catch (error) {
    return {
      diagnostics: [],
      parseError: `oxlint output was not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
  if (!parsed || typeof parsed !== 'object' || !Array.isArray(parsed.diagnostics)) {
    return {
      diagnostics: [],
      parseError: 'oxlint JSON output did not contain a diagnostics array',
    };
  }
  return { diagnostics: parsed.diagnostics, parseError: null };
}

/**
 * Normalizes a diagnostic's reported file to a comparable path.
 *
 * Oxlint reports forward slashes on every platform, and the canary's fixture
 * path is absolute, so both sides are normalized before comparison. A
 * diagnostic whose file cannot be resolved is left as an empty string, which
 * never matches a real fixture path — an unattributable diagnostic must not
 * be able to satisfy the attribution check.
 *
 * @param {string | undefined} filename
 * @returns {string}
 */
function normalizeDiagnosticPath(filename) {
  if (typeof filename !== 'string' || filename.length === 0) {
    return '';
  }
  const normalized = filename.replace(/\\/g, '/');
  const marker = '/';
  const slashIndex = normalized.lastIndexOf(marker);
  return slashIndex === -1 ? normalized : normalized.slice(slashIndex + 1);
}

/**
 * Evaluates one oxlint run against the expectation for its fixture.
 *
 * This is the pure core of the canary: it takes a completed run and the
 * expectation, and returns whether the run satisfied it plus the reasons it
 * did not. Keeping it free of process and filesystem concerns is what makes
 * the negative controls (malformed output, a diagnostic for the wrong file,
 * a diagnostic for the wrong rule, a spawn failure) directly testable with
 * synthetic runs instead of requiring the native tools to misbehave.
 *
 * @param {{status: number|null, stdout: string, stderr: string, signal?: string|null, error?: Error|null}} run
 * @param {{file: string, expectDiagnostics: boolean}} expectation
 * @returns {{ok: boolean, failures: string[]}}
 */
function evaluateRun(run, expectation) {
  const failures = [];

  if (run.error) {
    failures.push(`oxlint could not be executed: ${run.error.message}`);
  }
  if (run.signal) {
    failures.push(`oxlint was terminated by signal ${run.signal}`);
  }

  const { diagnostics, parseError } = parseOxlintJson(run.stdout);
  if (parseError) {
    failures.push(parseError);
  }

  const expectedFile = normalizeDiagnosticPath(expectation.file);
  const matching = diagnostics.filter(
    (diagnostic) =>
      diagnostic &&
      diagnostic.code === EXPECTED_RULE_CODE &&
      normalizeDiagnosticPath(diagnostic.filename) === expectedFile,
  );
  const otherDiagnostics = diagnostics.filter(
    (diagnostic) => !matching.includes(diagnostic),
  );

  if (expectation.expectDiagnostics) {
    if (matching.length === 0) {
      failures.push(
        `expected ${EXPECTED_RULE_CODE} to be reported for ${expectation.file}, ` +
          'but no such diagnostic was present',
      );
    }
    if (run.status === 0) {
      failures.push(
        `expected a failing exit status for ${expectation.file}, but oxlint exited 0`,
      );
    }
  } else {
    if (matching.length > 0) {
      failures.push(
        `expected no ${EXPECTED_RULE_CODE} diagnostic for ${expectation.file}, ` +
          `but ${matching.length} were reported`,
      );
    }
    if (run.status !== 0) {
      failures.push(
        `expected a clean exit status for ${expectation.file}, but oxlint exited ${run.status}`,
      );
    }
  }

  if (otherDiagnostics.length > 0) {
    failures.push(
      `oxlint reported ${otherDiagnostics.length} unexpected diagnostic(s) for ${expectation.file}: ` +
        otherDiagnostics
          .map((diagnostic) => `${diagnostic.code ?? '<no code>'} in ${diagnostic.filename ?? '<no file>'}`)
          .join(', '),
    );
  }

  return { ok: failures.length === 0, failures };
}

function fail(message, detail) {
  reporter.error(`FAIL: ${message}`);
  if (detail) {
    reporter.error(detail);
  }
  reporter.error(
    'A red result here means type-aware analysis either did not run, ' +
      'crashed, or silently fell back to syntax-only linting — none of which may pass ' +
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

  reporter.info(`oxlint@${oxlintVersion}, oxlint-tsgolint@${tsgolintVersion}`);
  reporter.info(
    'asserting type-aware mode is genuinely engaged (not a silent syntax-only fallback)...',
  );

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-oxlint-canary-'));
  let allPassed = true;

  try {
    fs.writeFileSync(path.join(tmpDir, 'tsconfig.json'), JSON.stringify(CANARY_TSCONFIG, null, 2));
    fs.writeFileSync(path.join(tmpDir, '.oxlintrc.json'), JSON.stringify(CANARY_OXLINTRC, null, 2));
    fs.writeFileSync(path.join(tmpDir, 'bad.ts'), BAD_TS);
    fs.writeFileSync(path.join(tmpDir, 'good.ts'), GOOD_TS);

    // Case (a): bare floating promise MUST be flagged. A clean/zero-exit
    // result here means the engine did not really run type-aware analysis.
    const badResult = runOxlint(tmpDir, 'bad.ts');
    const badVerdict = evaluateRun(badResult, { file: 'bad.ts', expectDiagnostics: true });
    if (!badVerdict.ok) {
      allPassed = false;
      fail(
        'case (a) — the deliberate floating promise in bad.ts was NOT flagged by typescript/no-floating-promises.',
        badVerdict.failures.join('\n'),
      );
    } else {
      reporter.info(
        'OK  case (a): bad.ts flagged by typescript/no-floating-promises (type-aware engine ran).',
      );
    }

    // Case (b): the awaited form MUST pass. A nonzero/error result here
    // means either a false positive, or (more likely under alpha tooling)
    // that type-aware initialization itself failed — both are RED.
    const goodResult = runOxlint(tmpDir, 'good.ts');
    const goodVerdict = evaluateRun(goodResult, { file: 'good.ts', expectDiagnostics: false });
    if (!goodVerdict.ok) {
      allPassed = false;
      fail(
        'case (b) — the properly-awaited good.ts was flagged, or the run failed to complete cleanly ' +
          '(this includes tsgolint failing to initialize).',
        goodVerdict.failures.join('\n'),
      );
    } else {
      reporter.info('OK  case (b): good.ts passes cleanly.');
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }

  if (allPassed) {
    reporter.info('PASS — type-aware typescript/no-floating-promises genuinely executed.');
    process.exitCode = 0;
  }
}

module.exports = {
  EXPECTED_RULE_CODE,
  evaluateRun,
  normalizeDiagnosticPath,
  parseOxlintJson,
};

if (require.main === module) {
  main();
}
