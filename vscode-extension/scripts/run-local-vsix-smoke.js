#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const repoRoot = path.resolve(root, '..');
const serverPath = process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
const serverSourceRevision = (process.env.PERL_LSP_SERVER_SOURCE_SHA || '').trim();

const npmCommand = process.platform === 'win32' ? 'npm.cmd' : 'npm';

function runNpm(args, env) {
  return spawnSync(npmCommand, args, {
    cwd: root,
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    windowsHide: true,
  });
}

function gitRevision() {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(`Unable to determine source revision: ${result.stderr || result.error}`);
  }
  return result.stdout.trim();
}

function ensureCleanWorkingTree() {
  const result = spawnSync('git', ['status', '--porcelain'], {
    cwd: root,
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    throw new Error(`Unable to inspect working tree: ${result.stderr || result.error}`);
  }
  if (result.stdout.trim()) {
    throw new Error(
      'Working tree has uncommitted changes; commit them before running the local VSIX smoke test.',
    );
  }
}

function bundleTargetForPlatform(platform = process.platform, arch = process.arch) {
  const supportedPlatforms = new Set(['darwin', 'linux', 'win32']);
  const supportedArchitectures = new Set(['x64', 'arm64']);
  if (!supportedPlatforms.has(platform) || !supportedArchitectures.has(arch)) {
    throw new Error(`Unsupported packaged server platform: ${platform}-${arch}`);
  }
  return {
    directory: `${platform}-${arch}`,
    binaryName: platform === 'win32' ? 'perllsp.exe' : 'perllsp',
  };
}

function stageServerForPackage(serverPath, extensionRoot = root) {
  const { directory, binaryName } = bundleTargetForPlatform();
  const binRoot = path.join(extensionRoot, 'bin');
  const platformRoot = path.join(binRoot, directory);
  const destination = path.join(platformRoot, binaryName);
  const existing = fs.existsSync(destination) ? fs.lstatSync(destination) : null;
  if (existing && (!existing.isFile() || existing.isSymbolicLink())) {
    throw new Error(`Refusing to replace non-regular packaged server path: ${destination}`);
  }
  const previous = existing ? { bytes: fs.readFileSync(destination), mode: existing.mode } : null;
  const createdBinRoot = !fs.existsSync(binRoot);
  const createdPlatformRoot = !fs.existsSync(platformRoot);

  const restore = () => {
    if (previous) {
      fs.writeFileSync(destination, previous.bytes);
      fs.chmodSync(destination, previous.mode);
    } else {
      fs.rmSync(destination, { force: true });
    }
    if (
      createdPlatformRoot &&
      fs.existsSync(platformRoot) &&
      fs.readdirSync(platformRoot).length === 0
    ) {
      fs.rmSync(platformRoot, { recursive: true, force: true });
    }
    if (createdBinRoot && fs.existsSync(binRoot) && fs.readdirSync(binRoot).length === 0) {
      fs.rmSync(binRoot, { recursive: true, force: true });
    }
  };

  try {
    fs.mkdirSync(platformRoot, { recursive: true });
    fs.copyFileSync(serverPath, destination);
    if (process.platform !== 'win32') {
      fs.chmodSync(destination, 0o755);
    }
  } catch (error) {
    try {
      restore();
    } catch (cleanupError) {
      process.stderr.write(
        `Unable to clean up failed VSIX server staging: ${
          cleanupError instanceof Error ? cleanupError.message : String(cleanupError)
        }\n`,
      );
    }
    throw error;
  }

  return restore;
}

function sha256File(filePath) {
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function safeFilePart(value) {
  return String(value || 'unknown').replace(/[^A-Za-z0-9_.-]+/g, '-');
}

function receiptsRoot() {
  const configured = (process.env.PERL_LSP_SMOKE_RECEIPTS_DIR || '').trim();
  return configured
    ? path.resolve(configured)
    : path.join(repoRoot, 'target', 'receipts', 'vscode-smoke');
}

function receiptPath(revision, vscodeVersion = process.env.PERL_LSP_VSCODE_VERSION) {
  return path.join(
    receiptsRoot(),
    `current-source-orchestration-${safeFilePart(vscodeVersion)}-${safeFilePart(revision)}.json`,
  );
}

function writeJsonAtomic(destination, value) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${process.pid}.tmp`;
  fs.writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(temporary, destination);
}

/**
 * One stage fact inside the orchestration receipt. Stages stay independent:
 * a stage carries its own status and never inherits another stage's verdict.
 *
 * @typedef {{
 *   status: string,
 *   reason?: string,
 *   exit_code?: number | null,
 *   classification?: string,
 *   behavior_safe?: boolean,
 *   transition_state?: string,
 *   violations?: string[],
 *   transition?: unknown,
 * }} SmokeStage
 */

/**
 * @typedef {{
 *   schema_version: string,
 *   receipt_kind: string,
 *   repository_sha: string,
 *   platform: string,
 *   architecture: string,
 *   vscode_version: string,
 *   source_label: string,
 *   server: { source_sha: string | null, path: string | null, sha256: string | null },
 *   vsix: { path: string | null, sha256: string | null },
 *   stages: {
 *     package_creation: SmokeStage,
 *     package_inventory: SmokeStage,
 *     behavioral_smoke: SmokeStage,
 *     activation_failure_journey: SmokeStage,
 *   },
 *   instrument_failure: string | null,
 *   cleanup_failure: Record<string, string> | null,
 *   overall: string,
 * }} SmokeReceipt
 */

/**
 * @param {string} revision
 * @returns {SmokeReceipt}
 */
function initialReceipt(revision) {
  const serverExists = Boolean(serverPath && fs.existsSync(serverPath));
  return {
    schema_version: 'vscode_current_source_smoke.v1',
    receipt_kind: 'vscode_current_source_smoke',
    repository_sha: revision,
    platform: process.platform,
    architecture: process.arch,
    // One default across receipt and child check: the extension-host child
    // records 'stable' when the matrix version is unset, so we do too.
    vscode_version: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
    source_label: (process.env.PERL_LSP_SMOKE_SOURCE_LABEL || '').trim() || 'local-current-source',
    server: {
      source_sha: serverSourceRevision || null,
      path: serverPath ? path.resolve(serverPath) : null,
      sha256: serverExists ? sha256File(serverPath) : null,
    },
    vsix: {
      path: null,
      sha256: null,
    },
    stages: {
      package_creation: { status: 'not_proven', reason: 'not_started' },
      package_inventory: {
        status: 'not_proven',
        classification: 'not_proven',
        behavior_safe: false,
        reason: 'not_started',
      },
      behavioral_smoke: { status: 'not_run', reason: 'not_started' },
      activation_failure_journey: { status: 'not_run', reason: 'not_started' },
    },
    instrument_failure: null,
    cleanup_failure: null,
    overall: 'not_proven',
  };
}

function shouldRunBehavioralSmoke(stages) {
  return (
    stages.package_creation.status === 'pass' &&
    stages.package_inventory.behavior_safe === true &&
    stages.package_inventory.status !== 'not_proven'
  );
}

/**
 * The packaged activation-failure journey (#7856) needs only a behavior-safe
 * package: it installs the exact VSIX into its own isolated profile and drives
 * both legs itself. It runs independently of the first-hour behavioral stage's
 * outcome so a first-hour regression does not hide activation-recovery
 * evidence (and vice versa).
 */
function shouldRunActivationFailureJourney(stages) {
  return (
    stages.package_creation.status === 'pass' &&
    stages.package_inventory.behavior_safe === true &&
    stages.package_inventory.status !== 'not_proven'
  );
}

function computeOverallStatus(stages, instrumentFailure = null, cleanupFailure = null) {
  if (Object.values(stages).some((stage) => stage.status === 'failed')) {
    return 'failed';
  }
  if (instrumentFailure || cleanupFailure) {
    return 'not_proven';
  }
  if (
    stages.package_creation.status !== 'pass' ||
    stages.package_inventory.status !== 'pass' ||
    stages.behavioral_smoke.status !== 'pass'
  ) {
    return 'not_proven';
  }
  const activationFailure = stages.activation_failure_journey;
  if (
    activationFailure &&
    activationFailure.status !== 'pass' &&
    activationFailure.status !== 'not_run'
  ) {
    return 'not_proven';
  }
  return 'pass';
}

function persistReceipt(destination, receipt) {
  receipt.overall = computeOverallStatus(
    receipt.stages,
    receipt.instrument_failure,
    receipt.cleanup_failure,
  );
  writeJsonAtomic(destination, receipt);
}

function interpretTransitionResult(
  result,
  expectedRevision,
  expectedPlatform = process.platform,
  expectedArchitecture = process.arch,
) {
  if (result.stdout) {
    process.stdout.write(result.stdout);
  }
  if (result.stderr) {
    process.stderr.write(result.stderr);
  }
  if (result.error) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: result.error.message,
      exit_code: null,
      violations: [],
    };
  }
  if (result.status === null) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: result.signal
        ? `transition process terminated by ${result.signal}`
        : 'transition process did not return a status',
      exit_code: null,
      violations: [],
    };
  }

  let report;
  try {
    report = JSON.parse(result.stdout || '');
  } catch (error) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: `transition output was not valid JSON: ${
        error instanceof Error ? error.message : String(error)
      }`,
      exit_code: result.status,
      violations: [],
    };
  }

  const identityViolations = [];
  if (report.schema_version !== 'vsix_inventory_transition.v1') {
    identityViolations.push('unsupported transition receipt schema');
  }
  if (report.receipt_kind !== 'vsix_inventory_transition') {
    identityViolations.push('unexpected transition receipt kind');
  }
  if (report.candidate_sha !== expectedRevision) {
    identityViolations.push('transition receipt candidate SHA does not match the smoke subject');
  }
  if (report.platform !== expectedPlatform || report.architecture !== expectedArchitecture) {
    identityViolations.push(
      'transition receipt platform/architecture does not match the smoke host',
    );
  }
  if (identityViolations.length > 0) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: identityViolations.join('; '),
      exit_code: result.status,
      violations: identityViolations,
      transition: report,
    };
  }

  const classification = report.package_policy_class;
  const knownClassification = ['pass', 'size_only', 'structural', 'not_proven'].includes(
    classification,
  );
  const policyViolations = Array.isArray(report.policy_violations) ? report.policy_violations : [];
  const declarationViolations = Array.isArray(report.declaration_violations)
    ? report.declaration_violations
    : [];
  const violations = [...policyViolations, ...declarationViolations];

  if (
    result.status === 0 &&
    report.passed === true &&
    report.state !== 'not_proven' &&
    classification === 'pass' &&
    report.behavior_safe === true
  ) {
    return {
      status: 'pass',
      classification,
      behavior_safe: true,
      transition_state: report.state,
      exit_code: 0,
      violations,
      transition: report,
    };
  }

  if (
    result.status === 1 &&
    report.passed === false &&
    report.state !== 'not_proven' &&
    knownClassification &&
    classification !== 'not_proven' &&
    typeof report.behavior_safe === 'boolean'
  ) {
    return {
      status: 'failed',
      classification,
      behavior_safe: report.behavior_safe,
      transition_state: report.state,
      exit_code: 1,
      violations,
      transition: report,
    };
  }

  if (
    result.status === 2 &&
    report.state === 'not_proven' &&
    report.passed === false &&
    classification === 'not_proven' &&
    report.behavior_safe === false
  ) {
    return {
      status: 'not_proven',
      classification,
      behavior_safe: false,
      transition_state: report.state,
      reason: report.reason || 'transition instrument did not prove package state',
      exit_code: 2,
      violations,
      transition: report,
    };
  }

  return {
    status: 'not_proven',
    classification: 'not_proven',
    behavior_safe: false,
    reason: `transition exit ${result.status} contradicted state ${String(
      report.state,
    )}, classification ${String(classification)}, and passed=${String(report.passed)}`,
    exit_code: result.status,
    violations,
    transition: report,
  };
}

function runInventoryTransition(env, expectedRevision, vsixPath) {
  const scriptPath = path.join(__dirname, 'check-vsix-inventory-transition.js');
  const args = [scriptPath, '--vsix', vsixPath];
  const explicitBase = (env.PERL_LSP_PACKAGE_BASE_SHA || '').trim();
  if (explicitBase) {
    args.push('--base', explicitBase);
  }
  const result = spawnSync(process.execPath, args, {
    cwd: root,
    env,
    encoding: 'utf8',
    windowsHide: true,
  });
  return interpretTransitionResult(result, expectedRevision);
}

const CHILD_RECEIPT_NAME = 'first_hour_vscode_receipt.json';

function smokeSourceLabel() {
  return (process.env.PERL_LSP_SMOKE_SOURCE_LABEL || '').trim() || 'local-current-source';
}

function smokePlatformLabel() {
  switch (process.platform) {
    case 'win32':
      return 'windows';
    case 'darwin':
      return 'macos';
    case 'linux':
      return 'linux';
    default:
      return process.platform;
  }
}

function childReceiptPath() {
  // The extension-host child nests its receipt by source label and platform
  // (receiptsDir() in src/test/integration/firstHourReceipt.test.ts); the
  // orchestrator must read and clear exactly where the child writes.
  return path.join(receiptsRoot(), smokeSourceLabel(), smokePlatformLabel(), CHILD_RECEIPT_NAME);
}

/**
 * A zero exit code from the extension-host smoke is not behavioral proof: a
 * no-op script, a swallowed receipt write, or a receipt left behind by an
 * earlier run would all produce it. Behavior only counts when the child wrote
 * a fresh receipt that binds itself to this candidate, this VSIX, this server,
 * and this matrix leg.
 *
 * @param {{
 *   receiptFile: string,
 *   expectedRevision: string,
 *   expectedVsixSha256: string | null,
 *   expectedServerSourceSha: string,
 *   expectedVscodeVersion: string,
 *   expectedSourceLabel: string,
 *   readFile?: (file: string) => string,
 *   exists?: (file: string) => boolean,
 * }} input
 * @returns {{ ok: true, receipt: any } | { ok: false, violations: string[] }}
 */
function validateChildSmokeReceipt({
  receiptFile,
  expectedRevision,
  expectedVsixSha256,
  expectedServerSourceSha,
  expectedVscodeVersion,
  expectedSourceLabel,
  readFile = (file) => fs.readFileSync(file, 'utf8'),
  exists = (file) => fs.existsSync(file),
}) {
  if (!exists(receiptFile)) {
    return { ok: false, violations: ['extension-host smoke did not write a first-hour receipt'] };
  }

  let receipt;
  try {
    receipt = JSON.parse(readFile(receiptFile));
  } catch (error) {
    return {
      ok: false,
      violations: [
        `first-hour receipt was not valid JSON: ${
          error instanceof Error ? error.message : String(error)
        }`,
      ],
    };
  }

  const violations = [];
  const environment =
    receipt &&
    typeof receipt === 'object' &&
    receipt.environment &&
    typeof receipt.environment === 'object'
      ? receipt.environment
      : null;
  if (!environment) {
    return { ok: false, violations: ['first-hour receipt is missing its environment identity'] };
  }

  if (receipt.outcome !== 'completed') {
    violations.push(
      `first-hour receipt outcome is ${JSON.stringify(receipt.outcome)}, not completed`,
    );
  }
  if (!Array.isArray(receipt.failures) || receipt.failures.length > 0) {
    violations.push('first-hour receipt reported failures');
  }
  if (environment.source_revision !== expectedRevision) {
    violations.push(
      `first-hour receipt source revision ${JSON.stringify(environment.source_revision)} is not the smoke subject`,
    );
  }
  if (environment.server_source_revision !== expectedServerSourceSha) {
    violations.push(
      `first-hour receipt server source revision ${JSON.stringify(environment.server_source_revision)} is not the staged server`,
    );
  }
  if (!expectedVsixSha256 || environment.vsix_sha256 !== expectedVsixSha256) {
    violations.push(
      `first-hour receipt VSIX digest ${JSON.stringify(environment.vsix_sha256)} is not the package this run created`,
    );
  }
  if (environment.requested_vscode_version !== expectedVscodeVersion) {
    violations.push(
      `first-hour receipt VS Code version ${JSON.stringify(environment.requested_vscode_version)} is not this matrix leg`,
    );
  }
  if (environment.extension_id !== 'EffortlessMetrics.perl-lsp-rs') {
    violations.push(
      `first-hour receipt extension id ${JSON.stringify(environment.extension_id)} is not the packaged extension`,
    );
  }
  if (expectedSourceLabel && receipt.source_label && receipt.source_label !== expectedSourceLabel) {
    violations.push(
      `first-hour receipt source label ${JSON.stringify(receipt.source_label)} is not this run's label`,
    );
  }

  return violations.length > 0 ? { ok: false, violations } : { ok: true, receipt };
}

