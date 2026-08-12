#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
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

function runNode(args, env) {
  return spawnSync(process.execPath, args, {
    cwd: root,
    env,
    stdio: 'inherit',
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

function main() {
  if (!serverPath || !fs.existsSync(serverPath)) {
    process.stderr.write(
      'PERL_LSP_FIRST_HOUR_SERVER_PATH must point to an existing server built from the current source revision.\n',
    );
    process.exit(2);
  }
  if (!serverSourceRevision) {
    process.stderr.write(
      'PERL_LSP_SERVER_SOURCE_SHA must identify the source revision used to build the server.\n',
    );
    process.exit(2);
  }
  const revision = gitRevision();
  ensureCleanWorkingTree();
  if (serverSourceRevision !== revision) {
    throw new Error(
      `Server source revision ${serverSourceRevision} does not match extension source revision ${revision}`,
    );
  }
  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const vsixPath = path.join(root, `${packageJson.name}-${packageJson.version}.vsix`);

  let smokeStatus = 1;
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
    if (packageResult.status !== 0) {
      smokeStatus = packageResult.status ?? 1;
    } else {
      if (!fs.existsSync(vsixPath)) {
        throw new Error(`vsce did not produce the expected VSIX: ${vsixPath}`);
      }

      const transitionResult = runNode(
        ['scripts/check-vsix-inventory-transition.js'],
        packageEnv,
      );
      if (transitionResult.status !== 0) {
        smokeStatus = transitionResult.status ?? 1;
      } else {
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
          PERL_LSP_SMOKE_SOURCE_LABEL:
            process.env.PERL_LSP_SMOKE_SOURCE_LABEL || 'local-current-source',
          PERL_LSP_VSIX_SHA256: crypto
            .createHash('sha256')
            .update(fs.readFileSync(vsixPath))
            .digest('hex'),
        };

        const smokeResult = runNpm(['run', 'test:published'], smokeEnv);
        smokeStatus = smokeResult.status ?? 1;
      }
    }
  } finally {
    fs.rmSync(vsixPath, { force: true });
    restoreStagedServer();
  }
  process.exit(smokeStatus);
}

if (require.main === module) {
  main();
}

module.exports = { bundleTargetForPlatform, stageServerForPackage };
