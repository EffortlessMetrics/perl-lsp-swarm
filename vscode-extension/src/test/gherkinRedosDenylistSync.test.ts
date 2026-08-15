import { isPotentiallyExpensiveRegex } from '../gherkinRedosGuard';

describe('gherkin ReDoS guard (#6154)', () => {
  test.each(['^(a|aa)+$', '^(a|a)*$', '(x|xy)+z'])(
    'rejects quantified alternation %s',
    (pattern) => {
      expect(isPotentiallyExpensiveRegex(pattern)).toBe(true);
    },
  );

  test.each(['(a+)+', '(a{2,})*'])('rejects nested quantifiers %s', (pattern) => {
    expect(isPotentiallyExpensiveRegex(pattern)).toBe(true);
  });

  test.each(['"([^\"]+)"', '([0-9]+)', '(\\w+)', '^(cat|dog)+$'])(
    'accepts linear patterns %s',
    (pattern) => {
      expect(isPotentiallyExpensiveRegex(pattern)).toBe(false);
    },
  );

  test.each(['^(a|aa){1}$', '^(a|aa){0,1}$', '^(a|aa){2,5}$'])(
    'accepts finite quantified alternation %s',
    (pattern) => {
      expect(isPotentiallyExpensiveRegex(pattern)).toBe(false);
    },
  );

  test.each(['^(a|aa){2,}$'])('rejects unbounded counted alternation %s', (pattern) => {
    expect(isPotentiallyExpensiveRegex(pattern)).toBe(true);
  });

  test.each(['^(foo\\|bar)+$', '^(foo[|]bar)+$'])(
    'accepts quantified literal pipes %s',
    (pattern) => {
      expect(isPotentiallyExpensiveRegex(pattern)).toBe(false);
    },
  );
});
