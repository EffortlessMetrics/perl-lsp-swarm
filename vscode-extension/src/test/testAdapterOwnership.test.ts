import * as vscode from 'vscode';

jest.mock('vscode-languageclient/node', () => ({
  State: { Stopped: 1, Running: 2, Starting: 3 },
  LanguageClient: jest.fn().mockImplementation(() => {
    let state = 1;
    const serverProcess = {
      pid: process.pid,
      exitCode: null as number | null,
      signalCode: null,
      once: jest.fn(),
      removeListener: jest.fn(),
    };
    return {
      get state() {
        return state;
      },
      serverProcess,
      initializeResult: { capabilities: {} },
      onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
      onNotification: jest.fn(() => ({ dispose: jest.fn() })),
      sendNotification: jest.fn(async () => undefined),
      setTrace: jest.fn(async () => undefined),
      start: jest.fn(async () => {
        state = 2;
      }),
      stop: jest.fn(async () => {
        state = 1;
        serverProcess.exitCode = 0;
      }),
      dispose: jest.fn(async () => undefined),
    };
  }),
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import { activate, deactivate } from '../extension';
import { PerlTestAdapter } from '../testAdapter';
import { setOpenDocuments } from './serverDemandDocuments';

async function waitUntil(condition: () => boolean): Promise<void> {
  const deadline = Date.now() + 2_000;
  while (!condition()) {
    if (Date.now() >= deadline) {
      throw new Error('adapter ownership condition was not reached');
    }
    await new Promise<void>((resolve) => setImmediate(resolve));
  }
}

function makeContext(): vscode.ExtensionContext {
  const state = { get: jest.fn(), update: jest.fn(async () => undefined) };
  return {
    extension: {
      packageJSON: { publisher: 'EffortlessMetrics', name: 'perl-lsp-rs', version: '0.17.0' },
    },
    extensionMode: vscode.ExtensionMode.Production,
    extensionPath: process.cwd(),
    globalState: state,
    workspaceState: state,
    subscriptions: [],
  } as unknown as vscode.ExtensionContext;
}

describe('test adapter ownership across ordinary restarts (#7854)', () => {
  const originalSkipStartup = process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
  const testUri = vscode.Uri.file('/workspace/restart-ownership.t');
  let enabled = true;
  let subtestName = 'initial';

  function controllers(): vscode.TestController[] {
    return jest
      .mocked(vscode.tests.createTestController)
      .mock.results.map((result) => result.value);
  }

  function changeTestIntegration(value: boolean): void {
    enabled = value;
    const event = {
      affectsConfiguration: (section: string) => section === 'perl-lsp.enableTestIntegration',
    } as vscode.ConfigurationChangeEvent;
    for (const [listener] of jest.mocked(vscode.workspace.onDidChangeConfiguration).mock.calls) {
      listener(event);
    }
  }

  beforeEach(() => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    setOpenDocuments([]);
    enabled = true;
    subtestName = 'initial';
    jest.mocked(vscode.workspace.getConfiguration).mockImplementation(
      () =>
        ({
          get: (key: string, fallback: unknown) => {
            if (key === 'serverPath') return process.execPath;
            if (key === 'autoDownload') return false;
            if (key === 'enableTestIntegration') return enabled;
            return fallback;
          },
          has: () => false,
          inspect: jest.fn(),
          update: jest.fn(async () => undefined),
        }) as unknown as vscode.WorkspaceConfiguration,
    );
    jest
      .mocked(vscode.workspace.findFiles)
      .mockImplementation(async (include) => (include === '**/*.t' ? [testUri] : []));
    jest.mocked(vscode.workspace.openTextDocument).mockImplementation(
      async () =>
        ({
          lineCount: 1,
          lineAt: () => ({ text: `subtest '${subtestName}' => sub {` }),
        }) as unknown as vscode.TextDocument,
    );
  });

  afterEach(async () => {
    await deactivate();
    setOpenDocuments([]);
    if (originalSkipStartup === undefined) {
      delete process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
    } else {
      process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = originalSkipStartup;
    }
    jest.clearAllMocks();
  });

  test.each([true, false])(
    'initial startup and three restarts retain no adapters, enabled=%s',
    async (integrationEnabled) => {
      enabled = integrationEnabled;
      const context = makeContext();
      const api = await activate(context);
      for (let generation = 0; generation < 4; generation += 1) {
        subtestName = `generation-${generation}`;
        await vscode.commands.executeCommand('perl-lsp.restart');
        expect(api.getLanguageClientStartupMetrics().startup_error).toBeNull();
        expect(api.getLanguageClientStartupMetrics().lifecycle_state).toBe('running');
        expect(controllers()).toHaveLength(enabled ? generation + 1 : 0);
        if (enabled) {
          const current = controllers()[generation]!;
          await waitUntil(() =>
            jest
              .mocked(current.createTestItem)
              .mock.calls.some(([id]) => id === `${testUri.toString()}::${subtestName}`),
          );
          expect(current.dispose).not.toHaveBeenCalled();
          for (const previous of controllers().slice(0, generation)) {
            expect(previous.dispose).toHaveBeenCalledTimes(1);
          }
        }
      }
      expect(context.subscriptions.filter((item) => item instanceof PerlTestAdapter)).toEqual([]);
      await deactivate();
      for (const controller of controllers()) {
        expect(controller.dispose).toHaveBeenCalledTimes(1);
      }
    },
  );

  test('configuration reconstruction keeps the same cleanup owner', async () => {
    const context = makeContext();
    await activate(context);
    await vscode.commands.executeCommand('perl-lsp.restart');
    await waitUntil(() => controllers().length === 1);
    changeTestIntegration(false);
    await waitUntil(() => jest.mocked(controllers()[0]!.dispose).mock.calls.length === 1);
    subtestName = 'reconstructed';
    changeTestIntegration(true);
    await waitUntil(() => controllers().length === 2);
    const current = controllers()[1]!;
    await waitUntil(() =>
      jest
        .mocked(current.createTestItem)
        .mock.calls.some(([id]) => id === `${testUri.toString()}::reconstructed`),
    );
    expect(current.dispose).not.toHaveBeenCalled();
    expect(context.subscriptions.filter((item) => item instanceof PerlTestAdapter)).toEqual([]);
    await deactivate();
    for (const controller of controllers()) {
      expect(controller.dispose).toHaveBeenCalledTimes(1);
    }
  });
});
