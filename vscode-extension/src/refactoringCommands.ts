import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';

type RefactoringClient = Pick<LanguageClient, 'sendRequest'> & {
  readonly protocol2CodeConverter: Pick<
    LanguageClient['protocol2CodeConverter'],
    'asWorkspaceEdit'
  >;
};

type RefactoringAction = {
  readonly title: string;
  readonly kind?: string;
  readonly edit?: unknown;
  readonly command?: unknown;
};

type CodeActionResult = RefactoringAction[] | null;

export interface RefactoringCommandDependencies {
  readonly activeClient?: RefactoringClient | undefined;
  readonly serverNotRunningMessage: () => string;
}

function selectionParams(editor: vscode.TextEditor) {
  const range = editor.selection;
  return {
    textDocument: { uri: editor.document.uri.toString() },
    range: {
      start: { line: range.start.line, character: range.start.character },
      end: { line: range.end.line, character: range.end.character },
    },
    context: { diagnostics: [], only: ['refactor.extract'], triggerKind: 2 },
  };
}

async function applyAction(
  client: RefactoringClient,
  action: RefactoringAction,
  unavailableMessage: string,
): Promise<void> {
  if (action.edit) {
    const workspaceEdit = await client.protocol2CodeConverter.asWorkspaceEdit(
      action.edit as Parameters<typeof client.protocol2CodeConverter.asWorkspaceEdit>[0],
    );
    if (workspaceEdit) {
      await vscode.workspace.applyEdit(workspaceEdit);
    }
    return;
  }

  if (action.command) {
    const command = action.command as { command: string; arguments?: unknown[] };
    await vscode.commands.executeCommand(command.command, ...(command.arguments ?? []));
    return;
  }

  vscode.window.showInformationMessage(unavailableMessage);
}

/** Request and apply the server's extract-variable code action. */
export async function extractVariableCommand(
  dependencies: RefactoringCommandDependencies,
): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'perl') {
    vscode.window.showErrorMessage(
      'Extract Variable requires an active Perl file with a selection',
    );
    return;
  }
  if (editor.selection.isEmpty) {
    vscode.window.showWarningMessage('Select an expression to extract as a variable');
    return;
  }
  const client = dependencies.activeClient;
  if (!client) {
    vscode.window.showWarningMessage(dependencies.serverNotRunningMessage());
    return;
  }

  const actions = await client.sendRequest<CodeActionResult>(
    'textDocument/codeAction',
    selectionParams(editor),
  );
  if (!actions || actions.length === 0) {
    vscode.window.showInformationMessage(
      'No extract actions available for the selected expression',
    );
    return;
  }

  const action =
    actions.find((candidate) => candidate.title.toLowerCase().includes('variable')) ?? actions[0];
  if (!action) {
    vscode.window.showInformationMessage(
      'No extract variable action is available for the current selection',
    );
    return;
  }

  await applyAction(
    client,
    action,
    'No extract variable action is available for the current selection',
  );
}

/** Request and apply the server's extract-method code action. */
export async function extractMethodCommand(
  dependencies: RefactoringCommandDependencies,
): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'perl') {
    vscode.window.showErrorMessage('Extract Method requires an active Perl file with a selection');
    return;
  }
  if (editor.selection.isEmpty) {
    vscode.window.showWarningMessage('Select code to extract as a method');
    return;
  }
  const client = dependencies.activeClient;
  if (!client) {
    vscode.window.showWarningMessage(dependencies.serverNotRunningMessage());
    return;
  }

  const actions = await client.sendRequest<CodeActionResult>(
    'textDocument/codeAction',
    selectionParams(editor),
  );
  if (!actions || actions.length === 0) {
    vscode.window.showInformationMessage('No extract actions available for the selected code');
    return;
  }

  const action =
    actions.find((candidate) => {
      const title = candidate.title.toLowerCase();
      return title.includes('subroutine') || title.includes('method') || title.includes('function');
    }) ?? actions.at(-1);
  if (!action) {
    vscode.window.showInformationMessage(
      'No extract method action is available for the current selection',
    );
    return;
  }

  await applyAction(
    client,
    action,
    'No extract method action is available for the current selection',
  );
}

/** Show the refactoring commands available for the active Perl document. */
export async function showRefactoringOptionsCommand(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'perl') {
    vscode.window.showErrorMessage('Refactoring options require an active Perl file');
    return;
  }

  const items: Array<vscode.QuickPickItem & { command: string; args?: unknown[] }> = [
    {
      label: '$(symbol-variable) Extract Variable',
      description: 'Shift+Alt+V',
      detail: editor.selection.isEmpty
        ? 'Select an expression first to extract it as a variable'
        : 'Extract selected expression as a local variable',
      command: 'perl-lsp.extractVariable',
    },
    {
      label: '$(symbol-method) Extract Method',
      description: 'Shift+Alt+M',
      detail: editor.selection.isEmpty
        ? 'Select code first to extract it as a subroutine'
        : 'Extract selected code as a named subroutine',
      command: 'perl-lsp.extractMethod',
    },
  ];

  const selection = await vscode.window.showQuickPick(items, {
    placeHolder: 'Perl Refactoring Options',
  });
  if (selection) {
    await vscode.commands.executeCommand(selection.command, ...(selection.args ?? []));
  }
}
