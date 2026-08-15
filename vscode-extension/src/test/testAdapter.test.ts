import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import type { ChildProcess } from 'child_process';
import {
  describeFileFailure,
  parseSubtestResults,
  parseTapOutput,
  runBoundedProcess,
} from '../testAdapter';

describe('test adapter TAP parsing', () => {
  test('summarizes top-level TAP without counting indented subtests', () => {
    const output = [
      '# Subtest: nested',
      '    ok 1 - inner',
      '    1..1',
      'ok 1 - nested',
      '1..1',
    ].join('\n');

    expect(parseTapOutput(output)).toEqual({
      total: 1,
      passed: 1,
      failed: 0,
      skipped: 0,
      bailOut: null,
    });
  });

  test('preserves bailout text and plan count', () => {
    expect(parseTapOutput('Bail out! database unavailable\n1..3')).toEqual({
      total: 3,
      passed: 0,
      failed: 0,
      skipped: 0,
      bailOut: 'database unavailable',
    });
  });

  test('counts TODO failures as skipped, not failed', () => {
    const output = ['not ok 1 - future feature # TODO not implemented yet', '1..1'].join('\n');
    expect(parseTapOutput(output)).toEqual({
      total: 1,
      passed: 0,
      failed: 0,
      skipped: 1,
      bailOut: null,
    });
  });

  test('counts SKIP passes as skipped, not passed', () => {
    const output = ['ok 1 - platform-specific # SKIP only on linux', '1..1'].join('\n');
    expect(parseTapOutput(output)).toEqual({
      total: 1,
      passed: 0,
      failed: 0,
      skipped: 1,
      bailOut: null,
    });
  });

  test('maps successful and failed subtests with diagnostics', () => {
    const output = [
      '# Subtest: passes',
      '    ok 1 - assertion',
      '    1..1',
      'ok 1 - passes',
      '# Subtest: fails',
      '    not ok 1 - assertion',
      "    # Failed test 'assertion'",
      '    1..1',
      'not ok 2 - fails',
    ].join('\n');

    expect([...parseSubtestResults(output).entries()]).toEqual([
      ['passes', { ok: true, diagnostic: '', duration: 0 }],
      ['fails', { ok: false, diagnostic: "# Failed test 'assertion'", duration: 0 }],
    ]);
  });
});

