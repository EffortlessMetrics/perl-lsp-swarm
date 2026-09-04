'use strict';

/**
 * Lifecycle proof for the fail-closed dev supervisor (#9848).
 *
 * The negative controls run against FIXTURE child processes (small scripts
 * written into a temp directory), never against the real infinite watchers —
 * except the one bounded real-loop smoke at the bottom, which is opt-in via
 * `PERL_LSP_DEV_SUPERVISOR_REAL_SMOKE=1` because it drives the real
 * `watch:types` + `watch:bundle` pair for a bounded readiness window.
 *
 * Every case must end with the supervisor's tree dead: fixture PIDs (and,
 * for the stubborn-grandchild control, the grandchild PID written by the
 * fixture itself) are probed with `process.kill(pid, 0)` and must all throw.
 */

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { test } = require('node:test');

const {
  REPORT_SCOPE,
  CONFIG_ENV,
  TYPES_READY_PATTERN,
  BUNDLE_READY_PATTERN,
  readyMessage,
  runDevSupervisor,
  createDefaultWatchChildren,
  parseSupervisorConfig,
} = require('./dev-supervisor');

const extensionRoot = path.resolve(__dirname, '..');
const IS_WINDOWS = process.platform === 'win32';

/* ---------------------------------------------------------------------- */
/* Fixture helpers                                                         */
/* ---------------------------------------------------------------------- */

/** @type {string[]} */
let tempDirs = [];

/**
 * The most recently created fixture directory (tests push before spawning).
 *
 * @returns {string}
 */
function lastDir() {
  const dir = tempDirs[tempDirs.length - 1];
  assert.ok(dir !== undefined, 'a fixture directory must have been created');
  return dir;
}

/**
 * @param {string} name
 * @param {string} source
 * @returns {string} Absolute fixture script path.
 */
function writeFixture(name, source) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-'));
  tempDirs.push(dir);
  const file = path.join(dir, name);
  fs.writeFileSync(file, source);
  return file;
}

/**
 * A fixture that reports readiness and stays alive until signaled. With
 * `FIXTURE_IGNORE_TERM=1` it ignores graceful SIGTERM (the POSIX stubborn
 * control; on Windows termination is forced and cannot be ignored).
 */
const READY_AND_STAY = `
const fs = require('node:fs');
if (process.env.FIXTURE_PIDS_FILE) fs.writeFileSync(process.env.FIXTURE_PIDS_FILE, JSON.stringify([process.pid]));
console.log(process.env.FIXTURE_MARKER ?? 'FIXTURE_READY');
process.on('SIGTERM', () => { if (process.env.FIXTURE_IGNORE_TERM !== '1') process.exit(0); });
process.on('SIGINT', () => { if (process.env.FIXTURE_IGNORE_INT !== '1') process.exit(0); });
setInterval(() => {}, 1000);
`;

/** A fixture that exits before ever reporting readiness. */
const EXIT_BEFORE_READY = `process.exit(Number(process.env.FIXTURE_EXIT_CODE ?? '3'));`;

/** A fixture that reports readiness, then exits a moment later on its own. */
const EXIT_AFTER_READY = `
console.log(process.env.FIXTURE_MARKER ?? 'FIXTURE_READY');
setTimeout(
  () => process.exit(Number(process.env.FIXTURE_EXIT_CODE ?? '7')),
  Number(process.env.FIXTURE_EXIT_DELAY_MS ?? '150'),
);
setInterval(() => {}, 1000);
`;

/**
 * The wrapper-that-leaves-a-descendant control: reports readiness, spawns a
 * long-lived grandchild, records both PIDs, and (POSIX only) ignores the
 * graceful signal so the escalation path must fire.
 */
const STUBBORN_WITH_GRANDCHILD = `
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const grandchild = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'], { stdio: 'ignore' });
fs.writeFileSync(process.env.FIXTURE_PIDS_FILE, JSON.stringify({ child: process.pid, grandchild: grandchild.pid }));
console.log('FIXTURE_READY');
if (process.platform !== 'win32') { process.on('SIGTERM', () => {}); }
process.on('SIGINT', () => {});
setInterval(() => {}, 1000);
`;

/** A fixture that echoes its argv to a file (path-with-spaces control). */
const ARGV_PROBE = `
const fs = require('node:fs');
fs.writeFileSync(process.env.FIXTURE_ARGV_FILE, JSON.stringify(process.argv));
console.log('FIXTURE_READY');
setInterval(() => {}, 1000);
`;

