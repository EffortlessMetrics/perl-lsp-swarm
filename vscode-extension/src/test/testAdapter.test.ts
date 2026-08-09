import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { parseSubtestResults, parseTapOutput, runBoundedProcess } from '../testAdapter';

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
