const assert = require('node:assert/strict');
const { spawn, execFileSync } = require('node:child_process');
const path = require('node:path');
const { test } = require('node:test');
const { suspendOwnedWindowsProcess, resumeOwnedWindowsProcess } = require(
  path.join(__dirname, '../out/test/published/windowsOwnedSuspension.js'),
);

const pause = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function until(predicate, description) {
  const deadline = Date.now() + 5000;
  while (!predicate() && Date.now() < deadline) await pause(25);
  assert.ok(predicate(), description);
}

async function ownedHeartbeat(run) {
  const windowsRoot = process.env.SystemRoot;
  if (windowsRoot === undefined) throw new Error('Windows proof requires SystemRoot');
  const child = spawn(path.join(windowsRoot, 'System32', 'PING.EXE'), ['127.0.0.1', '-t'], {
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  let bytes = 0;
  let exited = false;
  /** @type {Promise<void>} */
  const terminal = new Promise((resolve) =>
    child.once('close', () => {
      exited = true;
      resolve();
    }),
  );
  child.stdout.on('data', (chunk) => {
    bytes += chunk.length;
  });
  child.stderr.resume();
  try {
    await until(() => bytes > 0, 'owned heartbeat starts');
    const creation = execFileSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `[Diagnostics.Process]::GetProcessById(${child.pid}).StartTime.ToFileTimeUtc().ToString()`,
      ],
      { encoding: 'utf8', windowsHide: true },
    ).trim();
    await run({ child, creation, bytes: () => bytes, exited: () => exited });
  } finally {
    await resumeOwnedWindowsProcess(child.pid);
    if (!exited) child.kill();
    await Promise.race([terminal, pause(5000)]);
    assert.ok(exited, 'exact owned heartbeat child is reaped');
  }
}

void test(
  'Windows adapter proves a live hang and normal resume',
  { skip: process.platform !== 'win32' },
  async () => {
    await ownedHeartbeat(async (fixture) => {
      const suspended = await suspendOwnedWindowsProcess(fixture.child.pid, fixture.creation);
      assert.equal(suspended.outcome, 'suspended', suspended.detail);
      const before = fixture.bytes();
      await pause(1500);
      assert.equal(fixture.exited(), false);
      assert.equal(
        fixture.bytes(),
        before,
        'heartbeat stays frozen after the actual adapter handshake',
      );
      const resumed = await resumeOwnedWindowsProcess(fixture.child.pid);
      assert.equal(resumed.outcome, 'resumed', resumed.detail);
      await until(() => fixture.bytes() > before, 'heartbeat resumes');
    });
  },
);

void test(
  'Windows adapter rejects a stale creation identity without terminating the live child',
  { skip: process.platform !== 'win32' },
  async () => {
    await ownedHeartbeat(async (fixture) => {
      const result = await suspendOwnedWindowsProcess(
        fixture.child.pid,
        (BigInt(fixture.creation) + 1n).toString(),
      );
      assert.equal(result.outcome, 'error');
      assert.match(result.detail, /creation identity mismatch/);
      assert.equal(fixture.exited(), false);
      const before = fixture.bytes();
      await until(() => fixture.bytes() > before, 'wrong-identity child remains responsive');
    });
  },
);

void test(
  'Windows adapter recognizes the owned target exiting during a watchdog restart',
  { skip: process.platform !== 'win32' },
  async () => {
    await ownedHeartbeat(async (fixture) => {
      const result = await suspendOwnedWindowsProcess(fixture.child.pid, fixture.creation);
      assert.equal(result.outcome, 'suspended', result.detail);
      fixture.child.kill();
      await until(fixture.exited, 'the exact watchdog target exits');
      const resumed = await resumeOwnedWindowsProcess(fixture.child.pid);
      assert.equal(resumed.outcome, 'already_gone', resumed.detail);
    });
  },
);

void test(
  'Windows adapter releases a handshake timeout before killing its suspension owner',
  { skip: process.platform !== 'win32' },
  async () => {
    await ownedHeartbeat(async (fixture) => {
      const result = await suspendOwnedWindowsProcess(fixture.child.pid, fixture.creation, {
        handshakeMilliseconds: 1,
      });
      assert.equal(result.outcome, 'error');
      assert.equal(fixture.exited(), false, result.detail);
      const before = fixture.bytes();
      await until(() => fixture.bytes() > before, 'timed-out handshake releases the owned child');
    });
  },
);

for (const fault of ['helper-death', 'eof', 'hold-timeout']) {
  void test(
    `Windows adapter contains ${fault} after suspension`,
    { skip: process.platform !== 'win32' },
    async () => {
      await ownedHeartbeat(async (fixture) => {
        let helper;
        const result = await suspendOwnedWindowsProcess(fixture.child.pid, fixture.creation, {
          onHelperSpawn: (child) => {
            helper = child;
          },
          ...(fault === 'hold-timeout' ? { holdMilliseconds: 200 } : {}),
        });
        assert.equal(result.outcome, 'suspended', result.detail);
        assert.ok(helper);
        if (fault === 'helper-death') helper.kill();
        if (fault === 'eof') helper.stdin.end();
        await until(
          () => helper.exitCode !== null || helper.signalCode !== null,
          'fault helper exits',
        );
        const recovered = await resumeOwnedWindowsProcess(fixture.child.pid);
        assert.equal(recovered.outcome, 'error', recovered.detail);
        assert.match(recovered.detail, /exact owned target was terminated/);
        await until(fixture.exited, 'failed helper cannot strand the owned child');
      });
    },
  );
}
