const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const {
  activationFailureLegEnv,
  bundleTargetForPlatform,
  composeActivationRecoveryReceipt,
  computeOverallStatus,
  finalizeSmokeRun,
  interpretTransitionResult,
  shouldRunBehavioralSmoke,
  stageServerForPackage,
  validateActivationRecoveryChildReceipts,
  validateChildSmokeReceipt,
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

void test('maps a proven transition to a passing inventory stage', () => {
  const stage = interpretTransitionResult(
    {
      status: 0,
      stdout: JSON.stringify(
        transitionReport({
          state: 'transition_candidate',
          passed: true,
          package_policy_class: 'pass',
          behavior_safe: true,
          policy_violations: [],
        }),
      ),
      stderr: '',
      error: null,
    },
    'a'.repeat(40),
  );
  assert.equal(stage.status, 'pass');
  assert.equal(stage.classification, 'pass');
  assert.equal(stage.behavior_safe, true);
  assert.equal(stage.exit_code, 0);
});

void test('a contradictory zero-exit non-pass classification is not proven', () => {
  // exit 0 with passed:true but a size_only classification satisfies no known
  // branch and must fall to the catch-all rather than being promoted.
  const stage = interpretTransitionResult(
    {
      status: 0,
      stdout: JSON.stringify(
        transitionReport({
          state: 'transition_candidate',
          passed: true,
          package_policy_class: 'size_only',
          behavior_safe: true,
          policy_violations: [],
        }),
      ),
      stderr: '',
      error: null,
    },
    'a'.repeat(40),
  );
  assert.equal(stage.status, 'not_proven');
  assert.equal(stage.behavior_safe, false);
});

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

void test('the orchestrator reads the child receipt where the child writes it', () => {
  // The extension-host child nests by source label and platform
  // (firstHourReceipt.test.ts receiptsDir). A flat lookup here silently turns
  // every hosted run not_proven — the 0490e2962 smoke failure.
  const { childReceiptPath } = require('./run-local-vsix-smoke');
  const priorRoot = process.env.PERL_LSP_SMOKE_RECEIPTS_DIR;
  const priorLabel = process.env.PERL_LSP_SMOKE_SOURCE_LABEL;
  process.env.PERL_LSP_SMOKE_RECEIPTS_DIR = '/fixture/receipts-root';
  process.env.PERL_LSP_SMOKE_SOURCE_LABEL = 'unique-test-label';
  try {
    const platformLeg =
      { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform] ?? process.platform;
    assert.equal(
      childReceiptPath(),
      path.join(
        '/fixture/receipts-root',
        'unique-test-label',
        platformLeg,
        'first_hour_vscode_receipt.json',
      ),
    );
  } finally {
    if (priorRoot === undefined) delete process.env.PERL_LSP_SMOKE_RECEIPTS_DIR;
    else process.env.PERL_LSP_SMOKE_RECEIPTS_DIR = priorRoot;
    if (priorLabel === undefined) delete process.env.PERL_LSP_SMOKE_SOURCE_LABEL;
    else process.env.PERL_LSP_SMOKE_SOURCE_LABEL = priorLabel;
  }
});

const CHILD_SUBJECT = {
  expectedRevision: 'a'.repeat(40),
  expectedVsixSha256: 'b'.repeat(64),
  expectedServerSourceSha: 'a'.repeat(40),
  expectedVscodeVersion: 'stable',
  expectedSourceLabel: 'hosted-linux-current-source',
};

function childReceipt(overrides = {}, environmentOverrides = {}) {
  return {
    outcome: 'completed',
    failures: [],
    environment: {
      source_revision: CHILD_SUBJECT.expectedRevision,
      server_source_revision: CHILD_SUBJECT.expectedServerSourceSha,
      vsix_sha256: CHILD_SUBJECT.expectedVsixSha256,
      requested_vscode_version: CHILD_SUBJECT.expectedVscodeVersion,
      extension_id: 'EffortlessMetrics.perl-lsp-rs',
      ...environmentOverrides,
    },
    ...overrides,
  };
}

function validateChild(receipt, { present = true } = {}) {
  return validateChildSmokeReceipt({
    ...CHILD_SUBJECT,
    receiptFile: '/fixture/first_hour_vscode_receipt.json',
    exists: () => present,
    readFile: () => JSON.stringify(receipt),
  });
}

void test('a child receipt bound to this run admits behavioral proof', () => {
  const result = validateChild(childReceipt());
  assert.equal(result.ok, true);
});

void test('a missing child receipt is never behavioral proof', () => {
  const result = validateChild(childReceipt(), { present: false });
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /did not write a first-hour receipt/);
});