/**
 * @typedef {object} Run
 * @property {ReturnType<typeof runDevSupervisor>} controller
 * @property {Promise<import('./dev-supervisor').SupervisorResult>} exit
 * @property {string[]} infos
 * @property {string[]} errors
 */

/**
 * Runs the real supervisor over fixture child specs with short timers.
 *
 * @param {import('./dev-supervisor').WatchChildSpec[] | ((dir: string) => import('./dev-supervisor').WatchChildSpec[])} specs
 * @param {Partial<import('./dev-supervisor').SupervisorOptions>} [options]
 * @returns {Run}
 */
function runSupervisor(specs, options = {}) {
  /** @type {string[]} */
  const infos = [];
  /** @type {string[]} */
  const errors = [];
  const controller = runDevSupervisor({
    children:
      typeof specs === 'function'
        ? specs(lastDir())
        : /** @type {import('./dev-supervisor').WatchChildSpec[]} */ (specs),
    reporter: {
      info: (message) => infos.push(message),
      error: (message) => errors.push(message),
    },
    options: {
      readinessTimeoutMs: 5000,
      shutdownGraceMs: 250,
      // Fixture suites keep the test output clean; the real smoke opts in to
      // prove diagnostics passthrough.
      forwardOutput: false,
      ...options,
    },
  });
  return { controller, exit: controller.waitForExit(), infos, errors };
}

/**
 * Polls until the supervisor reported the combined ready edge (or fails the
 * test). Never a fixed sleep: readiness under load is not time-shaped.
 *
 * @param {Run} run
 * @param {number} [timeoutMs]
 * @returns {Promise<void>}
 */
async function waitForReady(run, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (run.infos.includes('ready (2/2 watchers healthy)')) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.fail(`readiness edge never appeared; got: ${run.infos.join(' | ')}`);
}

/**
 * A ready-and-stay fixture child spec. The fixture records its PID and is
 * parameterized (marker, stubbornness) through its environment.
 *
 * @param {string} name
 * @param {string} marker
 * @param {Record<string, string>} [extraEnv]
 * @returns {import('./dev-supervisor').WatchChildSpec}
 */
function readyChild(name, marker, extraEnv = {}) {
  const dir = lastDir();
  return {
    name,
    command: process.execPath,
    args: [writeFixture(`${name}-${marker}.cjs`, READY_AND_STAY)],
    cwd: dir,
    readyPattern: new RegExp(marker),
    env: {
      ...process.env,
      ...extraEnv,
      FIXTURE_MARKER: marker,
      FIXTURE_PIDS_FILE: path.join(dir, `${name}.pids.json`),
    },
  };
}

/**
 * Probes that every recorded PID is gone (ESRCH expected). A null pid means
 * the child never spawned — there is nothing to probe for it.
 *
 * @param {Array<number | null | undefined>} pids
 */
function assertAllGone(pids) {
  for (const pid of pids) {
    if (pid === null || pid === undefined) {
      continue;
    }
    assert.equal(typeof pid, 'number', `expected a recorded pid, got ${String(pid)}`);
    let alive = false;
    try {
      process.kill(pid, 0);
      alive = true;
    } catch {
      alive = false;
    }
    assert.equal(alive, false, `pid ${pid} is still alive — the tree was not cleaned up`);
  }
}

/** @returns {Array<number | null | undefined>} */
function resultPids(result) {
  return result.children.map((child) => child.pid);
}

/* ---------------------------------------------------------------------- */
/* Lifecycle cases                                                         */
/* ---------------------------------------------------------------------- */

