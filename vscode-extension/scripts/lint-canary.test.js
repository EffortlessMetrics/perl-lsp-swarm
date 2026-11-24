'use strict';

/**
 * Regression coverage for the type-aware Oxlint canary (#19098).
 *
 * The canary's whole purpose is to be a trustworthy red: a green
 * `npm run lint:canary` must mean "type-aware analysis genuinely ran and
 * genuinely found the rule", not "the tool printed something containing a
 * rule name somewhere". Before this change the canary accepted a rule-name
 * substring on either output stream of the bad-case run, checked only stdout
 * for the good case, and dropped the subprocess's error/signal state.
 *
 * That shape accepts three classes of false green:
 *
 *   1. a backend initialization failure whose error text happens to mention
 *      the rule name, with no diagnostic actually reported;
 *   2. a diagnostic for an unrelated file, where the expected rule name
 *      appears only inside the diagnostic's message;
 *   3. a run that could not execute at all (spawn error, signal), which
 *      reported as "clean" because there was nothing in stdout.
 *
 * The synthetic-run controls below state exactly the run shape each one is
 * rejecting. The subprocess-level controls additionally drive the real script
 * end to end against a disposable stand-in for oxlint, which proves the
 * script asks for structured JSON output and wires its own exit handling the
 * way the evaluator expects.
 */

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { spawnSync } = require('node:child_process');

const {
  EXPECTED_RULE_CODE,
  evaluateRun,
  normalizeDiagnosticPath,
  parseOxlintJson,
} = require('./lint-canary');

const SCRIPT = path.join(__dirname, 'lint-canary.js');
const REPORTER = path.join(__dirname, 'reporter.js');

/**
 * @param {Partial<{status: number|null, stdout: string, stderr: string, signal: string|null, error: Error|null}>} [overrides]
 */
function run(overrides = {}) {
  return {
    status: 1,
    stdout: '',
    stderr: '',
    signal: null,
    error: null,
    ...overrides,
  };
}

/**
 * @param {Array<{code?: string, filename?: string, severity?: string, message?: string}>} diagnostics
 */
function jsonOutput(diagnostics) {
  return JSON.stringify({ diagnostics, number_of_files: 1, number_of_rules: 1 });
}

/**
 * A well-formed run for the bad fixture: the expected rule fired on bad.ts.
 */
function badRunOk(overrides = {}) {
  return run({
    status: 1,
    stdout: jsonOutput([
      {
        message: 'Promises must be awaited, add void operator to ignore.',
        code: EXPECTED_RULE_CODE,
        severity: 'error',
        filename: '/tmp/canary/bad.ts',
      },
    ]),
    ...overrides,
  });
}

/**
 * A well-formed run for the good fixture: nothing reported, clean exit.
 */
function goodRunOk(overrides = {}) {
  return run({ status: 0, stdout: jsonOutput([]), ...overrides });
}

void test('a well-formed bad-case run is accepted', () => {
  const verdict = evaluateRun(badRunOk(), { file: 'bad.ts', expectDiagnostics: true });
  assert.equal(verdict.ok, true, verdict.failures.join('\n'));
  assert.deepEqual(verdict.failures, []);
});

void test('a well-formed good-case run is accepted', () => {
  const verdict = evaluateRun(goodRunOk(), { file: 'good.ts', expectDiagnostics: false });
  assert.equal(verdict.ok, true, verdict.failures.join('\n'));
  assert.deepEqual(verdict.failures, []);
});

void test('a backend initialization failure that merely mentions the rule name is rejected', () => {
  // The defect this canary must catch: oxlint failed to start the type-aware
  // backend and said so on stderr, naming the rule in the failure text. No
  // diagnostic was ever reported. The pre-change canary saw the rule name on
  // stderr and called it a pass.
  const verdict = evaluateRun(
    run({
      status: 2,
      stdout: '',
      stderr:
        'error: failed to initialize type-aware backend\n' +
        '  the rule typescript/no-floating-promises could not be evaluated\n',
    }),
    { file: 'bad.ts', expectDiagnostics: true },
  );
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('no such diagnostic was present')),
    verdict.failures.join('\n'),
  );
});