void test('a child receipt from another source revision is rejected', () => {
  const result = validateChild(childReceipt({}, { source_revision: 'c'.repeat(40) }));
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /source revision .* is not the smoke subject/);
});

void test('a child receipt describing a different VSIX is rejected', () => {
  const result = validateChild(childReceipt({}, { vsix_sha256: 'd'.repeat(64) }));
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /VSIX digest .* is not the package this run created/);
});

void test('a child receipt with no VSIX digest is rejected', () => {
  const result = validateChild(childReceipt({}, { vsix_sha256: null }));
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /VSIX digest/);
});

void test('a child receipt from another server build is rejected', () => {
  const result = validateChild(childReceipt({}, { server_source_revision: 'e'.repeat(40) }));
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /server source revision .* is not the staged server/);
});

void test('a child receipt from another matrix leg is rejected', () => {
  const result = validateChild(childReceipt({}, { requested_vscode_version: '1.125.0' }));
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /VS Code version .* is not this matrix leg/);
});

void test('a non-terminal or failing child receipt is rejected', () => {
  const incomplete = validateChild(childReceipt({ outcome: 'aborted' }));
  assert.equal(incomplete.ok, false);
  assert.match(incomplete.violations.join('; '), /outcome .* not completed/);

  const failed = validateChild(childReceipt({ failures: ['activation timed out'] }));
  assert.equal(failed.ok, false);
  assert.match(failed.violations.join('; '), /reported failures/);
});

void test('a malformed child receipt is rejected rather than parsed optimistically', () => {
  const result = validateChildSmokeReceipt({
    ...CHILD_SUBJECT,
    receiptFile: '/fixture/first_hour_vscode_receipt.json',
    exists: () => true,
    readFile: () => 'not json',
  });
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /was not valid JSON/);
});

void test('a child receipt without environment identity is rejected', () => {
  const result = validateChildSmokeReceipt({
    ...CHILD_SUBJECT,
    receiptFile: '/fixture/first_hour_vscode_receipt.json',
    exists: () => true,
    readFile: () => JSON.stringify({ outcome: 'completed', failures: [] }),
  });
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /missing its environment identity/);
});

void test('activation-failure journey stage is required for an overall pass', () => {
  const passingBase = {
    package_creation: { status: 'pass' },
    package_inventory: { status: 'pass', classification: 'pass', behavior_safe: true },
    behavioral_smoke: { status: 'pass' },
  };
  assert.equal(
    computeOverallStatus({ ...passingBase, activation_failure_journey: { status: 'pass' } }),
    'pass',
  );
  assert.equal(
    computeOverallStatus({
      ...passingBase,
      activation_failure_journey: { status: 'not_run', reason: 'not_started' },
    }),
    'pass',
    'a not_run journey (upstream package block) inherits the upstream verdict',
  );
  assert.equal(
    computeOverallStatus({
      ...passingBase,
      activation_failure_journey: { status: 'not_proven', reason: 'child receipts missing' },
    }),
    'not_proven',
  );
  assert.equal(
    computeOverallStatus({
      ...passingBase,
      activation_failure_journey: { status: 'failed', reason: 'leg failed' },
    }),
    'failed',
  );
});

