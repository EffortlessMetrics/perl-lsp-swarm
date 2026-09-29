import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { isPerlLanguageId } from './languageIdentity';

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
  readonly disabled?: unknown;
};

type CodeActionResult = RefactoringAction[] | null;

const VARIABLE_KIND = 'refactor.extract.variable';
const SUBROUTINE_KIND = 'refactor.extract.subroutine';

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function requestedAction(actions: CodeActionResult, kind: string): RefactoringAction | undefined {
  if (!Array.isArray(actions)) {
    return undefined;
  }
  return actions.find((action) => {
    if (!isRecord(action) || action.kind !== kind || 'disabled' in action) {
      return false;
    }
    const edit = action.edit;
    const command = action.command;
    const validEdit =
      isRecord(edit) &&
      ((isRecord(edit.changes) && Object.values(edit.changes).every(Array.isArray)) ||
        Array.isArray(edit.documentChanges));
    const validCommand =
      isRecord(command) &&
      typeof command.command === 'string' &&
      command.command.length > 0 &&
      (!('arguments' in command) || Array.isArray(command.arguments));
    return (
      (edit === undefined || validEdit) &&
      (command === undefined || validCommand) &&
      (validEdit || validCommand)
    );
  });
}

export interface RefactoringCommandDependencies {
  readonly activeClient?: RefactoringClient | undefined;
  readonly serverNotRunningMessage: () => string;
}

function selectionParams(editor: vscode.TextEditor, kind: string) {
  const range = editor.selection;
  return {
    textDocument: { uri: editor.document.uri.toString() },
    range: {
      start: { line: range.start.line, character: range.start.character },
      end: { line: range.end.line, character: range.end.character },
    },
    context: { diagnostics: [], only: [kind], triggerKind: 2 },
  };
}

async function applyAction(
  client: RefactoringClient,
  action: RefactoringAction,
  unavailableMessage: string,
): Promise<void> {
  if (action.edit) {
    try {
      const workspaceEdit = await client.protocol2CodeConverter.asWorkspaceEdit(
        action.edit as Parameters<typeof client.protocol2CodeConverter.asWorkspaceEdit>[0],
      );
      if (!workspaceEdit || !(await vscode.workspace.applyEdit(workspaceEdit))) {
        vscode.window.showInformationMessage(unavailableMessage);
        return;
      }
    } catch {
      vscode.window.showInformationMessage(unavailableMessage);
      return;
    }
  }

  if (action.command) {
    const command = action.command as { command: string; arguments?: unknown[] };
    await vscode.commands.executeCommand(command.command, ...(command.arguments ?? []));
    return;
  }

  if (!action.edit) {
    vscode.window.showInformationMessage(unavailableMessage);
  }
}

/** Request and apply the server's extract-variable code action. */
export async function extractVariableCommand(
  dependencies: RefactoringCommandDependencies,
): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || !isPerlLanguageId(editor.document.languageId)) {
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

  let actions: CodeActionResult;
  try {
    actions = await client.sendRequest<CodeActionResult>(
      'textDocument/codeAction',
      selectionParams(editor, VARIABLE_KIND),
    );
  } catch (error) {
    vscode.window.showErrorMessage(`Extract Variable request failed: ${String(error)}`);
    return;
  }
  if (!actions || actions.length === 0) {
    vscode.window.showInformationMessage(
      'No extract actions available for the selected expression',
    );
    return;
  }

  const action = requestedAction(actions, VARIABLE_KIND);
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
  if (!editor || !isPerlLanguageId(editor.document.languageId)) {
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

  let actions: CodeActionResult;
  try {
    actions = await client.sendRequest<CodeActionResult>(
      'textDocument/codeAction',
      selectionParams(editor, SUBROUTINE_KIND),
    );
  } catch (error) {
    vscode.window.showErrorMessage(`Extract Method request failed: ${String(error)}`);
    return;
  }
  if (!actions || actions.length === 0) {
    vscode.window.showInformationMessage('No extract actions available for the selected code');
    return;
  }

  const action = requestedAction(actions, SUBROUTINE_KIND);
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
  if (!editor || !isPerlLanguageId(editor.document.languageId)) {
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
