import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { execFileSync } from 'child_process';

type RegisteredJourney = () => Promise<void>;

interface Harness {
  journey: RegisteredJourney;
  calls: string[];
  firstProviderCall: Promise<void>;
  readinessEntered: Promise<void>;
  receiptDirectory: string;
  cleanup: () => void;
}

function mockJourneySupportPath(
  directory: string,
  receiptDirectory: string,
  actualSupportPath: string,
): string {
  const mockPath = path.join(directory, 'journeySupport.mock.js');
  fs.writeFileSync(
    mockPath,
    `
const vscode = require('vscode');
const actual = require(${JSON.stringify(actualSupportPath)});
module.exports = {
  ...actual,
  assertProviderSucceeded: () => {},
  bundledBinaryPath: (extensionPath) => require('node:path').join(extensionPath, 'bin', 'win32-x64', 'perllsp.exe'),
  bundledServerVersion: async () => ({ status: 'ok', version: '0.17.0', stdout: '0.17.0', stderr: '', outcome: 'completed', output_truncated: false, termination_confirmed: true }),
  pathsEquivalent: () => true,
  platformLabel: () => 'windows',
  providerPosition: () => ({ line: 0, character: 0 }),
  providerResult: async (label, command, ...args) => {
    const result = await vscode.commands.executeCommand(command, ...args);
    return { status: 'ok', label, item_count: Array.isArray(result) ? result.length : 0 };
  },
  receiptsDir: () => ${JSON.stringify(receiptDirectory)},
  sha256: () => 'a'.repeat(64),
  waitForStartupMetrics: async () => ({
    lifecycle_state: 'running',
    binary_resolution_source: 'bundled',
    binary_resolution_status: 'ok',
    binary_resolution_path: 'C:/extension/bin/win32-x64/perllsp.exe',
    server_start_status: 'ok',
    initialize_status: 'ok',
    server_version: '0.17.0',
  }),
};
`,
    'utf8',
  );
  return mockPath;
}

function fakeVscode(
  extensionPath: string,
  workspacePath: string,
  readiness: Promise<void>,
  calls: string[],
  firstProviderCall: () => void,
  readinessEntered: () => void,
  exposeReadiness: boolean,
): Record<string, unknown> {
  let edited = false;
  const document = {
    uri: { toString: () => 'file:///workspace/packaged_daily_driver.pl' },
    lineCount: 1,
    getText: () => (edited ? '# packaged edit' : 'my $value = 42;'),
  };
  const configuration = {
    inspect: () => ({ globalValue: undefined }),
    update: async () => undefined,
  };
  const extensionApi = {
    getLanguageClientStartupMetrics: () => ({
      lifecycle_state: 'running',
      binary_resolution_source: 'bundled',
      binary_resolution_status: 'ok',
      binary_resolution_path: 'C:/extension/bin/win32-x64/perllsp.exe',
      server_start_status: 'ok',
      initialize_status: 'ok',
      server_version: '0.17.0',
    }),
    getActiveDocumentReadiness: () => ({
      generation: 1,
      indexState: 'building',
      fullyReady: false,
    }),
    waitForActiveDocumentReady: async () => {
      readinessEntered();
      return await readiness;
    },
    stop: async () => undefined,
  };
  if (!exposeReadiness) {
    delete (extensionApi as { waitForActiveDocumentReady?: unknown }).waitForActiveDocumentReady;
  }
  class WorkspaceEdit {
    insert(): void {
      edited = true;
    }
    entries(): Array<[unknown, unknown]> {
      return [];
    }
  }
  class Position {
    constructor(
      readonly line: number,
      readonly character: number,
    ) {}
  }
  return {
    ConfigurationTarget: { Global: 1 },
    WorkspaceEdit,
    Position,
    workspace: {
      workspaceFolders: [{ uri: { fsPath: workspacePath } }],
      getConfiguration: () => configuration,
      openTextDocument: async () => document,
      applyEdit: async () => {
        edited = true;
        return true;
      },
      isTrusted: true,
    },
    window: { showTextDocument: async () => document },
    extensions: {
      getExtension: () => ({
        id: 'EffortlessMetrics.perl-lsp-rs',
        extensionPath,
        packageJSON: { version: '0.17.0' },
        activate: async () => extensionApi,
      }),
    },
    commands: {
      executeCommand: async (command: string) => {
        calls.push(command);
        firstProviderCall();
        return [];
      },
    },
    languages: { getDiagnostics: () => [] },
  };
}

