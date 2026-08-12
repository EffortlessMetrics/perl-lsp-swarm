const assert = require('node:assert/strict');
const { test } = require('node:test');
const {
  evaluateTransition,
  projectInventory,
  sha256Inventory,
  validateDeclaration,
} = require('./check-vsix-inventory-transition');

function inventory(files) {
  return {
    schema_version: 1,
    total_files: Object.keys(files).length,
    total_bytes: Object.values(files).reduce((total, bytes) => total + bytes, 0),
    files,
  };
}

function declaration(baseBaseline, candidateBaseline) {
  return {
    schema_version: 1,
    owner_issue: 6525,
    reason: 'Accept the measured candidate package inventory transition.',
    base_baseline_sha256: sha256Inventory(baseBaseline),
    candidate_baseline_sha256: sha256Inventory(candidateBaseline),
  };
}

void test('accepts a package whose exact inventory matches the unchanged baseline', () => {
  const baseline = inventory({ 'README.md': 2, 'out/extension.js': 8 });
  const result = evaluateTransition({
    actual: baseline,
    baseBaseline: baseline,
    candidateBaseline: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'no_change');
  assert.equal(result.passed, true);
});

void test('requires a transition when package output changes without a baseline update', () => {
  const baseline = inventory({ 'README.md': 2, 'out/extension.js': 8 });
  const actual = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual,
    baseBaseline: baseline,
    candidateBaseline: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_required');
  assert.equal(result.passed, false);
  assert.equal(result.delta.changed.length, 0);
});

void test('rejects an exact baseline update without a transition declaration', () => {
  const baseBaseline = inventory({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateBaseline = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual: candidateBaseline,
    baseBaseline,
    candidateBaseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'undeclared_transition');
  assert.equal(result.passed, false);
  assert.match(result.declaration_violations[0], /without scripts\/vsix-inventory-transition/);
});

void test('accepts a declared candidate-bound baseline transition', () => {
  const baseBaseline = inventory({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateBaseline = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual: candidateBaseline,
    baseBaseline,
    candidateBaseline,
    declaration: declaration(baseBaseline, candidateBaseline),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_candidate');
  assert.equal(result.passed, true);
  assert.deepEqual(result.delta.changed, [
    { file: 'out/extension.js', before: 8, after: 10, delta: 2 },
  ]);
});

void test('rejects a baseline update that does not match the produced package', () => {
  const baseBaseline = inventory({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateBaseline = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const actual = inventory({ 'README.md': 2, 'out/extension.js': 9 });
  const result = evaluateTransition({
    actual,
    baseBaseline,
    candidateBaseline,
    declaration: declaration(baseBaseline, candidateBaseline),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'invalid_baseline_update');
  assert.equal(result.passed, false);
});

void test('rejects a declaration copied from another baseline generation', () => {
  const baseBaseline = inventory({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateBaseline = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const invalid = declaration(baseBaseline, candidateBaseline);
  invalid.base_baseline_sha256 = '0'.repeat(64);

  assert.deepEqual(
    validateDeclaration(
      invalid,
      sha256Inventory(baseBaseline),
      sha256Inventory(candidateBaseline),
    ),
    ['transition declaration base_baseline_sha256 does not match the selected base'],
  );
});

void test('ignores the staged current-source server while retaining ordinary package files', () => {
  const projected = projectInventory(
    inventory({
      'README.md': 2,
      'out/extension.js': 8,
      'bin/linux-x64/perllsp': 100,
      'bin/win32-x64/perllsp.exe': 200,
    }),
    'linux',
    'x64',
    ['bin/linux-x64/perllsp'],
  );

  assert.deepEqual(projected.files, { 'README.md': 2, 'out/extension.js': 8 });
});
