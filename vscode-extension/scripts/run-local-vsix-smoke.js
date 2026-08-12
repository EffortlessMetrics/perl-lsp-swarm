#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
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

function initialReceipt(revision) {
  const serverExists = Boolean(serverPath && fs.existsSync(serverPath));
  return {
    schema_version: 'vscode_current_source_smoke.v1',
    receipt_kind: 'vscode_current_source_smoke',
    repository_sha: revision,
    platform: process.platform,
    architecture: process.arch,
    vscode_version: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'unknown',
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
    identityViolations.push('transition receipt platform/architecture does not match the smoke host');
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
  const policyViolations = Array.isArray(report.policy_violations)
    ? report.policy_violations
    : [];
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

function runInventoryTransition(env, expectedRevision) {
  const scriptPath = path.join(__dirname, 'check-vsix-inventory-transition.js');
  const args = [scriptPath];
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

function exitCodeFor(overall) {
  if (overall === 'pass') {
    return 0;
  }
  return overall === 'failed' ? 1 : 2;
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

      receipt.stages.package_inventory = runInventoryTransition(packageEnv, revision);
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
          PERL_LSP_SMOKE_SOURCE_LABEL:
            process.env.PERL_LSP_SMOKE_SOURCE_LABEL || 'local-current-source',
          PERL_LSP_VSIX_SHA256: receipt.vsix.sha256,
        };

        const smokeResult = runNpm(['run', 'test:published'], smokeEnv);
        if (smokeResult.error) {
          receipt.stages.behavioral_smoke = {
            status: 'not_proven',
            exit_code: null,
            reason: smokeResult.error.message,
          };
        } else if (smokeResult.status === 0) {
          receipt.stages.behavioral_smoke = { status: 'pass', exit_code: 0 };
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
  bundleTargetForPlatform,
  computeOverallStatus,
  finalizeSmokeRun,
  initialReceipt,
  interpretTransitionResult,
  receiptPath,
  shouldRunBehavioralSmoke,
  stageServerForPackage,
  writeJsonAtomic,
};