function exitCodeFor(overall) {
  if (overall === 'pass') {
    return 0;
  }
  return overall === 'failed' ? 1 : 2;
}

const ACTIVATION_FAILURE_LEG_SCHEMA = 'vscode_activation_recovery_leg.v1';
const ACTIVATION_FAILURE_RECEIPTS = {
  failure: 'activation_failure_journey_failure_receipt.json',
  retry: 'activation_failure_journey_retry_receipt.json',
  joined: 'vscode_activation_recovery_receipt.json',
};

/**
 * Validate both activation-failure journey child receipts against this run's
 * exact candidate identity. A zero leg exit code is not proof by itself: the
 * children must have written fresh, candidate-bound receipts with passing
 * verdicts, exactly like the first-hour behavioral child.
 *
 * @param {{
 *   failureReceiptFile: string,
 *   retryReceiptFile: string,
 *   expectedVsixSha256: string,
 *   expectedBundledServerSha256: string,
 *   expectedExtensionVersion: string,
 *   readFile?: (file: string) => string,
 *   exists?: (file: string) => boolean,
 * }} input
 * @returns {{ ok: boolean, violations: string[], failure: any | null, retry: any | null }}
 */
function validateActivationRecoveryChildReceipts({
  failureReceiptFile,
  retryReceiptFile,
  expectedVsixSha256,
  expectedBundledServerSha256,
  expectedExtensionVersion,
  readFile = (file) => fs.readFileSync(file, 'utf8'),
  exists = (file) => fs.existsSync(file),
}) {
  const violations = [];
  const readChild = (file, leg) => {
    if (!exists(file)) {
      violations.push(`the ${leg} leg did not write its journey receipt`);
      return null;
    }
    try {
      return JSON.parse(readFile(file));
    } catch (error) {
      violations.push(
        `the ${leg} leg receipt was not valid JSON: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
      return null;
    }
  };
  const failure = readChild(failureReceiptFile, 'failure');
  const retry = readChild(retryReceiptFile, 'retry');
  const bindChild = (receipt, leg) => {
    if (!receipt) {
      return;
    }
    if (receipt.schema_version !== ACTIVATION_FAILURE_LEG_SCHEMA) {
      violations.push(`the ${leg} leg receipt schema is ${JSON.stringify(receipt.schema_version)}`);
    }
    if (receipt.leg !== leg) {
      violations.push(`the ${leg} leg receipt records leg ${JSON.stringify(receipt.leg)}`);
    }
    if (receipt.verdict !== 'pass') {
      violations.push(`the ${leg} leg verdict is ${JSON.stringify(receipt.verdict)}`);
    }
    const candidate = receipt.candidate || {};
    if (candidate.vsix_sha256 !== expectedVsixSha256) {
      violations.push(
        `the ${leg} leg receipt VSIX digest ${JSON.stringify(candidate.vsix_sha256)} is not this run's package`,
      );
    }
    if (
      !candidate.bundled_server ||
      candidate.bundled_server.sha256 !== expectedBundledServerSha256
    ) {
      violations.push(
        `the ${leg} leg receipt bundled-server digest is not this run's staged server`,
      );
    }
    if (candidate.extension_version !== expectedExtensionVersion) {
      violations.push(
        `the ${leg} leg receipt extension version ${JSON.stringify(candidate.extension_version)} is not the packaged version`,
      );
    }
  };
  bindChild(failure, 'failure');
  bindChild(retry, 'retry');
  return { ok: violations.length === 0, violations, failure, retry };
}

