import * as vscode from 'vscode';
import {
  extractMethodCommand,
  extractVariableCommand,
  showRefactoringOptionsCommand,
} from '../refactoringCommands';

type TestEditor = {
  document: {
    languageId: string;
    uri: { toString: () => string };
  };
  selection: {
    isEmpty: boolean;
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
};

function makeEditor(overrides: Partial<TestEditor> = {}): TestEditor {
  return {
    document: {
      languageId: 'perl',
      uri: { toString: () => 'file:///workspace/lib/Example.pm' },
    },
    selection: {
      isEmpty: false,
      start: { line: 2, character: 1 },
      end: { line: 2, character: 12 },
    },
    ...overrides,
  };
}

function setActiveEditor(editor: TestEditor | undefined): void {
  Object.assign(vscode.window, { activeTextEditor: editor });
}

function dependencies(activeClient?: {
  sendRequest: jest.Mock;
  protocol2CodeConverter: { asWorkspaceEdit: jest.Mock };
}) {
  return {
    activeClient,
    serverNotRunningMessage: () => 'server unavailable',
  };
}

describe('refactoring command implementations', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    setActiveEditor(undefined);
  });

  test('validates the active Perl editor and selection before requesting a variable action', async () => {
    await extractVariableCommand(dependencies());
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Extract Variable requires an active Perl file with a selection',
    );

    setActiveEditor(
      makeEditor({
        selection: {
          isEmpty: true,
          start: { line: 0, character: 0 },
          end: { line: 0, character: 0 },
        },
      }),
    );
    await extractVariableCommand(dependencies());
    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
      'Select an expression to extract as a variable',
    );
  });

  test('reports unavailable server state without sending a request', async () => {
    setActiveEditor(makeEditor());

    await extractMethodCommand(dependencies());

    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith('server unavailable');
  });

  test('requests a variable action with the selected range and applies its edit', async () => {
    const editor = makeEditor();
    setActiveEditor(editor);
    const asWorkspaceEdit = jest.fn(async (edit: unknown) => ({ edit }));
    const sendRequest = jest.fn(async () => [
      { title: 'Extract Variable', edit: { changes: {} } },
      { title: 'Extract Method', command: { command: 'ignored' } },
    ]);
    const client = { sendRequest, protocol2CodeConverter: { asWorkspaceEdit } };

    await extractVariableCommand(dependencies(client));

    expect(sendRequest).toHaveBeenCalledWith('textDocument/codeAction', {
      textDocument: { uri: 'file:///workspace/lib/Example.pm' },
      range: {
        start: { line: 2, character: 1 },
        end: { line: 2, character: 12 },
      },
      context: { diagnostics: [], only: ['refactor.extract'], triggerKind: 2 },
    });
    expect(asWorkspaceEdit).toHaveBeenCalledWith({ changes: {} });
    expect(vscode.workspace.applyEdit).toHaveBeenCalledWith({ edit: { changes: {} } });
  });

  test('selects a method action and dispatches command-based edits', async () => {
    setActiveEditor(makeEditor());
    const sendRequest = jest.fn(async () => [
      { title: 'Unrelated action', command: { command: 'first' } },
      { title: 'Extract Method', command: { command: 'perl.extractMethod', arguments: ['x'] } },
    ]);
    const client = {
      sendRequest,
      protocol2CodeConverter: { asWorkspaceEdit: jest.fn() },
    };

    await extractMethodCommand(dependencies(client));

    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl.extractMethod', 'x');
  });

  test('reports when no extract action is available', async () => {
    setActiveEditor(makeEditor());
    const client = {
      sendRequest: jest.fn(async () => []),
      protocol2CodeConverter: { asWorkspaceEdit: jest.fn() },
    };

    await extractVariableCommand(dependencies(client));

    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'No extract actions available for the selected expression',
    );
  });

  test('shows refactoring options and dispatches the selected command', async () => {
    setActiveEditor(makeEditor());
    (vscode.window.showQuickPick as jest.Mock).mockResolvedValueOnce({
      command: 'perl-lsp.extractMethod',
      args: ['from-picker'],
    });

    await showRefactoringOptionsCommand();

    expect(vscode.window.showQuickPick).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ command: 'perl-lsp.extractVariable' }),
        expect.objectContaining({ command: 'perl-lsp.extractMethod' }),
      ]),
      { placeHolder: 'Perl Refactoring Options' },
    );
    // The organize-imports entry is withdrawn (#8305) and must stay absent.
    const items = (vscode.window.showQuickPick as jest.Mock).mock.calls[0][0] as Array<{
      command?: string;
    }>;
    expect(items.find((item) => item.command === 'perl-lsp.organizeImports')).toBeUndefined();
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
      'perl-lsp.extractMethod',
      'from-picker',
    );
  });
});
