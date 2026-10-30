import { strict as assert } from 'node:assert';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import Mocha from 'mocha';
import {
  assertCandidateBoundInstallSource,
  assertCandidateBoundPlatform,
  configureCurrentSourceSmoke,
  hasCompleteCandidateIdentity,
  isDeterministicPublishedInstallFailure,
  retryPublishedInstall,
} from './runPublishedSmoke';
import { assertSmokeSelector, run as runPublishedSuite } from './suite';

interface PublishedChildObservation {
  ownedDirectories: string[];
  snapshotPath?: string;
  snapshotExclusive?: boolean;
  downloadedPath?: string;
  downloadedBytesAtInstall?: string;
  installTargets: string[];
  installedBytes: string[];
  runners: { kind: string; digest?: string }[];
  childVsixPath?: string;
  firstHour?: {
    status: number;
    stderr: string;
    requestedVsixPath?: string;
    observedVsix?: { path: string; sha256: string; size: number };
  };
  waits: number[];
}

// Exercise private main and its real retry callback, rather than an exported
// snapshot helper. Only external host acquisition, CLI spawn and runners are fake.
function withPublishedChild(
  scenario: string,
  trust: 'disabled' | 'untrusted',
  check: (
    result: ReturnType<typeof spawnSync>,
    observation: PublishedChildObservation,
    paths: { original: string; profile: string; extensions: string; workspace: string },
  ) => void,
  source: 'vsix' | 'open-vsx' | 'marketplace' = 'vsix',
  firstHour = false,
): void {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'published-vsix-attribution-'));
  const original = path.join(root, 'candidate.vsix');
  const profile = path.join(root, 'shared-profile');
  const extensions = path.join(root, 'shared-extensions');
  const workspace = path.join(root, 'caller-workspace');
  const observationPath = path.join(root, 'observation.json');
  const preload = path.join(root, 'preload.cjs');
  const consumerPreload = path.join(root, 'first-hour-preload.cjs');
  const consumerObservation = path.join(root, 'first-hour-observation.json');
  const server = path.join(root, 'input-server');
  for (const directory of [profile, extensions, workspace]) {
    fs.mkdirSync(directory);
    fs.writeFileSync(path.join(directory, 'caller-owned'), 'preserve');
  }
  fs.writeFileSync(original, 'archive A');
  if (firstHour) {
    fs.writeFileSync(server, 'server bytes');
    fs.writeFileSync(path.join(workspace, 'smoke.pl'), 'print "ok";');
    const installedExtension = path.join(extensions, 'installed-extension');
    fs.mkdirSync(installedExtension);
    fs.writeFileSync(
      consumerPreload,
      `
const fs = require('node:fs');
const Module = require('node:module');
const observation = {};
const boundary = new Error('first-hour VSIX observation complete');
const load = Module._load;
global.suite = (_name, run) => run.call({ timeout() {} });
global.test = (_name, run) => {
  Promise.resolve().then(() => run.call({ timeout() {} })).catch(error => {
    if (error !== boundary) { console.error(error); process.exitCode = 1; }
  });
};
Module._load = function(request, ...args) {
  if (request === 'vscode') return {
    version: 'fixture', env: {},
    workspace: {
      workspaceFolders: [{ uri: { scheme: 'file', fsPath: process.env.PERL_LSP_SMOKE_WORKSPACE } }],
      textDocuments: [], isTrusted: true,
      openTextDocument: async file => ({ uri: { toString: () => file }, positionAt: () => ({ line: 0, character: 0 }) }),
    },
    window: { showTextDocument: async () => {} },
    commands: {
      getCommands: async () => ['perl-lsp.runHealthCheck'],
      executeCommand: async (_command, file) => ({ ok: file === process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH, checks: [{ label: 'LSP binary', status: 'error' }] }),
    },
    extensions: { getExtension: () => ({
      extensionPath: process.env.TEST_INSTALLED_EXTENSION, packageJSON: { version: 'fixture' },
      activate: async () => ({ waitForActiveDocumentReady: async () => {} }),
    }) },
  };
  const actual = load.call(this, request, ...args);
  if (request !== '../installedArtifactObservation') return actual;
  return { ...actual, observeArtifact: (...arguments_) => {
    const selectedVsix = arguments_[0] === process.env.PERL_LSP_PUBLISHED_VSIX_PATH;
    if (selectedVsix) observation.requestedVsixPath = arguments_[0];
    const observed = actual.observeArtifact(...arguments_);
    if (selectedVsix) {
      observation.observedVsix = observed;
      // Stop the actual receipt callback after its real byte observer, before
      // mocked editor/provider behavior could be mistaken for acceptance proof.
      throw boundary;
    }
    return observed;
  } };
};
process.once('exit', () => fs.writeFileSync(process.env.TEST_CONSUMER_OBSERVATION, JSON.stringify(observation)));
`,
    );
  }
  fs.writeFileSync(
    preload,
    `
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const { PassThrough } = require('node:stream');
const { EventEmitter } = require('node:events');
const spawnSync = require('node:child_process').spawnSync;
const scenario = process.env.TEST_SCENARIO;
const original = process.env.TEST_ORIGINAL;
const observation = { ownedDirectories: [], installTargets: [], installedBytes: [], runners: [], waits: [] };
const write = fs.writeFileSync;
const mkdirTemp = fs.mkdtempSync;
const createWriteStream = fs.createWriteStream;
const change = target => {
  if (scenario.startsWith('delete-')) fs.rmSync(target, { force: true });
  else write(target, 'archive B');
};
fs.mkdtempSync = (...args) => {
  const directory = mkdirTemp(...args);
  observation.ownedDirectories.push(directory);
  return directory;
};
fs.createWriteStream = (destination, ...args) => {
  observation.downloadedPath = destination;
  return createWriteStream(destination, ...args);
};
fs.writeFileSync = (destination, ...args) => {
  const result = write(destination, ...args);
  if (String(destination).endsWith('.vsix') && destination !== original) {
    observation.snapshotPath = destination;
    observation.snapshotExclusive = args[1]?.flag === 'wx';
    if (scenario === 'replace-original') write(original, 'archive B');
    if (scenario === 'delete-original') fs.rmSync(original);
  }
  return result;
};
global.setTimeout = (callback, milliseconds, ...args) => {
  observation.waits.push(milliseconds);
  if (scenario.endsWith('between-retries')) change(observation.installTargets.at(-1));
  queueMicrotask(() => callback(...args));
  return { unref() {} };
};
require('node:child_process').spawnSync = (_command, args) => {
  const target = args[args.indexOf('--install-extension') + 1];
  observation.installTargets.push(target);
  if (fs.existsSync(target)) observation.installedBytes.push(fs.readFileSync(target, 'utf8'));
  if (observation.downloadedPath) {
    observation.downloadedBytesAtInstall = fs.readFileSync(observation.downloadedPath, 'utf8');
  }
  if (scenario === 'replace-original') write(original, 'archive B');
  if (scenario === 'delete-original' && fs.existsSync(original)) fs.rmSync(original);
  if (scenario.endsWith('during-install')) change(target);
  if (scenario === 'installer-failure') return { status: 127, stderr: 'missing installer' };
  if ((scenario.endsWith('between-retries') || scenario === 'transient-retry') && observation.installTargets.length === 1) {
    return { status: 1, stderr: 'extension install lock is busy' };
  }
  return { status: 0 };
};
const runner = kind => async options => {
  observation.runners.push({ kind, digest: options.extensionTestsEnv.PERL_LSP_VSIX_SHA256 });
  observation.childVsixPath = options.extensionTestsEnv.PERL_LSP_PUBLISHED_VSIX_PATH;
  if (process.env.TEST_FIRST_HOUR === '1') {
    const result = spawnSync(process.execPath, ['--require', process.env.TEST_CONSUMER_PRELOAD, process.env.TEST_CONSUMER], {
      env: { ...process.env, ...options.extensionTestsEnv }, encoding: 'utf8', timeout: 5_000,
    });
    if (result.error) throw result.error;
    observation.firstHour = {
      status: result.status, stderr: result.stderr,
      ...JSON.parse(fs.readFileSync(process.env.TEST_CONSUMER_OBSERVATION, 'utf8')),
    };
  }
  if (scenario === 'runner-failure') throw new Error('fixture runner failure');
};
const load = Module._load;
Module._load = function(request, ...args) {
  if (request === '@vscode/test-electron') return {
    downloadAndUnzipVSCode: async () => '/fake/vscode',
    resolveCliArgsFromVSCodeExecutablePath: () => {
      if (scenario.endsWith('before-install')) change(observation.snapshotPath || original);
      return ['/fake/code'];
    },
    runTests: runner('trusted'),
  };
  if (request === '../runVsCodeTests') return { runWithoutForcedWorkspaceTrust: runner('untrusted') };
  return load.call(this, request, ...args);
};
require('node:https').get = (_url, callback) => {
  const request = new EventEmitter();
  request.setTimeout = () => request;
  queueMicrotask(() => {
    const response = new PassThrough();
    response.statusCode = 200;
    response.headers = {};
    callback(response);
    response.end('archive A');
  });
  return request;
};
process.once('exit', () => write(process.env.TEST_OBSERVATION, JSON.stringify(observation)));
`,
  );
  try {
    // Isolate selectors and candidate identities from the test caller's environment.
    const env: NodeJS.ProcessEnv = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith('PERL_LSP_')),
    );
    Object.assign(env, {
      TEST_SCENARIO: scenario,
      TEST_ORIGINAL: original,
      TEST_OBSERVATION: observationPath,
      TEST_FIRST_HOUR: firstHour ? '1' : '0',
      TEST_CONSUMER: path.join(__dirname, '../integration/firstHourReceipt.test.js'),
      TEST_CONSUMER_PRELOAD: consumerPreload,
      TEST_CONSUMER_OBSERVATION: consumerObservation,
      TEST_INSTALLED_EXTENSION: path.join(extensions, 'installed-extension'),
      PERL_LSP_PUBLISHED_EXTENSION_SOURCE: source,
      PERL_LSP_PUBLISHED_EXTENSION_VERSION: source === 'marketplace' ? '' : '0.17.0',
      PERL_LSP_PUBLISHED_VSIX_PATH: original,
      PERL_LSP_SMOKE_WORKSPACE: workspace,
      PERL_LSP_SMOKE_USER_DATA_DIR: profile,
      PERL_LSP_SMOKE_EXTENSIONS_DIR: extensions,
      PERL_LSP_SMOKE_RECEIPTS_DIR: path.join(root, 'receipts'),
      PERL_LSP_SMOKE_WORKSPACE_TRUST: trust,
      npm_config_user_agent: 'npm/11.18.0',
      // A stale ambient digest must be replaced or removed by main.
      PERL_LSP_VSIX_SHA256: 'stale digest',
    });
    if (firstHour) {
      env.PERL_LSP_CURRENT_SOURCE_SMOKE = '1';
      env.PERL_LSP_FIRST_HOUR_SERVER_PATH = server;
    }
    const result = spawnSync(
      process.execPath,
      ['--require', preload, path.join(__dirname, 'runPublishedSmoke.js')],
      { cwd: path.resolve(__dirname, '../../..'), env, encoding: 'utf8', timeout: 10_000 },
    );
    assert.ifError(result.error);
    const observation = JSON.parse(
      fs.readFileSync(observationPath, 'utf8'),
    ) as PublishedChildObservation;
    check(result, observation, { original, profile, extensions, workspace });
    if (observation.snapshotPath) {
      assert.equal(observation.snapshotExclusive, true);
      assert.ok(observation.ownedDirectories.includes(path.dirname(observation.snapshotPath)));
      assert.ok(
        observation.ownedDirectories.includes(path.dirname(path.dirname(observation.snapshotPath))),
      );
    }
    for (const directory of observation.ownedDirectories)
      assert.equal(fs.existsSync(directory), false, directory);
    for (const directory of [profile, extensions, workspace]) {
      assert.equal(fs.readFileSync(path.join(directory, 'caller-owned'), 'utf8'), 'preserve');
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

const archiveADigest = createHash('sha256').update('archive A').digest('hex');
for (const trust of ['disabled', 'untrusted'] as const) {
  for (const source of ['vsix', 'open-vsx'] as const) {
    for (const scenario of ['unchanged', 'replace-original', 'delete-original']) {
      void test(`compiled ${trust} ${source} first-hour consumer observes captured VSIX: ${scenario}`, () => {
        withPublishedChild(
          scenario,
          trust,
          (result, observation, { original, extensions }) => {
            assert.equal(result.status, 0, String(result.stderr));
            assert.equal(observation.installTargets.length, 1);
            assert.equal(observation.runners.length, 1);
            assert.ok(observation.firstHour);
            assert.equal(observation.firstHour.status, 0, observation.firstHour.stderr);
            assert.equal(observation.firstHour.observedVsix?.sha256, archiveADigest);
            assert.equal(observation.firstHour.requestedVsixPath, observation.snapshotPath);
            assert.deepEqual(observation.firstHour.observedVsix, {
              path: observation.snapshotPath,
              sha256: archiveADigest,
              size: Buffer.byteLength('archive A'),
            });
            assert.deepEqual(observation.installedBytes, ['archive A']);
            assert.equal(observation.runners[0]?.digest, archiveADigest);
            if (scenario === 'unchanged') {
              // Compose the actual child VSIX observation with the real parent
              // validator. Other receipt fields are fixtures, not editor proof.
              const { observeSmokeArtifact, validateChildSmokeReceipt } = require(
                path.resolve(__dirname, '../../../scripts/run-local-vsix-smoke.js'),
              );
              const extensionRoot = path.join(extensions, 'installed-extension');
              const installedServer = path.join(extensionRoot, 'server');
              fs.writeFileSync(installedServer, 'server bytes');
              const selectedServer = observeSmokeArtifact(installedServer);
              const metrics = {
                binary_resolution_source: 'bundled',
                binary_resolution_status: 'ok',
                server_start_status: 'ok',
                initialize_status: 'ok',
                binary_resolution_path: installedServer,
              };
              const receipt = {
                outcome: 'completed',
                failures: [],
                environment: {
                  source_revision: 'a'.repeat(40),
                  server_source_revision: 'a'.repeat(40),
                  vsix_sha256: observation.runners[0]?.digest,
                  requested_vscode_version: 'stable',
                  vscode_version: '1.125.0',
                  extension_id: 'EffortlessMetrics.perl-lsp-rs',
                },
                startup: { language_client: metrics },
                lifecycle: { restart: { language_client: metrics } },
                installed_byte_identity: {
                  schema_version: 'installed_lsp_vsix_bytes.v1',
                  extensions_root: extensions,
                  extension_root: extensionRoot,
                  before_provider: selectedServer,
                  after_restart: selectedServer,
                  after_shutdown: selectedServer,
                  vsix_before: observation.firstHour.observedVsix,
                  vsix_after: observation.firstHour.observedVsix,
                },
              };
              const verdict = validateChildSmokeReceipt({
                receiptFile: '/fixture/receipt.json',
                expectedRevision: 'a'.repeat(40),
                expectedServerSourceSha: 'a'.repeat(40),
                expectedVsixSha256: archiveADigest,
                expectedServerObservation: observeSmokeArtifact(
                  path.join(path.dirname(original), 'input-server'),
                ),
                expectedVsixObservation: observeSmokeArtifact(original),
                expectedExtensionsRoot: extensions,
                expectedVscodeVersion: 'stable',
                exists: () => true,
                readFile: () => JSON.stringify(receipt),
              });
              assert.equal(verdict.ok, true, verdict.violations?.join('; '));
            }
          },
          source,
          true,
        );
      });
    }
  }
}
for (const trust of ['disabled', 'untrusted'] as const) {
  for (const scenario of ['unchanged', 'replace-original', 'delete-original', 'transient-retry']) {
    void test(`compiled ${trust} smoke binds captured VSIX bytes: ${scenario}`, () => {
      withPublishedChild(scenario, trust, (result, observation, { original }) => {
        assert.equal(result.status, 0, String(result.stderr));
        assert.equal(observation.installTargets.length, scenario === 'transient-retry' ? 2 : 1);
        assert.deepEqual(observation.runners, [
          { kind: trust === 'untrusted' ? 'untrusted' : 'trusted', digest: archiveADigest },
        ]);
        assert.ok(observation.snapshotPath, 'installer must consume an invocation-owned snapshot');
        for (const target of observation.installTargets) {
          assert.equal(target, observation.snapshotPath);
          assert.notEqual(target, original);
          assert.equal(path.basename(target), path.basename(original));
        }
        assert.deepEqual(
          observation.installedBytes,
          observation.installTargets.map(() => 'archive A'),
        );
        assert.deepEqual(observation.waits, scenario === 'transient-retry' ? [20_000] : []);
        if (scenario === 'replace-original')
          assert.equal(fs.readFileSync(original, 'utf8'), 'archive B');
        else assert.equal(fs.existsSync(original), scenario !== 'delete-original');
      });
    });
  }
  for (const phase of ['before-install', 'during-install', 'between-retries']) {
    for (const mutation of ['corrupt', 'delete']) {
      void test(`compiled ${trust} smoke refuses snapshot ${mutation} ${phase}`, () => {
        withPublishedChild(`${mutation}-${phase}`, trust, (result, observation, { original }) => {
          assert.equal(result.status, 1, String(result.stderr));
          assert.match(String(result.stderr), /VSIX snapshot/);
          assert.equal(observation.installTargets.length, phase === 'before-install' ? 0 : 1);
          assert.deepEqual(observation.runners, []);
          assert.deepEqual(observation.waits, phase === 'between-retries' ? [20_000] : []);
          assert.equal(fs.readFileSync(original, 'utf8'), 'archive A');
        });
      });
    }
  }
  for (const failure of ['installer-failure', 'runner-failure']) {
    void test(`compiled ${trust} smoke cleans only owned files after ${failure}`, () => {
      withPublishedChild(failure, trust, (result, observation, { original }) => {
        assert.equal(result.status, 1, String(result.stderr));
        assert.equal(observation.installTargets.length, 1);
        assert.equal(observation.runners.length, failure === 'installer-failure' ? 0 : 1);
        assert.equal(fs.readFileSync(original, 'utf8'), 'archive A');
      });
    });
  }
}

void test('compiled Open VSX smoke snapshots without overwriting the downloaded archive', () => {
  withPublishedChild(
    'unchanged',
    'disabled',
    (result, observation) => {
      assert.equal(result.status, 0, String(result.stderr));
      assert.ok(observation.downloadedPath);
      assert.ok(observation.snapshotPath);
      assert.notEqual(observation.snapshotPath, observation.downloadedPath);
      assert.equal(
        path.dirname(path.dirname(observation.snapshotPath)),
        path.dirname(observation.downloadedPath),
      );
      assert.equal(
        path.basename(observation.snapshotPath),
        path.basename(observation.downloadedPath),
      );
      assert.equal(observation.downloadedBytesAtInstall, 'archive A');
      assert.deepEqual(observation.installedBytes, ['archive A']);
      assert.deepEqual(observation.runners, [{ kind: 'trusted', digest: archiveADigest }]);
    },
    'open-vsx',
  );
});

void test('compiled unbound Marketplace smoke installs the ID and clears an ambient digest', () => {
  withPublishedChild(
    'unchanged',
    'disabled',
    (result, observation, { original }) => {
      assert.equal(result.status, 0, String(result.stderr));
      assert.deepEqual(observation.installTargets, ['EffortlessMetrics.perl-lsp-rs']);
      assert.equal(observation.snapshotPath, undefined);
      assert.deepEqual(observation.runners, [{ kind: 'trusted' }]);
      assert.equal(observation.childVsixPath, original);
      assert.equal(fs.readFileSync(original, 'utf8'), 'archive A');
    },
    'marketplace',
  );
});

void test('current-source installed smoke selects the staged bundled server', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'published-bundled-selection-'));
  const previous = process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
  try {
    const server = path.join(directory, 'source-server');
    fs.writeFileSync(server, 'server bytes staged into the VSIX');
    process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH = server;
    const userData = path.join(directory, 'profile');
    configureCurrentSourceSmoke(userData, path.join(directory, 'extensions'), 'disabled');
    const settings = JSON.parse(
      fs.readFileSync(path.join(userData, 'User', 'settings.json'), 'utf8'),
    ) as Record<string, unknown>;
    assert.equal(settings['perl-lsp.serverPath'], '');
    assert.equal(settings['perl-lsp.autoDownload'], false);
  } finally {
    if (previous === undefined) delete process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
    else process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH = previous;
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

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

void test('published smoke rejects health and update-closure selectors before loading a suite', async () => {
  const previousFailure = process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
  const previousClosure = process.env.PERL_LSP_UPDATE_CLOSURE_SMOKE;
  const originalAddFile = Mocha.prototype.addFile;
  let suiteLoadCalls = 0;
  Mocha.prototype.addFile = function () {
    suiteLoadCalls += 1;
    return this;
  };
  try {
    process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE = '1';
    process.env.PERL_LSP_UPDATE_CLOSURE_SMOKE = '1';
    await assert.rejects(runPublishedSuite(), /Published smoke selectors are mutually exclusive/);
    assert.equal(suiteLoadCalls, 0);
  } finally {
    Mocha.prototype.addFile = originalAddFile;
    if (previousFailure === undefined) {
      delete process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE;
    } else {
      process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE = previousFailure;
    }
    if (previousClosure === undefined) {
      delete process.env.PERL_LSP_UPDATE_CLOSURE_SMOKE;
    } else {
      process.env.PERL_LSP_UPDATE_CLOSURE_SMOKE = previousClosure;
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

void test('candidate-bound installed acceptance admits Linux and Windows bindings', () => {
  assert.doesNotThrow(() => assertCandidateBoundPlatform('win32', true, true));
  assert.doesNotThrow(() => assertCandidateBoundPlatform('linux', true));
  assert.doesNotThrow(() => assertCandidateBoundPlatform('win32', false));
});

void test('partial Windows candidate identity remains not proven', () => {
  assert.throws(
    () => assertCandidateBoundPlatform('win32', true),
    /requires candidate ID, artifact-set ID, frozen product SHA, and artifact manifest/,
  );
});

void test('standalone first-hour source SHA does not request candidate validation', () => {
  assert.equal(
    hasCompleteCandidateIdentity({ PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40) }),
    false,
  );
  assert.doesNotThrow(() => assertCandidateBoundPlatform('win32', false));
});

void test('candidate identity is complete or fails before host installation', () => {
  const complete = {
    PERL_LSP_CANDIDATE_ID: 'candidate-1',
    PERL_LSP_ARTIFACT_SET_ID: 'set-1',
    PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40),
    PERL_LSP_CANDIDATE_ARTIFACT_MANIFEST: '{}',
  };
  assert.equal(hasCompleteCandidateIdentity(complete), true);
  for (const missing of Object.keys(complete)) {
    const partial: NodeJS.ProcessEnv = { ...complete };
    delete partial[missing];
    assert.throws(
      () => hasCompleteCandidateIdentity(partial),
      /requires candidate ID, frozen product SHA, artifact-set ID, and artifact manifest together/,
      missing,
    );
  }
});

void test('unsupported candidate-bound platform still throws the typed boundary error', () => {
  assert.throws(
    () => assertCandidateBoundPlatform('darwin', true),
    /supported only on Linux and Windows.*darwin bundled-server digest binding/,
  );
});

void test('published install fails fast for deterministic launcher failures', async () => {
  let attempts = 0;
  const waits: number[] = [];
  await assert.rejects(
    retryPublishedInstall(
      () => {
        attempts += 1;
        return { status: 127, stderr: 'error while loading shared libraries: libnspr4.so' };
      },
      async (milliseconds) => {
        waits.push(milliseconds);
      },
    ),
    /deterministically[\s\S]*libnspr4\.so/,
  );
  assert.equal(attempts, 1);
  assert.deepEqual(waits, []);
});

void test('published install fails fast for the WSL launcher refusal', async () => {
  let attempts = 0;
  await assert.rejects(
    retryPublishedInstall(
      () => {
        attempts += 1;
        return {
          status: 1,
          stderr: 'To use Visual Studio Code with the Windows Subsystem for Linux',
        };
      },
      async () => undefined,
    ),
    /deterministically[\s\S]*Windows Subsystem for Linux/,
  );
  assert.equal(attempts, 1);
});

void test('published install retries generic WSL environment and prompt diagnostics', async () => {
  let attempts = 0;
  const waits: number[] = [];
  await retryPublishedInstall(
    () => {
      attempts += 1;
      return attempts === 1
        ? { status: 1, stderr: 'DONT_PROMPT_WSL_INSTALL=1\nDo you want to continue anyway? [y/N]' }
        : { status: 0 };
    },
    async (milliseconds) => {
      waits.push(milliseconds);
    },
  );
  assert.equal(attempts, 2);
  assert.deepEqual(waits, [20_000]);
});

void test('published install keeps retrying transient failures', async () => {
  let attempts = 0;
  const waits: number[] = [];
  await retryPublishedInstall(
    () => {
      attempts += 1;
      return attempts < 3 ? { status: 1, stderr: 'extension install lock is busy' } : { status: 0 };
    },
    async (milliseconds) => {
      waits.push(milliseconds);
    },
  );
  assert.equal(attempts, 3);
  assert.deepEqual(waits, [20_000, 20_000]);
});

void test('published install exhausts the existing bound for unknown failures', async () => {
  let attempts = 0;
  const waits: number[] = [];
  await assert.rejects(
    retryPublishedInstall(
      () => {
        attempts += 1;
        return { status: 1, stderr: 'unknown install failure' };
      },
      async (milliseconds) => {
        waits.push(milliseconds);
      },
    ),
    /after 12 attempts[\s\S]*unknown install failure/,
  );
  assert.equal(attempts, 12);
  assert.equal(waits.length, 11);
});

void test('published install classifies spawn ENOENT as deterministic', () => {
  const missing = Object.assign(new Error('missing executable'), { code: 'ENOENT' as const });
  assert.equal(isDeterministicPublishedInstallFailure({ status: null, error: missing }), true);
});

void test('published install success wins over launcher-looking output', () => {
  assert.equal(
    isDeterministicPublishedInstallFailure({
      status: 0,
      stderr: 'DONT_PROMPT_WSL_INSTALL is recommended for this launcher',
    }),
    false,
  );
});

void test('the published-smoke child reserves exit 2 before host or receipt work on unsupported platforms', () => {
  const receiptRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl lsp-platform-child-'));
  const preloadPath = path.join(receiptRoot, 'force-darwin-platform.cjs');
  try {
    fs.writeFileSync(
      preloadPath,
      "Object.defineProperty(process, 'platform', { value: 'darwin' });\n" +
        "Object.defineProperty(process, 'arch', { value: 'x64' });\n",
    );
    const result = spawnSync(
      process.execPath,
      ['--require', preloadPath, path.join(__dirname, 'runPublishedSmoke.js')],
      {
        env: {
          ...process.env,
          PERL_LSP_CURRENT_SOURCE_SHA: 'candidate-sha',
          PERL_LSP_CANDIDATE_ID: 'candidate-1',
          PERL_LSP_ARTIFACT_SET_ID: 'set-1',
          PERL_LSP_CANDIDATE_ARTIFACT_MANIFEST: '{}',
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
    assert.match(result.stderr, /supported only on Linux and Windows/);
  } finally {
    fs.rmSync(receiptRoot, { recursive: true, force: true });
  }
});

void test('the compiled child reserves exit 2 with a file receipt root on unsupported platforms', () => {
  const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-platform-unwritable-'));
  const preloadPath = path.join(fixtureRoot, 'force-darwin-platform.cjs');
  const receiptsRoot = path.join(fixtureRoot, 'receipts-root-file');
  try {
    fs.writeFileSync(
      preloadPath,
      "Object.defineProperty(process, 'platform', { value: 'darwin' });\n",
    );
    fs.writeFileSync(receiptsRoot, 'this path is intentionally a file');
    const result = spawnSync(
      process.execPath,
      ['--require', preloadPath, path.join(__dirname, 'runPublishedSmoke.js')],
      {
        env: {
          ...process.env,
          PERL_LSP_CURRENT_SOURCE_SHA: 'candidate-sha',
          PERL_LSP_CANDIDATE_ID: 'candidate-1',
          PERL_LSP_ARTIFACT_SET_ID: 'set-1',
          PERL_LSP_CANDIDATE_ARTIFACT_MANIFEST: '{}',
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
    assert.match(result.stderr, /supported only on Linux and Windows/);

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
