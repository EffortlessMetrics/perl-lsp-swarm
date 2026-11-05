'use strict';

const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

/**
 * Run the production canary through its real subprocess boundary. Only the
 * installed Oxlint executable is replaced; no predicate from the canary is
 * imported into the oracle.
 * @param {import('node:test').TestContext} t
 * @param {string} [behavior]
 * @param {NodeJS.ProcessEnv} [env]
 */
function runCanary(t, behavior = '', env = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp lint-canary test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'scripts'));
  for (const name of ['lint-canary.js', 'reporter.js']) {
    fs.copyFileSync(path.join(__dirname, name), path.join(root, 'scripts', name));
  }
  for (const name of ['oxlint', 'oxlint-tsgolint']) {
    const directory = path.join(root, 'node_modules', name);
    fs.mkdirSync(directory, { recursive: true });
    fs.writeFileSync(path.join(directory, 'package.json'), JSON.stringify({ version: 'fixture' }));
  }
  const bin = path.join(root, 'node_modules', 'oxlint', 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(
    path.join(bin, 'oxlint'),
    `
const fs = require('node:fs');
const path = require('node:path');
const filename = process.argv[2];
const bad = path.basename(filename) === 'bad.ts';
const diagnostic = {
  code: 'typescript(no-floating-promises)',
  severity: 'error',
  filename,
};
let report = { diagnostics: bad ? [diagnostic] : [], number_of_files: 1 };
let exitCode = bad ? 1 : 0;
let output;
let stderr = '';
let signal;
${behavior}
fs.writeFileSync(1, output === undefined ? JSON.stringify(report) : output);
if (stderr) fs.writeFileSync(2, stderr);
if (signal) process.kill(process.pid, signal);
process.exitCode = exitCode;
`,
  );

  const result = spawnSync(process.execPath, [path.join(root, 'scripts', 'lint-canary.js')], {
    cwd: root,
    env: { ...process.env, ...env },
    encoding: 'utf8',
    timeout: 15_000,
    maxBuffer: 8 * 1024 * 1024,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, 'the canary itself must finish normally');
  return result;
}

void test('lint canary accepts the exact bad diagnostic and a clean good result', (t) => {
  const result = runCanary(t);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /PASS — type-aware/);
});

void test('lint canary resolves diagnostic paths against the Oxlint working directory', (t) => {
  const result = runCanary(t, 'diagnostic.filename = path.relative(process.cwd(), filename);');
  assert.equal(result.status, 0, result.stderr);
});

void test('lint canary identifies a configured type-aware backend override', (t) => {
  const override = path.join(os.tmpdir(), 'helper with spaces', 'tsrs-lint');
  const result = runCanary(t, '', { OXLINT_TSGOLINT_PATH: override });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /installed oxlint-tsgolint@fixture/);
  assert.ok(result.stdout.includes(`type-aware backend override: ${JSON.stringify(override)}`));
});

const invalidResults = [
  [
    'a backend initialization error mentioning the expected rule',
    `if (bad) {
      output = '';
      stderr = 'typescript/no-floating-promises: failed to initialize type-aware backend';
      exitCode = 2;
    }`,
  ],
  [
    'an unrelated rule whose message names the expected rule',
    `if (bad) {
      diagnostic.code = 'oxc/parser';
      diagnostic.message = 'Could not parse the no-floating-promises probe';
    }`,
  ],
  [
    'the expected rule for a different file with the same basename',
    `if (bad) diagnostic.filename = path.join(path.dirname(filename), 'other', 'bad.ts');`,
  ],
  ['a warning instead of the expected error', `if (bad) diagnostic.severity = 'warning';`],
  [
    'an unexpected diagnostic alongside the expected error',
    `if (bad) report.diagnostics.push({ code: 'oxc/parser', severity: 'error', filename });`,
  ],
  ['the expected diagnostic with an unexpected failure exit', 'if (bad) exitCode = 2;'],
  ['silent syntax-only success', 'if (bad) { report.diagnostics = []; exitCode = 0; }'],
  [
    'an error on stderr despite otherwise valid bad output',
    `if (bad) stderr = 'The type-aware backend crashed';`,
  ],
  [
    'an error on stderr despite otherwise clean good output',
    `if (!bad) stderr = 'typescript/no-floating-promises: backend failed';`,
  ],
  [
    'an unrelated warning on the good fixture',
    `if (!bad) report.diagnostics.push({ code: 'no-console', severity: 'warning', filename });`,
  ],
  ['no files processed for the good fixture', 'if (!bad) report.number_of_files = 0;'],
  ['multiple files processed for the bad fixture', 'if (bad) report.number_of_files = 2;'],
];

for (const [name, behavior] of invalidResults) {
  void test(`lint canary rejects ${name}`, (t) => {
    const result = runCanary(t, behavior);
    assert.equal(result.status, 1, result.stdout);
    assert.doesNotMatch(result.stdout, /PASS — type-aware/);
    assert.match(result.stderr, /FAIL:/);
  });
}

for (const output of [
  '',
  'not JSON',
  'null',
  '[]',
  '{}',
  '{"diagnostics":null,"number_of_files":1}',
]) {
  void test(`lint canary rejects invalid good output ${JSON.stringify(output)}`, (t) => {
    const result = runCanary(t, `if (!bad) output = ${JSON.stringify(output)};`);
    assert.equal(result.status, 1, result.stdout);
    assert.match(result.stderr, /FAIL:/);
  });
}

void test(
  'lint canary rejects a signalled process even after the expected diagnostic',
  { skip: process.platform === 'win32' ? 'POSIX signal termination control' : false },
  (t) => {
    const result = runCanary(t, `if (bad) signal = 'SIGTERM';`);
    assert.equal(result.status, 1, result.stdout);
    assert.match(result.stderr, /SIGTERM/);
  },
);

void test('lint canary rejects a spawn error even with a complete diagnostic in stdout', (t) => {
  const result = runCanary(
    t,
    `if (bad) output = JSON.stringify(report) + ' '.repeat(2 * 1024 * 1024);`,
  );
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, /ENOBUFS/);
});
