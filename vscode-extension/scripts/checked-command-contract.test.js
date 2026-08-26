'use strict';

const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');
const {
  evaluateCheckedCommandContract,
  checkCheckedCommandContract,
} = require('./checked-command-contract');

const EXTENSION_ROOT = path.resolve(__dirname, '..');

/**
 * The real script map, so each negative case differs from a healthy contract
 * by exactly one deliberate drift.
 *
 * @returns {Record<string, string>}
 */
function realScripts() {
  const packageJson = require(path.join(EXTENSION_ROOT, 'package.json'));
  return { ...packageJson.scripts };
}

/**
 * @param {Record<string, string>} scripts
 * @returns {{ok: boolean, failures: string[], facts: string[]}}
 */
function evaluate(scripts) {
  return evaluateCheckedCommandContract({ scripts });
}

/**
 * @param {{ok: boolean, failures: string[], facts: string[]}} result
 * @param {RegExp} pattern
 */
function assertFailedWith(result, pattern) {
  assert.equal(result.ok, false, `expected a red result, got: ${JSON.stringify(result)}`);
  assert.ok(
    result.failures.some((failure) => pattern.test(failure)),
    `no failure matched ${String(pattern)}; failures were ${JSON.stringify(result.failures)}`,
  );
}

void test('the real extension tree satisfies the checked command contract', () => {
  const result = checkCheckedCommandContract(EXTENSION_ROOT);
  assert.deepEqual(result.failures, [], JSON.stringify(result.failures));
  assert.equal(result.ok, true);
  assert.ok(result.facts.length > 0);
});

void test('the contract enumerates the public TypeScript execution paths', () => {
  const result = evaluate(realScripts());
  const governedFact = result.facts.find((fact) => fact.includes('invoke scripts/governed-tsc.js'));
  assert.ok(
    governedFact,
    `facts must enumerate direct seam users: ${JSON.stringify(result.facts)}`,
  );
  for (const name of [
    'typecheck',
    'typecheck:test',
    'typecheck:integration',
    'typecheck:published',
    'typecheck:scripts',
    'watch:types',
    'compile:test',
    'test:integration',
    'test:published',
  ]) {
    assert.ok(
      governedFact.includes(name),
      `${name} must be enumerated as a governed TypeScript path`,
    );
  }
  const aggregateFact = result.facts.find((fact) => fact.includes('compose governed subcommands'));
  assert.ok(aggregateFact, 'aggregates that only compose governed subcommands are enumerated');
  for (const name of ['build', 'test', 'test:ci', 'vscode:prepublish']) {
    assert.ok(aggregateFact.includes(name), `${name} must be enumerated as a governed aggregate`);
  }
});

void test('a bare tsc script is red and the failure names the script', () => {
  const scripts = realScripts();
  scripts['typecheck'] = 'tsc --noEmit -p ./tsconfig.json';
  assertFailedWith(evaluate(scripts), /script "typecheck" .* executes TypeScript outside/);
});

void test('a bare tsc watch bypass is red and names watch:types', () => {
  const scripts = realScripts();
  scripts['watch:types'] = 'tsc -watch --noEmit -p ./tsconfig.json';
  const result = evaluate(scripts);
  assertFailedWith(result, /executes TypeScript outside scripts\/governed-tsc\.js/);
  assertFailedWith(result, /watch:types/);
});

void test('npx and npm-exec tsc routes are red', () => {
  for (const bypass of [
    'npx tsc --noEmit -p ./tsconfig.json',
    'npm exec --no -- tsc --noEmit -p ./tsconfig.json',
    'node_modules/.bin/tsc --noEmit -p ./tsconfig.json',
  ]) {
    const scripts = realScripts();
    scripts['typecheck'] = bypass;
    assertFailedWith(evaluate(scripts), /script "typecheck" .* executes TypeScript outside/);
  }
});

void test('shell separators glued to an npm-run stage cannot smuggle tsc through', () => {
  for (const bypass of [
    // No whitespace after the script name: only the tightened trailing-text
    // and tsc-token rules see the second command.
    'npm run typecheck:test;tsc --noEmit -p ./tsconfig.json',
    'npm run typecheck:test||tsc -p ./tsconfig.json',
    'npm run typecheck:test&tsc -p ./tsconfig.json',
    // Windows shim spellings with backslashes and wrapper extensions.
    'node_modules\\.bin\\tsc.cmd --noEmit -p ./tsconfig.json',
    'node_modules/.bin/tsc.ps1 --noEmit -p ./tsconfig.json',
  ]) {
    const scripts = realScripts();
    scripts['typecheck'] = bypass;
    const result = evaluate(scripts);
    assertFailedWith(result, /script "typecheck"/);
    assert.ok(
      result.failures.some(
        (failure) =>
          /executes TypeScript outside/.test(failure) ||
          /composes `npm run` with additional shell text/.test(failure),
      ),
      `the glued bypass must be classified as ungoverned or unprovable: ${JSON.stringify(result.failures)}`,
    );
  }
});

