import * as vscode from 'vscode';
import {
  organizeImportsCommand,
  showStatusMenuCommand,
  showWorkspaceStatusCommand,
  showVersionCommand,
} from '../navigationCommands';

type TestEditor = {
  document: {
    languageId: string;
    uri: { fsPath: string };
  };
};

function setActiveEditor(editor: TestEditor | undefined): void {
  Object.assign(vscode.window, { activeTextEditor: editor });
}

function makeEditor(languageId = 'perl', fsPath = '/workspace/t/example.t'): TestEditor {
  return { document: { languageId, uri: { fsPath } } };
}

function dependencies(overrides: Partial<Parameters<typeof showVersionCommand>[0]> = {}) {
  return {
    currentServerPath: () => '/workspace/perllsp',
    outputChannel: { show: jest.fn() },
    serverNotRunningMessage: () => 'server unavailable',
    getServerVersion: jest.fn(async () => 'perllsp 0.17.0'),
    ...overrides,
  };
}

describe('navigation command implementations', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    setActiveEditor(undefined);
  });

  test('delegates organize imports to VS Code', async () => {
    await organizeImportsCommand();
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('editor.action.organizeImports');
  });

  test('offers recovery actions when the server is unavailable', async () => {
    const outputChannel = { show: jest.fn() };
    const deps = dependencies({
      currentServerPath: () => null,
      outputChannel,
    });
    (vscode.window.showErrorMessage as jest.Mock).mockResolvedValueOnce('Show Output');

    await showVersionCommand(deps);

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'server unavailable',
      'Restart Server',
      'Show Output',
      'Run Health Check',
    );
    expect(outputChannel.show).toHaveBeenCalledTimes(1);
  });

  test('shows and copies the active server version', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Copy');

    await showVersionCommand(deps);

    expect(deps.getServerVersion).toHaveBeenCalledWith('/workspace/perllsp');
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Perl LSP Version: perllsp 0.17.0',
      'Copy',
    );
    expect(vscode.env.clipboard.writeText).toHaveBeenCalledWith('perllsp 0.17.0');
  });

  test('shows a healthy workspace status with explicit product and observed server identity', async () => {
    const getWorkspaceStatus = jest.fn(() => ({
      mode: 'running' as const,
      version: 'perllsp 0.17.0',
      fileCount: 12,
      errorCount: 2,
    }));
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Show Output');

    await showWorkspaceStatusCommand({ getWorkspaceStatus });

    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Perl LSP workspace status\nProduct: perl-lsp\nServer state: running\nObserved server: perllsp 0.17.0\nWorkspace files: 12\nDiagnostics: 2 errors\nWorkspace index: legacy server (enhanced readiness unavailable)',
      'Run Health Check',
      'Show Output',
      'Open Actions',
    );
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl-lsp.showOutput');
  });

  test('preserves a compatibility or custom server identity instead of naming it perllsp', async () => {
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(undefined);

    await showWorkspaceStatusCommand({
      getWorkspaceStatus: () => ({
        mode: 'running',
        version: 'perl-lsp 0.17.0',
      }),
    });

    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Perl LSP workspace status\nProduct: perl-lsp\nServer state: running\nObserved server: perl-lsp 0.17.0\nWorkspace index: legacy server (enhanced readiness unavailable)',
      'Run Health Check',
      'Show Output',
      'Open Actions',
    );
  });

  test('shows lifecycle, readiness, and active-document status', async () => {
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(undefined);

    await showWorkspaceStatusCommand({
      getWorkspaceStatus: () => ({
        mode: 'running',
        lifecycle: 'ready_limited',
        readinessState: 'ready_limited',
        readinessReason: 'Workspace file limit reached',
        activeDocumentReady: false,
        nextAction: 'Wait for the active document to become ready.',
      }),
    });

    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Perl LSP workspace status\nProduct: perl-lsp\nServer state: running\nLifecycle: ready_limited\nWorkspace index: ready_limited\nActive document: not ready\nCoverage: Workspace file limit reached\nNext: Wait for the active document to become ready.',
      'Run Health Check',
      'Show Output',
      'Open Actions',
    );
  });

  test('omits active-document readiness for unsupported editors', async () => {
    setActiveEditor(makeEditor('markdown', '/workspace/README.md'));
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(undefined);

    await showWorkspaceStatusCommand({
      getWorkspaceStatus: () => ({
        mode: 'running',
        readinessState: 'legacy',
      }),
    });

    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Perl LSP workspace status\nProduct: perl-lsp\nServer state: running\nWorkspace index: legacy server (enhanced readiness unavailable)',
      'Run Health Check',
      'Show Output',
      'Open Actions',
    );
  });

  test('shows lifecycle detail when the server has a known failure cause', async () => {
    (vscode.window.showWarningMessage as jest.Mock).mockResolvedValueOnce(undefined);

    await showWorkspaceStatusCommand({
      getWorkspaceStatus: () => ({
        mode: 'stopped',
        lifecycle: 'failed',
        lifecycleDetail: 'Managed server binary is missing.',
        nextAction: 'Reinstall the server.',
      }),
    });

    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
      'Perl LSP workspace status\nProduct: perl-lsp\nServer state: stopped\nLifecycle: failed\nDetail: Managed server binary is missing.\nNext: Reinstall the server.',
      'Restart Server',
      'Run Health Check',
      'Show Output',
      'Open Actions',
    );
  });

  test('offers restart for a stopped workspace', async () => {
    (vscode.window.showWarningMessage as jest.Mock).mockResolvedValueOnce('Restart Server');

    await showWorkspaceStatusCommand({
      getWorkspaceStatus: () => ({
        mode: 'stopped',
        version: 'perllsp 0.16.0',
      }),
    });

    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
      'Perl LSP workspace status\nProduct: perl-lsp\nServer state: stopped',
      'Restart Server',
      'Run Health Check',
      'Show Output',
      'Open Actions',
    );
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl-lsp.restart');
  });

  test('offers reinstall when the version probe fails', async () => {
    const deps = dependencies({
      getServerVersion: jest.fn(async () => {
        throw new Error('binary failed');
      }),
    });
    (vscode.window.showErrorMessage as jest.Mock).mockResolvedValueOnce('Reinstall');

    await showVersionCommand(deps);

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining('binary failed'),
      'Reinstall',
    );
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl-lsp.reinstall');
  });

  test('builds a context-aware status menu and dispatches enabled actions', async () => {
    setActiveEditor(makeEditor());
    (vscode.window.showQuickPick as jest.Mock).mockResolvedValueOnce({
      command: 'perl-lsp.organizeImports',
    });

    await showStatusMenuCommand();

    const items = (vscode.window.showQuickPick as jest.Mock).mock.calls[0]?.[0] as Array<{
      command?: string;
      disabled?: boolean;
    }>;
    expect(items.find((item) => item.command === 'perl-lsp.organizeImports')?.disabled).toBe(false);
    expect(items.find((item) => item.command === 'perl-lsp.runTests')?.disabled).toBe(false);
    expect(items.find((item) => item.command === 'perl-lsp.showWorkspaceStatus')?.disabled).toBe(
      undefined,
    );
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl-lsp.organizeImports');
  });
});
