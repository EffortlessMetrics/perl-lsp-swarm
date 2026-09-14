import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals';
import * as processRunner from '../testAdapter';
import { terminateServerProcess } from './published/journeySupport';
import { spawn } from 'child_process';

const originalPlatform = process.platform;

describe('installed journey single-process crash injection', () => {
  beforeEach(() => {
    // The old Windows path must fail safely rather than spawning taskkill for
    // these synthetic PIDs. This also detects accidental helper regressions.
    jest.spyOn(processRunner, 'runBoundedProcess').mockResolvedValue({
      outcome: 'termination_failed',
      stdout: '',
      stderr: '',
      exitCode: null,
      signal: null,
      capturedOutputBytes: 0,
      diagnostic: 'unexpected helper process',
    });
  });

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform });
    jest.restoreAllMocks();
  });

  test.each(['win32', 'linux', 'darwin'] as const)(
    '%s delivers an external forced termination to exactly one PID',
    async (platform) => {
      Object.defineProperty(process, 'platform', { value: platform });
      const kill = jest.spyOn(process, 'kill').mockReturnValue(true);
      const result = await terminateServerProcess(4242);
      expect(result.outcome).toBe('terminated');
      expect(kill).toHaveBeenCalledTimes(1);
      expect(kill).toHaveBeenCalledWith(4242, 'SIGKILL');
      expect(processRunner.runBoundedProcess).not.toHaveBeenCalled();
    },
  );

  test.each(['win32', 'linux'] as const)(
    '%s recognizes typed process absence without relying on localized text',
    async (platform) => {
      Object.defineProperty(process, 'platform', { value: platform });
      jest.spyOn(process, 'kill').mockImplementation(() => {
        throw Object.assign(new Error('target missing'), { code: 'ESRCH' });
      });
      expect((await terminateServerProcess(4242)).outcome).toBe('already_gone');
    },
  );

  test.each(['EPERM', 'EACCES', 'EINVAL'])(
    'retains %s as a failure even if its message mentions ESRCH',
    async (code) => {
      jest.spyOn(process, 'kill').mockImplementation(() => {
        throw Object.assign(new Error('ESRCH was not confirmed'), { code });
      });
      const result = await terminateServerProcess(4242);
      expect(result.outcome).toBe('error');
      expect(result.detail).toContain('ESRCH was not confirmed');
    },
  );

  test.each([0, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY])(
    'rejects invalid PID %s before touching a process',
    async (pid) => {
      const kill = jest.spyOn(process, 'kill').mockReturnValue(true);
      expect((await terminateServerProcess(pid)).outcome).toBe('error');
      expect(kill).not.toHaveBeenCalled();
      expect(processRunner.runBoundedProcess).not.toHaveBeenCalled();
    },
  );

  test('the native helper terminates an owned child, with exit observed separately', async () => {
    const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
      windowsHide: true,
      stdio: 'ignore',
    });
    let closed = false;
    const exit = new Promise<void>((resolve, reject) => {
      child.once('error', reject);
      child.once('close', () => {
        closed = true;
        resolve();
      });
    });
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        if (!closed) child.kill('SIGKILL');
        reject(new Error('owned child did not close within 5 seconds'));
      }, 5_000);
    });
    let primaryFailure: { error: unknown } | undefined;
    try {
      if (child.pid === undefined) throw new Error('owned child has no PID');
      const result = await terminateServerProcess(child.pid);
      expect(result.outcome).toBe('terminated');
      await Promise.race([exit, deadline]);
      expect(closed).toBe(true);
      expect(processRunner.runBoundedProcess).not.toHaveBeenCalled();
    } catch (error) {
      primaryFailure = { error };
      throw error;
    } finally {
      if (timer !== undefined) clearTimeout(timer);
      if (!closed) {
        let cleanupTimer: ReturnType<typeof setTimeout> | undefined;
        try {
          child.kill('SIGKILL');
          await Promise.race([
            exit,
            new Promise<never>((_, reject) => {
              cleanupTimer = setTimeout(
                () => reject(new Error('owned child cleanup not observed')),
                1_000,
              );
            }),
          ]);
        } catch (cleanupError) {
          if (primaryFailure !== undefined) {
            throw new AggregateError(
              [primaryFailure.error, cleanupError],
              'test and owned-child cleanup failed',
            );
          }
          throw cleanupError;
        } finally {
          if (cleanupTimer !== undefined) clearTimeout(cleanupTimer);
        }
      }
    }
  }, 10_000);
});