/**
 * Compose the joined `vscode_activation_recovery.v1` receipt (#7856) from the
 * two child legs and the orchestrator's own post-host-exit process scan. The
 * verdict is fail-closed: any missing or contradictory child evidence leaves
 * the affected row `not_proven`, and a surviving bundled-server process after
 * the retry host exited fails the deactivation row outright.
 *
 * @param {{
 *   vsixSha256: string,
 *   extensionVersion: string,
 *   bundledServerSha256: string,
 *   serverSourceRevision: string | null,
 *   repositorySha: string,
 *   vscodeVersion: string,
 *   workspaceFixtureSha256: string,
 *   failure: any | null,
 *   retry: any | null,
 *   violations: string[],
 *   legExitCodes: { failure: number | null, retry: number | null },
 *   postHostExitProcesses: string[],
 * }} input
 */
function composeActivationRecoveryReceipt({
  vsixSha256,
  extensionVersion,
  bundledServerSha256,
  serverSourceRevision,
  repositorySha,
  vscodeVersion,
  workspaceFixtureSha256,
  failure,
  retry,
  violations,
  legExitCodes,
  postHostExitProcesses,
}) {
  const failureObservations = (failure && failure.observations) || {};
  const retryObservations = (retry && retry.observations) || {};
  // Both legs must have exited cleanly AND written passing, bound receipts; a
  // nonzero leg exit (runner or host teardown failure) cannot stand behind a
  // passing verdict even when the child observations passed.
  const legsExitedCleanly = legExitCodes.failure === 0 && legExitCodes.retry === 0;
  const childrenBound =
    violations.length === 0 &&
    legsExitedCleanly &&
    failure &&
    retry &&
    failure.verdict === 'pass' &&
    retry.verdict === 'pass';
  // An OBSERVED product failure (a child verdict of failed, or a leg that
  // exited nonzero after writing evidence) is a failed receipt, not an
  // unproven one; not_proven is reserved for missing or unbound evidence.
  const observedFailure =
    (failure && failure.verdict === 'failed') || (retry && retry.verdict === 'failed');

  const countObservation = (value) => (Array.isArray(value) ? value.length : null);
  const failureProcessesAfterDemand = countObservation(
    failureObservations.bundled_server_processes_after_demand_window,
  );
  const retryRunningProcesses = countObservation(
    retryObservations.bundled_server_processes_running,
  );
  const retryProcessesAfterSecondDemand = countObservation(
    retryObservations.bundled_server_processes_after_second_demand,
  );
  const retryProcessesAfterStop = countObservation(
    retryObservations.bundled_server_processes_after_stop,
  );

  let duplicateResources = 0;
  let duplicateObserved = true;
  for (const count of [retryRunningProcesses, retryProcessesAfterSecondDemand]) {
    if (count !== null && count >= 1) {
      duplicateResources += count - 1;
    } else {
      duplicateObserved = false;
    }
  }

  const stopClean = retryProcessesAfterStop === 0;
  const hostExitClean = postHostExitProcesses.length === 0;

  const legRow = (child, exitCode) => {
    if (!child) {
      return 'not_proven';
    }
    if (child.verdict !== 'pass') {
      return 'failed';
    }
    // The child observed passing behavior, but its leg run did not exit
    // cleanly: an execution-integrity gap is not a product failure, so the
    // row cannot claim a completed pass either.
    return exitCode === 0 ? 'pass' : 'not_proven';
  };
  const cleanupRow = legRow(failure, legExitCodes.failure);
  const retryRow = legRow(retry, legExitCodes.retry);
  const deactivationRow = !childrenBound
    ? 'not_proven'
    : stopClean && hostExitClean
      ? 'pass'
      : 'failed';

  let verdict;
  if (observedFailure) {
    verdict = 'failed';
  } else if (!childrenBound) {
    verdict = 'not_proven';
  } else if (!stopClean || !hostExitClean) {
    verdict = 'failed';
  } else {
    verdict = 'pass';
  }

  return {
    schema_version: 'vscode_activation_recovery.v1',
    receipt_kind: 'vscode_activation_recovery',
    repository_sha: repositorySha,
    candidate: {
      extension_id: 'EffortlessMetrics.perl-lsp-rs',
      extension_version: extensionVersion,
      vsix_sha256: vsixSha256,
      bundled_server_sha256: bundledServerSha256,
      server_source_sha: serverSourceRevision || null,
      vscode_version: vscodeVersion,
      platform: process.platform,
      architecture: process.arch,
      workspace_fixture_sha256: workspaceFixtureSha256,
      profile: 'one isolated shared profile across the failure and retry legs',
      activation_schema: 'ExtensionActivationOwner/ActivationTransaction (#7854 wiring)',
    },
    failure: {
      phase: 'debugger-1 (harness-injected pre-commit boundary)',
      terminal_state: 'activation_failed',
      process_remaining:
        failureProcessesAfterDemand === null ? null : failureProcessesAfterDemand > 0,
      crash_budget_consumed: false,
      crash_budget_evidence: failureObservations.crash_budget_evidence || [],
      cleanup: cleanupRow,
    },
    retry: {
      activation: retryRow,
      duplicate_resources: duplicateObserved ? duplicateResources : 'not_proven',
      provider_smoke: retryRow,
      server_identity: 'bundled',
    },
    deactivation: deactivationRow,
    legs: {
      failure_exit_code: legExitCodes.failure,
      retry_exit_code: legExitCodes.retry,
      failure_receipt: failure,
      retry_receipt: retry,
    },
    orchestrator_observations: {
      post_host_exit_bundled_server_processes: postHostExitProcesses,
    },
    instrument_violations: violations,
    verdict,
  };
}

