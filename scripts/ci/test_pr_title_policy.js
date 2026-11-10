'use strict';

// Falsifiers for the shared PR-title policy (#18255).
//
// Run with: node scripts/ci/test_pr_title_policy.js

const assert = require('node:assert');
const { validateTitle } = require('./pr-title-policy.js');

const cases = [
  {
    name: 'dependabot is exempt',
    pr: { title: 'chore: bump thing', user: { login: 'dependabot[bot]' }, labels: [] },
    expect: { ok: true, kind: 'exempt' },
  },
  {
    name: 'skip-title-check is exempt',
    pr: { title: 'wip', user: { login: 'alice' }, labels: [{ name: 'skip-title-check' }] },
    expect: { ok: true, kind: 'exempt' },
  },
  {
    name: 'release stamp is exempt',
    pr: { title: 'v1.2.3', user: { login: 'alice' }, labels: [] },
    expect: { ok: true, kind: 'exempt' },
  },
  {
    name: 'missing issue reference fails',
    pr: { title: 'feat: add thing', user: { login: 'alice' }, labels: [] },
    expect: { ok: false, kind: 'missing-issue' },
  },
  {
    name: 'real issue reference passes',
    pr: { title: 'feat: add thing (#42)', user: { login: 'alice' }, labels: [] },
    expect: { ok: true, kind: 'issue', issueNumber: 42 },
  },
  {
    name: 'placeholder #0000 passes non-blocking',
    pr: { title: 'feat: add thing (#0000)', user: { login: 'alice' }, labels: [] },
    expect: { ok: true, kind: 'placeholder' },
  },
  {
    name: 'placeholder plus real issue uses the real issue',
    pr: { title: 'feat: add thing (#0000) (#42)', user: { login: 'alice' }, labels: [] },
    expect: { ok: true, kind: 'issue', issueNumber: 42 },
  },
  {
    name: 'version-like text is not an issue reference',
    pr: { title: 'release 1.2.3', user: { login: 'alice' }, labels: [] },
    expect: { ok: false, kind: 'missing-issue' },
  },
];

let failed = 0;
for (const testCase of cases) {
  const verdict = validateTitle(testCase.pr);
  try {
    assert.strictEqual(verdict.ok, testCase.expect.ok, `${testCase.name}: ok`);
    assert.strictEqual(verdict.kind, testCase.expect.kind, `${testCase.name}: kind`);
    if (testCase.expect.issueNumber !== undefined) {
      assert.strictEqual(verdict.issueNumber, testCase.expect.issueNumber, `${testCase.name}: issueNumber`);
    }
    console.log(`ok - ${testCase.name}`);
  } catch (error) {
    failed += 1;
    console.error(`not ok - ${testCase.name}: ${error.message}`);
  }
}

if (failed > 0) {
  console.error(`${failed} policy case(s) failed`);
  process.exit(1);
}
console.log(`${cases.length} policy cases passed`);