void test('a diagnostic for an unrelated file is rejected even when it names the rule in its message', () => {
  // The defect: the expected rule name appears only inside another file's
  // diagnostic message. The pre-change canary matched it as a substring and
  // reported a pass.
  const verdict = evaluateRun(
    run({
      status: 1,
      stdout: jsonOutput([
        {
          message: 'unrelated file mentions typescript/no-floating-promises in prose',
          code: 'oxc/parser',
          severity: 'error',
          filename: '/tmp/canary/other.ts',
        },
      ]),
    }),
    { file: 'bad.ts', expectDiagnostics: true },
  );
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('no such diagnostic was present')),
    verdict.failures.join('\n'),
  );
});

void test('a diagnostic for the right file but the wrong rule is rejected', () => {
  const verdict = evaluateRun(
    run({
      status: 1,
      stdout: jsonOutput([
        {
          message: 'some other problem',
          code: 'oxc/parser',
          severity: 'error',
          filename: '/tmp/canary/bad.ts',
        },
      ]),
    }),
    { file: 'bad.ts', expectDiagnostics: true },
  );
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('no such diagnostic was present')),
    verdict.failures.join('\n'),
  );
});

void test('the good fixture is rejected when the rule is reported for it', () => {
  const verdict = evaluateRun(
    run({
      status: 1,
      stdout: jsonOutput([
        {
          message: 'Promises must be awaited.',
          code: EXPECTED_RULE_CODE,
          severity: 'error',
          filename: '/tmp/canary/good.ts',
        },
      ]),
    }),
    { file: 'good.ts', expectDiagnostics: false },
  );
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('but 1 were reported')),
    verdict.failures.join('\n'),
  );
});

void test('malformed tool output is rejected rather than read as "no problems"', () => {
  for (const stdout of ['', 'not json at all', '{"number_of_files": 1}', 'null']) {
    const verdict = evaluateRun(run({ status: 0, stdout }), {
      file: 'good.ts',
      expectDiagnostics: false,
    });
    assert.equal(verdict.ok, false, `expected a red verdict for stdout: ${JSON.stringify(stdout)}`);
    assert.ok(
      verdict.failures.some(
        (failure) =>
          failure.includes('no output to parse') ||
          failure.includes('not valid JSON') ||
          failure.includes('did not contain a diagnostics array'),
      ),
      verdict.failures.join('\n'),
    );
  }
});

void test('a run that could not execute is rejected', () => {
  const spawnFailure = evaluateRun(
    run({ status: null, stdout: '', error: new Error('spawnSync ENOENT') }),
    { file: 'bad.ts', expectDiagnostics: true },
  );
  assert.equal(spawnFailure.ok, false);
  assert.ok(
    spawnFailure.failures.some((failure) => failure.includes('could not be executed')),
    spawnFailure.failures.join('\n'),
  );

  const signaled = evaluateRun(run({ status: null, stdout: '', signal: 'SIGTERM' }), {
    file: 'good.ts',
    expectDiagnostics: false,
  });
  assert.equal(signaled.ok, false);
  assert.ok(
    signaled.failures.some((failure) => failure.includes('terminated by signal SIGTERM')),
    signaled.failures.join('\n'),
  );
});

void test('the bad fixture exiting zero is rejected', () => {
  // A silent syntax-only fallback would report nothing and exit cleanly. The
  // canary must be red in that case, not green.
  const verdict = evaluateRun(badRunOk({ status: 0 }), {
    file: 'bad.ts',
    expectDiagnostics: true,
  });
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('expected a failing exit status')),
    verdict.failures.join('\n'),
  );
});

void test('a clean exit on the good fixture is required, not merely the absence of a rule name', () => {
  const verdict = evaluateRun(goodRunOk({ status: 2 }), {
    file: 'good.ts',
    expectDiagnostics: false,
  });
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('expected a clean exit status')),
    verdict.failures.join('\n'),
  );
});

void test('unexpected diagnostics on the good fixture are rejected', () => {
  const verdict = evaluateRun(
    run({
      status: 0,
      stdout: jsonOutput([
        {
          message: 'something else',
          code: 'oxc/parser',
          severity: 'error',
          filename: '/tmp/canary/good.ts',
        },
      ]),
    }),
    { file: 'good.ts', expectDiagnostics: false },
  );
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('unexpected diagnostic')),
    verdict.failures.join('\n'),
  );
});

