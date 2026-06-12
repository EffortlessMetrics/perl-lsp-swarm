import * as path from 'path';
import * as vscode from 'vscode';
import {
  buildGeneratedStepPattern,
  buildGeneratedStepStub,
  buildStepDefinitionFileContent,
  classifyStepDefinitionStatus,
  parseGherkinStepLine,
  POTENTIALLY_EXPENSIVE_REGEX_RE as STEP_DEFS_RE,
  registerGherkinStepDefinitionSupport,
  scanStepDefinitions,
  suggestStepDefinitionPath,
} from '../gherkinStepDefinitions';
import { POTENTIALLY_EXPENSIVE_REGEX_RE as PROVIDERS_RE } from '../gherkinProviders';

describe('gherkin step definition support', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  test('registers code actions for gherkin documents', () => {
    registerGherkinStepDefinitionSupport();

    expect(vscode.languages.registerCodeActionsProvider).toHaveBeenCalledTimes(1);
    expect((vscode.languages.registerCodeActionsProvider as jest.Mock).mock.calls[0][0]).toEqual([
      { language: 'gherkin' },
    ]);
  });

  test('parses Given/When/Then step lines and skips non-step lines', () => {
    expect(parseGherkinStepLine('    Given a signed-in user', 4)).toEqual({
      keyword: 'Given',
      text: 'a signed-in user',
      line: 4,
      rawLine: '    Given a signed-in user',
    });
    expect(parseGherkinStepLine('Feature: Checkout', 0)).toBeNull();
    expect(parseGherkinStepLine('    * a bullet step', 2)).toBeNull();
  });

  test('builds conservative generated regex patterns', () => {
    expect(buildGeneratedStepPattern('a user exists with name "alice"')).toBe(
      '^a user exists with name "([^"]+)"$'
    );
    expect(buildGeneratedStepPattern('I add <item> to the cart')).toBe(
      '^I add (.+) to the cart$'
    );
    expect(buildGeneratedStepPattern('the total is 19.99')).toBe(
      '^the total is 19\\.99$'
    );
  });

  test('extracts slash-delimited step definitions and flags unsupported forms as ambiguous', () => {
    const supported = scanStepDefinitions([
      'use Test::BDD::Cucumber::StepFile;',
      '',
      'Given qr/^a user exists with name "([^"]+)"$/, sub {',
      '    return;',
      '};',
    ].join('\n'));

    expect(supported.ambiguous).toBe(false);
    expect(supported.definitions).toEqual([
      {
        keyword: 'Given',
        pattern: '^a user exists with name "([^"]+)"$',
      },
    ]);

    const ambiguous = scanStepDefinitions('Then qr{^the total is \\d+$}, sub { return; };');
    expect(ambiguous.ambiguous).toBe(true);
    expect(ambiguous.definitions).toHaveLength(0);
  });

  test('classifies step status conservatively', () => {
    const step = parseGherkinStepLine('Then the total should be "10"', 6);
    expect(step).not.toBeNull();

    expect(classifyStepDefinitionStatus(step!, [
      'Then qr/^the total should be "([^"]+)"$/, sub { return; };',
    ])).toBe('defined');

    expect(classifyStepDefinitionStatus(step!, [
      'Then qr{^the total should be "([^"]+)"$}, sub { return; };',
    ])).toBe('ambiguous');

    expect(classifyStepDefinitionStatus(step!, [
      'Given qr/^some other step$/, sub { return; };',
    ])).toBe('undefined');
  });

  test('treats potentially expensive step regexes as ambiguous', () => {
    const step = parseGherkinStepLine('Then aaaaaaaaaaaaaaaaaaaa!', 1);
    expect(step).not.toBeNull();

    expect(classifyStepDefinitionStatus(step!, [
      'Then qr/^(a+)+!$/, sub { return; };',
    ])).toBe('ambiguous');
  });

  test('treats bounded-quantifier inner group as ambiguous (ReDoS heuristic extension)', () => {
    // ([a-z]{2,5})+ — bounded inner quantifier {m,n} with outer +
    const step1 = parseGherkinStepLine('Then the code is abc', 1);
    expect(step1).not.toBeNull();
    expect(classifyStepDefinitionStatus(step1!, [
      'Then qr/^the code is ([a-z]{2,5})+$/, sub { return; };',
    ])).toBe('ambiguous');

    // (\d{1,3}){4} — bounded inner {m,n} with outer exact-count {n}
    const step2 = parseGherkinStepLine('Then the IP is 192.168.1.1', 1);
    expect(step2).not.toBeNull();
    expect(classifyStepDefinitionStatus(step2!, [
      'Then qr/^the IP is (\\d{1,3}.){4}$/, sub { return; };',
    ])).toBe('ambiguous');

    // (x{5})+ — exact-count inner {m} with outer +
    const step3 = parseGherkinStepLine('Then match xxxxxfoo', 1);
    expect(step3).not.toBeNull();
    expect(classifyStepDefinitionStatus(step3!, [
      'Then qr/^match (x{5})+foo$/, sub { return; };',
    ])).toBe('ambiguous');

    // (x{2,})+ — lower-bound-only {m,} with outer +
    const step4 = parseGherkinStepLine('Then match xxfoo', 1);
    expect(step4).not.toBeNull();
    expect(classifyStepDefinitionStatus(step4!, [
      'Then qr/^match (x{2,})+foo$/, sub { return; };',
    ])).toBe('ambiguous');

    // ([a-z]{2,5}){3,7} — bounded inner, bounded outer {m,n}
    const step5 = parseGherkinStepLine('Then match abcdef', 1);
    expect(step5).not.toBeNull();
    expect(classifyStepDefinitionStatus(step5!, [
      'Then qr/^match ([a-z]{2,5}){3,7}$/, sub { return; };',
    ])).toBe('ambiguous');
  });

  test('does not flag bounded-inner group with no outer quantifier (safe case)', () => {
    // ([a-z]{2,5}) with NO outer quantifier — linear-time, must not be flagged
    const step = parseGherkinStepLine('Then the code is abc', 1);
    expect(step).not.toBeNull();
    expect(classifyStepDefinitionStatus(step!, [
      'Then qr/^the code is ([a-z]{2,5})$/, sub { return; };',
    ])).not.toBe('ambiguous');

    // Non-numeric brace like (a{b})+ must not be flagged (not a valid quantifier)
    const step2 = parseGherkinStepLine('Then test xb', 1);
    expect(step2).not.toBeNull();
    expect(classifyStepDefinitionStatus(step2!, [
      'Then qr/^test (a{b})+$/, sub { return; };',
    ])).not.toBe('ambiguous');
  });

  test('POTENTIALLY_EXPENSIVE_REGEX_RE is identical in gherkinProviders and gherkinStepDefinitions (parity guard)', () => {
    expect(STEP_DEFS_RE.toString()).toBe(PROVIDERS_RE.toString());
  });

  test('does not treat named-capture groups as expensive (no false positive)', () => {
    const step = parseGherkinStepLine('Then I have 5 items in the cart', 1);
    expect(step).not.toBeNull();

    // Named captures (?<name>...) are safe and must not be blocked
    const result = classifyStepDefinitionStatus(step!, [
      'Then qr/I have (?<count>\\d+) items in the cart/, sub { return; };',
    ]);
    expect(result).not.toBe('ambiguous');
  });

  test('does not treat a single char-class quantifier as expensive (no false positive)', () => {
    // Regression for #859: `[0-9]+` / `[^"]+` are linear-time and safe. The
    // ReDoS guard previously flagged any character class followed by a
    // quantifier, misclassifying ordinary step definitions as `ambiguous`.
    const numericStep = parseGherkinStepLine('Then the total is 19', 1);
    expect(numericStep).not.toBeNull();
    expect(classifyStepDefinitionStatus(numericStep!, [
      'Then qr/^the total is [0-9]+$/, sub { return; };',
    ])).toBe('defined');

    const quotedStep = parseGherkinStepLine('Then the name is "alice"', 1);
    expect(quotedStep).not.toBeNull();
    expect(classifyStepDefinitionStatus(quotedStep!, [
      'Then qr/^the name is "([^"]+)"$/, sub { return; };',
    ])).toBe('defined');
  });

  test('suggests a deterministic feature-relative target file', () => {
    expect(
      suggestStepDefinitionPath(
        path.join('/workspace', 'features', 'checkout.feature'),
        '/workspace'
      )
    ).toBe(path.join('/workspace', 'features', 'step_definitions', 'checkout_steps.pm'));

    expect(
      suggestStepDefinitionPath(
        path.join('/workspace', 'spec', 'features', 'login.feature'),
        '/workspace'
      )
    ).toBe(path.join('/workspace', 'spec', 'features', 'step_definitions', 'login_steps.pm'));
  });

  test('builds new-file step definition content with boilerplate and TODO stub', () => {
    const step = parseGherkinStepLine('When I add <item> to the cart', 8);
    expect(step).not.toBeNull();

    const stub = buildGeneratedStepStub(step!, 'features/checkout.feature');
    expect(stub).toContain('# Auto-generated from features/checkout.feature:9');
    expect(stub).toContain('When qr/^I add (.+) to the cart$/, sub {');
    expect(stub).toContain('# TODO: implement step');

    const content = buildStepDefinitionFileContent(step!, 'features/checkout.feature');
    expect(content).toContain('use Test::BDD::Cucumber::StepFile;');
    expect(content).toContain('use strict;');
    expect(content).toContain('use warnings;');
    expect(content).toContain('When qr/^I add (.+) to the cart$/, sub {');
  });
});
