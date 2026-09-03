import {
  gherkinRedosGuardCacheStats,
  isPotentiallyExpensiveRegex,
  isSafeGherkinStepMatch,
  MAX_MATCH_REGEX_LENGTH,
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
    ['twenty-five, 1.8 s on main', `^${'(a|aa)'.repeat(25)}b$`],
    ['forty, 89.7 s on main', `^${'(a|aa)'.repeat(40)}b$`],
  ])('rejects unquantified ambiguous alternation chain: %s', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(true);
  });

  // Bounded ambiguity costs a constant factor per atom, not an unbounded run, so
  // a short chain of it is nothing like a short chain of `+`. Three `(a|aa)`
  // groups measure 0.1 ms — rejecting them on the strength of the forty-group
  // measurement above would be the #859 over-rejection wearing a citation.
  test.each([
    ['three ambiguous branches, 0.1 ms', `^${'(a|aa)'.repeat(3)}b$`],
    ['overlapping branches of differing width', '^(ab|aba)(ab|aba)(ab|aba)c$'],
    ['sixteen, 3.8 ms', `^${'(a|aa)'.repeat(16)}b$`],
  ])('keeps short bounded-ambiguity chain available: %s', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(false);
  });

  // The accept/reject edge. The last accepted length measures 6.9 ms and the
  // first rejected one is refused, so there is no cliff between them.
  test('places the bounded-ambiguity boundary between eighteen and nineteen', () => {
    expect(isPotentiallyExpensiveRegex(`^${'(a|aa)'.repeat(18)}b$`)).toBe(false);
    expect(isPotentiallyExpensiveRegex(`^${'(a|aa)'.repeat(19)}b$`)).toBe(true);
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

  // One step definition covering several phrasings is the standard cucumber
  // idiom, and every optional English phrase ends in a space — so stacked
  // optional prefixes always overlap on `' '`, and one beside a `\w+` capture
  // overlaps on letters. A rule that counts atoms instead of weighing them
  // rejects this whole family. Each source below was measured resolving its own
  // step text, and its worst near-miss under the 512-character bound, in under
  // 0.2 ms.
  test.each([
    ['stacked optional prefixes', '^(?:the )?(?:new )?(?:admin )?user "([^"]+)" exists$'],
    ['optional alternation prefix', '^(?:I |we )?(?:should )?(?:not )?see the dashboard$'],
    ['optional prefixes before a keyword', '^(?:Given )?(?:the )?(?:current )?user logs in$'],
    ['optional prefixes before a capture', '^(?:the )?(?:new )?(\\w+) is added$'],
    ['optional prefixes around two captures', '^(?:the )?(?:last )?(\\w+) has (\\d+) rows$'],
    ['optional prefixes before a bare verb', '^(?:I )?(?:do )?(?:not )?agree$'],
    ['three optional prefixes and a literal', '^(?:the )?(?:user )?(?:already )?exists$'],
    ['optional captured phrases', '^(?:(\\w+) )?(?:(\\w+) )?(?:(\\w+) )?logs in$'],
    ['optional prefixes before a path capture', '^(?:the )?(?:perl )?(?:module )?(\\S+) compiles$'],
    ['optional numeric parts', '^(\\+|-)?(\\d+)(\\.\\d+)?(e[+-]?\\d+)?$'],
    ['optional trailing clauses', '^I have (\\d+)(?: or (\\d+))?(?: and (\\d+))?$'],
  ])('keeps the flexible-step idiom available: %s', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(false);
  });
});

// Repeating a group places a seam between each pair of consecutive copies, so a
// group is charged against itself. Without that, an adversarial workspace could
// reach `RegExp.test()` through several shapes the older rules cannot see:
// `POTENTIALLY_EXPENSIVE_REGEX_RE` matches a quantifier inside one pair of
// parentheses, and `[^)]*` cannot cross a `)`, so one extra wrapper or one
// literal `)` hides it. Each source below was measured on `main`, which accepts
// every one of them.
describe('gherkin ReDoS guard: a repeated group seams with itself (#9806)', () => {
  test.each([
    ['nested wrapper, 81 s on thirty characters', '((a+))+b'],
    ['non-capturing wrapper', '(?:(a+))+b'],
    ['non-capturing inner', '((?:a+))+b'],
    ['named wrapper', '(?<n>(a+))+b'],
    ['escaped paren blocks the older rule, 103 s', '(\\)?a+)+b'],
    ['character-class paren blocks it too, 105 s', '([)]?a+)+b'],
    ['nullable inner under an exact repeat', '^((a?)){20}z$'],
    ['bounded repeat of an ambiguous alternation', '^((a+|a)){3,7}((a+|a)){3,7}z$'],
    // A fixed-width tail does not block a seam when it is built from the same
    // character the next copy consumes. 5.4 s at 120 characters, unfinished at
    // 200 — so the seam must be charged the wider edge, not the narrower.
    ['a fixed tail over the same character', '^((a+a{2})){5}$'],
  ])('rejects %s', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(true);
  });

  // What actually blocks a self-seam is a boundary the neighbouring copy cannot
  // cross. Without these, the rule would refuse most repeated groups outright.
  test.each([
    ['a fixed inner shape', '^((ab)){5}$'],
    ['equal-width alternation', '^(?:(cat|dog)){2}$'],
    ['two copies of a nullable inner', '^((a?)){2}z$'],
    ['a repeat count at the budget', '^((a?)){17}z$'],
    ['one more copy, still inside the budget', '^((a?)){18}z$'],
    // The tail and the head cannot compete for a character, so the boundary is
    // pinned however often the group repeats.
    ['edges over disjoint characters', '^((a+b+)){5}$'],
  ])('keeps %s available', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(false);
  });
});

