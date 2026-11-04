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

function extractEdit(uri = 'file:///workspace/lib/Example.pm') {
  return {
    changes: {
      [uri]: [
        {
          range: {
            start: { line: 2, character: 1 },
            end: { line: 2, character: 12 },
          },
          newText: 'extracted',
        },
      ],
    },
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

  test('reaches server-availability for a perl5 alias editor (#7699)', async () => {
    setActiveEditor(
      makeEditor({
        document: {
          languageId: 'perl5',
          uri: { toString: () => 'file:///workspace/lib/Example.pm' },
        },
      }),
    );

    await extractVariableCommand(dependencies());

    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith('server unavailable');
    expect(vscode.window.showErrorMessage).not.toHaveBeenCalled();
  });

  test('still refuses a non-Perl editor before any server interaction', async () => {
    setActiveEditor(
      makeEditor({
        document: {
          languageId: 'javascript',
          uri: { toString: () => 'file:///workspace/lib/Example.js' },
        },
      }),
    );

    await extractVariableCommand(dependencies());

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Extract Variable requires an active Perl file with a selection',
    );
  });

  test('requests a variable action with the selected range and applies its edit', async () => {
    const editor = makeEditor();
    setActiveEditor(editor);
    const asWorkspaceEdit = jest.fn(async (edit: unknown) => ({ edit }));
    const sendRequest = jest.fn(async () => [
      { title: 'Renamed action', kind: 'refactor.extract.variable', edit: extractEdit() },
      {
        title: 'Extract Variable',
        kind: 'refactor.extract.subroutine',
        command: { command: 'ignored' },
      },
    ]);
    const client = { sendRequest, protocol2CodeConverter: { asWorkspaceEdit } };

    await extractVariableCommand(dependencies(client));

    expect(sendRequest).toHaveBeenCalledWith('textDocument/codeAction', {
      textDocument: { uri: 'file:///workspace/lib/Example.pm' },
      range: {
        start: { line: 2, character: 1 },
        end: { line: 2, character: 12 },
      },
      context: { diagnostics: [], only: ['refactor.extract.variable'], triggerKind: 2 },
    });
    expect(asWorkspaceEdit).toHaveBeenCalledWith(extractEdit());
    expect(vscode.workspace.applyEdit).toHaveBeenCalledWith({ edit: extractEdit() });
  });

  test('selects a method action and dispatches command-based edits', async () => {
    setActiveEditor(makeEditor());
    const sendRequest = jest.fn(async () => [
      { title: 'Extract Method', kind: 'refactor.extract.variable', command: { command: 'first' } },
      {
        title: 'Renamed subroutine action',
        kind: 'refactor.extract.subroutine',
        command: { command: 'perl.extractMethod', arguments: ['x'] },
      },
    ]);
    const client = {
      sendRequest,
      protocol2CodeConverter: { asWorkspaceEdit: jest.fn() },
    };

    await extractMethodCommand(dependencies(client));

    expect(sendRequest).toHaveBeenCalledWith(
      'textDocument/codeAction',
      expect.objectContaining({
        context: { diagnostics: [], only: ['refactor.extract.subroutine'], triggerKind: 2 },
      }),
    );
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl.extractMethod', 'x');
  });

  test('applies the exact subroutine edit despite misleading titles and order', async () => {
    setActiveEditor(makeEditor());
    const asWorkspaceEdit = jest.fn(async (edit: unknown) => ({ edit }));
    const variableEdit = extractEdit('file:///variable.pl');
    const subroutineEdit = extractEdit('file:///subroutine.pl');
    const client = {
      sendRequest: jest.fn(async () => [
        { title: 'Extract Method', kind: 'refactor.extract.variable', edit: variableEdit },
        { title: 'Renamed action', kind: 'refactor.extract.subroutine', edit: subroutineEdit },
      ]),
      protocol2CodeConverter: { asWorkspaceEdit },
    };
    await extractMethodCommand(dependencies(client));
    expect(asWorkspaceEdit).toHaveBeenCalledWith(subroutineEdit);
    expect(vscode.workspace.applyEdit).toHaveBeenCalledWith({ edit: subroutineEdit });
    expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
  });

  test.each([
    ['variable', extractVariableCommand, 'refactor.extract.variable'],
    ['method', extractMethodCommand, 'refactor.extract.subroutine'],
  ])('runs a validated follow-up command after the %s edit succeeds', async (_name, run, kind) => {
    setActiveEditor(makeEditor());
    (vscode.workspace.applyEdit as jest.Mock).mockResolvedValueOnce(true);
    const edit = extractEdit();
    const client = {
      sendRequest: jest.fn(async () => [
        {
          title: 'Renamed action',
          kind,
          edit,
          command: { command: 'perl.afterExtract', arguments: ['x'] },
        },
      ]),
      protocol2CodeConverter: { asWorkspaceEdit: jest.fn(async () => ({ edit })) },
    };
    await run(dependencies(client));
    expect(vscode.workspace.applyEdit).toHaveBeenCalledWith({ edit });
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl.afterExtract', 'x');
    expect((vscode.workspace.applyEdit as jest.Mock).mock.invocationCallOrder[0]).toBeLessThan(
      (vscode.commands.executeCommand as jest.Mock).mock.invocationCallOrder[0] as number,
    );
  });

  test.each([
    [
      'variable',
      extractVariableCommand,
      'refactor.extract.variable',
      'No extract variable action is available for the current selection',
    ],
    [
      'method',
      extractMethodCommand,
      'refactor.extract.subroutine',
      'No extract method action is available for the current selection',
    ],
  ])(
    'refuses the %s follow-up command when applyEdit returns false',
    async (_name, run, kind, message) => {
      setActiveEditor(makeEditor());
      (vscode.workspace.applyEdit as jest.Mock).mockResolvedValueOnce(false);
      const client = {
        sendRequest: jest.fn(async () => [
          {
            title: 'Renamed action',
            kind,
            edit: extractEdit(),
            command: { command: 'perl.afterExtract' },
          },
        ]),
        protocol2CodeConverter: { asWorkspaceEdit: jest.fn(async () => ({ edit: true })) },
      };
      await run(dependencies(client));
      expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
      expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(message);
    },
  );

  test('refuses the follow-up command when workspace edit application rejects', async () => {
    setActiveEditor(makeEditor());
    (vscode.workspace.applyEdit as jest.Mock).mockRejectedValueOnce(new Error('edit rejected'));
    const client = {
      sendRequest: jest.fn(async () => [
        {
          title: 'Renamed action',
          kind: 'refactor.extract.variable',
          edit: extractEdit(),
          command: { command: 'perl.afterExtract' },
        },
      ]),
      protocol2CodeConverter: { asWorkspaceEdit: jest.fn(async () => ({ edit: true })) },
    };
    await extractVariableCommand(dependencies(client));
    expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'No extract variable action is available for the current selection',
    );
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

  test.each([
    [
      'variable',
      extractVariableCommand,
      'refactor.extract.subroutine',
      'No extract variable action is available for the current selection',
    ],
    [
      'method',
      extractMethodCommand,
      'refactor.extract.variable',
      'No extract method action is available for the current selection',
    ],
  ])(
    'refuses opposite-only %s responses with edits or commands',
    async (_name, run, kind, message) => {
      setActiveEditor(makeEditor());
      for (const payload of [{ edit: { changes: {} } }, { command: { command: 'opposite' } }]) {
        jest.clearAllMocks();
        const client = {
          sendRequest: jest.fn(async () => [
            { title: 'Extract Variable Method', kind, ...payload },
          ]),
          protocol2CodeConverter: { asWorkspaceEdit: jest.fn() },
        };
        await run(dependencies(client));
        expect(client.protocol2CodeConverter.asWorkspaceEdit).not.toHaveBeenCalled();
        expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
        expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
        expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(message);
      }
    },
  );

  test.each([
    [
      'variable',
      extractVariableCommand,
      'refactor.extract.variable',
      'No extract variable action is available for the current selection',
    ],
    [
      'method',
      extractMethodCommand,
      'refactor.extract.subroutine',
      'No extract method action is available for the current selection',
    ],
  ])('refuses generic, disabled, and malformed %s actions', async (_name, run, kind, message) => {
    setActiveEditor(makeEditor());
    const responses = [
      { title: 'Old server', kind: 'refactor.extract', edit: { changes: {} } },
      { kind, edit: extractEdit() },
      { title: 7, kind, edit: extractEdit() },
      {
        title: 'Disabled',
        kind,
        disabled: { reason: 'selection required' },
        edit: { changes: {} },
      },
      { title: 'Malformed edit', kind, edit: { changes: null } },
      { title: 'Malformed nested edit', kind, edit: { changes: { 'file:///x': 'not-an-array' } } },
      { title: 'Empty changes', kind, edit: { changes: {} }, command: { command: 'follow-up' } },
      {
        title: 'Empty text edits',
        kind,
        edit: { changes: { 'file:///x': [] } },
        command: { command: 'follow-up' },
      },
      {
        title: 'Empty document changes',
        kind,
        edit: { documentChanges: [] },
        command: { command: 'follow-up' },
      },
      { title: 'Malformed command', kind, command: { command: 1 } },
    ];
    for (const response of responses) {
      jest.clearAllMocks();
      const client = {
        sendRequest: jest.fn(async () => [response]),
        protocol2CodeConverter: { asWorkspaceEdit: jest.fn() },
      };
      await run(dependencies(client));
      expect(client.protocol2CodeConverter.asWorkspaceEdit).not.toHaveBeenCalled();
      expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
      expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
      expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(message);
    }
  });

  test.each([
    ['variable', extractVariableCommand, 'refactor.extract.variable'],
    ['method', extractMethodCommand, 'refactor.extract.subroutine'],
  ])('skips an empty %s edit before a substantive action', async (_name, run, kind) => {
    setActiveEditor(makeEditor());
    const edit = extractEdit();
    const asWorkspaceEdit = jest.fn(async (value: unknown) => ({ edit: value }));
    const client = {
      sendRequest: jest.fn(async () => [
        { title: 'Empty edit', kind, edit: { changes: {} } },
        { title: 'Substantive edit', kind, edit },
      ]),
      protocol2CodeConverter: { asWorkspaceEdit },
    };

    await run(dependencies(client));

    expect(asWorkspaceEdit).toHaveBeenCalledTimes(1);
    expect(asWorkspaceEdit).toHaveBeenCalledWith(edit);
    expect(vscode.workspace.applyEdit).toHaveBeenCalledWith({ edit });
  });

  test('refuses a typed edit when conversion rejects', async () => {
    setActiveEditor(makeEditor());
    const client = {
      sendRequest: jest.fn(async () => [
        {
          title: 'Variable',
          kind: 'refactor.extract.variable',
          edit: extractEdit(),
          command: { command: 'perl.afterExtract' },
        },
      ]),
      protocol2CodeConverter: {
        asWorkspaceEdit: jest.fn(async () => {
          throw new Error('bad edit');
        }),
      },
    };
    await extractVariableCommand(dependencies(client));
    expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
    expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'No extract variable action is available for the current selection',
    );
  });

  test.each([
    ['variable', extractVariableCommand],
    ['method', extractMethodCommand],
  ])('shows rejected %s requests to the user', async (_name, run) => {
    setActiveEditor(makeEditor());
    const client = {
      sendRequest: jest.fn(async () => {
        throw new Error('server rejected');
      }),
      protocol2CodeConverter: { asWorkspaceEdit: jest.fn() },
    };
    await run(dependencies(client));
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining('server rejected'),
    );
    expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
    expect(vscode.commands.executeCommand).not.toHaveBeenCalled();
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
