import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import {
  MANAGED_MUTATION_LEASE_FILE,
  ManagedCacheMutationBusyError,
  acquireManagedCacheMutationLock,
  buildManagedCacheMutationAttempt,
  resolveManagedBinaryPathInNamespace,
} from '../managedCacheLock';
import { buildManagedMutationLease } from '../managedCacheProtocol';

describe('managed cache mutation lock', () => {
  let baseDir: string;

  beforeEach(() => {
    baseDir = fs.mkdtempSync(path.join(os.tmpdir(), 'managed-cache-lock-'));
  });

  afterEach(() => {
    fs.rmSync(baseDir, { recursive: true, force: true });
  });

  test('resolves an installed binary through the current pointer', () => {
    const installDir = path.join(baseDir, 'v0.18.0-2026');
    fs.mkdirSync(installDir, { recursive: true });
    const binaryPath = path.join(
      installDir,
      process.platform === 'win32' ? 'perllsp.exe' : 'perllsp',
    );
    fs.writeFileSync(binaryPath, 'stub', 'utf8');
    fs.writeFileSync(path.join(baseDir, 'current'), `${path.basename(installDir)}\n`, 'utf8');

    expect(resolveManagedBinaryPathInNamespace(baseDir)).toBe(binaryPath);
  });

  test('blocks a second acquirer until the first lock is released', async () => {
    const attempt = buildManagedCacheMutationAttempt('x86_64-unknown-linux-gnu', 'install');
    const first = await acquireManagedCacheMutationLock(baseDir, attempt, { timeoutMs: 5_000 });

    const pending = acquireManagedCacheMutationLock(
      baseDir,
      buildManagedCacheMutationAttempt('x86_64-unknown-linux-gnu', 'install'),
      { timeoutMs: 200, pollIntervalMs: 25 },
    );

    await expect(pending).rejects.toBeInstanceOf(ManagedCacheMutationBusyError);

    first.release();

    const second = await acquireManagedCacheMutationLock(
      baseDir,
      buildManagedCacheMutationAttempt('x86_64-unknown-linux-gnu', 'install'),
      { timeoutMs: 5_000 },
    );
    second.release();
  });

  test('recovers an abandoned lease when the owner process is gone', async () => {
    const attempt = buildManagedCacheMutationAttempt('aarch64-apple-darwin', 'repair');
    const staleLease = buildManagedMutationLease(
      { ...attempt, owner_process_hint: 999_999_999 },
      'stale-token-abc',
    );
    fs.mkdirSync(baseDir, { recursive: true });
    fs.writeFileSync(
      path.join(baseDir, MANAGED_MUTATION_LEASE_FILE),
      `${JSON.stringify(staleLease, null, 2)}\n`,
      'utf8',
    );

    const handle = await acquireManagedCacheMutationLock(
      baseDir,
      buildManagedCacheMutationAttempt('aarch64-apple-darwin', 'repair'),
      { timeoutMs: 2_000, pollIntervalMs: 25 },
    );
    handle.release();
  });

  test('times out with a managed_install_busy error when the lock stays held', async () => {
    const attempt = buildManagedCacheMutationAttempt('x86_64-pc-windows-msvc', 'update');
    const holder = await acquireManagedCacheMutationLock(baseDir, attempt, { timeoutMs: 5_000 });

    await expect(
      acquireManagedCacheMutationLock(
        baseDir,
        buildManagedCacheMutationAttempt('x86_64-pc-windows-msvc', 'update'),
        { timeoutMs: 250, pollIntervalMs: 25 },
      ),
    ).rejects.toBeInstanceOf(ManagedCacheMutationBusyError);

    holder.release();
  });
});
