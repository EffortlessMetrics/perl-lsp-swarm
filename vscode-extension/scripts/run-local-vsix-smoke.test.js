const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const {
  activationFailureLegEnv,
  bundleTargetForPlatform,
  composeActivationRecoveryReceipt,
  composeCrashRecoveryReceipt,
  computeOverallStatus,
  crashRecoveryLegEnv,
  finalizeSmokeRun,
  interpretBehavioralSmokeExit,
  interpretTransitionResult,
  shouldRunBehavioralSmoke,
  shouldRunCrashRecoveryJourney,
  stageServerForPackage,
  validateActivationRecoveryChildReceipts,
  validateChildSmokeReceipt,
  validateCrashRecoveryChildReceipts,
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
  //
  // receiptsRoot() anchors the configured root to the host (path.resolve)
  // before nesting and before injecting it into the child environment, so a
  // POSIX-style rooted fixture and a relative fixture must anchor the same
  // way on the expectation side. Joining the raw configured string instead
  // diverges on Windows, where it stays drive-relative while the child env
  // receives the drive-absolute form.
  const { childReceiptPath } = require('./run-local-vsix-smoke');
  const priorRoot = process.env.PERL_LSP_SMOKE_RECEIPTS_DIR;
  const priorLabel = process.env.PERL_LSP_SMOKE_SOURCE_LABEL;
  process.env.PERL_LSP_SMOKE_SOURCE_LABEL = 'unique-test-label';
  try {
    const platformLeg =
      { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform] ?? process.platform;
    for (const configuredRoot of ['/fixture/receipts-root', 'fixture/receipts-root']) {
      process.env.PERL_LSP_SMOKE_RECEIPTS_DIR = configuredRoot;
      assert.equal(
        childReceiptPath(),
        path.join(
          path.resolve(configuredRoot),
          'unique-test-label',
          platformLeg,
          'first_hour_vscode_receipt.json',
        ),
      );
    }
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
      // The launched runtime identity the extension host actually observed;
      // the requested selector alone never proves the host.
      vscode_version: '1.130.2',
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

void test('a child receipt without the launched runtime version is rejected', () => {
  const result = validateChild(childReceipt({}, { vscode_version: undefined }));
  assert.equal(result.ok, false);
  assert.match(result.violations.join('; '), /launched VS Code runtime version/);
});

void test('a concrete leg rejects a different launched runtime version', () => {
  const concrete = {
    ...CHILD_SUBJECT,
    expectedVscodeVersion: '1.125.0',
    receiptFile: '/fixture/first_hour_vscode_receipt.json',
    exists: () => true,
  };
  const mismatched = validateChildSmokeReceipt({
    ...concrete,
    readFile: () =>
      JSON.stringify(
        childReceipt({}, { requested_vscode_version: '1.125.0', vscode_version: '1.126.0' }),
      ),
  });
  assert.equal(mismatched.ok, false);
  assert.match(mismatched.violations.join('; '), /launched VS Code .* requested the concrete/);

  const matched = validateChildSmokeReceipt({
    ...concrete,
    readFile: () =>
      JSON.stringify(
        childReceipt({}, { requested_vscode_version: '1.125.0', vscode_version: '1.125.0' }),
      ),
  });
  assert.equal(matched.ok, true);
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

void test('an unavailable host-resolution receipt is not a product smoke failure', () => {
  const hostFailure = {
    schema_version: 1,
    outcome: 'blocked',
    stage: 'vscode_host_resolution',
    requested_version: '1.125.0',
    platform: 'linux',
    arch: 'x64',
    disposition: 'unavailable',
    error: 'VS Code release 1.125.0 was not found',
  };
  const result = interpretBehavioralSmokeExit({
    status: 1,
    receiptsRoot: '/fixture',
    exists: (file) => file.endsWith('vscode_host_resolution_failure.json'),
    readFile: () => JSON.stringify(hostFailure),
  });
  assert.equal(result.status, 'not_proven');
  assert.equal(result.reason, 'vscode_host_resolution_unavailable');
  assert.equal(result.reason.includes('published_extension_smoke_failed'), false);
  assert.ok(result.host_resolution);
  assert.equal(result.host_resolution.requested_version, '1.125.0');
  assert.equal(result.host_resolution.requested_version, hostFailure.requested_version);
  assert.notEqual(result.host_resolution.requested_version, 'stable');
});

void test('network, cache, and runner host failures keep the host-resolution boundary', () => {
  for (const disposition of ['network', 'cache', 'runner']) {
    const result = interpretBehavioralSmokeExit({
      status: 1,
      receiptsRoot: '/fixture',
      exists: (file) => file.endsWith('vscode_host_resolution_failure.json'),
      readFile: () =>
        JSON.stringify({
          schema_version: 1,
          outcome: 'blocked',
          stage: 'vscode_host_resolution',
          requested_version: 'stable',
          disposition,
          error: `${disposition} failure`,
        }),
    });
    assert.equal(result.status, 'failed');
    assert.equal(result.reason, `vscode_host_resolution_${disposition}`);
    assert.ok(result.host_resolution);
    assert.equal(result.host_resolution.requested_version, 'stable');
  }
});

void test('a smoke failure without a host-resolution receipt remains a product failure', () => {
  const result = interpretBehavioralSmokeExit({
    status: 1,
    receiptsRoot: '/fixture',
    exists: () => false,
    readFile: () => {
      throw new Error('host receipt should not be read when absent');
    },
  });
  assert.equal(result.status, 'failed');
  assert.equal(result.reason, 'published_extension_smoke_failed');
  assert.equal('host_resolution' in result, false);
});

void test('1.125.0 and stable host-resolution receipts keep independent requested identity', () => {
  const readDisposition = (requested) =>
    interpretBehavioralSmokeExit({
      status: 1,
      receiptsRoot: '/fixture',
      exists: () => true,
      readFile: () =>
        JSON.stringify({
          schema_version: 1,
          outcome: 'blocked',
          stage: 'vscode_host_resolution',
          requested_version: requested,
          disposition: 'unavailable',
          error: `${requested} was not found`,
        }),
    });
  const minimum = readDisposition('1.125.0');
  const stable = readDisposition('stable');
  assert.ok(minimum.host_resolution);
  assert.ok(stable.host_resolution);
  assert.equal(minimum.host_resolution.requested_version, '1.125.0');
  assert.equal(stable.host_resolution.requested_version, 'stable');
  assert.notEqual(
    minimum.host_resolution.requested_version,
    stable.host_resolution.requested_version,
  );
});

void test('an unavailable host-resolution stage is not an overall product failure', () => {
  assert.equal(
    computeOverallStatus({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'pass', classification: 'pass' },
      behavioral_smoke: {
        status: 'not_proven',
        reason: 'vscode_host_resolution_unavailable',
      },
    }),
    'not_proven',
  );
});

void test('a malformed host-resolution receipt stays a host boundary, not a product failure', () => {
  const result = interpretBehavioralSmokeExit({
    status: 1,
    receiptsRoot: '/fixture',
    exists: () => true,
    readFile: () => 'not json',
  });
  assert.equal(result.status, 'not_proven');
  assert.equal(result.reason, 'vscode_host_resolution_receipt_invalid');
  assert.equal(result.reason.includes('published_extension_smoke_failed'), false);
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

  // Base env deliberately carries the fault variable: the retry leg must
  // REMOVE it, so deleting the else-branch cleanup would fail this assertion.
  const retryEnv = activationFailureLegEnv(
    { PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE: 'leaked-from-outer-env' },
    'retry',
    false,
    context,
  );
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

  const rejectionBranches = (overrides, pattern) => {
    const files = new Map([
      ['/receipts/failure.json', JSON.stringify(recoveryChild('failure', overrides.failure || {}))],
      ['/receipts/retry.json', JSON.stringify(recoveryChild('retry', overrides.retry || {}))],
    ]);
    const result = validateActivationRecoveryChildReceipts({
      failureReceiptFile: '/receipts/failure.json',
      retryReceiptFile: '/receipts/retry.json',
      expectedVsixSha256: 'v'.repeat(64),
      expectedBundledServerSha256: 's'.repeat(64),
      expectedExtensionVersion: '0.17.0',
      exists: (file) => files.has(file),
      readFile: (file) => files.get(file) ?? '',
    });
    assert.equal(result.ok, false);
    assert.match(result.violations.join('; '), pattern);
  };
  rejectionBranches({ failure: { schema_version: 'other.v1' } }, /schema is/);
  rejectionBranches({ failure: { leg: 'retry' } }, /records leg/);
  rejectionBranches({ failure: { verdict: 'failed' } }, /verdict is/);
  rejectionBranches({ failure: { candidate: { vsix_sha256: 'x'.repeat(64) } } }, /VSIX digest/);
  rejectionBranches(
    { failure: { candidate: { bundled_server: { sha256: 'x'.repeat(64) } } } },
    /bundled-server digest/,
  );
  rejectionBranches(
    { failure: { candidate: { extension_version: '9.9.9' } } },
    /extension version/,
  );

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
  // Two observed processes at both windows is ONE duplicate (max, not sum).
  assert.equal(duplicates.retry.duplicate_resources, 1);

  const absentStopObservation = composeActivationRecoveryReceipt({
    ...baseInput,
    failure: failureChild,
    // Every observation present except the post-stop scan: missing evidence
    // must degrade the deactivation row and verdict to not_proven.
    retry: recoveryChild('retry', {
      observations: {
        bundled_server_processes_running: ['<installed>/bin/perllsp.exe'],
        bundled_server_processes_after_second_demand: ['<installed>/bin/perllsp.exe'],
      },
    }),
    legExitCodes: { failure: 0, retry: 0 },
    postHostExitProcesses: [],
  });
  assert.equal(absentStopObservation.deactivation, 'not_proven');
  assert.equal(absentStopObservation.verdict, 'not_proven');

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

function crashChild(leg, overrides = {}) {
  return {
    schema_version: 'vscode_crash_recovery_leg.v1',
    leg,
    verdict: 'pass',
    candidate: {
      vsix_sha256: 'v'.repeat(64),
      bundled_server: { sha256: 's'.repeat(64) },
      extension_version: '0.17.0',
    },
    fault: {
      method: 'harness-external process termination (unexpected; not the user restart command)',
    },
    observations: {},
    ...overrides,
  };
}

function passingTransientChild(overrides = {}) {
  return crashChild('transient', {
    observations: {
      failed_generation: 1,
      replacement_generation: 2,
      replay: {
        'crash_transient_a.pl': 'ready_in_replacement_generation',
        'crash_transient_b.pl': 'ready_in_replacement_generation',
      },
      provider_after_recovery: {
        provider: { status: 'ok' },
        readiness_generation_at_request: 2,
      },
      recovery_samples: { max_simultaneous_server_processes: 1 },
      quiet_window: { failed_pid_resurrected: false },
      watchdog: { status: 'pass' },
    },
    ...overrides,
  });
}

function passingBreakerChild(overrides = {}) {
  return crashChild('breaker', {
    fault: {
      method: 'harness-external repeated process termination per the accepted crash budget',
    },
    observations: {
      automatic_budget: 3,
      exhausted: true,
      episodes: [
        { episode_index: 1, replacement_pid: 201 },
        { episode_index: 2, replacement_pid: 202, max_simultaneous_server_processes: 1 },
        { episode_index: 3, replacement_pid: 203, max_simultaneous_server_processes: 1 },
        {
          episode_index: 4,
          background_server_processes: [],
          generation_sequence_during_exhaustion: [4],
        },
      ],
      explicit_retry: {
        binary_resolution_source_after: 'bundled',
        readiness: 'ready_in_retry_generation',
        provider: { status: 'ok' },
      },
      action_required_dialog: { observable: false },
    },
    ...overrides,
  });
}

function crashComposeBase(overrides = {}) {
  return {
    vsixSha256: 'v'.repeat(64),
    extensionVersion: '0.17.0',
    bundledServerSha256: 's'.repeat(64),
    serverSourceRevision: 'r'.repeat(40),
    repositorySha: 'c'.repeat(40),
    vscodeVersion: 'stable',
    transient: passingTransientChild(),
    breaker: passingBreakerChild(),
    violations: [],
    legExitCodes: { transient: 0, breaker: 0 },
    postHostExitProcesses: [],
    ...overrides,
  };
}

void test('a fully passing crash-recovery journey composes a pass verdict with bound rows', () => {
  const joined = composeCrashRecoveryReceipt(crashComposeBase());
  assert.equal(joined.schema_version, 'vscode_crash_recovery.v1');
  assert.equal(joined.receipt_kind, 'vscode_crash_recovery');
  assert.equal(joined.verdict, 'pass');
  assert.equal(joined.transient_crash.failed_generation, 1);
  assert.equal(joined.transient_crash.replacement_generation, 2);
  assert.equal(joined.transient_crash.replay, 'pass');
  assert.equal(joined.transient_crash.provider_after_recovery, 'pass');
  assert.equal(joined.circuit_breaker.attempts, 3);
  assert.equal(joined.circuit_breaker.exhausted, true);
  assert.equal(joined.circuit_breaker.background_restart_after_exhaustion, false);
  assert.equal(joined.circuit_breaker.explicit_retry, 'pass');
  assert.equal(joined.watchdog, 'pass');
  assert.equal(joined.cleanup, 'pass');
  assert.equal(joined.negative_controls.user_restart_not_used_for_failure_injection, true);
  assert.equal(joined.negative_controls.replacement_servers_never_overlapped, true);
  assert.equal(joined.negative_controls.budget_exhaustion_spawned_no_background_server, true);
  assert.equal(joined.negative_controls.explicit_retry_did_not_substitute_binary_source, true);
  assert.equal(joined.negative_controls.failed_process_not_resurrected, true);
});

void test('an honestly not_proven watchdog row degrades only the overall verdict', () => {
  const transient = passingTransientChild({
    observations: {
      ...passingTransientChild().observations,
      watchdog: {
        status: 'not_proven',
        reason:
          'host platform cannot safely suspend the installed server process; deterministic watchdog mechanism proof is owned by #7846',
      },
    },
  });
  const joined = composeCrashRecoveryReceipt(crashComposeBase({ transient }));
  assert.equal(joined.watchdog, 'not_proven');
  assert.equal(joined.transient_crash.replay, 'pass');
  assert.equal(joined.transient_crash.provider_after_recovery, 'pass');
  assert.equal(joined.circuit_breaker.explicit_retry, 'pass');
  assert.equal(joined.cleanup, 'pass');
  assert.equal(joined.verdict, 'not_proven');
});

void test('a breaker that never exhausts fails the circuit-breaker rows', () => {
  const breaker = passingBreakerChild({
    observations: {
      ...passingBreakerChild().observations,
      exhausted: false,
      episodes: [
        { episode_index: 1, replacement_pid: 201 },
        { episode_index: 2, replacement_pid: 202 },
      ],
    },
  });
  const joined = composeCrashRecoveryReceipt(crashComposeBase({ breaker }));
  assert.equal(joined.verdict, 'failed');
  assert.equal(joined.circuit_breaker.attempts, 2);
});

void test('a background respawn after exhaustion fails the journey', () => {
  const breaker = passingBreakerChild({
    observations: {
      ...passingBreakerChild().observations,
      episodes: [
        { episode_index: 1, replacement_pid: 201 },
        { episode_index: 2, replacement_pid: 202 },
        { episode_index: 3, replacement_pid: 203 },
        { episode_index: 4, background_server_processes: [404] },
      ],
    },
  });
  const joined = composeCrashRecoveryReceipt(crashComposeBase({ breaker }));
  assert.equal(joined.verdict, 'failed');
  assert.equal(joined.circuit_breaker.background_restart_after_exhaustion, true);
});

void test('an explicit retry through a substituted binary source fails', () => {
  const breaker = passingBreakerChild({
    observations: {
      ...passingBreakerChild().observations,
      explicit_retry: {
        binary_resolution_source_after: 'path',
        readiness: 'ready_in_retry_generation',
        provider: { status: 'ok' },
      },
    },
  });
  const joined = composeCrashRecoveryReceipt(crashComposeBase({ breaker }));
  assert.equal(joined.circuit_breaker.explicit_retry, 'failed');
  assert.equal(joined.verdict, 'failed');
  assert.equal(joined.negative_controls.explicit_retry_did_not_substitute_binary_source, false);
});

void test('a replay row with an omitted document fails while provider evidence stands', () => {
  const transient = passingTransientChild({
    observations: {
      ...passingTransientChild().observations,
      replay: {
        'crash_transient_a.pl': 'ready_in_replacement_generation',
        'crash_transient_b.pl': 'Active document was not ready after 60000ms.',
      },
    },
  });
  const joined = composeCrashRecoveryReceipt(crashComposeBase({ transient }));
  assert.equal(joined.transient_crash.replay, 'failed');
  assert.equal(joined.transient_crash.provider_after_recovery, 'pass');
  assert.equal(joined.verdict, 'failed');
});

void test('unbound crash children leave rows not_proven, never failed', () => {
  const joined = composeCrashRecoveryReceipt(
    crashComposeBase({
      transient: null,
      breaker: null,
      violations: ['the transient leg did not write its journey receipt'],
      legExitCodes: { transient: 0, breaker: null },
    }),
  );
  assert.equal(joined.transient_crash.replay, 'not_proven');
  assert.equal(joined.circuit_breaker.attempts, null);
  assert.equal(joined.circuit_breaker.exhausted, null);
  assert.equal(joined.watchdog, 'not_proven');
  assert.equal(joined.cleanup, 'not_proven');
  assert.equal(joined.verdict, 'not_proven');
  assert.deepEqual(joined.instrument_violations, [
    'the transient leg did not write its journey receipt',
  ]);
});

void test('a leg exit failure degrades passing rows to not_proven', () => {
  const joined = composeCrashRecoveryReceipt(
    crashComposeBase({ legExitCodes: { transient: 1, breaker: 0 } }),
  );
  assert.equal(joined.transient_crash.replay, 'not_proven');
  assert.equal(joined.transient_crash.provider_after_recovery, 'not_proven');
  assert.equal(joined.watchdog, 'pass');
  assert.equal(joined.verdict, 'not_proven');
});

void test('a surviving bundled server after the crash journey hosts fails cleanup', () => {
  const joined = composeCrashRecoveryReceipt(
    crashComposeBase({ postHostExitProcesses: ['/extensions/bin/linux-x64/perllsp'] }),
  );
  assert.equal(joined.cleanup, 'failed');
  assert.equal(joined.verdict, 'failed');
});

void test('an observed child failure fails the crash-recovery receipt', () => {
  const transient = passingTransientChild({ verdict: 'failed' });
  const joined = composeCrashRecoveryReceipt(crashComposeBase({ transient }));
  assert.equal(joined.verdict, 'failed');
});

void test('crash recovery child receipts must bind this run exactly', () => {
  const files = new Map([
    ['/receipts/transient.json', JSON.stringify(passingTransientChild())],
    ['/receipts/breaker.json', JSON.stringify(passingBreakerChild())],
  ]);
  const read = (file) => files.get(file) ?? '';
  const exists = (file) => files.has(file);
  const bound = validateCrashRecoveryChildReceipts({
    transientReceiptFile: '/receipts/transient.json',
    breakerReceiptFile: '/receipts/breaker.json',
    expectedVsixSha256: 'v'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists,
    readFile: read,
  });
  assert.equal(bound.ok, true);

  const wrongVsix = validateCrashRecoveryChildReceipts({
    transientReceiptFile: '/receipts/transient.json',
    breakerReceiptFile: '/receipts/breaker.json',
    expectedVsixSha256: 'w'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists,
    readFile: read,
  });
  assert.equal(wrongVsix.ok, false);
  assert.match(wrongVsix.violations.join('; '), /VSIX digest .* is not this run's package/);

  const missing = validateCrashRecoveryChildReceipts({
    transientReceiptFile: '/receipts/missing.json',
    breakerReceiptFile: '/receipts/breaker.json',
    expectedVsixSha256: 'v'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists,
    readFile: read,
  });
  assert.equal(missing.ok, false);
  assert.match(missing.violations.join('; '), /transient leg did not write its journey receipt/);
});

void test('a crash leg receipt that used the user restart command does not bind', () => {
  const transient = passingTransientChild({
    fault: { method: 'command:perl-lsp.restart executed by the harness' },
  });
  const files = new Map([
    ['/receipts/transient.json', JSON.stringify(transient)],
    ['/receipts/breaker.json', JSON.stringify(passingBreakerChild())],
  ]);
  const result = validateCrashRecoveryChildReceipts({
    transientReceiptFile: '/receipts/transient.json',
    breakerReceiptFile: '/receipts/breaker.json',
    expectedVsixSha256: 'v'.repeat(64),
    expectedBundledServerSha256: 's'.repeat(64),
    expectedExtensionVersion: '0.17.0',
    exists: (file) => files.has(file),
    readFile: (file) => files.get(file) ?? '',
  });
  assert.equal(result.ok, false);
  assert.match(
    result.violations.join('; '),
    /harness-external process termination as the failure injection/,
  );
});

void test('crash recovery journey requires a behavior-safe package', () => {
  assert.equal(
    shouldRunCrashRecoveryJourney({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'pass', classification: 'pass', behavior_safe: true },
    }),
    true,
  );
  assert.equal(
    shouldRunCrashRecoveryJourney({
      package_creation: { status: 'pass' },
      package_inventory: { status: 'failed', classification: 'structural', behavior_safe: false },
    }),
    false,
  );
  assert.equal(
    shouldRunCrashRecoveryJourney({
      package_creation: { status: 'failed' },
      package_inventory: {
        status: 'not_proven',
        classification: 'not_proven',
        behavior_safe: false,
      },
    }),
    false,
  );
});

void test('overall status treats the crash journey like the activation journey', () => {
  const stages = passingReceipt().stages;
  assert.equal(
    computeOverallStatus({ ...stages, crash_recovery_journey: { status: 'pass' } }),
    'pass',
  );
  assert.equal(
    computeOverallStatus({ ...stages, crash_recovery_journey: { status: 'not_run' } }),
    'pass',
  );
  assert.equal(
    computeOverallStatus({ ...stages, crash_recovery_journey: { status: 'not_proven' } }),
    'not_proven',
  );
  assert.equal(
    computeOverallStatus({ ...stages, crash_recovery_journey: { status: 'failed' } }),
    'failed',
  );
});

void test('crash-recovery leg env arms one leg and strips foreign selectors', () => {
  const context = {
    vsixPath: '/tmp/perl-lsp-rs-0.17.0.vsix',
    vsixSha256: 'a'.repeat(64),
    revision: 'r'.repeat(40),
    serverSourceRevision: 'r'.repeat(40),
    workspacePath: '/tmp/workspace',
    userDataDir: '/tmp/profile/user-data',
    extensionsDir: '/tmp/profile/extensions',
  };
  const transientEnv = crashRecoveryLegEnv(
    {
      PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
      PERL_LSP_PACKAGED_BUNDLE_SMOKE: '1',
      PERL_LSP_FIRST_HOUR_SERVER_PATH: '/ambient/server',
      PERL_LSP_CURRENT_SOURCE_SHA: 'c'.repeat(40),
      PERL_LSP_ACTIVATION_FAILURE_SMOKE: '1',
      PERL_LSP_ACTIVATION_FAILURE_LEG: 'failure',
      PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE: 'stale-from-outer-env',
    },
    'transient',
    context,
  );
  assert.equal(transientEnv.PERL_LSP_CRASH_RECOVERY_SMOKE, '1');
  assert.equal(transientEnv.PERL_LSP_CRASH_RECOVERY_LEG, 'transient');
  assert.equal(transientEnv.PERL_LSP_PUBLISHED_EXTENSION_SOURCE, 'vsix');
  assert.equal(transientEnv.PERL_LSP_PUBLISHED_VSIX_PATH, context.vsixPath);
  assert.equal(transientEnv.PERL_LSP_VSIX_SHA256, context.vsixSha256);
  assert.equal(transientEnv.PERL_LSP_SERVER_SOURCE_SHA, context.serverSourceRevision);
  // Foreign selectors must not leak: the crash journey selects its own suite
  // and resolves the bundled candidate itself.
  assert.equal('PERL_LSP_CURRENT_SOURCE_SMOKE' in transientEnv, false);
  assert.equal('PERL_LSP_PACKAGED_BUNDLE_SMOKE' in transientEnv, false);
  assert.equal('PERL_LSP_FIRST_HOUR_SERVER_PATH' in transientEnv, false);
  assert.equal('PERL_LSP_CURRENT_SOURCE_SHA' in transientEnv, false);
  assert.equal('PERL_LSP_ACTIVATION_FAILURE_SMOKE' in transientEnv, false);
  assert.equal('PERL_LSP_ACTIVATION_FAILURE_LEG' in transientEnv, false);
  assert.equal('PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE' in transientEnv, false);

  const breakerEnv = crashRecoveryLegEnv({}, 'breaker', context);
  assert.equal(breakerEnv.PERL_LSP_CRASH_RECOVERY_LEG, 'breaker');
  assert.equal(breakerEnv.PERL_LSP_CRASH_RECOVERY_SMOKE, '1');
});

void test('parsed-but-unbound crash children cannot fail the joined receipt', () => {
  // A receipt that failed digest binding is not this candidate's evidence:
  // even observations that look like product failures must leave the rows
  // not_proven instead of turning an unbound observation into a failed row.
  const transient = passingTransientChild({
    observations: {
      ...passingTransientChild().observations,
      replay: { 'crash_transient_a.pl': 'Active document was not ready after 60000ms.' },
      watchdog: { status: 'failed' },
    },
    verdict: 'failed',
  });
  const breaker = passingBreakerChild({
    observations: {
      ...passingBreakerChild().observations,
      exhausted: false,
      explicit_retry: {
        binary_resolution_source_after: 'path',
        readiness: 'timeout',
        provider: { status: 'error' },
      },
    },
    verdict: 'failed',
  });
  const joined = composeCrashRecoveryReceipt(
    crashComposeBase({
      transient,
      breaker,
      violations: ["the transient leg receipt VSIX digest is not this run's package"],
    }),
  );
  assert.equal(joined.transient_crash.replay, 'not_proven');
  assert.equal(joined.transient_crash.provider_after_recovery, 'not_proven');
  assert.equal(joined.circuit_breaker.exhausted, null);
  assert.equal(joined.circuit_breaker.explicit_retry, 'not_proven');
  assert.equal(joined.watchdog, 'not_proven');
  assert.equal(joined.cleanup, 'not_proven');
  assert.equal(joined.verdict, 'not_proven');
  assert.equal(joined.negative_controls.user_restart_not_used_for_failure_injection, null);
  assert.equal(joined.negative_controls.budget_exhaustion_spawned_no_background_server, null);
});
