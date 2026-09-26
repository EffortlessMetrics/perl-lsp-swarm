import * as vscode from 'vscode';
import { FileKind } from '../fileCreation';
import { createPerlScaffold } from '../scaffoldCommands';

const root = vscode.Uri.file('/project');

beforeEach(() => {
  jest.clearAllMocks();
  (vscode.window as { activeTextEditor?: unknown }).activeTextEditor = undefined;
  (vscode.workspace as { workspaceFolders?: unknown }).workspaceFolders = [{ uri: root }];
  (vscode.workspace.getWorkspaceFolder as jest.Mock).mockImplementation((uri: vscode.Uri) =>
    uri.fsPath.startsWith('/project/') ? { uri: root } : undefined,
  );
  (vscode.workspace.applyEdit as jest.Mock).mockResolvedValue(true);
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(undefined);
});

test('explicit module creation submits one non-overwriting create with initial content', async () => {
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/project/lib/Foo/Bar.pm'),
  );

  expect(await createPerlScaffold(FileKind.Module)).toBe('applied');
  const edit = (vscode.workspace.applyEdit as jest.Mock).mock.calls[0]?.[0] as {
    creates: Array<{
      uri: vscode.Uri;
      options: { overwrite: boolean; ignoreIfExists: boolean; contents: Uint8Array };
    }>;
    inserts: unknown[];
  };
  expect(edit.creates).toHaveLength(1);
  expect(edit.creates[0]?.uri.fsPath).toBe('/project/lib/Foo/Bar.pm');
  expect(edit.creates[0]?.options).toMatchObject({ overwrite: false, ignoreIfExists: false });
  expect(Buffer.from(edit.creates[0]?.options.contents ?? []).toString('utf8')).toContain(
    'package Foo::Bar;',
  );
  expect(edit.inserts).toHaveLength(0);
  expect(vscode.window.showTextDocument).toHaveBeenCalledTimes(1);
  expect(vscode.workspace.openTextDocument).not.toHaveBeenCalled();
});

test('test creation uses the same non-overwriting transaction', async () => {
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/project/t/example.t'),
  );

  expect(await createPerlScaffold(FileKind.Test)).toBe('applied');
  const edit = (vscode.workspace.applyEdit as jest.Mock).mock.calls[0]?.[0] as {
    creates: Array<{ options: { contents: Uint8Array } }>;
  };
  expect(edit.creates).toHaveLength(1);
  expect(Buffer.from(edit.creates[0]?.options.contents ?? []).toString('utf8')).toContain(
    'use Test::More;',
  );
});

test('cancel, outside target, and ambiguous module root do not mutate', async () => {
  expect(await createPerlScaffold(FileKind.Module)).toBe('user_cancelled');
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/elsewhere/lib/Foo.pm'),
  );
  expect(await createPerlScaffold(FileKind.Module)).toBe('ineligible_target');
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/project/other/Foo.pm'),
  );
  expect(await createPerlScaffold(FileKind.Module)).toBe('ambiguous_module_root');
  expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
});

test('a rejected create is reported and never presented as opened', async () => {
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/project/lib/Foo.pm'),
  );
  (vscode.workspace.applyEdit as jest.Mock).mockResolvedValue(false);

  expect(await createPerlScaffold(FileKind.Module)).toBe('apply_rejected');
  expect(vscode.window.showErrorMessage).toHaveBeenCalledTimes(1);
  expect(vscode.window.showTextDocument).not.toHaveBeenCalled();
});

test('a destination dialog failure has no file effect', async () => {
  (vscode.window.showSaveDialog as jest.Mock).mockRejectedValueOnce(new Error('dialog failed'));
  expect(await createPerlScaffold(FileKind.Module)).toBe('failed');
  expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
});

test('an editor open failure does not misreport a successful create as a failed write', async () => {
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/project/lib/Foo.pm'),
  );
  (vscode.window.showTextDocument as jest.Mock).mockRejectedValueOnce(
    new Error('editor unavailable'),
  );

  expect(await createPerlScaffold(FileKind.Module)).toBe('open_failed');
  expect(vscode.workspace.applyEdit).toHaveBeenCalledTimes(1);
  expect(vscode.window.showWarningMessage).toHaveBeenCalledTimes(1);
});

test('a second workspace folder owns the chosen module name', async () => {
  const second = vscode.Uri.file('/second');
  (vscode.workspace as { workspaceFolders?: unknown }).workspaceFolders = [
    { uri: root },
    { uri: second },
  ];
  (vscode.workspace.getWorkspaceFolder as jest.Mock).mockImplementation((uri: vscode.Uri) =>
    uri.fsPath.startsWith('/second/') ? { uri: second } : { uri: root },
  );
  (vscode.window.showSaveDialog as jest.Mock).mockResolvedValue(
    vscode.Uri.file('/second/lib/Other.pm'),
  );

  expect(await createPerlScaffold(FileKind.Module)).toBe('applied');
  const edit = (vscode.workspace.applyEdit as jest.Mock).mock.calls[0]?.[0] as {
    creates: Array<{ options: { contents: Uint8Array } }>;
  };
  expect(Buffer.from(edit.creates[0]?.options.contents ?? []).toString('utf8')).toContain(
    'package Other;',
  );
});
