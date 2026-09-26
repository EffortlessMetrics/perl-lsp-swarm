'use strict';

const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { WatchGenerations, assertPhase, stopTree } = require('./typescript-watch-lifecycle');

function output(generations, text) {
  // Deliberately split both diagnostics and completion markers across writes.
  for (let n = 0; n < text.length; n += 7) {
    generations.push(Buffer.from(text.slice(n, n + 7)));
  }
}

void test('initial clean, distinct error and later repair carry ordered generations', async () => {
  const watch = new WatchGenerations();
  const exits = new Set();
  output(
    watch,
    'Starting compilation in watch mode...\nFound 0 errors. Watching for file changes.\n',
  );
  const first = await watch.waitAfter(0, () => true, 100, exits);
  assertPhase(first, 'initial', 0);
  assert.deepEqual([first.generation, first.errors, first.diagnostic], [1, 0, false]);
  output(
    watch,
    'File change detected. Starting incremental compilation...\nwatch-fixture.ts(1,14): error TS2322: Type string is not assignable.\nFound 1 error. Watching for file changes.\n',
  );
  const broken = await watch.waitAfter(first.generation, () => true, 100, exits);
  assertPhase(broken, 'error', first.generation);
  assert.deepEqual([broken.generation, broken.errors, broken.diagnostic], [2, 1, true]);
  output(
    watch,
    'File change detected. Starting incremental compilation...\nFound 0 errors. Watching for file changes.\n',
  );
  const repaired = await watch.waitAfter(broken.generation, () => true, 100, exits);
  assertPhase(repaired, 'repaired', broken.generation);
  assert.deepEqual([repaired.generation, repaired.errors, repaired.diagnostic], [3, 0, false]);
  assert.equal(
    watch.events.length,
    3,
    'an earlier completion must not be emitted again on later chunks',
  );
});

void test('wrong initial, missed error, failed repair, and stale generation are red', () => {
  assert.throws(
    () => assertPhase({ generation: 1, errors: 1 }, 'initial', 0),
    /initial generation/,
  );
  assert.throws(
    () => assertPhase({ generation: 2, errors: 1, diagnostic: false }, 'error', 1),
    /missed fixture TS2322/,
  );
  assert.throws(
    () => assertPhase({ generation: 3, errors: 1 }, 'repaired', 2),
    /repair generation/,
  );
  assert.throws(
    () => assertPhase({ generation: 2, errors: 0 }, 'repaired', 2),
    /reused stale generation/,
  );
});

void test('an earlier diagnostic cannot satisfy a later generation in the same chunk', () => {
  const watch = new WatchGenerations();
  watch.push(
    Buffer.from(
      'watch-fixture.ts(1,14): error TS2322: bad\nFound 1 error. Watching for file changes.\nFound 0 errors. Watching for file changes.\n',
    ),
  );
  assert.equal(watch.events[0].diagnostic, true);
  assert.equal(watch.events[1].diagnostic, false);
});

void test('stale clean output cannot satisfy a later recovery generation', async () => {
  const watch = new WatchGenerations();
  output(watch, 'Found 0 errors. Watching for file changes.\n');
  await assert.rejects(
    watch.waitAfter(1, () => true, 15, new Set()),
    /timed out/,
  );
});

void test('native watcher filesystem failure aborts a pending phase promptly', async () => {
  const watch = new WatchGenerations();
  const phase = watch.waitAfter(0, () => true, 1000, new Set());
  output(
    watch,
    "fanotify_mark on '/a path/lib' failed: name_to_handle_at: operation not supported\n",
  );
  await assert.rejects(phase, /fanotify support/);
});

void test('unexpected exit aborts a pending phase promptly', async () => {
  const watch = new WatchGenerations();
  const exits = new Set();
  const phase = watch.waitAfter(0, () => true, 1000, exits);
  for (const listener of exits) listener('code=1');
  await assert.rejects(phase, /exited before generation 1: code=1/);
});

void test('exit before a phase waiter attaches cannot be missed', async () => {
  const watch = new WatchGenerations();
  watch.abort('watch exited: code=1 signal=null');
  await assert.rejects(
    watch.waitAfter(0, () => true, 100, new Set()),
    /watch exited: code=1/,
  );
});

void test(
  'POSIX cleanup proves the watcher and its descendant group stopped',
  { skip: process.platform === 'win32' },
  async () => {
    const child = spawn(
      process.execPath,
      [
        '-e',
        'require("node:child_process").spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore"}); process.stdout.write("READY\\n"); setInterval(()=>{},1000)',
      ],
      {
        detached: true,
        stdio: ['ignore', 'pipe', 'ignore'],
      },
    );
    try {
      await new Promise((resolve, reject) => {
        child.stdout.once('data', resolve);
        child.once('error', reject);
        child.once('exit', () => reject(new Error('fixture exited before readiness')));
      });
      const cleanup = await stopTree(child);
      assert.equal(cleanup.tree_proven, true);
    } finally {
      // The same bounded cleanup path owns fixture failure as well as success.
      await stopTree(child);
    }
  },
);

