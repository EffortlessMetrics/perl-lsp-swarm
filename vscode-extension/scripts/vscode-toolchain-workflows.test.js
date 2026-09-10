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

void test('current-source Linux smoke enables the candidate-bound Test Explorer leg', () => {
  const source = readWorkflow('vscode-current-source-linux-smoke.yml');
  const smokeIndex = source.indexOf('- name: Run exact current-source smoke under Xvfb');
  assert.notEqual(smokeIndex, -1);
  const nextStepIndex = source.slice(smokeIndex + 1).search(/\r?\n\s+- name:/);
  const smokeStep = source.slice(
    smokeIndex,
    nextStepIndex === -1 ? source.length : smokeIndex + 1 + nextStepIndex,
  );
  assert.match(smokeStep, /PERL_LSP_TEST_EXPLORER_JOURNEY: '1'/);
  assert.match(smokeStep, /run: xvfb-run -a npm run test:published:local/);
  assert.match(
    smokeStep,
    /^[ \t]*PERL_LSP_FIRST_HOUR_SERVER_PATH:[ \t]*\$\{\{ runner\.temp \}\}\/perl-lsp-current-source-target\/release\/perllsp[ \t]*\r?$/m,
  );
  assert.match(
    smokeStep,
    /^[ \t]*PERL_LSP_SERVER_SOURCE_SHA:[ \t]*\$\{\{ env\.PERL_LSP_SMOKE_SUBJECT_SHA \}\}[ \t]*\r?$/m,
  );
  assert.match(source, /same staged VSIX\/server/);
});

void test('current-source inventory uses PR merge-base while manual runs keep accepted base', () => {
  const source = readWorkflow('vscode-current-source-linux-smoke.yml');
  assert.match(
    source,
    /^          PERL_LSP_PACKAGE_BASE_MODE: \$\{\{ github\.event_name == 'pull_request' && 'pull_request' \|\| 'accepted' \}\}$/m,
  );
  assert.match(
    source,
    /^          PERL_LSP_PACKAGE_PR_BASE_SHA: \$\{\{ github\.event_name == 'pull_request' && github\.event\.pull_request\.base\.sha \|\| '' \}\}$/m,
  );
  assert.match(
    source,
    /^          PERL_LSP_PACKAGE_BASE_SHA: \$\{\{ github\.event_name == 'workflow_dispatch' && inputs\.accepted_base_sha \|\| '' \}\}$/m,
  );
  assert.doesNotMatch(
    source,
    /PERL_LSP_PACKAGE_BASE_SHA: \$\{\{ github\.event_name == 'pull_request' && github\.event\.pull_request\.base\.sha/,
  );
});

void test('publisher workflow invokes both CLIs offline through npm exec', () => {
  const source = readWorkflow('publish-extension.yml');
  assert.match(source, /npm exec --offline --no -- @vscode\/vsce publish/);
  assert.match(source, /npm exec --offline --no -- ovsx publish/);
  assert.doesNotMatch(source, /^\s+run: (?:vsce|ovsx) publish/m);
  assert.doesNotMatch(source, /^\s+run: ovsx --version/m);
});

void test('managed Windows smoke packages and runs the current Test Explorer VSIX', () => {
  const source = readWorkflow('vscode-managed-binary-smoke.yml');
  const packageIndex = source.indexOf(
    '- name: Package current extension for Windows published smoke',
  );
  const explorerIndex = source.indexOf(
    '- name: Run current extension Test Explorer smoke (Windows)',
  );
  assert.notEqual(packageIndex, -1);
  assert.notEqual(explorerIndex, -1);
  assert.ok(packageIndex < explorerIndex);
  const nextStepIndex = (index) => {
    const offset = source.slice(index + 1).search(/\r?\n\s+- name:/);
    return offset === -1 ? -1 : index + 1 + offset;
  };
  const packageNextStepIndex = nextStepIndex(packageIndex);
  const explorerNextStepIndex = nextStepIndex(explorerIndex);
  const packageStep = source.slice(
    packageIndex,
    packageNextStepIndex === -1 ? source.length : packageNextStepIndex,
  );
  const explorerStep = source.slice(
    explorerIndex,
    explorerNextStepIndex === -1 ? source.length : explorerNextStepIndex,
  );
  assert.match(packageStep, /if: runner\.os == 'Windows'/);
  assert.match(packageStep, /run: npm run package/);
  assert.match(explorerStep, /if: runner\.os == 'Windows'/);
  assert.match(explorerStep, /GITHUB_TOKEN: \$\{\{ github\.token \}\}/);
  assert.match(explorerStep, /PERL_LSP_PUBLISHED_EXTENSION_SOURCE: vsix/);
  assert.match(explorerStep, /PERL_LSP_TEST_EXPLORER_SMOKE: '1'/);
  assert.match(explorerStep, /\$vsix\.Count -ne 1/);
  assert.match(explorerStep, /\$env:PERL_LSP_PUBLISHED_VSIX_PATH = \$vsix\[0\]\.FullName/);
  assert.match(explorerStep, /npm run test:published/);
  const vsixSelection = explorerStep.indexOf('$vsix = @(');
  const vsixAssignment = explorerStep.indexOf('$env:PERL_LSP_PUBLISHED_VSIX_PATH =');
  const publishedTest = explorerStep.indexOf('npm run test:published');
  assert.notEqual(vsixSelection, -1, 'the published VSIX must be enumerated');
  assert.notEqual(vsixAssignment, -1, 'the selected VSIX path must be assigned');
  assert.notEqual(publishedTest, -1, 'the published Test Explorer command must be present');
  assert.ok(
    vsixSelection < vsixAssignment,
    'the published VSIX must be selected before binding its path',
  );
  assert.ok(
    vsixAssignment < publishedTest,
    'the selected VSIX path must be bound before the published test',
  );
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
