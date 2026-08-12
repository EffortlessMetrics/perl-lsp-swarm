const assert = require('node:assert/strict');
const { test } = require('node:test');
const {
  ensureDistinctBase,
  evaluateTransition,
  notProvenReceipt,
  parseDeclarationDocument,
  parseInventoryDocument,
  projectInventory,
  semanticInventorySha256,
} = require('./check-vsix-inventory-transition');

function inventory(files, extra = {}) {
  return {
    schema_version: 1,
    total_files: Object.keys(files).length,
    total_bytes: Object.values(files).reduce((total, bytes) => total + bytes, 0),
    files,
    ...extra,
  };
}

function document(files) {
  const value = inventory(files);
  return parseInventoryDocument(`${JSON.stringify(value, null, 2)}\n`, 'fixture baseline');
}

function declaration(baseDocument, candidateDocument) {
  return {
    schema_version: 1,
    owner_issue: 7041,
    reason: 'Accept the exact candidate package inventory transition.',
    base_baseline_file_sha256: baseDocument.file_sha256,
    candidate_baseline_file_sha256: candidateDocument.file_sha256,
    base_inventory_sha256: baseDocument.inventory_sha256,
    candidate_inventory_sha256: candidateDocument.inventory_sha256,
  };
}

void test('accepts a canonical closed baseline document', () => {
  const parsed = document({ 'README.md': 2, 'out/extension.js': 8 });
  assert.equal(parsed.value.total_bytes, 10);
  assert.match(parsed.file_sha256, /^[0-9a-f]{64}$/);
  assert.match(parsed.inventory_sha256, /^[0-9a-f]{64}$/);
});

void test('rejects unsupported baseline fields and totals drift', () => {
  const withExtra = inventory({ 'README.md': 2 }, { policy: 'hidden' });
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(withExtra, null, 2)}\n`, 'extra fixture'),
    /unsupported fields/,
  );

  const wrongTotal = inventory({ 'README.md': 2 });
  wrongTotal.total_bytes = 99;
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(wrongTotal, null, 2)}\n`, 'total fixture'),
    /files sum to 2/,
  );
});

void test('rejects unsafe integers and non-canonical package paths', () => {
  const unsafe = inventory({ 'README.md': Number.MAX_SAFE_INTEGER + 1 });
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(unsafe, null, 2)}\n`, 'unsafe fixture'),
    /non-negative safe integer/,
  );

  const escaping = inventory({ '../README.md': 2 });
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(escaping, null, 2)}\n`, 'path fixture'),
    /canonical relative package path/,
  );
});

void test('rejects non-canonical bytes and duplicate JSON keys', () => {
  const compact = JSON.stringify(inventory({ 'README.md': 2 }));
  assert.throws(() => parseInventoryDocument(compact, 'compact fixture'), /canonical two-space/);

  const duplicate = `{
  "schema_version": 1,
  "total_files": 1,
  "total_bytes": 2,
  "files": {
    "README.md": 1,
    "README.md": 2
  }
}\n`;
  assert.throws(() => parseInventoryDocument(duplicate, 'duplicate fixture'), /canonical two-space/);
});

void test('requires a closed canonical transition declaration', () => {
  const baseDocument = document({ 'README.md': 2 });
  const candidateDocument = document({ 'README.md': 3 });
  const value = declaration(baseDocument, candidateDocument);
  const parsed = parseDeclarationDocument(`${JSON.stringify(value, null, 2)}\n`, 'declaration');
  assert.equal(parsed.owner_issue, 7041);

  const extra = { ...value, hidden: true };
  assert.throws(
    () => parseDeclarationDocument(`${JSON.stringify(extra, null, 2)}\n`, 'declaration'),
    /unsupported fields/,
  );
});

void test('accepts an unchanged exact package', () => {
  const baseline = document({ 'README.md': 2, 'out/extension.js': 8 });
  const result = evaluateTransition({
    actual: baseline.value,
    baseDocument: baseline,
    candidateDocument: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'no_change');
  assert.equal(result.passed, true);
  assert.equal(result.behavior_safe, true);
  assert.equal(result.package_policy_class, 'pass');
});

void test('keeps a size-only transition red while allowing behavioral smoke', () => {
  const baseline = document({ 'README.md': 2, 'out/extension.js': 8 });
  const actual = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual,
    baseDocument: baseline,
    candidateDocument: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_required');
  assert.equal(result.passed, false);
  assert.equal(result.package_policy_class, 'size_only');
  assert.equal(result.behavior_safe, true);
});

void test('does not execute a structural package transition', () => {
  const baseline = document({ 'README.md': 2, 'out/extension.js': 8 });
  const actual = inventory({ 'README.md': 2, 'new.js': 8 });
  const result = evaluateTransition({
    actual,
    baseDocument: baseline,
    candidateDocument: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_required');
  assert.equal(result.package_policy_class, 'structural');
  assert.equal(result.behavior_safe, false);
});

void test('preserves behavior evidence for an undeclared but structurally safe baseline move', () => {
  const baseDocument = document({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateDocument = document({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual: candidateDocument.value,
    baseDocument,
    candidateDocument,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'undeclared_transition');
  assert.equal(result.passed, false);
  assert.equal(result.package_policy_class, 'pass');
  assert.equal(result.behavior_safe, true);
});

void test('rejects a candidate baseline that does not match the produced package', () => {
  const baseDocument = document({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateDocument = document({ 'README.md': 2, 'out/extension.js': 10 });
  const actual = inventory({ 'README.md': 2, 'out/extension.js': 9 });
  const result = evaluateTransition({
    actual,
    baseDocument,
    candidateDocument,
    declaration: declaration(baseDocument, candidateDocument),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'invalid_baseline_update');
  assert.equal(result.behavior_safe, false);
});

void test('accepts a declared exact candidate transition', () => {
  const baseDocument = document({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateDocument = document({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual: candidateDocument.value,
    baseDocument,
    candidateDocument,
    declaration: declaration(baseDocument, candidateDocument),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_candidate');
  assert.equal(result.passed, true);
  assert.equal(result.behavior_safe, true);
  assert.deepEqual(result.delta.changed, [
    { file: 'out/extension.js', before: 8, after: 10, delta: 2 },
  ]);
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

void test('semantic digests ignore source object insertion order', () => {
  assert.equal(
    semanticInventorySha256(inventory({ b: 2, a: 1 })),
    semanticInventorySha256(inventory({ a: 1, b: 2 })),
  );
});

void test('rejects a requested base that resolves to the candidate', () => {
  assert.throws(
    () => ensureDistinctBase('a'.repeat(40), 'a'.repeat(40), 'manual base'),
    /candidate itself/,
  );
});

void test('instrument failures produce a bounded not-proven receipt', () => {
  const receipt = notProvenReceipt({
    candidateSha: 'a'.repeat(40),
    baseSha: null,
    reason: new Error('git show failed\nwith details'),
  });
  assert.equal(receipt.state, 'not_proven');
  assert.equal(receipt.passed, false);
  assert.equal(receipt.behavior_safe, false);
  assert.equal(receipt.package_policy_class, 'not_proven');
  assert.equal(receipt.reason, 'git show failed with details');
});