void test('activation-failure leg env arms only the failure leg with the fault', () => {
  const context = {
    vsixPath: '/tmp/perl-lsp-rs-0.17.0.vsix',
    vsixSha256: 'a'.repeat(64),
    revision: 'r'.repeat(40),
    serverSourceRevision: 'r'.repeat(40),
    workspacePath: '/tmp/workspace',
    userDataDir: '/tmp/profile/user-data',
    extensionsDir: '/tmp/profile/extensions',
  };
  const failureEnv = activationFailureLegEnv(
    {
      PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
      PERL_LSP_PACKAGED_BUNDLE_SMOKE: '1',
      PERL_LSP_FIRST_HOUR_SERVER_PATH: '/ambient/server',
      PERL_LSP_CURRENT_SOURCE_SHA: 'c'.repeat(40),
      PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE: 'stale-from-outer-env',
    },
    'failure',
    true,
    context,
  );
  assert.equal(failureEnv.PERL_LSP_ACTIVATION_FAILURE_SMOKE, '1');
  assert.equal(failureEnv.PERL_LSP_ACTIVATION_FAILURE_LEG, 'failure');
  assert.equal(failureEnv.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE, 'debugger');
  assert.equal(failureEnv.PERL_LSP_PUBLISHED_EXTENSION_SOURCE, 'vsix');
  assert.equal(failureEnv.PERL_LSP_PUBLISHED_VSIX_PATH, context.vsixPath);
  assert.equal(failureEnv.PERL_LSP_SMOKE_USER_DATA_DIR, context.userDataDir);
  assert.equal(failureEnv.PERL_LSP_SMOKE_EXTENSIONS_DIR, context.extensionsDir);
  assert.equal(failureEnv.PERL_LSP_CURRENT_SOURCE_SMOKE, undefined);
  assert.equal(failureEnv.PERL_LSP_PACKAGED_BUNDLE_SMOKE, undefined);
  assert.equal(failureEnv.PERL_LSP_FIRST_HOUR_SERVER_PATH, undefined);
  assert.equal(
    failureEnv.PERL_LSP_CURRENT_SOURCE_SHA,
    undefined,
    'candidate-bound mode is Linux-only and must not leak into the journey legs',
  );

  const retryEnv = activationFailureLegEnv({}, 'retry', false, context);
  assert.equal(retryEnv.PERL_LSP_ACTIVATION_FAILURE_LEG, 'retry');
  assert.equal(retryEnv.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE, undefined);
});

function recoveryChild(leg, overrides = {}) {
  return {
    schema_version: 'vscode_activation_recovery_leg.v1',
    leg,
    verdict: 'pass',
    candidate: {
      vsix_sha256: 'v'.repeat(64),
      bundled_server: { sha256: 's'.repeat(64) },
      extension_version: '0.17.0',
    },
    observations: {},
    ...overrides,
  };
}

