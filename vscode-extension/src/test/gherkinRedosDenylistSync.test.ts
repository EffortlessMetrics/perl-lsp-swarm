import {
  isPotentiallyExpensiveRegex,
  isSafeGherkinStepMatch,
  normalizeGherkinRegexFlags,
} from '../gherkinRedosGuard';

describe('gherkin ReDoS guard (#6154)', () => {
  test.each(['i', 'm', 's'])('accepts lowercase Perl regex flag %s', (flag) => {
    expect(normalizeGherkinRegexFlags(flag)).toBe(flag);
  });

  test.each(['I', 'M', 'S'])('rejects uppercase Perl regex modifier %s', (flag) => {
    expect(normalizeGherkinRegexFlags(flag)).toBeNull();
  });

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
    ['disjoint literal repetition', '^a+b*$', 'aaabbb'],
    ['separated numeric repetition', '^\\d+\\.\\d+$', '12.34'],
    ['disjoint character-class repetition', '^[a-z]+[0-9]+$', 'abc123'],
    ['escaped disjoint class member', '^[a\\-]+[0-9]+$', 'a-1'],
    ['literal dot character class', '^[.*]+$', '...'],
    ['literal plus character class', '^[.+]*$', '++'],
    ['finite adjacent repetition', '^a{1,3}a{1,3}$', 'aa'],
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
    ['quantified wildcard policy', '^.*$', 'anything'],
    ['identical adjacent repetition', '^a*a*$', 'aaaa'],
    ['overlapping adjacent classes', '^[a-z]+[m-z]+$', 'am'],
    ['equivalent adjacent digit classes', '^\\d+[0-9]+$', '12'],
    ['complemented adjacent classes fail closed', '^[^a]+[^b]+$', 'xy'],
    ['Unicode adjacent atoms fail closed', '^é+[é]+$', 'éé'],
    ['adjacent unbounded counted repetition', '^a{1,}a{1,}$', 'aa'],
  ])('rejects %s through the shared match policy', (_name, source, stepText) => {
    expect(isSafeGherkinStepMatch(source, stepText)).toBe(false);
  });

  test('rejects adjacent repetition that overlaps only after case folding', () => {
    expect(isSafeGherkinStepMatch('^a+A+$', 'aaAA', 'i')).toBe(false);
    expect(isSafeGherkinStepMatch('^a+A+$', 'aaAA')).toBe(true);
  });
});

// #9806. Groups were invisible to the adjacency scan, so a chain of adjacent
// variable-width groups reached `RegExp.test()` unclassified. Each rejected
// source below was measured on main against 511 `a`s (50/80 for the alternation
// chains) plus a final `!`, inside the module's own 256/512 bounds. These assert
// the guard's decision rather than a wall time: the measurements motivate the
// rule, they are not a stable oracle.
describe('gherkin ReDoS guard: variable-width atom chains (#9806)', () => {
  test.each([
    ['capturing group chain, 26.3 s on main', '^(a+)(a+)(a+)(a+)b$'],
    ['non-capturing spelling, 26.3 s on main', '^(?:a+)(?:a+)(?:a+)(?:a+)b$'],
    ['named-group spelling', '^(?<one>a+)(?<two>a+)(?<three>a+)b$'],
    ['minimal three-group chain', '^(a+)(a+)(a+)b$'],
    ['class chain', '^[ab]+[a-b]+[ab]+[a-b]+$'],
  ])('rejects %s without executing it', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(true);
  });

  test.each([
    ['three unquantified ambiguous branches', `^${'(a|aa)'.repeat(3)}b$`],
    ['twenty-five, 1.8 s on main', `^${'(a|aa)'.repeat(25)}b$`],
    ['forty, 89.7 s on main', `^${'(a|aa)'.repeat(40)}b$`],
    ['overlapping branches of differing width', '^(ab|aba)(ab|aba)(ab|aba)c$'],
  ])('rejects unquantified ambiguous alternation chain: %s', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(true);
  });

  test('rejects a chain nested inside another group', () => {
    expect(isPotentiallyExpensiveRegex('^((a+)(a+)(a+)b)$')).toBe(true);
  });

  test('rejects a chain a nullable atom cannot separate', () => {
    expect(isPotentiallyExpensiveRegex('^(a+)x?(a+)(a+)b$')).toBe(true);
  });

  test('rejects a group chain that overlaps only after case folding', () => {
    expect(isPotentiallyExpensiveRegex('^(a+)(A+)(a+)b$', 'i')).toBe(true);
    expect(isPotentiallyExpensiveRegex('^(a+)(A+)(a+)b$')).toBe(false);
  });

  // Negative controls. Each differs minimally from a rejected source above, so
  // a rule that over-rejects fails here rather than silently reintroducing the
  // #859 false negatives.
  test.each([
    ['two competing groups, 1.2 ms on main', '^(a+)(a+)b$'],
    ['two ambiguous alternations', `^${'(a|aa)'.repeat(2)}b$`],
    ['three groups over disjoint domains', '^(a+)(b+)(c+)d$'],
    ['a required separator between groups', '^(a+)x(a+)x(a+)b$'],
    ['a required separator inside each group', '^(\\w+ )(\\w+ )(\\w+ )$'],
    ['branches of equal width', '^(ab|cd)(ab|cd)(ab|cd)$'],
    ['a single quantified alternation', '^(cat|dog)+$'],
    ['ordinary quoted captures', '^I have "([^"]+)" and "([^"]+)"$'],
  ])('keeps %s available', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(false);
  });
});
