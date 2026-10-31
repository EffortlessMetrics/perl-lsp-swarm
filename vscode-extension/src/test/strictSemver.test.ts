import { compareStrictSemver, parseStrictSemver, type ParsedSemver } from '../strictSemver';

/**
 * Direct pins for the strict SemVer vocabulary (#17336).
 *
 * `strictSemver` is the version authority for managed release selection and
 * configuration-migration era gating; only consumer tests exercised it before,
 * so a comparator regression could silently re-point the managed download
 * channel without any suite noticing.
 */

function version(
  major: string,
  minor: string,
  patch: string,
  prerelease: string[] = [],
): ParsedSemver {
  return { major, minor, patch, prerelease };
}

describe('parseStrictSemver', () => {
  test.each([
    ['1.2.3', version('1', '2', '3')],
    ['v1.2.3', version('1', '2', '3')],
    ['0.0.0', version('0', '0', '0')],
    ['10.20.30', version('10', '20', '30')],
    ['1.2.3-alpha', version('1', '2', '3', ['alpha'])],
    ['1.2.3-alpha.1', version('1', '2', '3', ['alpha', '1'])],
    ['1.2.3-rc.1.0', version('1', '2', '3', ['rc', '1', '0'])],
    ['1.2.3-0', version('1', '2', '3', ['0'])],
  ])('parses %s', (input, expected) => {
    expect(parseStrictSemver(input)).toEqual(expected);
  });

  test('ignores build metadata when parsing precedence input', () => {
    expect(parseStrictSemver('1.2.3+build.7')).toEqual(version('1', '2', '3'));
    expect(parseStrictSemver('1.2.3-alpha+build.7')).toEqual(version('1', '2', '3', ['alpha']));
    expect(parseStrictSemver('1.2.3+001')).toEqual(version('1', '2', '3'));
  });

  test.each([
    '1.2',
    '1.2.3.4',
    '1.2.3-alpha.01',
    'not-a-version',
    '',
    '1.2.3-',
    '1.2.3+',
    '1 .2.3',
    '١.٢.٣',
  ])('rejects %s', (input) => {
    expect(parseStrictSemver(input)).toBeNull();
  });

  test('rejects leading zeros in core numbers', () => {
    expect(parseStrictSemver('01.2.3')).toBeNull();
    expect(parseStrictSemver('1.02.3')).toBeNull();
    expect(parseStrictSemver('1.2.03')).toBeNull();
  });

  test('rejects leading-zero numeric prerelease identifiers', () => {
    expect(parseStrictSemver('1.2.3-01')).toBeNull();
    expect(parseStrictSemver('1.2.3-00.1')).toBeNull();
    expect(parseStrictSemver('1.2.3-alpha.01')).toBeNull();
    // Alphanumeric identifiers may start with zero.
    expect(parseStrictSemver('1.2.3-0alpha')).toEqual(version('1', '2', '3', ['0alpha']));
  });

  test('rejects core numbers beyond the safe integer range', () => {
    expect(parseStrictSemver('99999999999999999999.0.0')).toBeNull();
    expect(parseStrictSemver('1.99999999999999999999.0')).toBeNull();
    expect(parseStrictSemver('1.2.99999999999999999999')).toBeNull();
  });

  test('rejects non-string input', () => {
    expect(parseStrictSemver(null)).toBeNull();
    expect(parseStrictSemver(undefined)).toBeNull();
    expect(parseStrictSemver(42)).toBeNull();
    expect(parseStrictSemver({ major: 1 })).toBeNull();
  });
});

describe('compareStrictSemver', () => {
  test('orders the canonical SemVer precedence chain', () => {
    const chain = [
      '1.0.0-alpha',
      '1.0.0-alpha.1',
      '1.0.0-alpha.beta',
      '1.0.0-beta',
      '1.0.0-beta.2',
      '1.0.0-beta.11',
      '1.0.0-rc.1',
      '1.0.0',
    ].map((value) => parseStrictSemver(value)) as ParsedSemver[];

    for (let i = 0; i < chain.length - 1; i += 1) {
      const lower = chain[i];
      const higher = chain[i + 1];
      if (!lower || !higher) throw new Error('chain entry failed to parse');
      expect(compareStrictSemver(lower, higher)).toBe(-1);
      expect(compareStrictSemver(higher, lower)).toBe(1);
    }
  });

  test.each([
    ['2.0.0', '1.9.9'],
    ['1.10.0', '1.9.0'],
    ['1.2.10', '1.2.9'],
    // Numeric identifiers compare by value, not string order.
    ['1.0.0-10', '1.0.0-9'],
    ['1.0.0-2', '1.0.0-1.99'],
    // Numeric identifiers sort before alphanumeric ones.
    ['1.0.0-alpha', '1.0.0-1'],
    // A longer prerelease wins over its prefix when the shared part is equal.
    ['1.0.0-alpha.1', '1.0.0-alpha'],
  ])('ranks %s above %s', (higher, lower) => {
    const higherParsed = parseStrictSemver(higher);
    const lowerParsed = parseStrictSemver(lower);
    if (!higherParsed || !lowerParsed) throw new Error('table entry failed to parse');
    expect(compareStrictSemver(higherParsed, lowerParsed)).toBe(1);
    expect(compareStrictSemver(lowerParsed, higherParsed)).toBe(-1);
  });

  test('treats a release as newer than any of its prereleases', () => {
    const release = parseStrictSemver('1.0.0');
    const prerelease = parseStrictSemver('1.0.0-zz.999');
    if (!release || !prerelease) throw new Error('entry failed to parse');
    expect(compareStrictSemver(release, prerelease)).toBe(1);
    expect(compareStrictSemver(prerelease, release)).toBe(-1);
  });

  test('build metadata never affects precedence', () => {
    const left = parseStrictSemver('1.2.3+build.1');
    const right = parseStrictSemver('1.2.3+build.999');
    if (!left || !right) throw new Error('entry failed to parse');
    expect(compareStrictSemver(left, right)).toBe(0);
  });

  test('a version equals itself including prerelease shape', () => {
    const left = parseStrictSemver('2.3.4-beta.11');
    const right = parseStrictSemver('2.3.4-beta.11');
    if (!left || !right) throw new Error('entry failed to parse');
    expect(compareStrictSemver(left, right)).toBe(0);
  });
});