/**
 * Orchestrator-side bundled-server process scan. Mirrors the in-host scan from
 * journeySupport.ts but runs in plain Node after the extension host exited,
 * proving the terminal deactivate path left no candidate server behind.
 * Fail-closed: a nonzero scanner exit is an instrument failure, never an
 * observation of zero processes.
 */
function scanBundledServerProcesses(directory) {
  const resolved = path.resolve(directory);
  let command;
  let args;
  if (process.platform === 'win32') {
    command = 'powershell.exe';
    args = [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '(Get-Process -Name perllsp,perl-lsp -ErrorAction SilentlyContinue).Path',
    ];
  } else {
    command = 'ps';
    args = ['-eo', 'args='];
  }
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    windowsHide: true,
    timeout: 30_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) {
    throw new Error(`bundled-server process scan failed to spawn: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `bundled-server process scan exited ${result.status}: ${(result.stderr || '').slice(0, 200)}`,
    );
  }
  const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin';
  const needle = caseInsensitive ? resolved.toLowerCase() : resolved;
  return (result.stdout || '')
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => {
      if (line.length === 0) {
        return false;
      }
      const haystack = caseInsensitive ? line.toLowerCase() : line;
      return haystack.startsWith(needle);
    });
}

/**
 * Environment for one activation-failure journey leg. Both legs install the
 * exact packaged VSIX into the SAME isolated profile; only the failure leg
 * carries the harness fault, so the retry leg is the explicit reload path a
 * user takes with the fault removed.
 */
function activationFailureLegEnv(baseEnv, leg, fault, context) {
  const env = {
    ...baseEnv,
    PERL_LSP_ACTIVATION_FAILURE_SMOKE: '1',
    PERL_LSP_ACTIVATION_FAILURE_LEG: leg,
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
    PERL_LSP_PUBLISHED_VSIX_PATH: context.vsixPath,
    PERL_LSP_SMOKE_WORKSPACE: context.workspacePath,
    PERL_LSP_SMOKE_USER_DATA_DIR: context.userDataDir,
    PERL_LSP_SMOKE_EXTENSIONS_DIR: context.extensionsDir,
    PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
    PERL_LSP_SMOKE_SOURCE_LABEL: smokeSourceLabel(),
    PERL_LSP_VSIX_SHA256: context.vsixSha256,
    // PERL_LSP_CURRENT_SOURCE_SHA deliberately NOT set: it switches the
    // published smoke into candidate-bound mode, which is Linux-only; the
    // repository SHA rides on this orchestration receipt instead.
    PERL_LSP_SERVER_SOURCE_SHA: context.serverSourceRevision,
  };
  if (fault) {
    env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE = 'debugger';
  } else {
    delete env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE;
  }
  // This journey selects its own suite and resolves the bundled candidate;
  // the first-hour selectors and the current-source server override must not
  // leak into it.
  delete env.PERL_LSP_CURRENT_SOURCE_SMOKE;
  delete env.PERL_LSP_PACKAGED_BUNDLE_SMOKE;
  delete env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
  delete env.PERL_LSP_CURRENT_SOURCE_SHA;
  return env;
}

/**
 * Run the two-leg packaged activation-failure journey (#7856) against the
 * exact VSIX this run packaged: failure leg (fault armed), retry leg (fault
 * removed, same installed profile — the explicit reload path), then the
 * orchestrator's post-host-exit scan and the joined receipt.
 */
function runActivationFailureJourneyStage(baseEnv, revision, vsixPath, vsixSha256) {
  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const extensionVersion = packageJson.version;
  const bundledServerSha256 = sha256File(serverPath);
  const workspacePath = fs.mkdtempSync(
    path.join(os.tmpdir(), 'perl-lsp-activation-failure-workspace-'),
  );
  const profilePath = fs.mkdtempSync(
    path.join(os.tmpdir(), 'perl-lsp-activation-failure-profile-'),
  );
  const userDataDir = path.join(profilePath, 'user-data');
  const extensionsDir = path.join(profilePath, 'extensions');
  fs.mkdirSync(userDataDir, { recursive: true });
  fs.mkdirSync(extensionsDir, { recursive: true });
  const fixturePath = path.join(workspacePath, 'activation_recovery.pl');
  fs.writeFileSync(fixturePath, 'use strict;\nuse warnings;\n\nmy $value = 42;\nprint $value;\n');
  const platformReceiptsDir = path.join(receiptsRoot(), smokeSourceLabel(), smokePlatformLabel());
  const failureReceiptFile = path.join(platformReceiptsDir, ACTIVATION_FAILURE_RECEIPTS.failure);
  const retryReceiptFile = path.join(platformReceiptsDir, ACTIVATION_FAILURE_RECEIPTS.retry);
  const joinedReceiptFile = path.join(platformReceiptsDir, ACTIVATION_FAILURE_RECEIPTS.joined);
  // Stale receipts from an earlier run can never stand in for this run's
  // evidence, exactly like the first-hour child receipt guard.
  for (const stale of [failureReceiptFile, retryReceiptFile, joinedReceiptFile]) {
    fs.rmSync(stale, { force: true });
  }
  const context = {
    vsixPath,
    vsixSha256,
    revision,
    serverSourceRevision,
    workspacePath,
    userDataDir,
    extensionsDir,
  };
  const paths = {
    failureReceiptFile,
    retryReceiptFile,
    joinedReceiptFile,
    extensionVersion,
    bundledServerSha256,
    fixturePath,
  };

  /** @type {string | null} */
  let cleanupFailure = null;
  let stage;
  try {
    stage = runActivationFailureJourneyAttempt(baseEnv, context, paths);
  } finally {
    for (const directory of [workspacePath, profilePath]) {
      try {
        fs.rmSync(directory, { recursive: true, force: true });
      } catch (error) {
        cleanupFailure = error instanceof Error ? error.message : String(error);
        process.stderr.write(
          `[activation-failure-journey] cleanup failed for ${directory}: ${cleanupFailure}\n`,
        );
      }
    }
  }
  // A journey whose isolated profile/workspace could not be removed is not a
  // clean pass — the same cleanup contract the outer VSIX/server staging
  // follows. An observed product failure stays failed; anything else that was
  // passing becomes not_proven.
  if (cleanupFailure && stage.status !== 'failed') {
    return {
      ...stage,
      status: 'not_proven',
      reason: `journey directories could not be cleaned: ${cleanupFailure}`,
    };
  }
  return stage;
}

/**
 * The two legs and the joined receipt, isolated from directory lifecycle so
 * the owning stage can apply its cleanup contract to the result.
 */
function runActivationFailureJourneyAttempt(baseEnv, context, paths) {
  const {
    failureReceiptFile,
    retryReceiptFile,
    joinedReceiptFile,
    extensionVersion,
    bundledServerSha256,
    fixturePath,
  } = paths;
  const revision = context.revision;
  const vsixSha256 = context.vsixSha256;
  const extensionsDir = context.extensionsDir;
  /** @type {{ failure: number | null, retry: number | null }} */
  const legExitCodes = { failure: null, retry: null };
  const failureResult = runNpm(
    ['run', 'test:published'],
    activationFailureLegEnv(baseEnv, 'failure', true, context),
  );
  legExitCodes.failure = failureResult.error ? null : (failureResult.status ?? null);
  if (failureResult.error) {
    return {
      status: 'not_proven',
      reason: `failure leg could not run: ${failureResult.error.message}`,
    };
  }
  const retryResult = runNpm(
    ['run', 'test:published'],
    activationFailureLegEnv(baseEnv, 'retry', false, context),
  );
  legExitCodes.retry = retryResult.error ? null : (retryResult.status ?? null);
  if (retryResult.error) {
    return {
      status: 'not_proven',
      reason: `retry leg could not run: ${retryResult.error.message}`,
      exit_codes: legExitCodes,
    };
  }

  let postHostExitProcesses;
  try {
    postHostExitProcesses = scanBundledServerProcesses(extensionsDir);
  } catch (error) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: `post-host-exit process scan failed: ${
        error instanceof Error ? error.message : String(error)
      }`,
    };
  }

  const validation = validateActivationRecoveryChildReceipts({
    failureReceiptFile,
    retryReceiptFile,
    expectedVsixSha256: vsixSha256,
    expectedBundledServerSha256: bundledServerSha256,
    expectedExtensionVersion: extensionVersion,
  });
  const joined = composeActivationRecoveryReceipt({
    vsixSha256,
    extensionVersion,
    bundledServerSha256,
    serverSourceRevision,
    repositorySha: revision,
    vscodeVersion: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
    workspaceFixtureSha256: sha256File(fixturePath),
    failure: validation.failure,
    retry: validation.retry,
    violations: validation.violations,
    legExitCodes,
    postHostExitProcesses,
  });
  writeJsonAtomic(joinedReceiptFile, joined);

  if (legExitCodes.failure !== 0 || legExitCodes.retry !== 0) {
    return {
      status: 'failed',
      exit_codes: legExitCodes,
      reason: 'activation_failure_journey_leg_failed',
      recovery_verdict: joined.verdict,
    };
  }
  if (!validation.ok) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: 'journey child receipts did not bind this run',
      violations: validation.violations,
      recovery_verdict: joined.verdict,
    };
  }
  return {
    status: joined.verdict === 'pass' ? 'pass' : 'not_proven',
    exit_codes: legExitCodes,
    recovery_verdict: joined.verdict,
    receipt: path.relative(repoRoot, joinedReceiptFile).replaceAll('\\', '/'),
  };
}

function finalizeSmokeRun(
  destination,
  receipt,
  vsixPath,
  restoreStagedServer,
  removeVsix = (target) => fs.rmSync(target, { force: true }),
  persist = persistReceipt,
) {
  const cleanupFailure = {};

  try {
    removeVsix(vsixPath);
  } catch (error) {
    cleanupFailure.vsix_deletion = error instanceof Error ? error.message : String(error);
    process.stderr.write(`Unable to delete staged VSIX: ${cleanupFailure.vsix_deletion}\n`);
  }

  try {
    restoreStagedServer();
  } catch (error) {
    cleanupFailure.staged_server_restoration =
      error instanceof Error ? error.message : String(error);
    process.stderr.write(
      `Unable to restore staged VSIX server: ${cleanupFailure.staged_server_restoration}\n`,
    );
  }

  receipt.cleanup_failure = Object.keys(cleanupFailure).length > 0 ? cleanupFailure : null;
  persist(destination, receipt);
  return exitCodeFor(receipt.overall);
}

function main() {
  let revision = 'unknown';
  try {
    revision = gitRevision();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  }

  const destination = receiptPath(revision);
  const receipt = initialReceipt(revision);
  persistReceipt(destination, receipt);

  const failInstrument = (error) => {
    const message = error instanceof Error ? error.message : String(error);
    receipt.instrument_failure = message;
    if (receipt.stages.package_creation.status === 'not_proven') {
      receipt.stages.package_creation = { status: 'not_proven', reason: message };
    }
    if (receipt.stages.package_inventory.reason === 'not_started') {
      receipt.stages.package_inventory = {
        status: 'not_proven',
        classification: 'not_proven',
        behavior_safe: false,
        reason: 'package_creation_or_input_validation_failed',
      };
    }
    if (receipt.stages.behavioral_smoke.reason === 'not_started') {
      receipt.stages.behavioral_smoke = {
        status: 'not_run',
        reason: 'package_creation_or_input_validation_failed',
      };
    }
    process.stderr.write(`${message}\n`);
    persistReceipt(destination, receipt);
  };

  if (!serverPath || !fs.existsSync(serverPath)) {
    failInstrument(
      new Error(
        'PERL_LSP_FIRST_HOUR_SERVER_PATH must point to an existing server built from the current source revision.',
      ),
    );
    return exitCodeFor(receipt.overall);
  }
  if (!serverSourceRevision) {
    failInstrument(
      new Error(
        'PERL_LSP_SERVER_SOURCE_SHA must identify the source revision used to build the server.',
      ),
    );
    return exitCodeFor(receipt.overall);
  }

  try {
    ensureCleanWorkingTree();
    if (revision === 'unknown') {
      throw new Error('Extension source revision is not available.');
    }
    if (serverSourceRevision !== revision) {
      throw new Error(
        `Server source revision ${serverSourceRevision} does not match extension source revision ${revision}`,
      );
    }
  } catch (error) {
    failInstrument(error);
    return exitCodeFor(receipt.overall);
  }

  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const vsixPath = path.join(root, `${packageJson.name}-${packageJson.version}.vsix`);
  let restoreStagedServer = () => {};

  const runStageBody = () => {
    try {
      restoreStagedServer = stageServerForPackage(serverPath);
      const packageEnv = {
        ...process.env,
        PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
        PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
      };
      const packageResult = runNpm(
        ['exec', '--offline', '--no', '--', '@vscode/vsce', 'package'],
        packageEnv,
      );
      if (packageResult.error) {
        receipt.stages.package_creation = {
          status: 'not_proven',
          reason: packageResult.error.message,
          exit_code: null,
        };
        receipt.stages.package_inventory = {
          status: 'not_proven',
          classification: 'not_proven',
          behavior_safe: false,
          reason: 'package_creation_not_proven',
        };
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason: 'package_creation_not_proven',
        };
        persistReceipt(destination, receipt);
        return;
      }
      if (packageResult.status !== 0) {
        receipt.stages.package_creation = {
          status: 'failed',
          exit_code: packageResult.status ?? null,
          reason: 'vsce_package_failed',
        };
        receipt.stages.package_inventory = {
          status: 'not_proven',
          classification: 'not_proven',
          behavior_safe: false,
          reason: 'package_creation_failed',
        };
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason: 'package_creation_failed',
        };
        persistReceipt(destination, receipt);
        return;
      }
      if (!fs.existsSync(vsixPath)) {
        receipt.stages.package_creation = {
          status: 'failed',
          exit_code: 0,
          reason: `vsce did not produce the expected VSIX: ${vsixPath}`,
        };
        receipt.stages.package_inventory = {
          status: 'not_proven',
          classification: 'not_proven',
          behavior_safe: false,
          reason: 'expected_vsix_missing',
        };
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason: 'expected_vsix_missing',
        };
        persistReceipt(destination, receipt);
        return;
      }

      receipt.vsix = {
        path: path.relative(repoRoot, vsixPath).replaceAll('\\', '/'),
        sha256: sha256File(vsixPath),
      };
      receipt.stages.package_creation = { status: 'pass', exit_code: 0 };
      persistReceipt(destination, receipt);

      receipt.stages.package_inventory = runInventoryTransition(packageEnv, revision, vsixPath);
      persistReceipt(destination, receipt);

      if (shouldRunBehavioralSmoke(receipt.stages)) {
        const smokeEnv = {
          ...process.env,
          PERL_LSP_CURRENT_SOURCE_SHA: revision,
          PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
          PERL_LSP_FIRST_HOUR_ONLY: '1',
          PERL_LSP_FIRST_HOUR_RECEIPT: '1',
          PERL_LSP_FIRST_HOUR_SERVER_PATH: path.resolve(serverPath),
          PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
          PERL_LSP_PUBLISHED_VSIX_PATH: vsixPath,
          PERL_LSP_SERVER_SOURCE_SHA: serverSourceRevision,
          PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
          PERL_LSP_SMOKE_SOURCE_LABEL: smokeSourceLabel(),
          PERL_LSP_VSIX_SHA256: receipt.vsix.sha256,
        };

        // Clear any receipt left by an earlier run so a stale artifact can
        // never be mistaken for this run's behavioral evidence.
        const childReceiptFile = childReceiptPath();
        try {
          fs.rmSync(childReceiptFile, { force: true });
        } catch (error) {
          receipt.stages.behavioral_smoke = {
            status: 'not_proven',
            exit_code: null,
            reason: `unable to clear the previous first-hour receipt: ${
              error instanceof Error ? error.message : String(error)
            }`,
          };
          persistReceipt(destination, receipt);
          return;
        }

        const smokeResult = runNpm(['run', 'test:published'], smokeEnv);
        if (smokeResult.error) {
          receipt.stages.behavioral_smoke = {
            status: 'not_proven',
            exit_code: null,
            reason: smokeResult.error.message,
          };
        } else if (smokeResult.status === 0) {
          const childReceipt = validateChildSmokeReceipt({
            receiptFile: childReceiptFile,
            expectedRevision: revision,
            expectedVsixSha256: receipt.vsix.sha256,
            expectedServerSourceSha: serverSourceRevision,
            // Mirror the child's own default so an unset matrix version is not
            // reported as an identity mismatch.
            expectedVscodeVersion: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
            expectedSourceLabel: receipt.source_label,
          });
          receipt.stages.behavioral_smoke = childReceipt.ok
            ? { status: 'pass', exit_code: 0 }
            : {
                status: 'not_proven',
                exit_code: 0,
                reason: 'child_receipt_did_not_bind_this_run',
                violations: childReceipt.violations,
              };
        } else {
          receipt.stages.behavioral_smoke = {
            status: 'failed',
            exit_code: smokeResult.status ?? null,
            reason: 'published_extension_smoke_failed',
          };
        }
      } else {
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason:
            receipt.stages.package_creation.status !== 'pass'
              ? 'package_creation_not_passed'
              : `inventory_${
                  receipt.stages.package_inventory.transition_state ||
                  receipt.stages.package_inventory.classification ||
                  'not_proven'
                }`,
        };
      }

      // The packaged activation-failure journey (#7856): two legs against the
      // exact VSIX this run packaged, joined into one candidate-bound
      // vscode_activation_recovery.v1 receipt.
      if (shouldRunActivationFailureJourney(receipt.stages)) {
        receipt.stages.activation_failure_journey = runActivationFailureJourneyStage(
          packageEnv,
          revision,
          vsixPath,
          receipt.vsix.sha256,
        );
      } else {
        receipt.stages.activation_failure_journey = {
          status: 'not_run',
          reason:
            receipt.stages.package_creation.status !== 'pass'
              ? 'package_creation_not_passed'
              : `inventory_${
                  receipt.stages.package_inventory.transition_state ||
                  receipt.stages.package_inventory.classification ||
                  'not_proven'
                }`,
        };
      }
      persistReceipt(destination, receipt);
    } catch (error) {
      failInstrument(error);
    }
  };

  runStageBody();
  return finalizeSmokeRun(destination, receipt, vsixPath, restoreStagedServer);
}

if (require.main === module) {
  process.exit(main());
}

module.exports = {
  ACTIVATION_FAILURE_RECEIPTS,
  activationFailureLegEnv,
  bundleTargetForPlatform,
  childReceiptPath,
  composeActivationRecoveryReceipt,
  computeOverallStatus,
  finalizeSmokeRun,
  initialReceipt,
  interpretTransitionResult,
  receiptPath,
  scanBundledServerProcesses,
  shouldRunActivationFailureJourney,
  shouldRunBehavioralSmoke,
  validateActivationRecoveryChildReceipts,
  validateChildSmokeReceipt,
  stageServerForPackage,
  writeJsonAtomic,
};
