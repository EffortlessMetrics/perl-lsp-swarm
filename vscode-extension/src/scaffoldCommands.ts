import * as path from 'path';
import * as vscode from 'vscode';
import { FileKind, scaffoldContent } from './fileCreation';

export type ScaffoldOutcome =
  | 'applied'
  | 'user_cancelled'
  | 'ineligible_target'
  | 'ambiguous_module_root'
  | 'apply_rejected'
  | 'open_failed'
  | 'failed';

/** An explicit user action; never runs in response to ordinary file creation. */
export async function createPerlScaffold(kind: FileKind): Promise<ScaffoldOutcome> {
  const activeUri = vscode.window.activeTextEditor?.document.uri;
  const folder =
    (activeUri && vscode.workspace.getWorkspaceFolder(activeUri)) ??
    vscode.workspace.workspaceFolders?.[0];
  if (!folder || folder.uri.scheme !== 'file') {
    void vscode.window.showWarningMessage(
      'Open a local workspace folder before creating a Perl file.',
    );
    return 'ineligible_target';
  }

  const extension = kind === FileKind.Module ? 'pm' : 't';
  const defaultUri = vscode.Uri.joinPath(
    folder.uri,
    kind === FileKind.Module ? 'lib/NewModule.pm' : 't/new_test.t',
  );
  let uri: vscode.Uri | undefined;
  try {
    uri = await vscode.window.showSaveDialog({
      defaultUri,
      filters: { 'Perl files': [extension] },
      saveLabel: kind === FileKind.Module ? 'Create Module' : 'Create Test',
    });
  } catch {
    void vscode.window.showErrorMessage('Could not choose a Perl file destination.');
    return 'failed';
  }
  if (!uri) return 'user_cancelled';

  const owner = vscode.workspace.getWorkspaceFolder(uri);
  if (
    uri.scheme !== 'file' ||
    !owner ||
    owner.uri.scheme !== 'file' ||
    path.extname(uri.fsPath).toLowerCase() !== `.${extension}`
  ) {
    void vscode.window.showWarningMessage(
      `Choose a .${extension} file inside a local workspace folder.`,
    );
    return 'ineligible_target';
  }

  const content = scaffoldContent(kind, owner.uri.fsPath, uri.fsPath);
  if (!content) {
    void vscode.window.showWarningMessage(
      kind === FileKind.Module
        ? 'Choose a valid module path under this workspace folder’s lib directory.'
        : 'Choose a valid test path inside this workspace folder.',
    );
    return 'ambiguous_module_root';
  }

  const edit = new vscode.WorkspaceEdit();
  edit.createFile(uri, {
    overwrite: false,
    ignoreIfExists: false,
    contents: Buffer.from(content, 'utf8'),
  });
  try {
    if (!(await vscode.workspace.applyEdit(edit))) {
      void vscode.window.showErrorMessage(
        'Could not create the Perl file; the destination may already exist.',
      );
      return 'apply_rejected';
    }
  } catch {
    void vscode.window.showErrorMessage('Could not create the Perl file.');
    return 'failed';
  }
  try {
    await vscode.window.showTextDocument(uri);
    return 'applied';
  } catch {
    void vscode.window.showWarningMessage(
      'The Perl file was created, but the editor could not open it.',
    );
    return 'open_failed';
  }
}