void test('diagnostic file paths are compared on their final segment', () => {
  assert.equal(normalizeDiagnosticPath('/tmp/canary/bad.ts'), 'bad.ts');
  assert.equal(normalizeDiagnosticPath('C:\\tmp\\canary\\bad.ts'), 'bad.ts');
  assert.equal(normalizeDiagnosticPath('bad.ts'), 'bad.ts');
  assert.equal(normalizeDiagnosticPath(undefined), '');
  assert.equal(normalizeDiagnosticPath(''), '');
});

void test('a diagnostic with no file cannot satisfy the attribution check', () => {
  const verdict = evaluateRun(
    run({
      status: 1,
      stdout: jsonOutput([{ message: 'no file', code: EXPECTED_RULE_CODE, severity: 'error' }]),
    }),
    { file: 'bad.ts', expectDiagnostics: true },
  );
  assert.equal(verdict.ok, false);
  assert.ok(
    verdict.failures.some((failure) => failure.includes('no such diagnostic was present')),
    verdict.failures.join('\n'),
  );
});

void test('parseOxlintJson distinguishes an empty report from a report with no diagnostics', () => {
  // The real oxlint always emits the JSON envelope, so an empty stdout means
  // the tool did not report — which is a red condition, not "no problems".
  const empty = parseOxlintJson('');
  assert.notEqual(empty.parseError, null);
  assert.deepEqual(empty.diagnostics, []);

  const malformed = parseOxlintJson('{oops');
  assert.notEqual(malformed.parseError, null);
  assert.deepEqual(malformed.diagnostics, []);

  const reported = parseOxlintJson(jsonOutput([{ code: EXPECTED_RULE_CODE }]));
  assert.equal(reported.parseError, null);
  assert.equal(reported.diagnostics.length, 1);
});

/**
 * Runs the real canary script against a disposable stand-in for oxlint.
 *
 * The script resolves oxlint relative to its own directory
 * (`<dir>/node_modules/oxlint/bin/oxlint`) and reads the two pinned versions
 * from that same tree, so the harness lays out a self-contained copy of the
 * extension's script directory with its own `node_modules` skeleton. The
 * script under test is then executed unmodified, which means the subprocess
 * controls exercise the real argument construction and the real exit handling
 * rather than a re-implementation of them.
 *
 * @param {(args: string[]) => {status: number, stdout?: string, stderr?: string}} behavior
 */
