const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { bundleTargetForPlatform, stageServerForPackage } = require('./run-local-vsix-smoke');

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