// `\b` and `\B` are zero-width, so they separate nothing. `readRegexAtom` hands
// them back as ordinary fixed atoms, which made `^a+\Ba+\Ba+\Ba+b$` — the same
// shape as `^(a+)(a+)(a+)(a+)b$`, and 26.8 s on the same input — look separated.
describe('gherkin ReDoS guard: zero-width assertions (#9806)', () => {
  test.each([
    ['\\B between every atom', '^a+\\Ba+\\Ba+\\Ba+b$'],
    ['\\b between every atom', '^a+\\ba+\\ba+\\ba+b$'],
    ['a chain of nullable groups joined by \\B', `^${'(a?\\B)'.repeat(40)}z$`],
  ])('does not let %s separate a chain', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(true);
  });

  test.each([
    ['an ordinary word boundary', '^\\bword\\b$'],
    ['boundaries around a capture', '^I have \\b(\\d+)\\b items$'],
  ])('keeps %s available', (_name, source) => {
    expect(isPotentiallyExpensiveRegex(source)).toBe(false);
  });
});

// The memos cap entry counts, but each key carries the pattern text, so a count
// alone does not bound memory. The guard's own input ceiling is enforced at
// admission rather than being inherited from `isSafeGherkinStepMatch`, so the
// bound holds for any caller.
describe('gherkin ReDoS guard: bounded memo keys (#9806)', () => {
  const oversized = (seed: number) => `^${'a'.repeat(MAX_MATCH_REGEX_LENGTH)}${seed}b+c+d+$`;

  test('does not retain an oversized pattern, however many arrive', () => {
    // Warm the caches so a wholesale clear mid-test cannot mask growth.
    isPotentiallyExpensiveRegex('^warm (\\w+) up$');
    const before = gherkinRedosGuardCacheStats();

    for (let seed = 0; seed < 200; seed += 1) {
      isPotentiallyExpensiveRegex(oversized(seed));
    }

    const after = gherkinRedosGuardCacheStats();
    expect(after.verdictEntries).toBe(before.verdictEntries);
    // The short atoms *inside* an oversized pattern are themselves within the
    // ceiling, so they remain cacheable; what must hold is the declared cap.
    expect(after.atomDomainEntries).toBeLessThanOrEqual(1024);
  });

  test('does not retain an oversized atom', () => {
    // One character class longer than the ceiling is a single oversized atom,
    // which is the key the atom memo must refuse. Each iteration presents a
    // distinct one, so retention would show up one-for-one in the count.
    isPotentiallyExpensiveRegex('^warm (\\d+) up$');
    const before = gherkinRedosGuardCacheStats().atomDomainEntries;

    // A–Z only. `\` and `]` are char codes 92 and 93, and either one changes how
    // the class parses — the escape stops it terminating, the bracket ends it
    // early and leaves a short atom behind — which would grow the memo for a
    // reason that has nothing to do with retention.
    for (let seed = 0; seed < 26; seed += 1) {
      const hugeClass = `[${'a'.repeat(MAX_MATCH_REGEX_LENGTH)}${String.fromCharCode(65 + seed)}]`;
      expect(hugeClass.length).toBeGreaterThan(MAX_MATCH_REGEX_LENGTH);
      isPotentiallyExpensiveRegex(`^${hugeClass}+$`);
    }

    expect(gherkinRedosGuardCacheStats().atomDomainEntries).toBe(before);
  });

  test('still answers an oversized pattern, and answers it identically each time', () => {
    // Refusing to retain must not change the verdict, and an uncached path must
    // not drift from a cached one.
    const chained = `^${'x'.repeat(MAX_MATCH_REGEX_LENGTH)}(a+)(a+)(a+)b$`;
    expect(chained.length).toBeGreaterThan(MAX_MATCH_REGEX_LENGTH);
    expect(isPotentiallyExpensiveRegex(chained)).toBe(true);
    expect(isPotentiallyExpensiveRegex(chained)).toBe(true);

    const benign = `^${'x'.repeat(MAX_MATCH_REGEX_LENGTH)} plain$`;
    expect(isPotentiallyExpensiveRegex(benign)).toBe(false);
    expect(isPotentiallyExpensiveRegex(benign)).toBe(false);
  });

  test('retains a pattern of exactly the ceiling length', () => {
    // `isSafeGherkinStepMatch` admits a source of exactly MAX_MATCH_REGEX_LENGTH,
    // so the memo must too; a strict `<` here would refuse to cache the longest
    // pattern the guard actually serves.
    const atCeiling = `^${'b'.repeat(MAX_MATCH_REGEX_LENGTH - 2)}$`;
    expect(atCeiling.length).toBe(MAX_MATCH_REGEX_LENGTH);

    const before = gherkinRedosGuardCacheStats().verdictEntries;
    // Well clear of the cap, so no wholesale clear can hide the admission and
    // make this assertion vacuous.
    expect(before).toBeLessThan(400);

    isPotentiallyExpensiveRegex(atCeiling);
    expect(gherkinRedosGuardCacheStats().verdictEntries).toBe(before + 1);
    expect(isPotentiallyExpensiveRegex(atCeiling)).toBe(false);
  });

  test('never exceeds its declared entry bounds under sustained distinct load', () => {
    for (let seed = 0; seed < 3000; seed += 1) {
      isPotentiallyExpensiveRegex(`^step ${seed} has (\\w+) and (\\d+)$`);
    }
    const stats = gherkinRedosGuardCacheStats();
    expect(stats.verdictEntries).toBeLessThanOrEqual(512);
    expect(stats.atomDomainEntries).toBeLessThanOrEqual(1024);
  });
});
