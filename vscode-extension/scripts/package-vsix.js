#!/usr/bin/env node

const path = require('node:path');
const fs = require('node:fs');
const { spawnSync } = require('node:child_process');

const extensionRoot = path.resolve(__dirname, '..');
const packageManifest = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
);
const vsixName = `perl-lsp-rs-${packageManifest.version}.vsix`;
const vsixPath = path.join(extensionRoot, vsixName);
const vsceEntry = path.join(extensionRoot, 'node_modules', '@vscode', 'vsce', 'vsce');

function runNode(script, args) {
  const result = spawnSync(process.execPath, [script, ...args], {
    cwd: extensionRoot,
    stdio: 'inherit',
    windowsHide: true,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exitCode = result.status ?? 1;
    return false;
  }
  return true;
}

function packageVsix(run = runNode, fileSystem = fs) {
  if (fileSystem.existsSync(vsixPath)) {
    fileSystem.rmSync(vsixPath, { force: true });
  }
  if (!run(vsceEntry, ['package', '--out', vsixName])) {
    return false;
  }
  let artifact;
  try {
    artifact = fileSystem.statSync(vsixPath);
  } catch {
    throw new Error(`packager exited successfully without producing ${vsixName}`);
  }
  if (!artifact.isFile() || artifact.size <= 0) {
    throw new Error(`packager exited successfully without producing ${vsixName}`);
  }
  return run(path.join(__dirname, 'check-vsix-inventory.js'), ['--vsix', vsixName]);
}

if (require.main === module) {
  try {
    if (!packageVsix()) {
      process.exitCode ||= 1;
    }
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}

module.exports = { packageVsix, vsixName, vsceEntry };
