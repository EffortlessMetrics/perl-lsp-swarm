import * as vscode from 'vscode';
import {
  organizeImportsCommand,
  showStatusMenuCommand,
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
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl-lsp.organizeImports');
  });
});
