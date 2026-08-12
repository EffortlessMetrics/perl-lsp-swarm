const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const {
  bundleTargetForPlatform,
  computeOverallStatus,
  shouldRunBehavioralSmoke,
  stageServerForPackage,
  writeJsonAtomic,
} = require('./run-local-vsix-smoke');

void test('stages and restores the current platform server for packaging', () => {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-smoke-'));
  const sourcePath = path.join(extensionRoot, 'perllsp-source');
  fs.writeFileSync(sourcePath, 'current source server');

  try {
    const restore = stageServerForPackage(sourcePath, extensionRoot);
    const target = bundleTargetForPlatform();
    const destination = path.join(extensionRoot, 'bin', target.directory, target.binaryName);
    assert.equal(fs.readFileSync(destination, 'utf8'), 'current source server');
    restore();
    assert.equal(fs.existsSync(destination), false);
  } finally {
    fs.rmSync(extensionRoot, { recursive: true, force: true });
  }
});

void test('restores an existing packaged server after staging', () => {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-smoke-'));
  const sourcePath = path.join(extensionRoot, 'perllsp-source');
  const target = bundleTargetForPlatform();
  const destination = path.join(extensionRoot, 'bin', target.directory, target.binaryName);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(sourcePath, 'current source server');
  fs.writeFileSync(destination, 'bundled server');
  fs.chmodSync(destination, 0o640);
  const originalMode = fs.statSync(destination).mode & 0o777;

  try {
    const restore = stageServerForPackage(sourcePath, extensionRoot);
    assert.equal(fs.readFileSync(destination, 'utf8'), 'current source server');
    restore();
    assert.equal(fs.readFileSync(destination, 'utf8'), 'bundled server');
    assert.equal(fs.statSync(destination).mode & 0o777, originalMode);
  } finally {
    fs.rmSync(extensionRoot, { recursive: true, force: true });
  }
});

void test('cleans failed staging after creating the platform directory', () => {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-smoke-'));
  const missingSource = path.join(extensionRoot, 'missing-server');
  const target = bundleTargetForPlatform();
  const platformRoot = path.join(extensionRoot, 'bin', target.directory);

  try {
    assert.throws(() => stageServerForPackage(missingSource, extensionRoot), /ENOENT/);
    assert.equal(fs.existsSync(platformRoot), false);
    assert.equal(fs.existsSync(path.join(extensionRoot, 'bin')), false);
  } finally {
    fs.rmSync(extensionRoot, { recursive: true, force: true });
  }
});

void test('runs behavioral smoke after a size-only inventory rejection', () => {
  assert.equal(
    shouldRunBehavioralSmoke({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'failed', classification: 'size_only' },
      behavioral_smoke: { status: 'not_run' },
    }),
    true,
  );
});

void test('does not execute an unsafe structural package', () => {
  assert.equal(
    shouldRunBehavioralSmoke({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'failed', classification: 'structural' },
      behavioral_smoke: { status: 'not_run' },
    }),
    false,
  );
});

void test('keeps aggregate failure when behavior passes after size-only rejection', () => {
  assert.equal(
    computeOverallStatus({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'failed', classification: 'size_only' },
      behavioral_smoke: { status: 'pass' },
    }),
    'failed',
  );
});

void test('reports not-proven rather than pass when behavior did not run', () => {
  assert.equal(
    computeOverallStatus({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'pass', classification: 'pass' },
      behavioral_smoke: { status: 'not_run', reason: 'instrument_failure' },
    }),
    'not_proven',
  );
});

void test('writes a complete receipt through an atomic replacement', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-receipt-'));
  const destination = path.join(directory, 'receipt.json');
  try {
    writeJsonAtomic(destination, { schema_version: 'test.v1', result: 'pass' });
    assert.deepEqual(JSON.parse(fs.readFileSync(destination, 'utf8')), {
      schema_version: 'test.v1',
      result: 'pass',
    });
    assert.deepEqual(fs.readdirSync(directory), ['receipt.json']);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
