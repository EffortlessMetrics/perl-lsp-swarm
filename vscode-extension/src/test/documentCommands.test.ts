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
      callback(
        Object.assign(new Error('perl failed'), { code: 255 }),
        '',
        'syntax error at Example.pm line 1',
      );
    });
    (vscode.window.showErrorMessage as jest.Mock).mockResolvedValueOnce('Show Output');

    await runCheckSyntaxCommand({
      outputChannel,
      serverNotRunningMessage: () => 'server unavailable',
      execFile,
    });

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Syntax check failed: syntax error at Example.pm line 1',
      'Show Output',
    );
    expect(outputChannel.appendLine).toHaveBeenCalledWith(
      '[check-syntax] Syntax check failed: syntax error at Example.pm line 1',
    );
    expect(outputChannel.show).toHaveBeenCalledTimes(1);
  });

  test.each([
    {
      name: 'missing Perl',
      error: Object.assign(new Error('spawn perl ENOENT'), { code: 'ENOENT' }),
      message:
        'Could not check syntax: Perl was not found on PATH. Install Perl and add it to PATH.',
    },
    {
      name: 'timed-out Perl',
      error: Object.assign(new Error('Command failed'), { code: null, killed: true }),
      message: 'Could not check syntax: perl -c timed out after 10 seconds.',
    },
    {
      name: 'another spawn failure',
      error: Object.assign(new Error('spawn perl EACCES'), { code: 'EACCES' }),
      message: 'Could not check syntax: spawn perl EACCES',
    },
    {
      name: 'a killed maxBuffer failure',
      error: Object.assign(new Error('stdout maxBuffer length exceeded'), {
        code: 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER',
        killed: true,
      }),
      message: 'Could not check syntax: stdout maxBuffer length exceeded',
    },
    {
      name: 'an empty nonzero Perl exit',
      error: Object.assign(new Error('Command failed'), { code: 255 }),
      message: 'Syntax check failed: perl -c exited with code 255 without output.',
    },
  ])('reports $name without an empty syntax verdict', async ({ error, message }) => {
    setActiveEditor(makeEditor());
    const outputChannel = makeOutputChannel();
    const execFile: ExecFileLike = jest.fn((_file, _args, _options, callback) => {
      callback(error, '', '');
    });
    (vscode.window.showErrorMessage as jest.Mock).mockResolvedValueOnce('Show Output');

    await runCheckSyntaxCommand({
      outputChannel,
      serverNotRunningMessage: () => 'server unavailable',
      execFile,
    });

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(message, 'Show Output');
    expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
    expect(outputChannel.appendLine).toHaveBeenCalledWith(`[check-syntax] ${message}`);
    expect(outputChannel.show).toHaveBeenCalledTimes(1);
  });

  test('keeps partial Perl output available after a timeout', async () => {
    setActiveEditor(makeEditor());
    const outputChannel = makeOutputChannel();
    const execFile: ExecFileLike = jest.fn((_file, _args, _options, callback) => {
      callback(
        Object.assign(new Error('Command failed'), { code: null, killed: true }),
        '',
        'BEGIN warning before timeout',
      );
    });
    (vscode.window.showErrorMessage as jest.Mock).mockResolvedValueOnce('Show Output');

    await runCheckSyntaxCommand({
      outputChannel,
      serverNotRunningMessage: () => 'server unavailable',
      execFile,
    });

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Could not check syntax: perl -c timed out after 10 seconds.',
      'Show Output',
    );
    expect(outputChannel.appendLine).toHaveBeenNthCalledWith(
      1,
      '[check-syntax] Could not check syntax: perl -c timed out after 10 seconds.',
    );
    expect(outputChannel.appendLine).toHaveBeenNthCalledWith(2, 'BEGIN warning before timeout');
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

  test('delegates formatting for an active perl5 alias editor (#7699)', async () => {
    setActiveEditor(makeEditor({ languageId: 'perl5' }));

    await formatDocumentCommand();

    expect(vscode.commands.executeCommand).toHaveBeenCalledWith('editor.action.formatDocument');
    expect(vscode.window.showErrorMessage).not.toHaveBeenCalled();
  });

  test('still refuses formatting for a non-Perl editor', async () => {
    setActiveEditor(makeEditor({ languageId: 'javascript' }));

    await formatDocumentCommand();

    expect(vscode.commands.executeCommand).not.toHaveBeenCalledWith('editor.action.formatDocument');
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith('No active Perl file to format');
  });
});
