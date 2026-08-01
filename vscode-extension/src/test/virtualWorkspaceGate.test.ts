/**
 * Virtual-workspace startup gate tests.
 *
 * Verifies that the extension:
 *  1. Decides deferral only when every open folder is virtual.
 *  2. Does not download or spawn the native server in a virtual workspace.
 *  3. Reports the real boundary through the status bar and through
 *     `serverNotRunningMessage()` instead of a restart or health-check prompt.
 *  4. Starts normally once a file-backed folder is opened.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const mockLanguageClientStart = jest.fn(() => new Promise<void>(() => undefined));

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: jest.fn().mockImplementation(() => ({
    initializeResult: { capabilities: {} },
    onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
    setTrace: jest.fn(async () => undefined),
    start: mockLanguageClientStart,
    stop: jest.fn(async () => undefined),
    dispose: jest.fn(async () => undefined),
  })),
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import { activate, deactivate, serverNotRunningMessage } from '../extension';
import { decideVirtualWorkspaceGate } from '../virtualWorkspaceGate';

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
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

function makeServerBinary(): { extensionRoot: string; serverPath: string } {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-virtual-'));
  const serverPath = path.join(
    extensionRoot,
    process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
  );
  fs.writeFileSync(serverPath, '');
  return { extensionRoot, serverPath };
}

// ---------------------------------------------------------------------------
// Decision contract
// ---------------------------------------------------------------------------
describe('virtual workspace gate decision', () => {
  test('starts when no folder is open, so single loose files stay served', () => {
    expect(decideVirtualWorkspaceGate({ folders: [] })).toEqual({ kind: 'start' });
  });

  test('starts for file-backed folders', () => {
    expect(
      decideVirtualWorkspaceGate({ folders: [{ uri: { scheme: 'file', fsPath: '/workspace' } }] }),
    ).toEqual({ kind: 'start' });
  });

  test('starts for a mixed workspace with at least one file-backed folder', () => {
    // The file-backed folder is fully serveable; blocking it because a virtual
    // folder is also open would remove working features from the user.
    expect(
      decideVirtualWorkspaceGate({
        folders: [{ uri: { scheme: 'vscode-vfs' } }, { uri: { scheme: 'file', fsPath: '/w' } }],
      }),
    ).toEqual({ kind: 'start' });
  });

  test('defers when every folder is virtual and names the schemes once each', () => {
    const decision = decideVirtualWorkspaceGate({
      folders: [
        { uri: { scheme: 'vscode-vfs' } },
        { uri: { scheme: 'vscode-vfs' } },
        { uri: { scheme: 'git' } },
      ],
    });

    expect(decision.kind).toBe('defer');
    if (decision.kind !== 'defer') {
      throw new Error('expected a deferral');
    }
    expect(decision.folderSchemes).toEqual(['vscode-vfs', 'git']);
    expect(decision.logMessage).toContain('vscode-vfs:, git:');
    expect(decision.userMessage).toMatch(/file-backed folder/);
  });

  test('treats a folder with no scheme and no path as virtual rather than file-backed', () => {
    expect(decideVirtualWorkspaceGate({ folders: [{ uri: {} }] }).kind).toBe('defer');
  });
});

// ---------------------------------------------------------------------------
// Activation gate
// ---------------------------------------------------------------------------
describe('virtual workspace activation gate', () => {
  const originalSkipStartup = process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;

  afterEach(async () => {
    if (originalSkipStartup === undefined) {
      delete process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
    } else {
      process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = originalSkipStartup;
    }
    (vscode.workspace as { isTrusted: boolean }).isTrusted = true;
    (vscode.workspace as { workspaceFolders: unknown }).workspaceFolders = undefined;
    await deactivate();
    jest.clearAllMocks();
  });

  test('does not start the language server when every folder is virtual', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { workspaceFolders: unknown }).workspaceFolders = [
      { uri: { scheme: 'vscode-vfs', fsPath: '' } },
    ];
    const { extensionRoot, serverPath } = makeServerBinary();
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));
    await delay(100);

    expect(mockLanguageClientStart).not.toHaveBeenCalled();
    expect(vscode.workspace.onDidChangeWorkspaceFolders).toHaveBeenCalled();
  });

  test('explains the virtual-workspace boundary instead of prompting for a restart', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { workspaceFolders: unknown }).workspaceFolders = [
      { uri: { scheme: 'vscode-vfs', fsPath: '' } },
    ];
    const { extensionRoot, serverPath } = makeServerBinary();
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));

    const message = serverNotRunningMessage();
    expect(message).toMatch(/virtual workspace/i);
    expect(message).toMatch(/vscode-vfs:/);
    expect(message).not.toMatch(/Health Check/);
  });

  test('starts once a file-backed folder is added to the virtual workspace', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    (vscode.workspace as { workspaceFolders: unknown }).workspaceFolders = [
      { uri: { scheme: 'vscode-vfs', fsPath: '' } },
    ];
    const { extensionRoot, serverPath } = makeServerBinary();
    mockConfig(serverPath);

    let onFoldersChanged: (() => void) | undefined;
    (vscode.workspace.onDidChangeWorkspaceFolders as jest.Mock).mockImplementation(
      (callback: () => void) => {
        onFoldersChanged = callback;
        return { dispose: jest.fn() };
      },
    );

    await activate(makeContext(extensionRoot));
    expect(mockLanguageClientStart).not.toHaveBeenCalled();

    (vscode.workspace as { workspaceFolders: unknown }).workspaceFolders = [
      { uri: { scheme: 'vscode-vfs', fsPath: '' } },
      { uri: { scheme: 'file', fsPath: extensionRoot } },
    ];
    onFoldersChanged?.();
    await delay(100);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
    // The deferral no longer applies, so the boundary must stop being reported.
    expect(serverNotRunningMessage()).not.toMatch(/virtual workspace/i);
  });

  test('starts normally in a file-backed workspace (regression guard)', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '0';
    const { extensionRoot, serverPath } = makeServerBinary();
    (vscode.workspace as { workspaceFolders: unknown }).workspaceFolders = [
      { uri: { scheme: 'file', fsPath: extensionRoot } },
    ];
    mockConfig(serverPath);

    await activate(makeContext(extensionRoot));
    await delay(100);

    expect(mockLanguageClientStart).toHaveBeenCalledTimes(1);
  });
});
