'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');
const { runGovernedTsc, spawnPinnedTsc } = require('./governed-tsc');
const { declaredTscBin } = require('./check-typescript-authority');

const EXTENSION_ROOT = path.resolve(__dirname, '..');

/** @returns {{infos: string[], errors: string[], info: (message: string) => void, error: (message: string) => void}} */
function captureReporter() {
  return {
    infos: [],
    errors: [],
    /** @param {string} message */
    info(message) {
      this.infos.push(message);
    },
    /** @param {string} message */
    error(message) {
      this.errors.push(message);
    },
  };
}

const GREEN_AUTHORITY = () => ({ ok: true, failures: [], facts: ['synthetic green'] });

void test('a red authority gate refuses to compile and names the drift', async () => {
  const reporter = captureReporter();
  /** @type {{command: string, argv: string[]} | null} */
  let spawned = null;
  const result = await runGovernedTsc({
    extensionRoot: EXTENSION_ROOT,
    args: ['--noEmit', '-p', './tsconfig.json'],
    reporter,
    authorityCheck: () => ({
      ok: false,
      failures: ['declared typescript range "^6.0.3" floors at major 6, not the authority major 7'],
      facts: [],
    }),
    spawnChild: (command, argv) => {
      spawned = { command, argv };
      return { code: 0, signal: null };
    },
  });
  assert.equal(result.code, 1);
  assert.equal(result.spawned, false);
  assert.equal(spawned, null, 'no compiler may run when the gate is red');
  assert.ok(
    reporter.errors.some((line) => line.includes('floors at major 6')),
    'the authority failure is surfaced verbatim',
  );
  assert.ok(
    reporter.errors.some((line) => line.includes('refused to compile')),
    'the refusal names the fail-closed behavior',
  );
  assert.ok(
    reporter.errors.some((line) => line.includes('npm run typecheck:authority')),
    'the repair path names the verification command',
  );
});

void test('a green gate executes the pinned package tsc with forwarded args and exit code', async () => {
  const reporter = captureReporter();
  /** @type {{command: string, argv: string[]} | null} */
  let spawned = null;
  const result = await runGovernedTsc({
    extensionRoot: EXTENSION_ROOT,
    args: ['--noEmit', '-p', './tsconfig.json'],
    reporter,
    authorityCheck: GREEN_AUTHORITY,
    spawnChild: (command, argv) => {
      spawned = { command, argv };
      return { code: 2, signal: null };
    },
  });
  assert.equal(result.code, 2, 'the compiler exit code is forwarded, not flattened');
  assert.equal(result.spawned, true);

  const declared = declaredTscBin(path.join(EXTENSION_ROOT, 'node_modules', 'typescript'));
  assert.ok('binPath' in declared, 'the real tree must resolve a pinned tsc entry point');
  assert.deepEqual(spawned, {
    command: process.execPath,
    argv: [declared.binPath, '--noEmit', '-p', './tsconfig.json'],
  });
});

void test('a child killed by a signal does not read as success', async () => {
  const result = await runGovernedTsc({
    extensionRoot: EXTENSION_ROOT,
    args: ['--version'],
    reporter: captureReporter(),
    authorityCheck: GREEN_AUTHORITY,
    spawnChild: () => ({ code: null, signal: 'SIGINT' }),
  });
  assert.equal(result.code, 1);
});

void test('a launch failure settles as a red result instead of hanging', async () => {
  const reporter = captureReporter();
  const result = await runGovernedTsc({
    extensionRoot: EXTENSION_ROOT,
    args: ['--version'],
    reporter,
    authorityCheck: GREEN_AUTHORITY,
    spawnChild: () => ({ code: null, signal: null, error: 'spawn ENOENT (synthetic)' }),
  });
  assert.equal(result.code, 1);
  assert.ok(
    reporter.errors.some((line) => line.includes('could not be launched')),
    'the launch failure is named',
  );
});

void test('the real spawner settles on error events for a command that cannot launch', async () => {
  const result = await spawnPinnedTsc('definitely-not-a-real-command-9842', ['--version']);
  assert.equal(result.code, null);
  assert.ok(typeof result.error === 'string' && result.error.length > 0);
});

void test('no forwarded arguments is a usage error, not an implicit project compile', async () => {
  /** @type {{command: string, argv: string[]} | null} */
  let spawned = null;
  const result = await runGovernedTsc({
    extensionRoot: EXTENSION_ROOT,
    args: [],
    reporter: captureReporter(),
    authorityCheck: GREEN_AUTHORITY,
    spawnChild: (command, argv) => {
      spawned = { command, argv };
      return { code: 0, signal: null };
    },
  });
  assert.equal(result.code, 2);
  assert.equal(spawned, null);
});

void test('a real type error exits nonzero through the seam (type failure blocks bundling at the tsc layer)', async () => {
  const fixtureDir = fs.mkdtempSync(path.join(os.tmpdir(), 'governed-tsc-fixture-'));
  try {
    fs.writeFileSync(
      path.join(fixtureDir, 'broken.ts'),
      'const answer: number = "not a number";\nexport { answer };\n',
    );
    fs.writeFileSync(
      path.join(fixtureDir, 'tsconfig.json'),
      JSON.stringify({
        compilerOptions: {
          noEmit: true,
          strict: true,
        },
        files: ['broken.ts'],
      }),
    );
    const result = await runGovernedTsc({
      extensionRoot: EXTENSION_ROOT,
      args: ['-p', path.join(fixtureDir, 'tsconfig.json')],
      reporter: captureReporter(),
      authorityCheck: GREEN_AUTHORITY,
    });
    assert.equal(result.spawned, true);
    assert.notEqual(result.code, 0, 'a type error must fail the governed compile');
  } finally {
    fs.rmSync(fixtureDir, { recursive: true, force: true });
  }
});

void test('the real tree passes the gate and runs the pinned compiler end to end', async () => {
  const result = await runGovernedTsc({
    extensionRoot: EXTENSION_ROOT,
    args: ['--version'],
    reporter: captureReporter(),
  });
  assert.equal(result.spawned, true);
  assert.equal(result.code, 0);
});
