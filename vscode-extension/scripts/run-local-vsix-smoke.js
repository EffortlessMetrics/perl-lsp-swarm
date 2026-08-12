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
    source_label:
      (process.env.PERL_LSP_SMOKE_SOURCE_LABEL || '').trim() || 'local-current-source',
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
      package_inventory: { status: 'not_proven', reason: 'not_started' },
      behavioral_smoke: { status: 'not_run', reason: 'not_started' },
    },
    instrument_failure: null,
    cleanup_failure: null,
    overall: 'not_proven',
  };
}

function shouldRunBehavioralSmoke(stages) {
  if (stages.package_creation.status !== 'pass') {
    return false;
  }
  return (
    stages.package_inventory.status === 'pass' ||
    (stages.package_inventory.status === 'failed' &&
      stages.package_inventory.classification === 'size_only')
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

function runInventoryCheck(env) {
  const scriptPath = path.join(__dirname, 'check-vsix-inventory.js');
  const result = spawnSync(process.execPath, [scriptPath], {
    cwd: root,
    env,
    encoding: 'utf8',
    windowsHide: true,
  });
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
      reason: result.error.message,
      exit_code: null,
      violations: [],
    };
  }
  if (result.status === null) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      reason: result.signal
        ? `inventory process terminated by ${result.signal}`
        : 'inventory process did not return a status',
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
      reason: `inventory output was not valid JSON: ${
        error instanceof Error ? error.message : String(error)
      }`,
      exit_code: result.status,
      violations: [],
    };
  }

  const classification = report.classification;
  const violations = Array.isArray(report.violations) ? report.violations : [];
  if (result.status === 0 && classification === 'pass' && violations.length === 0) {
    return {
      status: 'pass',
      classification,
      exit_code: 0,
      violations,
      inventory: report,
    };
  }
  if (
    result.status !== 0 &&
    (classification === 'size_only' || classification === 'structural') &&
    violations.length > 0
  ) {
    return {
      status: 'failed',
      classification,
      exit_code: result.status,
      violations,
      inventory: report,
    };
  }
  return {
    status: 'not_proven',
    classification: 'not_proven',
    reason: `inventory exit ${result.status} contradicted classification ${String(classification)}`,
    exit_code: result.status,
    violations,
    inventory: report,
  };
}

function exitCodeFor(overall) {
  if (overall === 'pass') {
    return 0;
  }
  return overall === 'failed' ? 1 : 2;
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

  try {
    restoreStagedServer = stageServerForPackage(serverPath);
    const packageEnv = {
      ...process.env,
      PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
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
        reason: 'package_creation_not_proven',
      };
      receipt.stages.behavioral_smoke = {
        status: 'not_run',
        reason: 'package_creation_not_proven',
      };
      persistReceipt(destination, receipt);
      return exitCodeFor(receipt.overall);
    }
    if (packageResult.status !== 0) {
      receipt.stages.package_creation = {
        status: 'failed',
        exit_code: packageResult.status ?? null,
        reason: 'vsce_package_failed',
      };
      receipt.stages.package_inventory = {
        status: 'not_proven',
        reason: 'package_creation_failed',
      };
      receipt.stages.behavioral_smoke = {
        status: 'not_run',
        reason: 'package_creation_failed',
      };
      persistReceipt(destination, receipt);
      return exitCodeFor(receipt.overall);
    }
    if (!fs.existsSync(vsixPath)) {
      receipt.stages.package_creation = {
        status: 'failed',
        exit_code: 0,
        reason: `vsce did not produce the expected VSIX: ${vsixPath}`,
      };
      receipt.stages.package_inventory = {
        status: 'not_proven',
        reason: 'expected_vsix_missing',
      };
      receipt.stages.behavioral_smoke = {
        status: 'not_run',
        reason: 'expected_vsix_missing',
      };
      persistReceipt(destination, receipt);
      return exitCodeFor(receipt.overall);
    }

    receipt.vsix = {
      path: path.relative(repoRoot, vsixPath).replaceAll('\\', '/'),
      sha256: sha256File(vsixPath),
    };
    receipt.stages.package_creation = { status: 'pass', exit_code: 0 };
    persistReceipt(destination, receipt);

    receipt.stages.package_inventory = runInventoryCheck(packageEnv);
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
            : `inventory_${receipt.stages.package_inventory.classification || 'not_proven'}`,
      };
    }
    persistReceipt(destination, receipt);
    return exitCodeFor(receipt.overall);
  } catch (error) {
    failInstrument(error);
    return exitCodeFor(receipt.overall);
  } finally {
    fs.rmSync(vsixPath, { force: true });
    try {
      restoreStagedServer();
    } catch (error) {
      receipt.cleanup_failure = error instanceof Error ? error.message : String(error);
      process.stderr.write(`Unable to restore staged VSIX server: ${receipt.cleanup_failure}\n`);
      persistReceipt(destination, receipt);
    }
  }
}

if (require.main === module) {
  process.exit(main());
}

module.exports = {
  bundleTargetForPlatform,
  computeOverallStatus,
  initialReceipt,
  receiptPath,
  shouldRunBehavioralSmoke,
  stageServerForPackage,
  writeJsonAtomic,
};