async function cleanupFixtureGroup(pidFile) {
  if (!fs.existsSync(pidFile)) return;
  const [groupPid] = fs.readFileSync(pidFile, 'utf8').trim().split(/\s+/).map(Number);
  if (groupPid === undefined || !Number.isSafeInteger(groupPid) || groupPid <= 0)
    throw new Error('invalid fixture group PID');
  const cleanup = await stopTree({ pid: groupPid });
  assert.equal(
    cleanup.tree_proven,
    true,
    'fixture group must be stopped even after assertion failure',
  );
}

for (const signal of /** @type {NodeJS.Signals[]} */ (['SIGINT', 'SIGTERM', 'SIGHUP'])) {
  void test(
    `signal ${signal} cleans an active watcher and descendant`,
    { skip: process.platform === 'win32' },
    async () => {
      const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'perl lsp signal '));
      const pidFile = path.join(temp, 'fixture.pid');
      const fixture = path.join(temp, 'fixture.js');
      fs.writeFileSync(
        fixture,
        `const fs=require('node:fs'); const {spawn}=require('node:child_process'); fs.writeFileSync(process.env.PROOF_PID_FILE, String(process.pid)); const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'}); fs.appendFileSync(process.env.PROOF_PID_FILE, ' '+child.pid); setInterval(()=>{},1000);`,
      );
      const modulePath = path.join(__dirname, 'typescript-watch-lifecycle.js');
      const root = path.resolve(__dirname, '..');
      const code = `const {runWatchLifecycle,receiptExitCode}=require(${JSON.stringify(modulePath)}); const {spawn}=require('node:child_process'); runWatchLifecycle(${JSON.stringify(root)},{compilerVersion:'fixture',spawnWatch:(_cmd,_args,opts)=>spawn(process.execPath,[${JSON.stringify(fixture)}],{...opts,env:{...process.env,PROOF_PID_FILE:${JSON.stringify(pidFile)}}})}).then(r=>{process.stdout.write(JSON.stringify(r)+'\\n'); process.exitCode=receiptExitCode(r)});`;
      const harness = spawn(process.execPath, ['-e', code], { stdio: ['ignore', 'pipe', 'pipe'] });
      let output = '';
      harness.stdout.setEncoding('utf8').on('data', (chunk) => {
        output += chunk;
      });
      let timeout;
      try {
        const deadline = Date.now() + 5000;
        while (
          (!fs.existsSync(pidFile) ||
            fs.readFileSync(pidFile, 'utf8').trim().split(/\s+/).length < 2) &&
          Date.now() < deadline
        )
          await new Promise((resolve) => setTimeout(resolve, 20));
        assert.ok(fs.existsSync(pidFile), 'fixture watcher did not start');
        assert.equal(
          fs.readFileSync(pidFile, 'utf8').trim().split(/\s+/).length,
          2,
          'fixture descendant did not start',
        );
        timeout = setTimeout(() => harness.kill('SIGKILL'), 15000);
        harness.kill(signal);
        const result = await new Promise((resolve) =>
          harness.once('close', (code, exitSignal) => resolve({ code, exitSignal })),
        );
        assert.equal(result.exitSignal, null, output);
        assert.equal(
          result.code,
          signal === 'SIGINT' ? 130 : signal === 'SIGTERM' ? 143 : 129,
          output,
        );
        const receipt = JSON.parse(output.trim());
        assert.equal(receipt.interrupt_signal, signal);
        assert.equal(receipt.verdict, 'not_proven');
        assert.equal(receipt.cleanup.tree_proven, true);
      } finally {
        if (timeout) clearTimeout(timeout);
        if (harness.exitCode === null) harness.kill('SIGKILL');
        try {
          await cleanupFixtureGroup(pidFile);
        } finally {
          fs.rmSync(temp, { recursive: true, force: true });
        }
      }
    },
  );
}

void test(
  'fixture cleanup still stops a group after an injected assertion failure',
  { skip: process.platform === 'win32' },
  async () => {
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'perl lsp failure '));
    const pidFile = path.join(temp, 'fixture.pid');
    const child = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
      detached: true,
      stdio: 'ignore',
    });
    fs.writeFileSync(pidFile, String(child.pid));
    try {
      await assert.rejects(async () => {
        throw new Error('injected assertion failure');
      }, /injected assertion failure/);
    } finally {
      try {
        await cleanupFixtureGroup(pidFile);
      } finally {
        fs.rmSync(temp, { recursive: true, force: true });
      }
    }
  },
);
