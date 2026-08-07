import {
  BoundedTextBuffer,
  TapStreamParser,
  normalizeProveTimeoutMs,
  parseSubtestResults,
  parseTapOutput,
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

  test('parses TAP incrementally across arbitrary chunk boundaries', () => {
    const parser = new TapStreamParser();
    parser.push('# Subtest: split\n    not ok 1 - asser');
    parser.push("tion\n    # Failed test 'split'\n    1..1\nnot ok 1 - split\n1..");
    parser.push('1');
    parser.finish();

    expect(parser.getSummary()).toEqual({
      total: 1,
      passed: 0,
      failed: 1,
      skipped: 0,
      bailOut: null,
    });
    expect(parser.getSubtestResults().get('split')).toEqual({
      ok: false,
      diagnostic: "# Failed test 'split'",
      duration: 0,
    });
  });
});

describe('bounded prove process state', () => {
  test('keeps only the configured tail of noisy stderr', () => {
    const buffer = new BoundedTextBuffer(8);
    buffer.append('1234');
    buffer.append('56789');

    expect(buffer.toString()).toBe('[earlier output truncated]\n23456789');
  });

  test('clamps invalid and extreme timeout settings to a hard envelope', () => {
    expect(normalizeProveTimeoutMs(undefined)).toBe(300000);
    expect(normalizeProveTimeoutMs(Number.NaN)).toBe(300000);
    expect(normalizeProveTimeoutMs(0)).toBe(1000);
    expect(normalizeProveTimeoutMs(2.5)).toBe(2500);
    expect(normalizeProveTimeoutMs(100000)).toBe(3600000);
  });
});
