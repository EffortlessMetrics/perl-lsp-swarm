const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const {
  bundleTargetForPlatform,
  computeOverallStatus,
  finalizeSmokeRun,
  interpretTransitionResult,
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

void test('runs behavioral smoke when a size-only policy result remains red', () => {
  assert.equal(
    shouldRunBehavioralSmoke({
      package_creation: { status: 'pass' },
      package_inventory: {
        status: 'failed',
        classification: 'size_only',
        behavior_safe: true,
      },
      behavioral_smoke: { status: 'not_run' },
    }),
    true,
  );
});

void test('runs behavioral smoke for an undeclared but structurally safe transition', () => {
  assert.equal(
    shouldRunBehavioralSmoke({
      package_creation: { status: 'pass' },
      package_inventory: {
        status: 'failed',
        classification: 'pass',
        behavior_safe: true,
        transition_state: 'undeclared_transition',
      },
      behavioral_smoke: { status: 'not_run' },
    }),
    true,
  );
});

void test('does not execute a structural or not-proven package', () => {
  for (const stage of [
    { status: 'failed', classification: 'structural', behavior_safe: false },
    { status: 'not_proven', classification: 'not_proven', behavior_safe: false },
  ]) {
    assert.equal(
      shouldRunBehavioralSmoke({
        package_creation: { status: 'pass' },
        package_inventory: stage,
        behavioral_smoke: { status: 'not_run' },
      }),
      false,
    );
  }
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

function transitionReport(overrides = {}) {
  return {
    schema_version: 'vsix_inventory_transition.v1',
    receipt_kind: 'vsix_inventory_transition',
    candidate_sha: 'a'.repeat(40),
    base_sha: 'b'.repeat(40),
    platform: process.platform,
    architecture: process.arch,
    state: 'transition_required',
    passed: false,
    behavior_safe: true,
    package_policy_class: 'size_only',
    policy_violations: ['file out/extension.js grew from 8 to 10 bytes'],
    declaration_violations: [],
    ...overrides,
  };
}

void test('maps size-only transition policy red to a safe failed inventory stage', () => {
  const stage = interpretTransitionResult(
    { status: 1, stdout: JSON.stringify(transitionReport()), stderr: '', error: null },
    'a'.repeat(40),
  );
  assert.equal(stage.status, 'failed');
  assert.equal(stage.classification, 'size_only');
  assert.equal(stage.behavior_safe, true);
  assert.equal(stage.transition_state, 'transition_required');
});

void test('rejects a transition receipt for another candidate', () => {
  const stage = interpretTransitionResult(
    {
      status: 1,
      stdout: JSON.stringify(transitionReport({ candidate_sha: 'c'.repeat(40) })),
      stderr: '',
      error: null,
    },
    'a'.repeat(40),
  );
  assert.equal(stage.status, 'not_proven');
  assert.equal(stage.behavior_safe, false);
  assert.match(stage.reason, /candidate SHA/);
});

void test('preserves typed not-proven transition failures', () => {
  const stage = interpretTransitionResult(
    {
      status: 2,
      stdout: JSON.stringify(
        transitionReport({
          state: 'not_proven',
          passed: false,
          behavior_safe: false,
          package_policy_class: 'not_proven',
          reason: 'unable to resolve base',
          policy_violations: [],
        }),
      ),
      stderr: '',
      error: null,
    },
    'a'.repeat(40),
  );
  assert.equal(stage.status, 'not_proven');
  assert.equal(stage.classification, 'not_proven');
  assert.equal(stage.behavior_safe, false);
  assert.equal(stage.reason, 'unable to resolve base');
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

function passingReceipt() {
  return {
    stages: {
      package_creation: { status: 'pass' },
      package_inventory: { status: 'pass', classification: 'pass', behavior_safe: true },
      behavioral_smoke: { status: 'pass' },
    },
    instrument_failure: null,
    cleanup_failure: null,
    overall: 'pass',
  };
}

void test('a VSIX deletion failure is persisted and changes the final exit code', () => {
  const receipt = passingReceipt();
  let persisted;

  const exitCode = finalizeSmokeRun(
    '/receipt.json',
    receipt,
    '/extension.vsix',
    () => {},
    () => {
      throw new Error('cannot delete VSIX');
    },
    (_destination, value) => {
      value.overall = computeOverallStatus(
        value.stages,
        value.instrument_failure,
        value.cleanup_failure,
      );
      persisted = structuredClone(value);
    },
  );

  assert.equal(exitCode, 2);
  assert.equal(persisted.overall, 'not_proven');
  assert.deepEqual(persisted.cleanup_failure, { vsix_deletion: 'cannot delete VSIX' });
});

void test('a staged-server restoration failure changes the receipt and exit code', () => {
  const receipt = passingReceipt();
  let persisted;

  const exitCode = finalizeSmokeRun(
    '/receipt.json',
    receipt,
    '/extension.vsix',
    () => {
      throw new Error('cannot restore server');
    },
    () => {},
    (_destination, value) => {
      value.overall = computeOverallStatus(
        value.stages,
        value.instrument_failure,
        value.cleanup_failure,
      );
      persisted = structuredClone(value);
    },
  );

  assert.equal(exitCode, 2);
  assert.equal(persisted.overall, 'not_proven');
  assert.deepEqual(persisted.cleanup_failure, {
    staged_server_restoration: 'cannot restore server',
  });
});

void test('independent cleanup failures are accumulated before receipt persistence', () => {
  const receipt = passingReceipt();
  let persisted;

  const exitCode = finalizeSmokeRun(
    '/receipt.json',
    receipt,
    '/extension.vsix',
    () => {
      throw new Error('restore failed');
    },
    () => {
      throw new Error('delete failed');
    },
    (_destination, value) => {
      value.overall = computeOverallStatus(
        value.stages,
        value.instrument_failure,
        value.cleanup_failure,
      );
      persisted = structuredClone(value);
    },
  );

  assert.equal(exitCode, 2);
  assert.deepEqual(persisted.cleanup_failure, {
    vsix_deletion: 'delete failed',
    staged_server_restoration: 'restore failed',
  });
});
