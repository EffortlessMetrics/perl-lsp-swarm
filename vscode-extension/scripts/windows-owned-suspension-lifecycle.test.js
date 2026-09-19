const { EventEmitter } = require('node:events');
const childProcess = require('node:child_process');
const path = require('node:path');
const { test } = require('node:test');
const assert = require('node:assert/strict');

void test('failed cleanup keeps the owned pid reserved until the original worker exits', async () => {
  const workers = [];
  const originalSpawn = childProcess.spawn;
  const originalSetTimeout = global.setTimeout;
  const modulePath = path.join(__dirname, '../out/test/published/windowsOwnedSuspension.js');
  delete require.cache[require.resolve(modulePath)];
  /** @type {any} */ (global).setTimeout = (callback, milliseconds, ...args) =>
    originalSetTimeout(callback, Math.min(milliseconds ?? 0, 20), ...args);
  /** @type {typeof childProcess.spawn} */
  childProcess.spawn = (...args) => {
    const processId = args[1][args[1].indexOf('-ProcessId') + 1];
    const cleanup = args[1].includes('-Cleanup');
    const prior = workers.filter((worker) => worker.processId === processId);
    /** @type {any} */
    const child = new EventEmitter();
    child.processId = processId;
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.stdin = new EventEmitter();
    Object.assign(child.stdin, {
      destroyed: false,
      writableEnded: false,
      end: () => {
        child.stdin.writableEnded = true;
        if (cleanup) setImmediate(() => child.close(1));
        if (!cleanup && prior.length >= 2) setImmediate(() => child.close(0));
      },
    });
    child.completed = false;
    child.exitCode = null;
    child.signalCode = null;
    child.kill = () => {
      child.close(null);
      return true;
    };
    child.close = (code) => {
      if (child.completed) return;
      child.completed = true;
      child.exitCode = code;
      child.emit('close', code);
    };
    workers.push(child);
    if (!cleanup && (processId === '9876' || prior.length >= 2)) {
      setImmediate(() => child.stdout.emit('data', Buffer.from('SUSPENDED 1 THREADS\r\n')));
    }
    return child;
  };
  try {
    const { suspendOwnedWindowsProcess, resumeOwnedWindowsProcess } = require(modulePath);
    const failed = await suspendOwnedWindowsProcess(4321, '123', { handshakeMilliseconds: 1 });
    assert.equal(failed.outcome, 'error', failed.detail);
    const duplicate = await suspendOwnedWindowsProcess(4321, '123');
    assert.equal(duplicate.outcome, 'error');
    assert.match(duplicate.detail, /duplicate/);

    workers[0].close(1);
    await new Promise((resolve) => setImmediate(resolve));
    const reusable = await suspendOwnedWindowsProcess(4321, '123');
    assert.equal(reusable.outcome, 'suspended', reusable.detail);
    const resumed = await resumeOwnedWindowsProcess(4321);
    assert.equal(resumed.outcome, 'resumed', resumed.detail);

    const suspended = await suspendOwnedWindowsProcess(9876, '456');
    assert.equal(suspended.outcome, 'suspended', suspended.detail);
    const failedResume = await resumeOwnedWindowsProcess(9876);
    assert.equal(failedResume.outcome, 'error', failedResume.detail);
    const duplicateAfterFailedResume = await suspendOwnedWindowsProcess(9876, '456');
    assert.equal(duplicateAfterFailedResume.outcome, 'error');
    assert.match(duplicateAfterFailedResume.detail, /duplicate/);
    workers.find((worker) => worker.processId === '9876' && !worker.completed).close(1);
    await new Promise((resolve) => setImmediate(resolve));
    const reusableAfterFailedResume = await suspendOwnedWindowsProcess(9876, '456');
    assert.equal(reusableAfterFailedResume.outcome, 'suspended', reusableAfterFailedResume.detail);
    const resumedAfterFailedResume = await resumeOwnedWindowsProcess(9876);
    assert.equal(resumedAfterFailedResume.outcome, 'resumed', resumedAfterFailedResume.detail);
  } finally {
    childProcess.spawn = originalSpawn;
    global.setTimeout = originalSetTimeout;
  }
});
