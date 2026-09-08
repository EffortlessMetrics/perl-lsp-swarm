import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const mockLanguageClientStart = jest.fn(async () => undefined);
const mockLanguageClientStop = jest.fn(async () => undefined);
const mockLanguageClientDispose = jest.fn(async () => undefined);
const mockLanguageClientSetTrace = jest.fn(async () => undefined);
const mockLanguageClientOnDidChangeState = jest.fn(() => ({ dispose: jest.fn() }));
const mockLanguageClientOnNotification = jest.fn(() => ({ dispose: jest.fn() }));
const mockLanguageClientSendNotification = jest.fn(async () => undefined);
const mockExecFile = jest.fn((...args: unknown[]) => {
  const callback = args[args.length - 1] as (
    error: Error | null,
    stdout: string,
    stderr: string,
  ) => void;
  callback(null, '', '');
});

jest.mock('child_process', () => ({
  ...jest.requireActual('child_process'),
  execFile: (...args: unknown[]) => mockExecFile(...args),
}));

jest.mock('vscode-languageclient/node', () => ({
  State: { Stopped: 1, Running: 2, Starting: 3 },
  LanguageClient: jest.fn().mockImplementation(() => {
    let state = 1;
    const serverProcess = {
      // Use the live Jest process so the production liveness probe cannot
      // mistake a synthetic PID for an exited child on rejected cleanup.
      pid: process.pid,
      exitCode: null as number | null,
      signalCode: null as string | null,
      exitListeners: [] as Array<(code: number, signal: string | null) => void>,
      once(_event: 'exit', listener: (code: number, signal: string | null) => void) {
        this.exitListeners.push(listener);
        return this;
      },
      removeListener(_event: 'exit', listener: (code: number, signal: string | null) => void) {
        const index = this.exitListeners.indexOf(listener);
        if (index >= 0) {
          this.exitListeners.splice(index, 1);
        }
        return this;
      },
      exit() {
        this.exitCode = 0;
        for (const listener of [...this.exitListeners]) {
          listener(0, null);
        }
      },
    };
    return {
      get state() {
        return state;
      },
      get serverProcess() {
        return serverProcess;
      },
      initializeResult: { capabilities: {} },
      onDidChangeState: mockLanguageClientOnDidChangeState,
      onNotification: mockLanguageClientOnNotification,
      sendNotification: mockLanguageClientSendNotification,
      setTrace: mockLanguageClientSetTrace,
      async start() {
        state = 3;
        await mockLanguageClientStart();
        state = 2;
      },
      async stop() {
        try {
          await mockLanguageClientStop();
          serverProcess.exit();
        } finally {
          // Match the client's terminal state even when its handshake rejects.
          state = 1;
        }
      },
      dispose: mockLanguageClientDispose,
    };
  }),
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import { activate, deactivate, serverNotRunningMessage } from '../extension';
import { fakeDocument, setOpenDocuments, type FakeDocument } from './serverDemandDocuments';

function makeContext(extensionPath: string): vscode.ExtensionContext {
  const state = {
    get: jest.fn(() => undefined),
    update: jest.fn(async () => undefined),
  };

  return {
    extension: {
      packageJSON: {
        publisher: 'EffortlessMetrics',
        name: 'perl-lsp-rs',
        version: '0.17.0',
      },
    },
    extensionMode: vscode.ExtensionMode.Production,
    extensionPath,
    globalState: state,
    subscriptions: [],
    workspaceState: state,
  } as unknown as vscode.ExtensionContext;
}

function mockConfig(serverPath: string): void {
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((key: string, defaultValue?: unknown) => {
      if (key === 'serverPath') {
        return serverPath;
      }
      if (key === 'autoDownload') {
        return false;
      }
      return defaultValue;
    }),
    has: jest.fn(() => false),
    inspect: jest.fn(),
    update: jest.fn(async () => undefined),
  }));
}

/**
 * Wait for a condition the code under test is expected to reach.
 *
 * Positive assertions must not depend on a fixed number of event-loop turns:
 * the demand path awaits several calls, and adding one more await would turn a
 * correct implementation into an intermittent CI failure.
 */
