#!/usr/bin/env node

const path = require('node:path');
const { spawnSync } = require('node:child_process');

const extensionRoot = path.resolve(__dirname, '..');
const vsixName = 'perl-lsp-rs.vsix';
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

try {
  if (!runNode(vsceEntry, ['package', '--out', vsixName])) {
    process.exit(process.exitCode);
  }
  runNode(path.join(__dirname, 'check-vsix-inventory-transition.js'), ['--vsix', vsixName]);
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