void test('an npm-run cycle is red and names the cycle', () => {
  const scripts = realScripts();
  scripts['loop:a'] = 'npm run loop:b';
  scripts['loop:b'] = 'npm run loop:a';
  assertFailedWith(evaluate(scripts), /recursive npm-run cycle:.*loop:a -> loop:b -> loop:a/);
});

void test('a dangling npm-run reference is red', () => {
  const scripts = realScripts();
  scripts['typecheck'] = 'npm run typecheck:missing';
  assertFailedWith(evaluate(scripts), /unknown npm script "typecheck:missing"/);
});

void test('shell text after npm run is red — ordering would not be provable', () => {
  const scripts = realScripts();
  scripts['typecheck:all'] = 'npm run typecheck & npm run typecheck:test';
  assertFailedWith(evaluate(scripts), /composes `npm run` with additional shell text/);
});

void test('build must start with the authority gate', () => {
  const scripts = realScripts();
  scripts['build'] = 'npm run check:tsconfig-inventory && npm run typecheck:all && npm run bundle';
  assertFailedWith(evaluate(scripts), /"build" must be exactly `npm run typecheck:authority/);
});

void test('build without the config inventory stage is red', () => {
  const scripts = realScripts();
  scripts['build'] = 'npm run typecheck:authority && npm run typecheck:all && npm run bundle';
  assertFailedWith(evaluate(scripts), /"build" must be exactly `npm run typecheck:authority/);
});

void test('build that type-checks before the config inventory gate is red', () => {
  const scripts = realScripts();
  scripts['build'] =
    'npm run typecheck:authority && npm run typecheck:all && npm run check:tsconfig-inventory && npm run bundle';
  assertFailedWith(evaluate(scripts), /"build" must be exactly/);
  assertFailedWith(
    evaluate(scripts),
    /received `npm run typecheck:authority && npm run typecheck:all/,
  );
});

void test('build with bundling before the type-check is red', () => {
  const scripts = realScripts();
  scripts['build'] =
    'npm run typecheck:authority && npm run bundle && npm run check:tsconfig-inventory && npm run typecheck:all';
  assertFailedWith(evaluate(scripts), /"build" must be exactly/);
  assertFailedWith(evaluate(scripts), /received `npm run typecheck:authority && npm run bundle/);
});

void test('build whose bundle is not final is red — a bundle failure must fail the build', () => {
  const scripts = realScripts();
  scripts['build'] =
    'npm run typecheck:authority && npm run check:tsconfig-inventory && npm run typecheck:all && npm run bundle && npm run clean:out';
  assertFailedWith(evaluate(scripts), /"build" must be exactly/);
});

void test('build with an extra middle stage is red', () => {
  const scripts = realScripts();
  scripts['build'] =
    'npm run typecheck:authority && npm run check:tsconfig-inventory && npm run lint && npm run typecheck:all && npm run bundle';
  assertFailedWith(evaluate(scripts), /"build" must be exactly/);
});

void test('a bundle command that type-checks is red — bundle means Rolldown only', () => {
  const scripts = realScripts();
  scripts['bundle'] = 'npm run clean:out && npm run typecheck && rolldown -c rolldown.config.mjs';
  assertFailedWith(evaluate(scripts), /"bundle" must be exactly/);
  assertFailedWith(evaluate(scripts), /"bundle" executes TypeScript/);
});

void test('a bundle command that is not Rolldown is red', () => {
  const scripts = realScripts();
  scripts['bundle'] = 'npm run clean:out && npm run compile:test';
  assertFailedWith(evaluate(scripts), /"bundle" must be exactly/);
  assertFailedWith(evaluate(scripts), /"bundle" executes TypeScript/);
});

void test('a bundle command with an extra post-bundle stage is red', () => {
  const scripts = realScripts();
  scripts['bundle'] =
    'npm run clean:out && rolldown -c rolldown.config.mjs && node scripts/publish.js';
  assertFailedWith(evaluate(scripts), /"bundle" must be exactly/);
});

void test('watch:bundle without --watch is red', () => {
  const scripts = realScripts();
  scripts['watch:bundle'] = 'npm run clean:out && rolldown -c rolldown.config.mjs';
  assertFailedWith(evaluate(scripts), /"watch:bundle" must be exactly/);
});

void test('watch:types watching a non-source config is red', () => {
  const scripts = realScripts();
  scripts['watch:types'] = 'node scripts/governed-tsc.js -watch --noEmit -p ./tsconfig.test.json';
  assertFailedWith(
    evaluate(scripts),
    /"watch:types" must watch the source config \.\/tsconfig\.json/,
  );
});

void test('a missing contract script is red by name', () => {
  for (const name of ['build', 'bundle', 'watch:bundle', 'watch:types']) {
    const scripts = realScripts();
    delete scripts[name];
    assertFailedWith(evaluate(scripts), new RegExp(`"${name}" is missing`));
  }
});
