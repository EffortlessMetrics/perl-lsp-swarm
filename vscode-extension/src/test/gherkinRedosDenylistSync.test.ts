/**
 * The ReDoS denylist is duplicated in two modules that both carry a
 * "Keep this in sync with ..." comment. Until now nothing enforced it.
 *
 * #6154 extended the denylist in gherkinStepDefinitions.ts to catch quantified
 * alternation (`^(a|aa)+$`) and renamed the constant, but left
 * gherkinProviders.ts on the old pattern and the old name. The result was a
 * security fix applied to one of two production paths, plus a call site in
 * gherkinStepDefinitions.ts still referencing the pre-rename identifier —
 * which broke `tsc --noEmit` on main.
 *
 * These tests read the declarations out of both sources so the duplication
 * cannot silently diverge again, and pin the behaviour the denylist exists to
 * provide.
 */

import * as fs from 'fs';
import * as path from 'path';

// Tests execute from `out-test/test`, so resolve the real sources relative to
// the extension root rather than to the compiled output.
const SRC = path.resolve(__dirname, '..', '..', 'src');

function readDenylist(file: string): { name: string; pattern: RegExp } {
  const source = fs.readFileSync(path.join(SRC, file), 'utf8');
  // `\s*` rather than `\s*\n\s*`: the declaration currently wraps after the
  // `=`, but a formatter is free to collapse it onto one line, and this test
  // failing to *find* the constant would be a confusing way to learn that.
  const match = source.match(/const (POTENTIALLY_\w+_REGEX_RE) =\s*(\/.*\/);/);
  if (!match?.[1] || !match[2]) {
    throw new Error(`could not locate the ReDoS denylist declaration in ${file}`);
  }
  // Strip the leading and trailing slash of the regex literal.
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
    // The two files document each other as the sync target; this is the
    // assertion that makes those comments load-bearing.
    expect(providers.pattern.source).toBe(stepDefinitions.pattern.source);
  });

  describe.each([
    ['gherkinStepDefinitions.ts', () => stepDefinitions.pattern],
    ['gherkinProviders.ts', () => providers.pattern],
  ])('%s', (_file, denylist) => {
    // Quantified alternation creates overlapping match paths that backtrack
    // exponentially. This is the class #6154 was written to catch, and the
    // class gherkinProviders.ts silently did not catch.
    test.each(['^(a|aa)+$', '^(a|a)*$', '(x|xy)+z'])('rejects %s', (pattern) => {
      expect(denylist().test(pattern)).toBe(true);
    });

    // Nested quantifiers — the original denylist class, kept as a guard that
    // the alternation clause did not displace the earlier one.
    test.each(['(a+)+', '(a{2,})*'])('rejects %s', (pattern) => {
      expect(denylist().test(pattern)).toBe(true);
    });

    // Direction control: a single character class with one quantifier is
    // linear-time. Flagging these previously suppressed step-definition links
    // for ordinary patterns (#859), so over-blocking is a real regression.
    test.each(['"([^"]+)"', '([0-9]+)', '(\\w+)'])('accepts %s', (pattern) => {
      expect(denylist().test(pattern)).toBe(false);
    });
  });
});
