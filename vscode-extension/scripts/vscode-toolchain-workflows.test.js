'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..');
const extensionWorkflows = [
  'vscode-current-source-linux-smoke.yml',
  'vscode-managed-binary-smoke.yml',
  'vscode-published-extension-smoke.yml',
  'ux-regression-gate.yml',
  'publish-extension.yml',
];

function readWorkflow(name) {
  return fs.readFileSync(path.join(repositoryRoot, '.github', 'workflows', name), 'utf8');
}

void test('extension workflows use the repository-owned setup action', () => {
  for (const name of extensionWorkflows) {
    const source = readWorkflow(name);
    assert.match(source, /uses: \.\/\.github\/actions\/setup-vscode-toolchain/);
    assert.doesNotMatch(source, /actions\/setup-node@/);
    assert.doesNotMatch(source, /npm install --global npm@/);
    assert.doesNotMatch(source, /npm install -g (?:@vscode\/vsce|ovsx)/);
    assert.doesNotMatch(source, /npx ovsx/);
  }
});

void test('the setup action verifies the authority before npm ci', () => {
  const source = fs.readFileSync(
    path.join(repositoryRoot, '.github', 'actions', 'setup-vscode-toolchain', 'action.yml'),
    'utf8',
  );
  const verifyIndex = source.indexOf('run: npm run doctor');
  const installIndex = source.indexOf('run: npm ci');
  assert.notEqual(verifyIndex, -1);
  assert.notEqual(installIndex, -1);
  assert.ok(verifyIndex < installIndex);
  assert.match(source, /node-version: ['"]26\.5\.0['"]/);
  assert.match(source, /npm install --global npm@11\.18\.0/);
});

void test('current-source smoke does not reinstall dependencies after setup', () => {
  const source = readWorkflow('vscode-current-source-linux-smoke.yml');
  assert.doesNotMatch(source, /name: Install extension dependencies/);
  assert.doesNotMatch(source, /\bnpm\s+(?:ci|install)\b/);
});

void test('publisher workflow invokes both CLIs offline through npm exec', () => {
  const source = readWorkflow('publish-extension.yml');
  assert.match(source, /npm exec --offline --no -- @vscode\/vsce publish/);
  assert.match(source, /npm exec --offline --no -- ovsx publish/);
  assert.doesNotMatch(source, /^\s+run: (?:vsce|ovsx) publish/m);
  assert.doesNotMatch(source, /^\s+run: ovsx --version/m);
});

void test('managed-binary smoke proves TypeScript authority before compilation on every OS', () => {
  const source = readWorkflow('vscode-managed-binary-smoke.yml');
  const setupIndex = source.indexOf('- name: Setup VS Code toolchain');
  const shimTestIndex = source.indexOf('- name: Test TypeScript authority shim parsing');
  const authorityIndex = source.indexOf('- name: Verify TypeScript 7 compiler authority');
  const compileIndex = source.indexOf('- name: Compile extension');
  const integrationIndex = source.indexOf('- name: Run extension-host smoke');

  for (const [label, index] of [
    ['toolchain setup', setupIndex],
    ['shim parser tests', shimTestIndex],
    ['TypeScript authority', authorityIndex],
    ['compile', compileIndex],
    ['integration smoke', integrationIndex],
  ]) {
    assert.notEqual(index, -1, `${label} step is missing`);
  }

  assert.ok(setupIndex < shimTestIndex, 'shim tests require the installed repository toolchain');
  assert.ok(
    shimTestIndex < authorityIndex,
    'shim parser tests must precede the real authority probe',
  );
  assert.ok(setupIndex < authorityIndex, 'authority must run after repository toolchain setup');
  assert.ok(authorityIndex < compileIndex, 'authority must run before compilation');
  assert.ok(authorityIndex < integrationIndex, 'authority must run before integration smoke');
  assert.match(
    source,
    /os:\s*\[windows-latest,\s*ubuntu-latest,\s*macos-latest\]/,
    'the shared authority step must cover Windows, Ubuntu, and macOS',
  );
  assert.equal(
    (
      source.match(
        /run: node --test scripts\/check-typescript-authority-windows-shim\.test\.js/g,
      ) ?? []
    ).length,
    1,
    'the shared matrix job should run the shim parser fixtures exactly once',
  );
  assert.equal(
    (source.match(/run: npm run typecheck:authority/g) ?? []).length,
    1,
    'the shared matrix job should invoke authority exactly once',
  );
  assert.doesNotMatch(
    source,
    /run: npm run typecheck:all/,
    'the hosted OS matrix should prove executable identity without tripling all-config proof',
  );
});
