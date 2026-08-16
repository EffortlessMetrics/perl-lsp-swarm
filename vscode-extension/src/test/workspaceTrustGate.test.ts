/**
 * Workspace Trust gate tests (#4631).
 *
 * Verifies that the extension:
 *  1. Declares untrustedWorkspaces.supported = false in the manifest.
 *  2. Does not start the language server when the workspace is untrusted.
 *  3. Registers an onDidGrantWorkspaceTrust listener when untrusted.
 *  4. Starts the language server normally when the workspace is trusted.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const mockLanguageClientStart = jest.fn(() => new Promise<void>(() => undefined));
const mockLanguageClientStop = jest.fn(async () => undefined);
const mockLanguageClientDispose = jest.fn(async () => undefined);
const mockLanguageClientSetTrace = jest.fn(async () => undefined);
const mockLanguageClientOnDidChangeState = jest.fn(() => ({ dispose: jest.fn() }));
const mockLanguageClientOnNotification = jest.fn(() => ({ dispose: jest.fn() }));

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: jest.fn().mockImplementation(() => ({
    initializeResult: { capabilities: {} },
    onDidChangeState: mockLanguageClientOnDidChangeState,
    onNotification: mockLanguageClientOnNotification,
    setTrace: mockLanguageClientSetTrace,
    start: mockLanguageClientStart,
    stop: mockLanguageClientStop,
    dispose: mockLanguageClientDispose,
  })),
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import { activate, deactivate } from '../extension';

function delay(ms: number): Promise<'timeout'> {
  return new Promise((resolve) => {
    setTimeout(() => resolve('timeout'), ms);
  });
}

async function waitUntil(condition: () => boolean, timeoutMs: number): Promise<void> {
  const startedAt = Date.now();
  while (!condition()) {
    if (Date.now() - startedAt > timeoutMs) {
      throw new Error('condition not met before timeout');
    }
    await new Promise((resolve) => {
      setTimeout(resolve, 10);
    });
  }
}

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

// ---------------------------------------------------------------------------
// Manifest contract
// ---------------------------------------------------------------------------
/**
 * Language-server startup is demand-driven (#8180). A trust test that wants to
 * observe a real start has to supply the demand an `onLanguage:perl`
 * activation would carry.
 */
function openPerlDocument(): () => void {
  const workspaceMock = vscode.workspace as unknown as { textDocuments: unknown[] };
  const original = workspaceMock.textDocuments;
  workspaceMock.textDocuments = [
    {
      languageId: 'perl',
      uri: { scheme: 'file', toString: () => 'file:///workspace/demo.pl' },
      version: 1,
      getText: () => '',
    },
  ];
  return () => {
    workspaceMock.textDocuments = original;
  };
}

describe('workspace trust manifest contract (#4631)', () => {
  const EXT_ROOT = path.resolve(__dirname, '..', '..');
  const pkg = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));

  test('declares untrustedWorkspaces.supported as false', () => {
    expect(pkg.capabilities?.untrustedWorkspaces?.supported).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Activation trust gate
// ---------------------------------------------------------------------------
describe('workspace trust activation gate (#4631)', () => {
  const originalSkipStartup = process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;

  afterEach(async () => {
    if (originalSkipStartup === undefined) {
      delete process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
    } else {
      process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = originalSkipStartup;
    }

    // Reset trust state to default.
    (vscode.workspace as { isTrusted: boolean }).isTrusted = true;
    await deactivate();
    jest.clearAllMocks();
  });

  test('does not start the language server when workspace is untrusted', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { isTrusted: boolean }).isTrusted = false;

    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-trust-'));
    const serverPath = path.join(
      extensionRoot,
      process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
    );
    fs.writeFileSync(serverPath, '');
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));

    // Give a brief window for any background work to surface.
    await delay(100);

    // The language client must not have been started.
    expect(mockLanguageClientStart).not.toHaveBeenCalled();
  });

  test('presents untrusted deferral as configuration action, not an endless start', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { isTrusted: boolean }).isTrusted = false;

    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-trust-'));
    const serverPath = path.join(
      extensionRoot,
      process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
    );
    fs.writeFileSync(serverPath, '');
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));

    const statusBarItem = jest.mocked(vscode.window.createStatusBarItem).mock.results[0]?.value as {
      text: string;
      tooltip: string;
      backgroundColor: unknown;
    };

    // The failure this guards against is the widget sitting on the spinner
    // forever, which reads as a hung server rather than a decision the user
    // has to make.
    expect(statusBarItem.text).not.toContain('sync~spin');
    expect(statusBarItem.text).toContain('action required');
    expect(statusBarItem.backgroundColor).toBeDefined();

    // The reason and the repair must both be legible without opening logs.
    expect(statusBarItem.tooltip).toContain('not trusted');
    expect(statusBarItem.tooltip).toContain('Trust this workspace');
  });

  test('clears the action-required state once trust is granted', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { isTrusted: boolean }).isTrusted = false;

    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-trust-'));
    const serverPath = path.join(
      extensionRoot,
      process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
    );
    fs.writeFileSync(serverPath, '');
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));

    const statusBarItem = jest.mocked(vscode.window.createStatusBarItem).mock.results[0]?.value as {
      text: string;
      backgroundColor: unknown;
    };
    expect(statusBarItem.text).toContain('action required');

    // Fire the trust-granted callback the extension registered.
    const grantTrust = jest.mocked(vscode.workspace.onDidGrantWorkspaceTrust).mock
      .calls[0]?.[0] as () => void;
    expect(grantTrust).toBeDefined();
    (vscode.workspace as { isTrusted: boolean }).isTrusted = true;
    grantTrust();

    // An action-required state that outlives its cause is its own defect.
    expect(statusBarItem.text).not.toContain('action required');
    expect(statusBarItem.backgroundColor).toBeUndefined();
  });

  test('registers onDidGrantWorkspaceTrust listener when workspace is untrusted', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { isTrusted: boolean }).isTrusted = false;

    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-trust-'));
    const serverPath = path.join(
      extensionRoot,
      process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
    );
    fs.writeFileSync(serverPath, '');
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));

    expect(vscode.workspace.onDidGrantWorkspaceTrust).toHaveBeenCalled();
  });

  test('starts the language server when workspace is trusted (regression guard)', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { isTrusted: boolean }).isTrusted = true;
    const restoreDocuments = openPerlDocument();

    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-trust-'));
    const serverPath = path.join(
      extensionRoot,
      process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
    );
    fs.writeFileSync(serverPath, '');
    mockConfig(serverPath);

    const activation = activate(makeContext(extensionRoot));

    await expect(
      Promise.race([activation.then(() => 'activated' as const), delay(250)]),
    ).resolves.toBe('activated');
    await waitUntil(() => mockLanguageClientStart.mock.calls.length > 0, 500);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
    // onDidGrantWorkspaceTrust should NOT be registered when already trusted.
    expect(vscode.workspace.onDidGrantWorkspaceTrust).not.toHaveBeenCalled();
    restoreDocuments();
  });
});
