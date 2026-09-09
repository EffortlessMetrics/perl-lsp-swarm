import { strict as assert } from 'node:assert';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import Mocha from 'mocha';
import {
  assertCandidateBoundInstallSource,
  assertCandidateBoundPlatform,
} from './runPublishedSmoke';
import { assertSmokeSelector, run as runPublishedSuite } from './suite';

void test('published smoke rejects a recovery leg without its failure selector', () => {
  assert.throws(
    () =>
      assertSmokeSelector({
        PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE: '1',
      }),
    /PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE requires PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE=1/,
  );
  assert.doesNotThrow(() =>
    assertSmokeSelector({
      PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE: '1',
      PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE: '1',
    }),
  );
});

void test('published smoke run rejects the invalid recovery selector before loading a suite', async () => {
  const previousFailure = process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
  const previousRecovery = process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE;
  const originalAddFile = Mocha.prototype.addFile;
  let suiteLoadCalls = 0;
  Mocha.prototype.addFile = function () {
    suiteLoadCalls += 1;
    return this;
  };
  try {
    delete process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
    process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE = '1';
    await assert.rejects(
      runPublishedSuite(),
      /PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE requires PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE=1/,
    );
    assert.equal(suiteLoadCalls, 0);
  } finally {
    Mocha.prototype.addFile = originalAddFile;
    if (previousFailure === undefined) {
      delete process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
    } else {
      process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE = previousFailure;
    }
    if (previousRecovery === undefined) {
      delete process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE;
    } else {
      process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE = previousRecovery;
    }
  }
});

void test('published smoke rejects health and Test Explorer selectors before loading a suite', async () => {
  const previousFailure = process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
  const previousExplorer = process.env.PERL_LSP_TEST_EXPLORER_SMOKE;
  const originalAddFile = Mocha.prototype.addFile;
  let suiteLoadCalls = 0;
  Mocha.prototype.addFile = function () {
    suiteLoadCalls += 1;
    return this;
  };
  try {
    process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE = '1';
    process.env.PERL_LSP_TEST_EXPLORER_SMOKE = '1';
    await assert.rejects(runPublishedSuite(), /Published smoke selectors are mutually exclusive/);
    assert.equal(suiteLoadCalls, 0);
  } finally {
    Mocha.prototype.addFile = originalAddFile;
    if (previousFailure === undefined) {
      delete process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
    } else {
      process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE = previousFailure;
    }
    if (previousExplorer === undefined) {
      delete process.env.PERL_LSP_TEST_EXPLORER_SMOKE;
    } else {
      process.env.PERL_LSP_TEST_EXPLORER_SMOKE = previousExplorer;
    }
  }
});

void test('candidate-bound Marketplace latest is refused before installation', () => {
  assert.throws(
    () =>
      assertCandidateBoundInstallSource({
        source: 'marketplace',
        version: '',
        vsixPath: '',
        candidateBound: true,
      }),
    /refuses Marketplace latest.*exact VSIX path and observed digest/,
  );
});

void test('scheduled unbound Marketplace smoke remains allowed', () => {
  assert.doesNotThrow(() =>
    assertCandidateBoundInstallSource({
      source: 'marketplace',
      version: '',
      vsixPath: '',
      candidateBound: false,
    }),
  );
});

void test('candidate-bound installed acceptance refuses non-Linux platform binding', () => {
  assert.throws(
    () => assertCandidateBoundPlatform('windows', true),
    /restricted to Linux.*windows bundled-server digest binding/,
  );
  assert.doesNotThrow(() => assertCandidateBoundPlatform('windows', false));
  assert.doesNotThrow(() => assertCandidateBoundPlatform('linux', true));
});

void test('unsupported candidate-bound platform throws the typed boundary error', () => {
  assert.throws(
    () => assertCandidateBoundPlatform('win32', true),
    /restricted to Linux.*win32 bundled-server digest binding/,
  );
});

void test('the published-smoke child reserves exit 2 before host or receipt work', () => {
  const receiptRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl lsp-platform-child-'));
  const preloadPath = path.join(receiptRoot, 'force-windows-platform.cjs');
  try {
    fs.writeFileSync(
      preloadPath,
      "Object.defineProperty(process, 'platform', { value: 'win32' });\n" +
        "Object.defineProperty(process, 'arch', { value: 'x64' });\n",
    );
    const result = spawnSync(
      process.execPath,
      ['--require', preloadPath, path.join(__dirname, 'runPublishedSmoke.js')],
      {
        env: {
          ...process.env,
          PERL_LSP_CURRENT_SOURCE_SHA: 'candidate-sha',
          PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
          PERL_LSP_PUBLISHED_EXTENSION_VERSION: '0.17.0',
          PERL_LSP_PUBLISHED_VSIX_PATH: path.join(receiptRoot, 'candidate.vsix'),
          PERL_LSP_SMOKE_RECEIPTS_DIR: receiptRoot,
        },
        encoding: 'utf8',
        windowsHide: true,
      },
    );
    if (result.error) {
      throw new Error(`unsupported-platform child failed to spawn: ${result.error.message}`);
    }
    assert.equal(result.status, 2);
    assert.match(result.stderr, /restricted to Linux/);
  } finally {
    fs.rmSync(receiptRoot, { recursive: true, force: true });
  }
});

void test('the compiled child reserves exit 2 with a file receipt root before filesystem work', () => {
  const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-platform-unwritable-'));
  const preloadPath = path.join(fixtureRoot, 'force-windows-platform.cjs');
  const receiptsRoot = path.join(fixtureRoot, 'receipts-root-file');
  try {
    fs.writeFileSync(
      preloadPath,
      "Object.defineProperty(process, 'platform', { value: 'win32' });\n",
    );
    fs.writeFileSync(receiptsRoot, 'this path is intentionally a file');
    const result = spawnSync(
      process.execPath,
      ['--require', preloadPath, path.join(__dirname, 'runPublishedSmoke.js')],
      {
        env: {
          ...process.env,
          PERL_LSP_CURRENT_SOURCE_SHA: 'candidate-sha',
          PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
          PERL_LSP_PUBLISHED_EXTENSION_VERSION: '0.17.0',
          PERL_LSP_PUBLISHED_VSIX_PATH: path.join(fixtureRoot, 'candidate.vsix'),
          PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot,
        },
        encoding: 'utf8',
        windowsHide: true,
      },
    );
    if (result.error) {
      throw new Error(`unwritable-root child failed to spawn: ${result.error.message}`);
    }
    assert.equal(result.status, 2);
    assert.match(result.stderr, /restricted to Linux/);

    const { interpretBehavioralSmokeExit } = require('../../../scripts/run-local-vsix-smoke.js');
    const parent = interpretBehavioralSmokeExit({
      status: result.status,
      candidateBound: true,
      platform: 'win32',
      receiptsRoot,
    });
    assert.equal(parent.status, 'not_proven');
    assert.equal(parent.reason, 'candidate_bound_platform_unavailable');
  } finally {
    fs.rmSync(fixtureRoot, { recursive: true, force: true });
  }
});
