import { isPotentiallyExpensiveRegex, isSafeGherkinStepMatch } from '../gherkinRedosGuard';

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

  test.each([
    ['anchored capture', '^I have "([^\"]+)"$', 'I have "alice"'],
    ['alternation', '^status: (pass|fail)$', 'status: pass'],
    ['character class', '^item [A-Z]+$', 'item ABC'],
    ['fixed repetition', '^code [0-9]{4}$', 'code 1234'],
    ['escaping', '^path\\/to\\/file$', 'path/to/file'],
    ['supported flags', '^status: pass$', 'STATUS: PASS'],
  ])('accepts %s through the shared match policy', (_name, source, stepText) => {
    expect(isSafeGherkinStepMatch(source, stepText)).toBe(true);
  });

  test.each([
    ['oversized regex', `^${'a'.repeat(257)}$`, 'a'],
    ['oversized input', '^a$', 'a'.repeat(513)],
    ['nested quantifier', '^(a+)+$', 'aaaaaaaa'],
    ['quantified alternation', '^(a|aa)+$', 'aaaaaaaa'],
    ['backreference', '^(a)\\1$', 'aa'],
    ['named backreference', '^(?<value>a)\\k<value>$', 'a'],
    ['lookahead', '^(?=a)a$', 'a'],
    ['quantified wildcard', '^.*$', 'anything'],
    ['adjacent variable repetition', '^a*a*$', 'aaaa'],
  ])('rejects %s through the shared match policy', (_name, source, stepText) => {
    expect(isSafeGherkinStepMatch(source, stepText)).toBe(false);
  });
});
