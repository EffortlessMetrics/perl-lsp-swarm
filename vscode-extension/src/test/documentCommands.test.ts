import * as vscode from 'vscode';
import * as path from 'path';
import {
  formatDocumentCommand,
  openPerlModuleCommand,
  runCheckSyntaxCommand,
  showIncPathsCommand,
  showParserAstCommand,
  type ExecFileLike,
} from '../documentCommands';

type TestEditor = {
  document: {
    languageId: string;
    isDirty: boolean;
    uri: { fsPath: string; toString: () => string };
    save: jest.Mock<Promise<void>, []>;
  };
};

function makeEditor(overrides: Partial<TestEditor['document']> = {}): TestEditor {
  return {
    document: {
      languageId: 'perl',
      isDirty: false,
      uri: {
        fsPath: '/workspace/lib/Example.pm',
        toString: () => 'file:///workspace/lib/Example.pm',
      },
      save: jest.fn(async () => undefined),
      ...overrides,
    },
  };
}

function setActiveEditor(editor: TestEditor | undefined): void {
  Object.assign(vscode.window, { activeTextEditor: editor });
}

function makeOutputChannel(): { clear: jest.Mock; appendLine: jest.Mock; show: jest.Mock } {
  return { clear: jest.fn(), appendLine: jest.fn(), show: jest.fn() };
}

describe('document command implementations', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    setActiveEditor(undefined);
    Object.assign(vscode.workspace, { workspaceFolders: undefined });
    (vscode.workspace.getWorkspaceFolder as jest.Mock).mockReturnValue(undefined);
    (vscode.workspace.asRelativePath as jest.Mock).mockImplementation(
      (uri: { fsPath: string }) => uri.fsPath,
    );
  });

  test('rejects syntax checks without an active Perl editor', async () => {
    await runCheckSyntaxCommand({
      outputChannel: makeOutputChannel(),
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'No active Perl file to check syntax',
    );
  });

  test('saves dirty documents and reports successful syntax checks', async () => {
    const editor = makeEditor({ isDirty: true });
    setActiveEditor(editor);
    (vscode.workspace.getWorkspaceFolder as jest.Mock).mockReturnValue({
      uri: { fsPath: '/workspace' },
    });
    const execFile: ExecFileLike = jest.fn((_file, args, _options, callback) => {
      expect(args).toEqual([
        '-I',
        path.join('/workspace', 'lib'),
        '-I',
        path.join('/workspace', 'local/lib/perl5'),
        '-c',
        editor.document.uri.fsPath,
      ]);
      callback(null, '', '');
    });

    await runCheckSyntaxCommand({
      outputChannel: makeOutputChannel(),
      serverNotRunningMessage: () => 'server unavailable',
      execFile,
    });

    expect(editor.document.save).toHaveBeenCalledTimes(1);
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith('Syntax OK: Example.pm');
  });

  test('offers syntax failure output and writes it to the injected channel', async () => {
    const editor = makeEditor();
    setActiveEditor(editor);
    const outputChannel = makeOutputChannel();
    const execFile: ExecFileLike = jest.fn((_file, _args, _options, callback) => {
      callback(new Error('perl failed'), '', 'syntax error at Example.pm line 1');
    });
    (vscode.window.showErrorMessage as jest.Mock).mockResolvedValueOnce('Show Output');

    await runCheckSyntaxCommand({
      outputChannel,
      serverNotRunningMessage: () => 'server unavailable',
      execFile,
    });

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Syntax error: syntax error at Example.pm line 1',
      'Show Output',
    );
    expect(outputChannel.appendLine).toHaveBeenCalledWith(
      '[check-syntax] syntax error at Example.pm line 1',
    );
    expect(outputChannel.show).toHaveBeenCalledTimes(1);
  });

  test('shows @INC paths in a dedicated output channel', async () => {
    const outputChannel = makeOutputChannel();
    (vscode.window.createOutputChannel as jest.Mock).mockReturnValueOnce(outputChannel);
    const execFile: ExecFileLike = jest.fn((_file, args, _options, callback) => {
      expect(args).toEqual(['-e', 'print join("\\n", @INC)']);
      callback(null, '/one\n/two\n', '');
    });

    await showIncPathsCommand(execFile);

    expect(outputChannel.clear).toHaveBeenCalledTimes(1);
    expect(outputChannel.appendLine).toHaveBeenCalledWith('Perl @INC paths:');
    expect(outputChannel.appendLine).toHaveBeenCalledWith('  /one');
    expect(outputChannel.appendLine).toHaveBeenCalledWith('  /two');
    expect(outputChannel.show).toHaveBeenCalledTimes(1);
  });

  test('opens the selected workspace module', async () => {
    const first = { fsPath: '/workspace/lib/Zed.pm' };
    const second = { fsPath: '/workspace/lib/Alpha.pm' };
    Object.assign(vscode.workspace, { workspaceFolders: [{ uri: { fsPath: '/workspace' } }] });
    (vscode.workspace.asRelativePath as jest.Mock).mockImplementation((uri: { fsPath: string }) =>
      uri.fsPath.replace('/workspace/', ''),
    );
    (vscode.workspace.findFiles as jest.Mock).mockResolvedValueOnce([first, second]);
    const selected = { label: 'Alpha', description: 'lib/Alpha.pm', uri: second };
    (vscode.window.showQuickPick as jest.Mock).mockResolvedValueOnce(selected);
    const document = { uri: second };
    (vscode.workspace.openTextDocument as jest.Mock).mockResolvedValueOnce(document);

    await openPerlModuleCommand();

    expect(vscode.window.showQuickPick).toHaveBeenCalledWith(
      [
        { label: 'Alpha', description: 'lib/Alpha.pm', uri: second },
        { label: 'Zed', description: 'lib/Zed.pm', uri: first },
      ],
      { placeHolder: 'Search Perl modules...', matchOnDescription: true },
    );
    expect(vscode.workspace.openTextDocument).toHaveBeenCalledWith(second);
    expect(vscode.window.showTextDocument).toHaveBeenCalledWith(document);
  });

  test('reports unavailable parser AST requests without a client', async () => {
    setActiveEditor(makeEditor());

    await showParserAstCommand({
      outputChannel: makeOutputChannel(),
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith('server unavailable');
  });

  test('requests and displays the parser AST from the active client', async () => {
    const editor = makeEditor();
    setActiveEditor(editor);
    const outputChannel = makeOutputChannel();
    (vscode.window.createOutputChannel as jest.Mock).mockReturnValueOnce(outputChannel);
    const sendRequest = jest.fn(async () => '(program)');

    await showParserAstCommand({
      activeClient: { sendRequest },
      outputChannel: makeOutputChannel(),
      serverNotRunningMessage: () => 'server unavailable',
    });

    expect(sendRequest).toHaveBeenCalledWith('perl/showAst', {
      uri: 'file:///workspace/lib/Example.pm',
    });
    expect(outputChannel.appendLine).toHaveBeenCalledWith('AST for: /workspace/lib/Example.pm');
    expect(outputChannel.appendLine).toHaveBeenCalledWith('(program)');
    expect(outputChannel.show).toHaveBeenCalledTimes(1);
  });

  test('delegates formatting only for an active Perl editor', async () => {
    setActiveEditor(makeEditor());

    await formatDocumentCommand();

    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('editor.action.formatDocument');
  });
});