function runScriptAgainstStub(behavior) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'lint-canary-stub-'));
  try {
    const argsFile = path.join(tmpDir, 'args.json');
    // The script resolves its extension root as `__dirname/..` and its oxlint
    // entry point as `<root>/node_modules/oxlint/bin/oxlint`, so the harness
    // reproduces that layout: `<root>/scripts/lint-canary.js` beside
    // `<root>/node_modules/...`.
    const root = path.join(tmpDir, 'extension');
    const scriptDir = path.join(root, 'scripts');
    const oxlintDir = path.join(root, 'node_modules', 'oxlint');
    const tsgolintDir = path.join(root, 'node_modules', 'oxlint-tsgolint');

    fs.mkdirSync(scriptDir, { recursive: true });
    fs.mkdirSync(path.join(oxlintDir, 'bin'), { recursive: true });
    fs.mkdirSync(tsgolintDir, { recursive: true });

    // The script under test, and the reporter it depends on, copied verbatim.
    fs.copyFileSync(SCRIPT, path.join(scriptDir, 'lint-canary.js'));
    fs.copyFileSync(REPORTER, path.join(scriptDir, 'reporter.js'));

    fs.writeFileSync(
      path.join(oxlintDir, 'package.json'),
      JSON.stringify({ name: 'oxlint', version: '0.0.0-stub' }),
    );
    fs.writeFileSync(
      path.join(tsgolintDir, 'package.json'),
      JSON.stringify({ name: 'oxlint-tsgolint', version: '0.0.0-stub' }),
    );
    fs.writeFileSync(
      path.join(oxlintDir, 'bin', 'oxlint'),
      [
        "'use strict';",
        "const fs = require('node:fs');",
        'const args = process.argv.slice(2);',
        `fs.writeFileSync(${JSON.stringify(argsFile)}, JSON.stringify(args));`,
        // The stand-in runs in its own scope, so the rule code it reports is
        // restated here rather than referenced from the test module.
        `const RULE_CODE = ${JSON.stringify(EXPECTED_RULE_CODE)};`,
        `const behavior = ${behavior.toString()};`,
        'const outcome = behavior(args);',
        'if (outcome.stdout) process.stdout.write(outcome.stdout);',
        'if (outcome.stderr) process.stderr.write(outcome.stderr);',
        // Set exitCode rather than calling process.exit(): stdout is a pipe
        // here, so writes are asynchronous and process.exit() would discard
        // the buffered output before it reached the parent.
        'process.exitCode = outcome.status;',
        '',
      ].join('\n'),
      'utf8',
    );

    const result = spawnSync(process.execPath, [path.join(scriptDir, 'lint-canary.js')], {
      cwd: root,
      encoding: 'utf8',
    });

    return {
      status: result.status,
      stdout: result.stdout || '',
      stderr: result.stderr || '',
      args: fs.existsSync(argsFile) ? JSON.parse(fs.readFileSync(argsFile, 'utf8')) : null,
    };
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

void test('the script asks oxlint for structured JSON output', () => {
  const result = runScriptAgainstStub(() => ({
    status: 0,
    stdout: JSON.stringify({ diagnostics: [], number_of_files: 1, number_of_rules: 1 }),
  }));

  assert.ok(result.args, 'the stand-in was never invoked');
  assert.ok(
    result.args.includes('--format') && result.args.includes('json'),
    `expected --format json in the oxlint arguments, got: ${JSON.stringify(result.args)}`,
  );
});

void test('the script is red when the type-aware backend fails to initialize', () => {
  // The regression this issue reports: a backend initialization failure whose
  // text names the rule must not produce a green canary.
  const result = runScriptAgainstStub(() => ({
    status: 2,
    stderr:
      'error: failed to initialize type-aware backend\n' +
      '  the rule typescript/no-floating-promises could not be evaluated\n',
  }));

  assert.notEqual(result.status, 0, 'the canary must be red when the backend fails to initialize');
  assert.match(result.stderr, /FAIL/);
});

void test('the script is green when the structured report is exactly as expected', () => {
  const result = runScriptAgainstStub((args) => {
    const target = args.find((arg) => arg.endsWith('bad.ts') || arg.endsWith('good.ts'));
    if (target && target.endsWith('bad.ts')) {
      return {
        status: 1,
        stdout: JSON.stringify({
          diagnostics: [
            {
              message: 'Promises must be awaited.',
              code: 'typescript(no-floating-promises)',
              severity: 'error',
              filename: target,
            },
          ],
          number_of_files: 1,
          number_of_rules: 1,
        }),
      };
    }
    return {
      status: 0,
      stdout: JSON.stringify({ diagnostics: [], number_of_files: 1, number_of_rules: 1 }),
    };
  });

  assert.equal(result.status, 0, `expected a green canary, got: ${result.stderr}`);
  assert.match(result.stdout, /PASS/);
});

void test('the script is red when the rule is reported for the good fixture', () => {
  const result = runScriptAgainstStub((args) => {
    const target = args.find((arg) => arg.endsWith('bad.ts') || arg.endsWith('good.ts'));
    if (target && target.endsWith('good.ts')) {
      return {
        status: 1,
        stdout: JSON.stringify({
          diagnostics: [
            {
              message: 'Promises must be awaited.',
              code: 'typescript(no-floating-promises)',
              severity: 'error',
              filename: target,
            },
          ],
          number_of_files: 1,
          number_of_rules: 1,
        }),
      };
    }
    return {
      status: 1,
      stdout: JSON.stringify({
        diagnostics: [
          {
            message: 'Promises must be awaited.',
            code: 'typescript(no-floating-promises)',
            severity: 'error',
            filename: '/tmp/canary/bad.ts',
          },
        ],
        number_of_files: 1,
        number_of_rules: 1,
      }),
    };
  });

  assert.notEqual(result.status, 0, 'the canary must be red when the good fixture is flagged');
  assert.match(result.stderr, /FAIL/);
});
