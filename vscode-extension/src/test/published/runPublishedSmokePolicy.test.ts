import { strict as assert } from 'node:assert';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import {
  assertCandidateBoundInstallSource,
  assertCandidateBoundPlatform,
  buildCandidatePlatformUnavailableReceipt,
  CANDIDATE_PLATFORM_UNAVAILABLE_RECEIPT_NAME,
  writeCandidatePlatformUnavailableReceipt,
} from './runPublishedSmoke';

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

void test('unsupported candidate-bound platform writes a typed unavailable boundary', () => {
  const receiptRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-platform-unavailable-'));
  try {
    const receiptPath = writeCandidatePlatformUnavailableReceipt(receiptRoot, 'win32', 'x64');
    assert.deepEqual(JSON.parse(fs.readFileSync(receiptPath, 'utf8')), {
      ...buildCandidatePlatformUnavailableReceipt('win32', 'x64'),
    });
    assert.throws(() => assertCandidateBoundPlatform('win32', true), /restricted to Linux/);
  } finally {
    fs.rmSync(receiptRoot, { recursive: true, force: true });
  }
});

void test('the published-smoke child emits the unsupported-platform boundary before host work', () => {
  const receiptRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-platform-child-'));
  try {
    const result = spawnSync(process.execPath, [path.join(__dirname, 'runPublishedSmoke.js')], {
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
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /restricted to Linux/);
    const receiptPath = path.join(receiptRoot, CANDIDATE_PLATFORM_UNAVAILABLE_RECEIPT_NAME);
    assert.deepEqual(JSON.parse(fs.readFileSync(receiptPath, 'utf8')), {
      ...buildCandidatePlatformUnavailableReceipt('win32', 'x64'),
    });
  } finally {
    fs.rmSync(receiptRoot, { recursive: true, force: true });
  }
});