void test('both watchers reach readiness, then a SIGINT stop exits with the governed interrupt result', async () => {
  tempDirs.push(fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-')));
  const run = runSupervisor(() => [
    readyChild('types', 'FIXTURE_READY_A'),
    readyChild('bundle', 'FIXTURE_READY_B'),
  ]);
  await waitForReady(run);
  const result = await run.controller.stop('SIGINT');
  assert.equal(result.code, 130, 'SIGINT must exit 128+2');
  assert.match(result.reason, /signal:SIGINT/);
  assert.deepEqual(result.failures, []);
  assert.ok(
    run.infos.includes('ready (2/2 watchers healthy)'),
    `expected the combined ready line, got: ${run.infos.join(' | ')}`,
  );
  assert.ok(run.infos.includes('watcher "types" ready'));
  assert.ok(run.infos.includes('watcher "bundle" ready'));
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

void test('type watcher exiting before readiness is red and stops the bundle sibling', async () => {
  const run = runSupervisor(() => [
    {
      name: 'types',
      command: process.execPath,
      args: [writeFixture('types-exit.cjs', EXIT_BEFORE_READY)],
      cwd: lastDir(),
      readyPattern: /NEVER_READY/,
      env: { ...process.env, FIXTURE_EXIT_CODE: '3' },
    },
    readyChild('bundle', 'FIXTURE_READY_B'),
  ]);
  const result = await run.exit;
  assert.equal(result.code, 3, 'the failing child exit code must be preserved');
  assert.match(result.reason, /child-failure:types/);
  assert.ok(
    result.failures.some((f) => f.includes('watcher "types" failed (phase=pending')),
    `expected a named pending-phase failure, got: ${result.failures.join(' | ')}`,
  );
  assert.ok(
    run.infos.some(
      (m) => m.includes('watcher "bundle"') && (m.includes('SIGTERM') || m.includes('taskkill')),
    ),
    `the bundle sibling must have been stopped, got: ${run.infos.join(' | ')}`,
  );
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

void test('bundle watcher exiting before readiness is red and stops the type sibling', async () => {
  const run = runSupervisor(() => [
    readyChild('types', 'FIXTURE_READY_A'),
    {
      name: 'bundle',
      command: process.execPath,
      args: [writeFixture('bundle-exit.cjs', EXIT_BEFORE_READY)],
      cwd: lastDir(),
      readyPattern: /NEVER_READY/,
      env: { ...process.env, FIXTURE_EXIT_CODE: '4' },
    },
  ]);
  const result = await run.exit;
  assert.equal(result.code, 4);
  assert.match(result.reason, /child-failure:bundle/);
  assert.ok(result.failures.some((f) => f.includes('watcher "bundle" failed (phase=pending')));
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

void test('a watcher exiting after both are ready is red and stops the sibling', async () => {
  const run = runSupervisor(() => [
    {
      name: 'types',
      command: process.execPath,
      args: [writeFixture('types-late-exit.cjs', EXIT_AFTER_READY)],
      cwd: lastDir(),
      readyPattern: /FIXTURE_READY/,
      env: { ...process.env, FIXTURE_EXIT_CODE: '7', FIXTURE_EXIT_DELAY_MS: '250' },
    },
    readyChild('bundle', 'FIXTURE_READY_B'),
  ]);
  const result = await run.exit;
  assert.equal(result.code, 7, 'the post-ready child exit code must be preserved');
  assert.match(result.reason, /child-failure:types/);
  assert.ok(result.failures.some((f) => f.includes('watcher "types" failed (phase=ready')));
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

void test('readiness window expiry is red and names the pending watchers', async () => {
  const run = runSupervisor(
    () => [
      readyChild('types', 'FIXTURE_READY_A'),
      {
        name: 'bundle',
        command: process.execPath,
        args: [writeFixture('bundle-silent.cjs', READY_AND_STAY)],
        cwd: lastDir(),
        readyPattern: /NEVER_READY/,
        env: { ...process.env, FIXTURE_MARKER: 'SILENT' },
      },
    ],
    { readinessTimeoutMs: 400 },
  );
  const result = await run.exit;
  assert.equal(result.code, 1);
  assert.match(result.reason, /readiness-timeout/);
  assert.ok(
    result.failures.some((f) => f.includes('watcher "bundle" never became ready')),
    `expected the pending child named, got: ${result.failures.join(' | ')}`,
  );
  assert.ok(run.errors.some((e) => e.includes('pending: bundle')));
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

void test('a child that cannot spawn is red and does not leave the sibling running', async () => {
  tempDirs.push(fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-')));
  const run = runSupervisor(() => [
    {
      name: 'types',
      command: 'perl-lsp-dev-supervisor-no-such-binary',
      args: [],
      cwd: lastDir(),
      readyPattern: /NEVER_READY/,
    },
    readyChild('bundle', 'FIXTURE_READY_B'),
  ]);
  const result = await run.exit;
  assert.equal(result.code, 1);
  assert.match(result.reason, /child-failure:types/);
  assert.ok(result.failures.some((f) => f.includes('watcher "types"') && f.includes('error=')));
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

void test('a stubborn child is killed by bounded escalation and its grandchild does not survive', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-stubborn-'));
  tempDirs.push(dir);
  const pidsFile = path.join(dir, 'stubborn.pids.json');
  const run = runSupervisor(() => [
    {
      name: 'types',
      command: process.execPath,
      args: [writeFixture('stubborn.cjs', STUBBORN_WITH_GRANDCHILD)],
      cwd: dir,
      readyPattern: /FIXTURE_READY/,
      env: {
        ...process.env,
        ...(IS_WINDOWS ? {} : { FIXTURE_IGNORE_TERM: '1' }),
        FIXTURE_PIDS_FILE: pidsFile,
      },
    },
    readyChild('bundle', 'FIXTURE_READY_B'),
  ]);
  await waitForReady(run);
  const result = await run.controller.stop('SIGTERM');
  assert.equal(result.code, 143, 'SIGTERM must exit 128+15');
  if (!IS_WINDOWS) {
    assert.ok(
      result.escalations.includes('types'),
      `the stubborn child must be recorded as escalated, got: ${result.escalations.join(' | ')}`,
    );
  }
  const { child, grandchild } = JSON.parse(fs.readFileSync(pidsFile, 'utf8'));
  assertAllGone([...resultPids(result), child, grandchild]);
  assertAllGone(readPidFiles());
});

void test('paths containing spaces and non-ASCII characters keep argv intact', async () => {
  const spacedDir = fs.mkdtempSync(path.join(os.tmpdir(), 'dev supervisor – 日本語 '));
  tempDirs.push(spacedDir);
  const argvFile = path.join(spacedDir, 'argv.json');
  const fixturePath = path.join(spacedDir, 'argv probe.cjs');
  fs.writeFileSync(fixturePath, ARGV_PROBE);
  const run = runSupervisor(() => [
    {
      name: 'types',
      command: process.execPath,
      args: [fixturePath, 'an argument with spaces'],
      cwd: spacedDir,
      readyPattern: /FIXTURE_READY/,
      env: { ...process.env, FIXTURE_ARGV_FILE: argvFile },
    },
    readyChild('bundle', 'FIXTURE_READY_B'),
  ]);
  await waitForReady(run);
  const result = await run.controller.stop();
  assert.equal(result.code, 0);
  const argv = JSON.parse(fs.readFileSync(argvFile, 'utf8'));
  assert.equal(argv[1], fixturePath, 'the fixture path with spaces must arrive as ONE argv entry');
  assert.equal(argv[2], 'an argument with spaces');
  assertAllGone(resultPids(result));
  assertAllGone(readPidFiles());
});

/* ---------------------------------------------------------------------- */
/* CLI + proof harness                                                     */
/* ---------------------------------------------------------------------- */

/**
 * Spawns the real CLI (`node scripts/dev-supervisor.js`) with a config file.
 *
 * @param {unknown} config
 * @returns {Promise<{code: number | null, stdout: string, stderr: string}>}
 */
function runCli(config) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-cli-'));
  tempDirs.push(dir);
  const configFile = path.join(dir, 'config.json');
  fs.writeFileSync(configFile, JSON.stringify(config));
  return new Promise((resolve) => {
    const child = spawn(
      process.execPath,
      [path.join(extensionRoot, 'scripts', 'dev-supervisor.js')],
      {
        env: { ...process.env, [CONFIG_ENV]: configFile },
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
      },
    );
    /** @type {Buffer[]} */
    const out = [];
    /** @type {Buffer[]} */
    const err = [];
    child.stdout?.on('data', (c) => out.push(/** @type {Buffer} */ (c)));
    child.stderr?.on('data', (c) => err.push(/** @type {Buffer} */ (c)));
    child.once('error', (error) => {
      resolve({ code: null, stdout: Buffer.concat(out).toString('utf8'), stderr: String(error) });
    });
    child.once('exit', (code) => {
      resolve({
        code,
        stdout: Buffer.concat(out).toString('utf8'),
        stderr: Buffer.concat(err).toString('utf8'),
      });
    });
  });
}

void test('the CLI proof harness reaches readiness and performs the owned stop (exit 0)', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-run-'));
  tempDirs.push(dir);
  const pidsFile = path.join(dir, 'pids.json');
  const fixtureA = path.join(dir, 'a.cjs');
  const fixtureB = path.join(dir, 'b.cjs');
  fs.writeFileSync(fixtureA, READY_AND_STAY);
  fs.writeFileSync(fixtureB, READY_AND_STAY);
  const { code, stdout } = await runCli({
    children: [
      {
        name: 'one',
        command: process.execPath,
        args: [fixtureA],
        readyPattern: 'FIXTURE_READY',
        env: { FIXTURE_PIDS_FILE: pidsFile },
      },
      {
        name: 'two',
        command: process.execPath,
        args: [fixtureB],
        readyPattern: 'FIXTURE_READY',
        env: { FIXTURE_PIDS_FILE: pidsFile },
      },
    ],
    readinessTimeoutMs: 8000,
    shutdownGraceMs: 250,
    stopWhenReady: true,
  });
  assert.equal(code, 0, `expected a green stop, stderr/stdout: ${stdout}`);
  assert.match(stdout, new RegExp(`\\[${REPORT_SCOPE}\\] starting watcher "one"`));
  assert.match(
    stdout,
    new RegExp(
      `\\[${REPORT_SCOPE}\\] ${readyMessage(2, 2).replace('(', '\\(').replace(')', '\\)')}`,
    ),
  );
  const pids = JSON.parse(fs.readFileSync(pidsFile, 'utf8'));
  assertAllGone(pids);
});

void test('an invalid proof-harness config is red and names the offending field', async () => {
  const { code, stderr } = await runCli({ children: [], bogus: true });
  assert.equal(code, 2);
  assert.match(stderr, /"bogus"/);
});

void test('an unknown child field in the proof-harness config is red by name', async () => {
  const { code, stderr } = await runCli({
    children: [{ name: 'x', command: process.execPath, args: [], readyPattern: 'R', sabotage: 1 }],
  });
  assert.equal(code, 2);
  assert.match(stderr, /"sabotage"/);
});

void test(
  'the CLI forwards SIGTERM into the owned shutdown path and exits 143 (POSIX)',
  { skip: IS_WINDOWS },
  async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-sig-'));
    tempDirs.push(dir);
    const pidsFile = path.join(dir, 'pids.json');
    const fixture = path.join(dir, 'a.cjs');
    fs.writeFileSync(fixture, READY_AND_STAY);
    const configFile = path.join(dir, 'config.json');
    fs.writeFileSync(
      configFile,
      JSON.stringify({
        children: [
          {
            name: 'one',
            command: process.execPath,
            args: [fixture],
            readyPattern: 'FIXTURE_READY',
          },
        ],
        readinessTimeoutMs: 15000,
      }),
    );
    await new Promise((resolve) => {
      const child = spawn(
        process.execPath,
        [path.join(extensionRoot, 'scripts', 'dev-supervisor.js')],
        {
          env: { ...process.env, [CONFIG_ENV]: configFile, FIXTURE_PIDS_FILE: pidsFile },
          stdio: ['ignore', 'ignore', 'ignore'],
          windowsHide: true,
        },
      );
      const poll = setInterval(() => {
        if (fs.existsSync(pidsFile)) {
          clearInterval(poll);
          process.kill(child.pid ?? 0, 'SIGTERM');
        }
      }, 50);
      child.once('exit', (code) => {
        clearInterval(poll);
        resolve(/** @type {number | null} */ (code));
      });
    }).then((code) => {
      assert.equal(code, 143, 'SIGTERM to the supervisor must exit 128+15');
    });
    const pids = JSON.parse(fs.readFileSync(pidsFile, 'utf8'));
    assertAllGone(pids);
  },
);

/* ---------------------------------------------------------------------- */
/* Canonical surface cross-checks                                          */
/* ---------------------------------------------------------------------- */

void test('the default children table starts exactly the canonical #9842 watch surfaces', () => {
  const packageJson = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));
  const [types, bundle] = createDefaultWatchChildren(extensionRoot);
  assert.ok(types !== undefined && bundle !== undefined, 'both canonical children must exist');
  assert.ok(packageJson.scripts !== undefined);

  assert.deepEqual(types.args, ['--run', 'watch:types']);
  assert.deepEqual(bundle.args, ['--run', 'watch:bundle']);
  assert.equal(types.cwd, extensionRoot);
  assert.equal(bundle.cwd, extensionRoot);

  assert.equal(packageJson.scripts.dev, 'node scripts/dev-supervisor.js');
  assert.match(String(packageJson.scripts['watch:types']), /governed-tsc\.js/);
  assert.match(String(packageJson.scripts['watch:types']), /--noEmit/);
  assert.match(String(packageJson.scripts['watch:types']), /\.\/tsconfig\.json/);
  assert.match(
    String(packageJson.scripts['watch:bundle']),
    /rolldown -c rolldown\.config\.mjs --watch/,
  );

  assert.equal(TYPES_READY_PATTERN.source, 'Watching for file changes\\.');
  assert.equal(BUNDLE_READY_PATTERN.source, 'built out in');
  assert.equal(BUNDLE_READY_PATTERN.flags, 'i');
});

void test('the proof-harness config parser rejects malformed specs by name', () => {
  assert.throws(() => parseSupervisorConfig('not json', 'x.json'), /not valid JSON/);
  assert.throws(() => parseSupervisorConfig('[]', 'x.json'), /must be a JSON object/);
  assert.throws(
    () =>
      parseSupervisorConfig(
        JSON.stringify({ children: [{ name: 'a', readyPattern: 'R' }] }),
        'x.json',
      ),
    /children\[0\]\.command/,
  );
  assert.throws(
    () =>
      parseSupervisorConfig(
        JSON.stringify({ children: [{ name: 'a', command: 'x', readyPattern: '(' }] }),
        'x.json',
      ),
    /readyPattern is not a valid regular expression/,
  );
  assert.throws(
    () =>
      parseSupervisorConfig(
        JSON.stringify({
          children: [{ name: 'a', command: 'x', readyPattern: 'R' }],
          readinessTimeoutMs: -5,
        }),
        'x.json',
      ),
    /"readinessTimeoutMs" must be a positive integer/,
  );
  const parsed = parseSupervisorConfig(
    JSON.stringify({
      children: [{ name: 'a', command: 'node', args: ['-e', '1'], readyPattern: 'R', cwd: '.' }],
    }),
    'x.json',
  );
  const first = parsed.children[0];
  assert.ok(first !== undefined, 'the single child spec must parse');
  assert.equal(first.name, 'a');
  assert.deepEqual(first.args, ['-e', '1']);
});

/* ---------------------------------------------------------------------- */
/* Bounded real-loop smoke (opt-in)                                        */
/* ---------------------------------------------------------------------- */

void test(
  'bounded real-loop smoke: real watchers reach readiness and the owned stop leaves no tree',
  { skip: process.env.PERL_LSP_DEV_SUPERVISOR_REAL_SMOKE !== '1' },
  async () => {
    // Self-sufficient: creates its own fixture state so it runs standalone
    // under --test-name-pattern as well as in the full suite.
    tempDirs.push(fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-real-')));
    const run = runSupervisor(() => createDefaultWatchChildren(extensionRoot), {
      readinessTimeoutMs: 180000,
      shutdownGraceMs: 2000,
      stopWhenReady: true,
      forwardOutput: true,
    });
    const result = await run.exit;
    assert.equal(
      result.code,
      0,
      `expected the real smoke to stop green, got: ${JSON.stringify(result)}`,
    );
    assert.ok(
      run.infos.includes('ready (2/2 watchers healthy)'),
      `expected the real combined ready line, got: ${run.infos.join(' | ')}`,
    );
    assertAllGone(resultPids(result));
  },
);

/* ---------------------------------------------------------------------- */
/* Exited-leader descendants and repeated interrupts                       */
/* ---------------------------------------------------------------------- */

/**
 * A watcher that exits non-zero on its own while leaving a long-lived
 * grandchild in the process group it led — the daemonizing-watcher control.
 */
const EXIT_WITH_LIVE_GRANDCHILD = `
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const grandchild = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'], { stdio: 'ignore' });
fs.writeFileSync(process.env.FIXTURE_PIDS_FILE, JSON.stringify({ child: process.pid, grandchild: grandchild.pid }));
setTimeout(() => process.exit(Number(process.env.FIXTURE_EXIT_CODE ?? '5')), 120);
setInterval(() => {}, 1000);
`;

void test(
  'a watcher that exits leaving a live grandchild has its whole group terminated (POSIX)',
  { skip: IS_WINDOWS },
  async () => {
    tempDirs.push(fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-orphan-')));
    const pidsFile = path.join(lastDir(), 'orphan.pids.json');
    const run = runSupervisor(() => [
      {
        name: 'types',
        command: process.execPath,
        args: [writeFixture('orphan-exit.cjs', EXIT_WITH_LIVE_GRANDCHILD)],
        cwd: lastDir(),
        readyPattern: /NEVER_READY/,
        env: { ...process.env, FIXTURE_PIDS_FILE: pidsFile, FIXTURE_EXIT_CODE: '5' },
      },
      readyChild('bundle', 'FIXTURE_READY_B'),
    ]);
    const result = await run.exit;
    assert.equal(result.code, 5);
    assert.match(result.reason, /child-failure:types/);
    assert.ok(
      run.infos.some((m) => m.includes('orphaned process group')),
      `expected the orphaned-group termination receipt, got: ${run.infos.join(' | ')}`,
    );
    const { child, grandchild } = JSON.parse(fs.readFileSync(pidsFile, 'utf8'));
    assertAllGone([...resultPids(result), child, grandchild]);
  },
);

void test(
  'the CLI keeps repeated interrupts inside the owned shutdown path (POSIX)',
  { skip: IS_WINDOWS },
  async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-dev-supervisor-dblsig-'));
    tempDirs.push(dir);
    const pidsFile = path.join(dir, 'pids.json');
    const fixture = path.join(dir, 'stubborn.cjs');
    fs.writeFileSync(fixture, STUBBORN_WITH_GRANDCHILD);
    const configFile = path.join(dir, 'config.json');
    fs.writeFileSync(
      configFile,
      JSON.stringify({
        children: [
          {
            name: 'one',
            command: process.execPath,
            args: [fixture],
            readyPattern: 'FIXTURE_READY',
            env: { FIXTURE_IGNORE_TERM: '1', FIXTURE_PIDS_FILE: pidsFile },
          },
        ],
        readinessTimeoutMs: 15000,
        shutdownGraceMs: 5000,
      }),
    );
    const exitCode = await new Promise((resolve) => {
      const child = spawn(
        process.execPath,
        [path.join(extensionRoot, 'scripts', 'dev-supervisor.js')],
        {
          env: { ...process.env, [CONFIG_ENV]: configFile },
          stdio: ['ignore', 'ignore', 'ignore'],
          windowsHide: true,
        },
      );
      let signalsSent = 0;
      const poll = setInterval(() => {
        if (!fs.existsSync(pidsFile)) {
          return;
        }
        clearInterval(poll);
        process.kill(child.pid ?? 0, 'SIGINT');
        signalsSent += 1;
        // Second interrupt while the graceful shutdown is still in flight:
        // it must escalate inside the owned path, never kill the supervisor
        // with watchers still alive.
        setTimeout(() => {
          if (signalsSent === 1) {
            process.kill(child.pid ?? 0, 'SIGINT');
          }
        }, 100);
      }, 50);
      child.once('exit', (code) => {
        clearInterval(poll);
        resolve(/** @type {number | null} */ (code));
      });
    });
    assert.equal(
      exitCode,
      130,
      'repeated SIGINT must still exit 128+2, not die to the default handler',
    );
    const pids = JSON.parse(fs.readFileSync(pidsFile, 'utf8'));
    assertAllGone([pids.child, pids.grandchild]);
  },
);

/* ---------------------------------------------------------------------- */
/* Cleanup                                                                 */
/* ---------------------------------------------------------------------- */

/**
 * Reads every fixture pid file written so far.
 *
 * @returns {Array<number | null | undefined>}
 */
function readPidFiles() {
  /** @type {Array<number | null | undefined>} */
  const pids = [];
  for (const dir of tempDirs) {
    for (const file of fs.existsSync(dir) ? fs.readdirSync(dir) : []) {
      if (file.endsWith('.pids.json')) {
        const parsed = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
        if (Array.isArray(parsed)) {
          pids.push(...parsed);
        } else {
          pids.push(parsed.child, parsed.grandchild);
        }
      }
    }
  }
  return pids;
}

void test('no fixture process or descendant survives the suite', async () => {
  // Give any in-flight kill a final grace, then demand an empty field.
  await new Promise((resolve) => setTimeout(resolve, 250));
  assertAllGone(readPidFiles());
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