describe('bounded prove process execution', () => {
  test('returns normal output without truncation', async () => {
    const result = await runBoundedProcess(process.execPath, ['-e', 'process.stdout.write("ok")'], {
      shell: false,
      timeoutMs: 5_000,
      maxOutputBytes: 32,
      terminationGraceMs: 25,
    });

    expect(result).toMatchObject({
      outcome: 'completed',
      stdout: 'ok',
      stderr: '',
      exitCode: 0,
    });
  }, 30_000);

  test('terminates a process that exceeds the wall-clock deadline', async () => {
    const result = await runBoundedProcess(process.execPath, ['-e', 'setTimeout(() => {}, 5000)'], {
      shell: false,
      timeoutMs: 100,
      maxOutputBytes: 32,
      terminationGraceMs: 25,
    });

    expect(result.outcome).toBe('timed_out');
    expect(result.diagnostic).toContain('deadline');
  }, 30_000);

  test('terminates a process that exceeds the combined output ceiling', async () => {
    const result = await runBoundedProcess(
      process.execPath,
      ['-e', 'process.stdout.write("x".repeat(4096))'],
      {
        shell: false,
        timeoutMs: 5_000,
        maxOutputBytes: 128,
        terminationGraceMs: 25,
      },
    );

    expect(result.outcome).toBe('output_limit');
    expect(result.capturedOutputBytes).toBe(128);
    expect(result.stdout.length).toBeLessThanOrEqual(128);
    expect(result.diagnostic).toContain('capture limit');
  }, 30_000);

  test('enforces the combined stdout/stderr byte ceiling across both streams', async () => {
    const result = await runBoundedProcess(
      process.execPath,
      ['-e', 'process.stdout.write("a".repeat(80)); process.stderr.write("b".repeat(80));'],
      {
        shell: false,
        timeoutMs: 5_000,
        maxOutputBytes: 100,
        terminationGraceMs: 25,
        terminationWatchdogMs: 1_000,
      },
    );

    expect(result.outcome).toBe('output_limit');
    expect(result.capturedOutputBytes).toBe(100);
    expect(result.stdout.length + result.stderr.length).toBeLessThanOrEqual(100);
    expect(result.stdout.includes('b')).toBe(false);
    expect(result.stderr.includes('a')).toBe(false);
  }, 30_000);

  test('keeps stdout and stderr separate while streaming', async () => {
    const result = await runBoundedProcess(
      process.execPath,
      ['-e', 'process.stdout.write("out"); process.stderr.write("err");'],
      {
        shell: false,
        timeoutMs: 5_000,
        maxOutputBytes: 64,
        terminationGraceMs: 25,
      },
    );

    expect(result).toMatchObject({
      outcome: 'completed',
      stdout: 'out',
      stderr: 'err',
    });
  }, 30_000);

  test('waits for close after escalating past an ignored SIGTERM', async () => {
    if (process.platform === 'win32') {
      return;
    }

    const started = Date.now();
    const result = await runBoundedProcess(
      process.execPath,
      ['-e', 'process.on("SIGTERM", () => {}); setTimeout(() => {}, 5000);'],
      {
        shell: false,
        timeoutMs: 100,
        maxOutputBytes: 32,
        terminationGraceMs: 50,
        terminationWatchdogMs: 2_000,
      },
    );

    expect(result.outcome).toBe('timed_out');
    expect(Date.now() - started).toBeGreaterThanOrEqual(140);
    expect(result.diagnostic).toContain('deadline');
  }, 30_000);

  test('surfaces termination_failed when forced kill never yields close', async () => {
    const live: ChildProcess[] = [];
    try {
      const result = await runBoundedProcess(
        process.execPath,
        ['-e', 'setTimeout(() => {}, 5000)'],
        {
          shell: false,
          timeoutMs: 50,
          maxOutputBytes: 32,
          terminationGraceMs: 25,
          terminationWatchdogMs: 150,
          killProcess: (proc) => {
            live.push(proc);
            return false;
          },
        },
      );

      expect(result.outcome).toBe('termination_failed');
      expect(result.diagnostic).toContain('forced termination');
    } finally {
      for (const proc of live) {
        try {
          proc.kill('SIGKILL');
        } catch {
          // Best-effort cleanup for the intentionally unkillable seam.
        }
      }
    }
  }, 30_000);

  test('resolves only after a delayed SIGKILL close is observed', async () => {
    if (process.platform === 'win32') {
      return;
    }

    let killCount = 0;
    const started = Date.now();
    const result = await runBoundedProcess(
      process.execPath,
      ['-e', 'process.on("SIGTERM", () => {}); setTimeout(() => {}, 5000);'],
      {
        shell: false,
        timeoutMs: 50,
        maxOutputBytes: 32,
        terminationGraceMs: 25,
        terminationWatchdogMs: 2_000,
        killProcess: (proc, signal) => {
          killCount += 1;
          if (signal === 'SIGKILL') {
            setTimeout(() => {
              try {
                proc.kill('SIGKILL');
              } catch {
                // Child may have exited while the delayed kill was queued.
              }
            }, 150);
            return true;
          }
          return false;
        },
      },
    );

    expect(result.outcome).toBe('timed_out');
    expect(killCount).toBeGreaterThanOrEqual(2);
    expect(Date.now() - started).toBeGreaterThanOrEqual(200);
  }, 30_000);

  test('terminates a process when the caller aborts', async () => {
    const controller = new AbortController();
    const resultPromise = runBoundedProcess(
      process.execPath,
      ['-e', 'setTimeout(() => {}, 5000)'],
      {
        shell: false,
        signal: controller.signal,
        timeoutMs: 5_000,
        maxOutputBytes: 32,
        terminationGraceMs: 25,
      },
    );
    controller.abort();

    const result = await resultPromise;
    expect(result.outcome).toBe('cancelled');
    expect(result.diagnostic).toContain('cancelled');
  }, 30_000);

  test('terminates the Windows shell child, not only cmd.exe', async () => {
    if (process.platform !== 'win32') {
      return;
    }

    const marker = path.join(os.tmpdir(), `perl-lsp-test-adapter-${process.pid}.txt`);
    const script = path.join(os.tmpdir(), `perl-lsp-test-adapter-${process.pid}.js`);
    fs.writeFileSync(
      script,
      `setTimeout(() => require('fs').writeFileSync(${JSON.stringify(marker)}, 'survived'), 1000);\n` +
        'setTimeout(() => {}, 5000);\n',
      'utf8',
    );
    try {
      const result = await runBoundedProcess(process.execPath, [script], {
        shell: true,
        timeoutMs: 100,
        maxOutputBytes: 32,
        terminationGraceMs: 25,
      });

      expect(result.outcome).toBe('timed_out');
      await new Promise((resolve) => setTimeout(resolve, 1_250));
      expect(fs.existsSync(marker)).toBe(false);
    } finally {
      fs.rmSync(script, { force: true });
      fs.rmSync(marker, { force: true });
    }
  }, 30_000);
});

describe('file-level failure explanation', () => {
  test('uses the singular noun for a single-test file', () => {
    expect(describeFileFailure(1, { total: 1, failed: 1, bailOut: null })).toBe(
      '1 of 1 test failed',
    );
  });

  test('uses the plural noun for a multi-test file', () => {
    expect(describeFileFailure(1, { total: 4, failed: 2, bailOut: null })).toBe(
      '2 of 4 tests failed',
    );
  });

  test('names the bail out alongside the failing assertions', () => {
    expect(describeFileFailure(1, { total: 3, failed: 1, bailOut: 'db offline' })).toBe(
      '1 of 3 tests failed (Bail out! db offline)',
    );
  });

  test('reports a bail out that failed no assertion as a bail out', () => {
    // "0 of 2 tests failed" contradicts the failed run the user is looking at.
    expect(describeFileFailure(1, { total: 2, failed: 0, bailOut: 'db offline' })).toBe(
      'Test run bailed out: db offline',
    );
  });

  test('reports a non-zero exit with no failing assertion as a process failure', () => {
    expect(describeFileFailure(9, { total: 3, failed: 0, bailOut: null })).toBe(
      'No assertion failed, but the test process exited with 9.',
    );
  });

  test('reports a run that produced no TAP results at all', () => {
    expect(describeFileFailure(2, { total: 0, failed: 0, bailOut: null })).toBe(
      'No test results were reported; the test process exited with 2.',
    );
  });

  test('names a signal termination instead of a missing exit code', () => {
    expect(describeFileFailure(null, { total: 0, failed: 0, bailOut: null })).toBe(
      'No test results were reported; the test process was terminated by a signal.',
    );
  });
});
