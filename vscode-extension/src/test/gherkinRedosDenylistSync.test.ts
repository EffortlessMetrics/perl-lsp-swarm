/**
 * The ReDoS denylist is duplicated in two modules that both carry a
 * "Keep this in sync with ..." comment. Until now nothing enforced it.
 *
 * #6154 extended the denylist in gherkinStepDefinitions.ts to catch quantified
 * alternation and renamed the constant, but left gherkinProviders.ts on the
 * old pattern and old name. These tests keep the duplicated security seam
 * aligned and pin the behavior the denylist exists to provide.
 */

import * as fs from 'fs';
import * as path from 'path';

const SRC = path.resolve(__dirname, '..', '..', 'src');

function readDenylist(file: string): { name: string; pattern: RegExp } {
  const source = fs.readFileSync(path.join(SRC, file), 'utf8');
  const match = source.match(/const (POTENTIALLY_\w+_REGEX_RE) =\s*(\/.*\/);/);
  if (!match?.[1] || !match[2]) {
    throw new Error(`could not locate the ReDoS denylist declaration in ${file}`);
  }
  const body = match[2].slice(1, -1);
  return { name: match[1], pattern: new RegExp(body) };
}

describe('gherkin ReDoS denylist duplication (#6154)', () => {
  const stepDefinitions = readDenylist('gherkinStepDefinitions.ts');
  const providers = readDenylist('gherkinProviders.ts');

  test('both modules declare the denylist under the same name', () => {
    expect(providers.name).toBe(stepDefinitions.name);
  });

  test('both modules use a byte-identical pattern', () => {
    expect(providers.pattern.source).toBe(stepDefinitions.pattern.source);
  });

  describe.each([
    ['gherkinStepDefinitions.ts', () => stepDefinitions.pattern],
    ['gherkinProviders.ts', () => providers.pattern],
  ])('%s', (_file, denylist) => {
    test.each(['^(a|aa)+$', '^(a|a)*$', '(x|xy)+z'])('rejects %s', (pattern) => {
      expect(denylist().test(pattern)).toBe(true);
    });

    test.each(['(a+)+', '(a{2,})*'])('rejects %s', (pattern) => {
      expect(denylist().test(pattern)).toBe(true);
    });

    test.each(['"([^\"]+)"', '([0-9]+)', '(\\w+)'])('accepts %s', (pattern) => {
      expect(denylist().test(pattern)).toBe(false);
    });
  });
});