void test('activation recovery child receipts must bind this run exactly', () => {
  const files = new Map([
    ['/receipts/failure.json', JSON.stringify(recoveryChild('failure'))],
    ['/receipts/retry.json', JSON.stringify(recoveryChild('retry'))],
  ]);
  const bound = validateActivationRecoveryChildReceipts({
    failureReceiptFile: '/receipts/failure.json',
    retryReceiptFile: '/receipts/retry.json',
    expectedVsixSha256: 'v'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists: (file) => files.has(file),
    readFile: (file) => files.get(file) ?? '',
  });
  assert.equal(bound.ok, true);

  const wrongVsix = validateActivationRecoveryChildReceipts({
    failureReceiptFile: '/receipts/failure.json',
    retryReceiptFile: '/receipts/retry.json',
    expectedVsixSha256: 'w'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists: (file) => files.has(file),
    readFile: (file) => files.get(file) ?? '',
  });
  assert.equal(wrongVsix.ok, false);
  assert.match(wrongVsix.violations.join('; '), /VSIX digest .* is not this run's package/);

  const missing = validateActivationRecoveryChildReceipts({
    failureReceiptFile: '/receipts/missing.json',
    retryReceiptFile: '/receipts/retry.json',
    expectedVsixSha256: 'v'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists: () => false,
    readFile: () => '',
  });
  assert.equal(missing.ok, false);
  assert.match(missing.violations.join('; '), /failure leg did not write its journey receipt/);
});

void test('the joined activation recovery receipt is fail-closed', () => {
  const baseInput = {
    vsixSha256: 'v'.repeat(64),
    extensionVersion: '0.17.0',
    bundledServerSha256: 's'.repeat(64),
    serverSourceRevision: 'r'.repeat(40),
    repositorySha: 'r'.repeat(40),
    vscodeVersion: 'stable',
    workspaceFixtureSha256: 'f'.repeat(64),
    violations: [],
    legExitCodes: { failure: 0, retry: 0 },
  };
  const failureChild = recoveryChild('failure', {
    observations: {
      bundled_server_processes_after_failure: [],
      bundled_server_processes_after_demand_window: [],
      crash_budget_evidence: ['no bundled-server process at any scan'],
    },
  });
  const retryChild = recoveryChild('retry', {
    observations: {
      bundled_server_processes_running: ['<installed>/bin/win32-x64/perllsp.exe'],
      bundled_server_processes_after_second_demand: ['<installed>/bin/win32-x64/perllsp.exe'],
      bundled_server_processes_after_stop: [],
      stop_seam: 'stopped',
    },
  });

  const passReceipt = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: failureChild,
    retry: retryChild,
    postHostExitProcesses: [],
  });
  assert.equal(passReceipt.schema_version, 'vscode_activation_recovery.v1');
  assert.equal(passReceipt.verdict, 'pass');
  assert.equal(passReceipt.failure.cleanup, 'pass');
  assert.equal(passReceipt.failure.process_remaining, false);
  assert.equal(passReceipt.failure.crash_budget_consumed, false);
  assert.equal(passReceipt.retry.activation, 'pass');
  assert.equal(passReceipt.retry.duplicate_resources, 0);
  assert.equal(passReceipt.deactivation, 'pass');

  const survivor = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: failureChild,
    retry: retryChild,
    postHostExitProcesses: ['<installed>/bin/win32-x64/perllsp.exe'],
  });
  assert.equal(survivor.verdict, 'failed');
  assert.equal(survivor.deactivation, 'failed');

  const unbound = composeActivationRecoveryReceipt({
    ...baseInput,
    violations: ['the retry leg did not write its journey receipt'],
    failure: failureChild,
    retry: null,
    postHostExitProcesses: [],
  });
  assert.equal(unbound.verdict, 'not_proven');
  assert.equal(unbound.retry.activation, 'not_proven');
  assert.equal(unbound.deactivation, 'not_proven');

  const duplicates = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: failureChild,
    retry: recoveryChild('retry', {
      observations: {
        bundled_server_processes_running: ['a', 'b'],
        bundled_server_processes_after_second_demand: ['a', 'b'],
        bundled_server_processes_after_stop: [],
      },
    }),
    postHostExitProcesses: [],
  });
  assert.equal(duplicates.retry.duplicate_resources, 2);

  const unobservedDuplicates = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: failureChild,
    retry: recoveryChild('retry', {
      observations: { bundled_server_processes_after_stop: [] },
    }),
    postHostExitProcesses: [],
  });
  assert.equal(unobservedDuplicates.retry.duplicate_resources, 'not_proven');
});

void test('a nonzero leg exit code cannot stand behind a passing joined verdict', () => {
  const baseInput = {
    vsixSha256: 'v'.repeat(64),
    extensionVersion: '0.17.0',
    bundledServerSha256: 's'.repeat(64),
    serverSourceRevision: 'r'.repeat(40),
    repositorySha: 'r'.repeat(40),
    vscodeVersion: 'stable',
    workspaceFixtureSha256: 'f'.repeat(64),
    violations: [],
  };
  const failureChild = recoveryChild('failure', {
    observations: {
      bundled_server_processes_after_failure: [],
      bundled_server_processes_after_demand_window: [],
    },
  });
  const retryChild = recoveryChild('retry', {
    observations: {
      bundled_server_processes_running: ['<installed>/bin/perllsp.exe'],
      bundled_server_processes_after_second_demand: ['<installed>/bin/perllsp.exe'],
      bundled_server_processes_after_stop: [],
    },
  });

  const teardownFailure = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: failureChild,
    retry: retryChild,
    legExitCodes: { failure: 0, retry: 1 },
    postHostExitProcesses: [],
  });
  assert.equal(teardownFailure.verdict, 'not_proven');
  assert.equal(teardownFailure.failure.cleanup, 'pass');
  assert.equal(teardownFailure.retry.activation, 'not_proven');

  const observedProductFailure = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: recoveryChild('failure', { verdict: 'failed' }),
    retry: retryChild,
    legExitCodes: { failure: 0, retry: 0 },
    postHostExitProcesses: [],
  });
  assert.equal(observedProductFailure.verdict, 'failed');
  assert.equal(observedProductFailure.failure.cleanup, 'failed');
});