function loadRegisteredJourney(
  source: string,
  harness: { calls: string[]; receiptDirectory: string; vscode: Record<string, unknown> },
): RegisteredJourney {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-4346-registered-'));
  const sourceRoot = path.join(directory, 'src');
  const sourcePath = path.join(sourceRoot, 'test', 'published', 'packagedBundleJourney.test.ts');
  const supportSourcePath = path.join(sourceRoot, 'test', 'published', 'journeySupport.ts');
  const serverVersionSourcePath = path.join(sourceRoot, 'packagedServerVersion.ts');
  const adapterSourcePath = path.join(sourceRoot, 'testAdapter.ts');
  fs.mkdirSync(path.dirname(sourcePath), { recursive: true });
  fs.copyFileSync(
    path.resolve(__dirname, '../../src/test/published/journeySupport.ts'),
    supportSourcePath,
  );
  fs.copyFileSync(
    path.resolve(__dirname, '../../src/packagedServerVersion.ts'),
    serverVersionSourcePath,
  );
  fs.copyFileSync(path.resolve(__dirname, '../../src/testAdapter.ts'), adapterSourcePath);
  fs.writeFileSync(sourcePath, source, 'utf8');
  const outputRoot = path.join(directory, 'out');
  try {
    execFileSync(
      process.execPath,
      [
        path.resolve(__dirname, '../../node_modules/typescript/lib/tsc.js'),
        sourcePath,
        supportSourcePath,
        serverVersionSourcePath,
        adapterSourcePath,
        '--rootDir',
        sourceRoot,
        '--target',
        'ES2022',
        '--module',
        'commonjs',
        '--noCheck',
        '--noResolve',
        '--ignoreConfig',
        '--outDir',
        outputRoot,
      ],
      { windowsHide: true, stdio: 'pipe' },
    );
  } catch (error: unknown) {
    const details = error as { stderr?: Buffer; stdout?: Buffer };
    throw new Error(
      `packaged journey transpilation failed: ${details.stderr?.toString() ?? ''}${details.stdout?.toString() ?? ''}${error}`,
    );
  }
  const actualSupportPath = path.join(outputRoot, 'test', 'published', 'journeySupport.js');
  const supportPath = mockJourneySupportPath(
    directory,
    harness.receiptDirectory,
    actualSupportPath,
  );
  const compiledJourneyPath = path.join(
    outputRoot,
    'test',
    'published',
    'packagedBundleJourney.test.js',
  );
  const transpiled = fs
    .readFileSync(compiledJourneyPath, 'utf8')
    .replace(/require\("\.\/journeySupport"\)/g, `require(${JSON.stringify(supportPath)})`);
  fs.writeFileSync(compiledJourneyPath, transpiled, 'utf8');

  let journey: RegisteredJourney | undefined;
  const previousSuite = (globalThis as { suite?: unknown }).suite;
  const previousTest = (globalThis as { test?: unknown }).test;
  (globalThis as { suite?: unknown }).suite = (_name: string, callback: () => void) =>
    callback.call({ timeout: () => undefined });
  (globalThis as { test?: unknown }).test = (_name: string, callback: RegisteredJourney) => {
    journey = callback;
  };
  try {
    jest.isolateModules(() => {
      // The published journey is loaded through the same suite/test registration
      // boundary as runPublishedSmoke.ts; only VS Code and journeySupport are controlled.
      jest.doMock('vscode', () => harness.vscode, { virtual: true });
      require(compiledJourneyPath);
    });
  } finally {
    (globalThis as { suite?: unknown }).suite = previousSuite;
    (globalThis as { test?: unknown }).test = previousTest;
    jest.dontMock('vscode');
    fs.rmSync(directory, { recursive: true, force: true });
  }
  if (!journey) throw new Error('packaged journey did not register a test callback');
  return journey;
}

