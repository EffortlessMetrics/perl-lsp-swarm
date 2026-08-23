import { parsePackagedServerVersionStdout } from '../packagedServerVersion';

describe('packaged server version stdout parsing', () => {
  test('accepts a newline-terminated stdout version line', () => {
    expect(parsePackagedServerVersionStdout('perllsp 0.12.4\n')).toBe('0.12.4');
  });

  test('accepts stdout without a trailing newline and ignores stderr fusion hazards', () => {
    // A fused "stdout+stderr" buffer would reject this valid version line.
    expect(parsePackagedServerVersionStdout('perllsp 0.12.4')).toBe('0.12.4');
  });

  test('rejects a canonical-looking version that appears only on stderr', () => {
    const stdout = '';
    const stderr = 'perllsp 9.9.9\n';
    expect(parsePackagedServerVersionStdout(stdout)).toBeNull();
    // Parsing stderr would falsely accept a substituted binary; callers must not.
    expect(parsePackagedServerVersionStdout(stderr)).toBe('9.9.9');
  });

  test('accepts a valid stdout line even when stderr carries a warning fragment', () => {
    // Provenance stays split: parser sees only stdout, never a fused buffer.
    expect(parsePackagedServerVersionStdout('perllsp 0.12.4')).toBe('0.12.4');
  });

  test('does not accept stderr text smuggled through a combined buffer', () => {
    expect(parsePackagedServerVersionStdout('warning: noisy\nperllsp 0.12.4\n')).toBeNull();
    expect(parsePackagedServerVersionStdout('perllsp 0.12.4warning: noisy')).toBeNull();
  });
});
