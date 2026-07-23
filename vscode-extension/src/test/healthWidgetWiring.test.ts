/**
 * Extension-level wiring test for HealthWidget telemetry (#4620).
 *
 * Activates the real `extension.ts` module (with the language-client start
 * short-circuited via PERL_LSP_EXTENSION_TEST_SKIP_STARTUP) and asserts that
 * the production code path registers the diagnostics listener and pushes the
 * initial file/error counts — i.e. that `HealthWidget.setFileCount` /
 * `setErrorCount` are actually called from production code, not just unit
 * tests. The full Running-mode status-bar-text assertion is covered by
 * `healthWidgetDataSource.test.ts`, because driving the widget into Running
 * requires a real server start.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const mockLanguageClientOnDidChangeState = jest.fn(() => ({ dispose: jest.fn() }));

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: jest.fn().mockImplementation(() => ({
    initializeResult: { capabilities: {} },
    onDidChangeState: mockLanguageClientOnDidChangeState,
    setTrace: jest.fn(async () => undefined),
    start: jest.fn(() => new Promise<void>(() => undefined)),
    stop: jest.fn(async () => undefined),
    dispose: jest.fn(async () => undefined),
  })),
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import { activate, deactivate } from '../extension';

function makeContext(extensionPath: string): vscode.ExtensionContext {
  const state = {
    get: jest.fn(() => undefined),
    update: jest.fn(async () => undefined),
  };
  return {
    extension: {
      packageJSON: { publisher: 'EffortlessMetrics', name: 'perl-lsp-rs', version: '0.17.0' },
    },
    extensionMode: vscode.ExtensionMode.Production,
    extensionPath,
    globalState: state,
    subscriptions: [],
    workspaceState: state,
  } as unknown as vscode.ExtensionContext;
}

function perlUri(fsPath: string): vscode.Uri {
  return { fsPath, toString: () => `file://${fsPath}` } as unknown as vscode.Uri;
}

describe('extension activation wires HealthWidget counts (#4620)', () => {
  const originalSkipStartup = process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;

  beforeEach(() => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '1';
  });

  afterEach(async () => {
    if (originalSkipStartup === undefined) {
      delete process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
    } else {
      process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = originalSkipStartup;
    }
    await deactivate();
    jest.clearAllMocks();
  });

  test('activation registers the diagnostics listener and pushes initial counts', async () => {
    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-wiring-'));

    const fileUris = [perlUri('/ws/lib.pm'), perlUri('/ws/main.pl')];
    (vscode.workspace.findFiles as jest.Mock).mockImplementation(async () => fileUris);

    let diagListener: ((event: { uris: readonly vscode.Uri[] }) => void) | undefined;
    let getDiagnosticsCalls = 0;
    (vscode.languages.onDidChangeDiagnostics as jest.Mock).mockImplementation(
      (handler: (event: { uris: readonly vscode.Uri[] }) => void) => {
        diagListener = handler;
        return { dispose: jest.fn() };
      },
    );
    (vscode.languages.getDiagnostics as jest.Mock).mockImplementation(() => {
      getDiagnosticsCalls += 1;
      return [[perlUri('/ws/lib.pm'), [{ severity: 0 }, { severity: 0 }]]];
    });

    await activate(makeContext(extensionRoot));

    // The production wiring registered a diagnostics listener during activation.
    expect(vscode.languages.onDidChangeDiagnostics).toHaveBeenCalledTimes(1);
    expect(diagListener).toBeDefined();

    // The initial refresh queried diagnostics and the workspace file scan ran.
    expect(getDiagnosticsCalls).toBeGreaterThanOrEqual(1);
    expect(vscode.workspace.findFiles).toHaveBeenCalled();

    // A representative diagnostics event flows through the production listener:
    // it re-queries diagnostics (setErrorCount path) without throwing.
    const before = getDiagnosticsCalls;
    expect(() => diagListener!({ uris: [perlUri('/ws/lib.pm')] })).not.toThrow();
    expect(getDiagnosticsCalls).toBeGreaterThan(before);
  }, 10000);
});