async function makeHarness(
  source: string,
  readiness: Promise<void>,
  exposeReadiness = true,
): Promise<Harness> {
  const receiptDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-4346-receipts-'));
  const workspacePath = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-4346-workspace-'));
  const extensionPath = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-4346-extension-'));
  fs.mkdirSync(path.join(extensionPath, 'bin', 'win32-x64'), { recursive: true });
  fs.writeFileSync(path.join(extensionPath, 'bin', 'win32-x64', 'perllsp.exe'), 'server');
  const calls: string[] = [];
  let firstProviderResolve: (() => void) | undefined;
  const firstProviderCall = new Promise<void>((resolve) => {
    firstProviderResolve = resolve;
  });
  let readinessEnteredResolve: (() => void) | undefined;
  const readinessEntered = new Promise<void>((resolve) => {
    readinessEnteredResolve = resolve;
  });
  const vscode = fakeVscode(
    extensionPath,
    workspacePath,
    readiness,
    calls,
    () => firstProviderResolve?.(),
    () => readinessEnteredResolve?.(),
    exposeReadiness,
  );
  const journey = loadRegisteredJourney(source, { calls, receiptDirectory, vscode });
  return {
    journey,
    calls,
    firstProviderCall,
    readinessEntered,
    receiptDirectory,
    cleanup: () => {
      fs.rmSync(receiptDirectory, { recursive: true, force: true });
      fs.rmSync(workspacePath, { recursive: true, force: true });
      fs.rmSync(extensionPath, { recursive: true, force: true });
    },
  };
}

describe('registered packaged journey readiness contract', () => {
  function journeySource(): string {
    const sourcePath = process.env.PERL_LSP_REGISTERED_JOURNEY_SOURCE
      ? path.resolve(process.env.PERL_LSP_REGISTERED_JOURNEY_SOURCE)
      : path.resolve(__dirname, '../../src/test/published/packagedBundleJourney.test.ts');
    return fs.readFileSync(sourcePath, 'utf8');
  }

  function readReceipt(receiptDirectory: string): Record<string, unknown> {
    return JSON.parse(
      fs.readFileSync(path.join(receiptDirectory, 'packaged_bundle_journey_receipt.json'), 'utf8'),
    ) as Record<string, unknown>;
  }

  function assertProvidersNotProven(receipt: Record<string, unknown>): void {
    const requests = receipt.requests as { immediate?: Record<string, unknown> } | undefined;
    const immediate = requests?.immediate;
    expect(immediate).toBeDefined();
    for (const provider of ['completion', 'hover', 'definition', 'references', 'symbols']) {
      expect(immediate?.[provider]).toMatchObject({ status: 'not_proven' });
    }
  }

  test('candidate registered callback withholds providers until readiness resolves', async () => {
    const source = journeySource();
    let release: (() => void) | undefined;
    const readiness = new Promise<void>((resolve) => {
      release = resolve;
    });
    const harness = await makeHarness(source, readiness);
    const run = harness.journey.call({ timeout: () => undefined });
    try {
      const readinessObserved = await Promise.race([
        harness.readinessEntered.then(() => 'entered' as const),
        harness.firstProviderCall.then(() => 'provider' as const),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('readiness gate was not entered')), 1_000),
        ),
      ]);
      expect(readinessObserved).toBe('entered');
      expect(harness.calls).toEqual([]);
      release?.();
      await run;
      expect(harness.calls).toContain('vscode.executeCompletionItemProvider');
      expect(harness.calls).toContain('vscode.executeDocumentSymbolProvider');
      const receipt = JSON.parse(
        fs.readFileSync(
          path.join(harness.receiptDirectory, 'packaged_bundle_journey_receipt.json'),
          'utf8',
        ),
      ) as {
        readiness_wait?: Record<string, unknown>;
        readiness_after?: Record<string, unknown>;
        requests?: { immediate_phase?: unknown };
      };
      expect(receipt.readiness_wait).toMatchObject({
        scope: 'active_document',
        status: 'ready',
      });
      expect(receipt.readiness_after).toMatchObject({
        indexState: 'building',
        fullyReady: false,
      });
      expect(receipt.requests?.immediate_phase).toBe('after_active_document_readiness');
    } finally {
      release?.();
      await run.catch(() => undefined);
      harness.cleanup();
    }
  });

  test.each([
    ['rejected readiness', true],
    ['missing readiness API', false],
  ])('%s withholds providers and records not proven', async (_label, exposeReadiness) => {
    let rejectReadiness: ((error: Error) => void) | undefined;
    const readiness = new Promise<void>((_resolve, reject) => {
      rejectReadiness = reject;
    });
    const harness = await makeHarness(journeySource(), readiness, exposeReadiness);
    try {
      const run = harness.journey.call({ timeout: () => undefined });
      if (exposeReadiness) {
        await harness.readinessEntered;
        rejectReadiness?.(new Error('readiness refused'));
      }
      await run;
      expect(harness.calls).toEqual([]);
      const receipt = readReceipt(harness.receiptDirectory);
      expect(receipt.readiness_wait).toMatchObject({
        scope: 'active_document',
        status: 'not_proven',
      });
      assertProvidersNotProven(receipt);
    } finally {
      harness.cleanup();
    }
  });
});
