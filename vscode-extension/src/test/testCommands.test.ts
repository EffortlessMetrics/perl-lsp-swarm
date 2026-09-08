import * as vscode from 'vscode';
import {
  runAllTestsWithProve,
  runCurrentTestWithProve,
  runTestAtCursorCommand,
  runTestsCommand,
} from '../testCommands';

type TestEditor = {
  document: {
    languageId: string;
    isDirty: boolean;
    uri: { fsPath: string; toString: () => string };
    save: jest.Mock<Promise<void>, []>;
  };
  selection: { active: { line: number; character: number } };
};

function makeEditor(overrides: Partial<TestEditor['document']> = {}): TestEditor {
  return {
    document: {
      languageId: 'perl',
      isDirty: false,
      uri: { fsPath: '/workspace/t/example.t', toString: () => 'file:///workspace/t/example.t' },
      save: jest.fn(async () => undefined),
      ...overrides,
    },
    selection: { active: { line: 3, character: 2 } },
  };
}

function setActiveEditor(editor: TestEditor | undefined): void {
  Object.assign(vscode.window, { activeTextEditor: editor });
}

describe('test command implementations', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    setActiveEditor(undefined);
    Object.assign(vscode.workspace, { workspaceFolders: undefined });
    (vscode.workspace.getWorkspaceFolder as jest.Mock).mockReturnValue(undefined);
  });

  test('runs the selected file and restores status-bar state after adapter failure', async () => {
    const editor = makeEditor();
    setActiveEditor(editor);
    const statusBarItem = { text: 'Ready', tooltip: 'ready' };
    const runFileTests = jest.fn(async () => {
      throw new Error('prove failed');
    });

    await expect(
      runTestsCommand(undefined, {
        testAdapter: { runFileTests },
        statusBarItem,
        serverNotRunningMessage: () => 'server unavailable',
      }),
    ).rejects.toThrow('prove failed');

    expect(runFileTests).toHaveBeenCalledWith(editor.document.uri);
    expect(statusBarItem).toEqual({ text: 'Ready', tooltip: 'ready' });
  });

  test('rejects non-test files before invoking the adapter', async () => {
    setActiveEditor(
      makeEditor({
        uri: {
          fsPath: '/workspace/lib/Example.pm',
          toString: () => 'file:///workspace/lib/Example.pm',
        },
      }),
    );
    const runFileTests = jest.fn(async () => undefined);

    await runTestsCommand(undefined, {
      testAdapter: { runFileTests },
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(runFileTests).not.toHaveBeenCalled();
    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
      'Run Tests is only available for .t and .pl files',
    );
  });

  test('uses a debug-selected test target without requiring an active editor', async () => {
    const runFileTests = jest.fn(async () => undefined);

    await runTestsCommand(
      {
        label: 'selected-test',
        uri: { fsPath: '/workspace/t/selected.t' },
        args: [],
      },
      {
        testAdapter: { runFileTests },
        serverNotRunningMessage: () => 'server unavailable',
      },
    );

    expect(runFileTests).toHaveBeenCalledWith(
      expect.objectContaining({ fsPath: '/workspace/t/selected.t' }),
    );
  });

  test('saves the document, selects the matching code lens, and dispatches it', async () => {
    const editor = makeEditor({ isDirty: true });
    setActiveEditor(editor);
    const sendRequest = jest.fn(async () => [
      {
        range: {
          start: { line: 3, character: 0 },
          end: { line: 4, character: 10 },
        },
        command: { command: 'perl.runTest', arguments: ['t/example.t'] },
      },
    ]);

    await runTestAtCursorCommand({
      activeClient: { sendRequest },
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(editor.document.save).toHaveBeenCalledTimes(1);
    expect(sendRequest).toHaveBeenCalledWith('textDocument/codeLens', {
      textDocument: { uri: 'file:///workspace/t/example.t' },
    });
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('perl.runTest', 't/example.t');
  });

  test('saves and requests code lenses for a perl5 alias editor (#7699)', async () => {
    const editor = makeEditor({ languageId: 'perl5', isDirty: true });
    setActiveEditor(editor);
    const sendRequest = jest.fn(async () => [
      {
        range: {
          start: { line: 3, character: 0 },
          end: { line: 4, character: 10 },
        },
        command: { command: 'perl.runTest', arguments: ['t/example.t'] },
      },
    ]);

    await runTestAtCursorCommand({
      activeClient: { sendRequest },
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(editor.document.save).toHaveBeenCalledTimes(1);
    expect(sendRequest).toHaveBeenCalledWith('textDocument/codeLens', {
      textDocument: { uri: 'file:///workspace/t/example.t' },
    });
  });

  test('reports server availability before requesting code lenses', async () => {
    setActiveEditor(makeEditor());
    const sendRequest = jest.fn(async () => []);

    await runTestAtCursorCommand({
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(sendRequest).not.toHaveBeenCalled();
    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith('server unavailable');
  });

  test('builds a current-file prove task with the workspace cwd', async () => {
    const editor = makeEditor({ isDirty: true });
    setActiveEditor(editor);
    (vscode.workspace.getWorkspaceFolder as jest.Mock).mockReturnValue({
      uri: { fsPath: '/workspace' },
    });

    await runCurrentTestWithProve();

    const task = (vscode.tasks.executeTask as jest.Mock).mock.calls[0]?.[0];
    expect(task?.name).toBe('Perl Tests: Current File');
    expect(task?.execution.command).toBe('prove');
    expect(task?.execution.args).toEqual(['-v', '/workspace/t/example.t']);
    expect(task?.execution.options).toEqual({ cwd: '/workspace' });
    expect(editor.document.save).toHaveBeenCalledTimes(1);
  });

  test('builds a recursive prove task for the first workspace folder', async () => {
    Object.assign(vscode.workspace, { workspaceFolders: [{ uri: { fsPath: '/workspace' } }] });

    await runAllTestsWithProve();

    const task = (vscode.tasks.executeTask as jest.Mock).mock.calls[0]?.[0];
    expect(task?.name).toBe('Perl Tests: All');
    expect(task?.execution.args).toEqual(['-r', 't/']);
    expect(task?.execution.options).toEqual({ cwd: '/workspace' });
  });
});
