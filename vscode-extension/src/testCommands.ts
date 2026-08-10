import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { parseDebugTestLaunchTarget } from './debugAdapter';
import type { PerlTestAdapter } from './testAdapter';
import { selectTestCommandAtPosition } from './runTestAtCursor';

type TestLanguageClient = Pick<LanguageClient, 'sendRequest'>;
type TestAdapter = Pick<PerlTestAdapter, 'runFileTests'>;
type StatusBar = Pick<vscode.StatusBarItem, 'text' | 'tooltip'>;

export interface TestCommandDependencies {
  readonly activeClient?: TestLanguageClient | undefined;
  readonly testAdapter?: TestAdapter | undefined;
  readonly statusBarItem?: StatusBar | undefined;
  readonly serverNotRunningMessage: () => string;
}

type TestLens = {
  range?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  command?: { command: string; arguments?: unknown[] };
};

/** Run the active or debug-selected Perl test through the Test Explorer adapter. */
export async function runTestsCommand(
  test: unknown,
  dependencies: TestCommandDependencies,
): Promise<void> {
  let targetUri: vscode.Uri | undefined;

  if (test) {
    const target = parseDebugTestLaunchTarget(test);
    if (target?.program) {
      targetUri = vscode.Uri.file(target.program);
    }
  }

  if (!targetUri) {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'perl') {
      vscode.window.showErrorMessage('No active Perl file to test');
      return;
    }
    targetUri = editor.document.uri;
  }

  const filePath = targetUri.fsPath;
  if (!filePath.endsWith('.t') && !filePath.endsWith('.pl')) {
    vscode.window.showWarningMessage('Run Tests is only available for .t and .pl files');
    return;
  }

  if (!dependencies.testAdapter) {
    vscode.window.showWarningMessage(
      'Test adapter is not available. It might still be initializing.',
    );
    return;
  }

  const statusBarItem = dependencies.statusBarItem;
  const originalText = statusBarItem?.text;
  const originalTooltip = statusBarItem?.tooltip;

  if (statusBarItem) {
    statusBarItem.text = '$(beaker~spin) Running Tests...';
    statusBarItem.tooltip = 'Executing Perl tests in current file';
  }

  try {
    await dependencies.testAdapter.runFileTests(targetUri);
  } finally {
    if (statusBarItem && originalText !== undefined) {
      statusBarItem.text = originalText;
      statusBarItem.tooltip = originalTooltip;
    }
  }
}

/** Run the test code lens that contains the active cursor. */
export async function runTestAtCursorCommand(dependencies: TestCommandDependencies): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'perl') {
    vscode.window.showErrorMessage('Run Test at Cursor requires an active Perl file');
    return;
  }

  if (editor.document.isDirty) {
    await editor.document.save();
  }

  if (!dependencies.activeClient) {
    vscode.window.showWarningMessage(dependencies.serverNotRunningMessage());
    return;
  }

  const lenses = await dependencies.activeClient.sendRequest<TestLens[] | null>(
    'textDocument/codeLens',
    { textDocument: { uri: editor.document.uri.toString() } },
  );
  const command = selectTestCommandAtPosition(lenses ?? [], editor.selection.active);
  if (!command) {
    vscode.window.showWarningMessage('No runnable test was found at the cursor position');
    return;
  }

  await vscode.commands.executeCommand(command.command, ...(command.arguments ?? []));
}

async function runProveTask(name: string, args: string[], cwd?: string): Promise<void> {
  const scope = cwd
    ? (vscode.workspace.getWorkspaceFolder(vscode.Uri.file(cwd)) ?? vscode.TaskScope.Global)
    : vscode.TaskScope.Global;
  const execution = new vscode.ProcessExecution('prove', args, cwd ? { cwd } : undefined);
  const task = new vscode.Task({ type: 'perl-lsp' }, scope, name, 'perl-lsp', execution);
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    clear: false,
    showReuseMessage: false,
  };
  await vscode.tasks.executeTask(task);
}

/** Run prove against the active Perl file. */
export async function runCurrentTestWithProve(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'perl') {
    vscode.window.showErrorMessage('No active Perl file to run');
    return;
  }

  if (editor.document.isDirty) {
    await editor.document.save();
  }

  const filePath = editor.document.uri.fsPath;
  const workspaceFolder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
  await runProveTask('Perl Tests: Current File', ['-v', filePath], workspaceFolder?.uri.fsPath);
}

/** Run prove recursively against the first workspace folder. */
export async function runAllTestsWithProve(): Promise<void> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  const firstFolder = workspaceFolders?.[0];
  if (!firstFolder) {
    vscode.window.showErrorMessage('No workspace folder open');
    return;
  }

  await runProveTask('Perl Tests: All', ['-r', 't/'], firstFolder.uri.fsPath);
}