async function waitUntil(condition: () => boolean, timeoutMs = 1_000): Promise<void> {
  const startedAt = Date.now();
  while (!condition()) {
    if (Date.now() - startedAt > timeoutMs) {
      throw new Error('condition not met before timeout');
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

/** Wait until the language client has been started `count` times. */
function waitForStarts(count: number, timeoutMs = 1_000): Promise<void> {
  return waitUntil(() => mockLanguageClientStart.mock.calls.length >= count, timeoutMs);
}

/**
 * Drain pending work so an "it did NOT start" assertion is meaningful.
 *
 * Absence cannot be waited for, only bounded: this gives the demand path more
 * than enough turns to start a server if it were going to.
 */
async function settle(): Promise<void> {
  for (let index = 0; index < 20; index += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** Drive the listener the extension most recently armed for Perl documents. */
function fireDocumentOpened(document: FakeDocument): void {
  const calls = jest.mocked(vscode.workspace.onDidOpenTextDocument).mock.calls;
  const listener = calls[calls.length - 1]?.[0] as ((document: FakeDocument) => void) | undefined;
  expect(listener).toBeDefined();
  listener?.(document);
}

function fireActiveEditorChanged(document: FakeDocument): void {
  const calls = jest.mocked(vscode.window.onDidChangeActiveTextEditor).mock.calls;
  const listener = calls[calls.length - 1]?.[0] as
    | ((editor: { document: FakeDocument } | undefined) => void)
    | undefined;
  expect(listener).toBeDefined();
  listener?.({ document });
}

/** Temporary extension roots created by the current test, removed in afterEach. */
const createdRoots: string[] = [];

function makeExtensionRoot(): string {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-demand-'));
  createdRoots.push(extensionRoot);
  const serverPath = path.join(
    extensionRoot,
    process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
  );
  fs.writeFileSync(serverPath, '');
  mockConfig(serverPath);
  return extensionRoot;
}

describe('deferred language-server startup (#8180)', () => {
  const originalSkipStartup = process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;

  beforeEach(() => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { isTrusted: boolean }).isTrusted = true;
    setOpenDocuments([]);
  });

  afterEach(async () => {
    if (originalSkipStartup === undefined) {
      delete process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
    } else {
      process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = originalSkipStartup;
    }
    await deactivate();
    setOpenDocuments([]);
    // Each test mkdtemps a root with a fake server binary; without this the
    // suite leaves one temp directory per test behind on every run.
    while (createdRoots.length > 0) {
      fs.rmSync(createdRoots.pop() as string, { recursive: true, force: true });
    }
    jest.clearAllMocks();
  });

  test('activation without a Perl document does not start the language server', async () => {
    await activate(makeContext(makeExtensionRoot()));
    await settle();

    // This is the regression #8180 exists to remove: Gherkin-only,
    // walkthrough-only, and debug-only sessions paid for a server they never
    // used.
    expect(mockLanguageClientStart).not.toHaveBeenCalled();
  });

  test('a Gherkin document alone does not start the language server', async () => {
    setOpenDocuments([fakeDocument('gherkin')]);

    await activate(makeContext(makeExtensionRoot()));
    await settle();
    fireDocumentOpened(fakeDocument('gherkin'));
    await settle();

    expect(mockLanguageClientStart).not.toHaveBeenCalled();
  });

  test('an already-open Perl document starts the language server', async () => {
    setOpenDocuments([fakeDocument('perl')]);

    await activate(makeContext(makeExtensionRoot()));
    await waitForStarts(1);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
  });

  test('a Perl document opened after a non-LSP activation starts the server once', async () => {
    setOpenDocuments([fakeDocument('gherkin')]);

    await activate(makeContext(makeExtensionRoot()));
    await settle();
    expect(mockLanguageClientStart).not.toHaveBeenCalled();

    // Without this the user would have to reload the window to get any Perl
    // language features at all.
    fireDocumentOpened(fakeDocument('perl'));
    await waitForStarts(1);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
  });

  test('a Perl document becoming active starts the server once', async () => {
    await activate(makeContext(makeExtensionRoot()));
    await settle();

    // A document restored with the window never fires onDidOpenTextDocument.
    fireActiveEditorChanged(fakeDocument('perl'));
    await waitForStarts(1);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
  });

  test('repeated Perl document events start exactly one client generation', async () => {
    await activate(makeContext(makeExtensionRoot()));
    await settle();

    fireDocumentOpened(fakeDocument('perl'));
    fireDocumentOpened(fakeDocument('perl'));
    fireActiveEditorChanged(fakeDocument('perl'));
    await waitForStarts(1);
    // The failure mode here is a *second* generation, so drain before asserting:
    // stopping at the first start would pass before a duplicate could appear.
    await settle();

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
  });

  test('a Perl document in a virtual scheme does not start the server', async () => {
    await activate(makeContext(makeExtensionRoot()));
    await settle();

    fireDocumentOpened(fakeDocument('perl', 'git'));
    await settle();

    expect(mockLanguageClientStart).not.toHaveBeenCalled();
  });

  test('a stopped client can be started again by fresh demand', async () => {
    setOpenDocuments([fakeDocument('perl')]);

    await activate(makeContext(makeExtensionRoot()));
    await waitForStarts(1);
    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);

    // deactivate() is the terminal path (#7854): the committed activation
    // runtime releases the armed demand listeners and the coordinator
    // projections along with the client. A production host never delivers
    // demand to a deactivated extension, so fresh demand has to arrive in a
    // fresh session.
    await deactivate();
    await settle();

    // The fresh session starts dormant: no open Perl document, and no
    // stranded "server is running" belief carried over from the stopped
    // session.
    setOpenDocuments([]);
    await activate(makeContext(makeExtensionRoot()));
    await settle();
    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);

    // Fresh demand in the fresh session starts a second client generation —
    // the stop never strands the user on a dead server.
    fireDocumentOpened(fakeDocument('perl'));
    await waitForStarts(2);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(2);
  });

  test('an on-first-use command starts a dormant server', async () => {
    await activate(makeContext(makeExtensionRoot()));
    await settle();
    expect(mockLanguageClientStart).not.toHaveBeenCalled();

    // showVersion is ledgered `on-first-use`. Before this it only read the
    // cached path, so a dormant session answered an explicit version request
    // with "server is not running".
    await vscode.commands.executeCommand('perl-lsp.showVersion');
    await waitForStarts(1);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
  });

  test('a failed demand start is surfaced, not swallowed', async () => {
    // ensureStarted reports failure through its state instead of rejecting, so
    // a try/catch around it never runs. The restart command must still tell the
    // user their explicit request failed.
    mockLanguageClientStart.mockImplementationOnce(async () => {
      throw new Error('spawn refused');
    });

    await activate(makeContext(makeExtensionRoot()));
    await settle();

    await vscode.commands.executeCommand('perl-lsp.restart');
    await settle();

    const errorMessages = jest
      .mocked(vscode.window.showErrorMessage)
      .mock.calls.map((call) => String(call[0]));
    expect(errorMessages.some((message) => message.includes('Failed to start'))).toBe(true);
  });

  test('registered health check rejects an existing binary after activation startup fails', async () => {
    mockLanguageClientStart.mockImplementationOnce(async () => {
      throw new Error('health-check startup refused');
    });

    await activate(makeContext(makeExtensionRoot()));

    const result = (await vscode.commands.executeCommand('perl-lsp.runHealthCheck')) as {
      ok: boolean;
      checks: Array<{ label: string; status: string; detail: string }>;
    };
    const runtime = result.checks.find((check) => check.label === 'LSP runtime');

    expect(result.ok).toBe(false);
    expect(runtime).toEqual({
      label: 'LSP runtime',
      status: 'error',
      detail: 'Language server failed to start: health-check startup refused',
    });
    expect(vscode.window.showInformationMessage).not.toHaveBeenCalledWith(
      'Perl LSP health check passed.',
      'Show Output',
    );
  });

  test('registered health check settles while startup failure dialog remains open', async () => {
    mockLanguageClientStart.mockImplementationOnce(async () => {
      throw new Error('dialog-blocked startup refusal');
    });
    jest
      .mocked(vscode.window.showErrorMessage)
      .mockImplementationOnce(() => new Promise<undefined>(() => undefined));

    await activate(makeContext(makeExtensionRoot()));

    const result = await Promise.race([
      vscode.commands.executeCommand('perl-lsp.runHealthCheck') as Promise<{
        ok: boolean;
        checks: Array<{ label: string; status: string }>;
      }>,
      new Promise<never>((_, reject) => {
        setTimeout(() => reject(new Error('health check remained blocked by startup dialog')), 500);
      }),
    ]);
    expect(result.ok).toBe(false);
    expect(result.checks.find((check) => check.label === 'LSP runtime')?.status).toBe('error');
  });

  test('a late startup probe cannot overwrite a recovered generation', async () => {
    let deferredProbe: ((error: Error | null, stdout: string, stderr: string) => void) | undefined;
    mockExecFile.mockImplementationOnce((...args: unknown[]) => {
      deferredProbe = args[args.length - 1] as typeof deferredProbe;
    });
    mockLanguageClientStart
      .mockImplementationOnce(async () => {
        throw new Error('deferred diagnosis startup refusal');
      })
      .mockImplementationOnce(async () => undefined);

    setOpenDocuments([fakeDocument('perl')]);
    await activate(makeContext(makeExtensionRoot()));
    await waitForStarts(1);
    await vscode.commands.executeCommand('perl-lsp.restart');
    await waitForStarts(2);
    expect(deferredProbe).toBeDefined();
    const oldProbeError = Object.assign(new Error('old generation probe'), { code: 'EACCES' });
    deferredProbe?.(oldProbeError, '', '');
    await settle();

    expect(serverNotRunningMessage()).toContain('Language Server is not running');
    expect(serverNotRunningMessage()).not.toContain('old generation probe');
    expect(serverNotRunningMessage()).not.toContain('permission denied');
  });

  test('a later health check reports recovery while keeping optional warnings separate', async () => {
    mockLanguageClientStart
      .mockImplementationOnce(async () => {
        throw new Error('first health-check startup refused');
      })
      .mockImplementationOnce(async () => undefined);

    await activate(makeContext(makeExtensionRoot()));

    const first = (await vscode.commands.executeCommand('perl-lsp.runHealthCheck')) as {
      ok: boolean;
      checks: Array<{ label: string; status: string; detail: string }>;
    };
    const second = (await vscode.commands.executeCommand('perl-lsp.runHealthCheck')) as {
      ok: boolean;
      checks: Array<{ label: string; status: string; detail: string }>;
    };

    expect(first.ok).toBe(false);
    expect(first.checks.find((check) => check.label === 'LSP runtime')).toMatchObject({
      status: 'error',
    });
    expect(second.ok).toBe(true);
    expect(second.checks.find((check) => check.label === 'LSP runtime')).toEqual({
      label: 'LSP runtime',
      status: 'ok',
      detail: 'Language server is running.',
    });
    expect(
      second.checks
        .filter((check) => check.status === 'warning')
        .every((check) => check.label !== 'LSP runtime'),
    ).toBe(true);
  });

  test('a failed restart does not suppress fresh demand', async () => {
    // First start succeeds, so demand reaches `running`.
    mockLanguageClientStart.mockImplementationOnce(async () => undefined);
    setOpenDocuments([fakeDocument('perl')]);

    await activate(makeContext(makeExtensionRoot()));
    await waitForStarts(1);

    // Fresh demand is correctly ignored while the server is running.
    fireDocumentOpened(fakeDocument('perl'));
    await settle();
    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);

    // A failed explicit restart stops the running generation and fails to
    // replace it. The demand owner must learn the server is gone, otherwise
    // its stale `running` belief turns every later Perl document into a no-op.
    mockLanguageClientStart.mockImplementationOnce(async () => {
      throw new Error('spawn refused');
    });
    await vscode.commands.executeCommand('perl-lsp.restart');
    await settle();

    fireDocumentOpened(fakeDocument('perl'));
    await waitForStarts(3);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(3);
  });

  test('the status widget reports dormant rather than starting', async () => {
    await activate(makeContext(makeExtensionRoot()));
    await settle();

    const statusBarItem = jest.mocked(vscode.window.createStatusBarItem).mock.results[0]?.value as {
      text: string;
    };

    // Reporting `starting` while no start is intended is indistinguishable
    // from a hung server.
    expect(statusBarItem.text).not.toContain('sync~spin');
    expect(statusBarItem.text).toContain('not started');
  });
});
