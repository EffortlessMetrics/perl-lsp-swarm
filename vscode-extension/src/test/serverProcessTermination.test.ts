import { afterEach, describe, expect, jest, test } from '@jest/globals';
import { EventEmitter } from 'events';
import {
  awaitServerProcessExit,
  hasExited,
  serverProcessOf,
  type ServerProcessLike,
} from '../serverProcessTermination';

class FakeChild extends EventEmitter implements ServerProcessLike {
  pid: number | undefined = 4242;
  exitCode: number | null = null;
  signalCode: string | null = null;

  exit(code: number | null = 0, signal: string | null = null): void {
    this.exitCode = code;
    this.signalCode = signal;
    this.emit('exit', code, signal);
  }
}

const alive = (): boolean => true;
const gone = (): boolean => false;

describe('server process termination observation (#14155)', () => {
  afterEach(() => {
    jest.useRealTimers();
  });

  test('serverProcessOf reads the client getter and rejects non-process values', () => {
    const child = new FakeChild();
    expect(serverProcessOf({ serverProcess: child })).toBe(child);
    expect(serverProcessOf({ serverProcess: undefined })).toBeUndefined();
    expect(serverProcessOf({ serverProcess: { pid: 1 } })).toBeUndefined();
    expect(serverProcessOf({})).toBeUndefined();
    expect(serverProcessOf(undefined)).toBeUndefined();
  });

  test('hasExited is true only once an exit code, a signal, or a dead pid is observed', () => {
    const child = new FakeChild();
    expect(hasExited(child, alive)).toBe(false);
    expect(hasExited(child, gone)).toBe(true);
    child.exitCode = 0;
    expect(hasExited(child, alive)).toBe(true);

    const signalled = new FakeChild();
    signalled.signalCode = 'SIGTERM';
    expect(hasExited(signalled, alive)).toBe(true);

    const neverSpawned = new FakeChild();
    neverSpawned.pid = undefined;
    expect(hasExited(neverSpawned, alive)).toBe(true);
  });

  test('an absent handle or an already-exited child resolves immediately', async () => {
    await expect(awaitServerProcessExit(undefined, 10)).resolves.toBe(true);
    const child = new FakeChild();
    child.exit(0);
    await expect(awaitServerProcessExit(child, 10, alive)).resolves.toBe(true);
  });

  test('a child that exits inside the grace resolves true and detaches its listener', async () => {
    jest.useFakeTimers();
    const child = new FakeChild();
    const pending = awaitServerProcessExit(child, 4_000, alive);
    expect(child.listenerCount('exit')).toBe(1);

    await jest.advanceTimersByTimeAsync(2_000);
    child.exit(null, 'SIGTERM');

    await expect(pending).resolves.toBe(true);
    expect(child.listenerCount('exit')).toBe(0);
  });

  test('a child still alive at the grace bound resolves false', async () => {
    jest.useFakeTimers();
    const child = new FakeChild();
    const pending = awaitServerProcessExit(child, 4_000, alive);

    await jest.advanceTimersByTimeAsync(4_000);

    await expect(pending).resolves.toBe(false);
    expect(child.listenerCount('exit')).toBe(0);
  });

  test('a child whose pid died without an exit event is still recognised at the bound', async () => {
    jest.useFakeTimers();
    const child = new FakeChild();
    let probeAlive = true;
    const pending = awaitServerProcessExit(child, 4_000, () => probeAlive);

    probeAlive = false;
    await jest.advanceTimersByTimeAsync(4_000);

    await expect(pending).resolves.toBe(true);
  });
});
