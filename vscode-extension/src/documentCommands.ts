import * as path from 'path';
import { execFile } from 'child_process';
import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { machineScopedExternalIncludePaths } from './languageClientConfiguration';
import { isPerlLanguageId } from './languageIdentity';

type DocumentClient = Pick<LanguageClient, 'sendRequest'>;
type DocumentOutputChannel = Pick<vscode.OutputChannel, 'appendLine' | 'show'>;
export type ExecFileLike = (
  file: string,
  args: string[],
  options: { timeout: number },
  callback: (error: Error | null, stdout: string, stderr: string) => void,
) => void;

export interface DocumentCommandDependencies {
  readonly activeClient?: DocumentClient | undefined;
  readonly outputChannel: DocumentOutputChannel;
  readonly serverNotRunningMessage: () => string;
  readonly execFile?: ExecFileLike | undefined;
}

// Cached output channels to avoid creating a new one per invocation. (UX polish)
let incPathsChannel: vscode.OutputChannel | undefined;
let parserAstChannel: vscode.OutputChannel | undefined;

/** Check the active Perl document with the local Perl interpreter. */
export async function runCheckSyntaxCommand(
  dependencies: DocumentCommandDependencies,
): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || !isPerlLanguageId(editor.document.languageId)) {
    vscode.window.showErrorMessage('No active Perl file to check syntax');
    return;
  }

  if (editor.document.isDirty) {
    await editor.document.save();
  }

  const filePath = editor.document.uri.fsPath;
  const config = vscode.workspace.getConfiguration('perl-lsp');
  const includePaths: string[] = config.get('includePaths', ['lib', 'local/lib/perl5']);
  // Machine scope only — never honor workspace/folder externalIncludePaths (#4998).
  const externalIncludePaths = machineScopedExternalIncludePaths(config);
  const workspaceRoot = vscode.workspace.getWorkspaceFolder(editor.document.uri)?.uri.fsPath;
  const perlArgs: string[] = [];
  for (const includePath of [...includePaths, ...externalIncludePaths]) {
    const resolved =
      workspaceRoot && !path.isAbsolute(includePath)
        ? path.join(workspaceRoot, includePath)
        : includePath;
    perlArgs.push('-I', resolved);
  }
  perlArgs.push('-c', filePath);

  const run = dependencies.execFile ?? execFile;
  await new Promise<void>((resolve) => {
    run('perl', perlArgs, { timeout: 10_000 }, (error, stdout, stderr) => {
      const output = (stdout + stderr).trim();
      if (error) {
        vscode.window
          .showErrorMessage(`Syntax error: ${output}`, 'Show Output')
          .then((selection) => {
            if (selection === 'Show Output') {
              dependencies.outputChannel.appendLine(`[check-syntax] ${output}`);
              dependencies.outputChannel.show();
            }
            resolve();
          });
        return;
      }

      vscode.window.showInformationMessage(`Syntax OK: ${path.basename(filePath)}`).then(() => {
        resolve();
      });
    });
  });
}

/** Invoke the editor's native document formatter for the active Perl file. */
export async function formatDocumentCommand(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || !isPerlLanguageId(editor.document.languageId)) {
    vscode.window.showErrorMessage('No active Perl file to format');
    return;
  }

  await vscode.commands.executeCommand('editor.action.formatDocument');
}

/** Show the local Perl interpreter's @INC paths in a dedicated output channel. */
export async function showIncPathsCommand(execFileOverride?: ExecFileLike): Promise<void> {
  const run = execFileOverride ?? execFile;
  await new Promise<void>((resolve) => {
    run('perl', ['-e', 'print join("\\n", @INC)'], { timeout: 5000 }, (error, stdout) => {
      if (error) {
        vscode.window
          .showErrorMessage(
            `Could not read Perl @INC paths: ${error.message}. ` +
              `Make sure 'perl' is installed and on your PATH, or set perl-lsp.includePaths in settings.`,
          )
          .then(() => {
            resolve();
          });
        return;
      }

      const lines = stdout
        .trim()
        .split('\n')
        .filter((line) => line.length > 0);
      // Reuse the channel instead of creating a new one each invocation. (UX polish)
      if (!incPathsChannel) {
        incPathsChannel = vscode.window.createOutputChannel('Perl @INC');
      }
      incPathsChannel.clear();
      incPathsChannel.appendLine('Perl @INC paths:');
      incPathsChannel.appendLine('');
      for (const line of lines) {
        incPathsChannel.appendLine(`  ${line}`);
      }
      incPathsChannel.show();
      resolve();
    });
  });
}

/** Search workspace modules and open the selected Perl module. */
export async function openPerlModuleCommand(): Promise<void> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders || workspaceFolders.length === 0) {
    vscode.window.showErrorMessage('No workspace folder open');
    return;
  }

  const moduleFiles = await vscode.workspace.findFiles(
    '**/*.pm',
    '{**/node_modules/**,**/blib/**}',
    500,
  );
  if (moduleFiles.length === 0) {
    vscode.window.showInformationMessage('No .pm module files found in workspace');
    return;
  }

  const items = moduleFiles
    .map((uri) => {
      const relativePath = vscode.workspace.asRelativePath(uri).replaceAll('\\', '/');
      const moduleName = relativePath
        .replace(/^(lib|local\/lib\/perl5)\//, '')
        .replace(/\.pm$/, '')
        .replace(/\//g, '::');
      return { label: moduleName, description: relativePath, uri };
    })
    .sort((left, right) => left.label.localeCompare(right.label));

  const selected = await vscode.window.showQuickPick(items, {
    placeHolder: 'Search Perl modules...',
    matchOnDescription: true,
  });
  if (selected) {
    const document = await vscode.workspace.openTextDocument(selected.uri);
    await vscode.window.showTextDocument(document);
  }
}

/** Request and display the parser AST for the active Perl document. */
export async function showParserAstCommand(
  dependencies: DocumentCommandDependencies,
): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || !isPerlLanguageId(editor.document.languageId)) {
    vscode.window.showErrorMessage('No active Perl file to show AST');
    return;
  }

  if (!dependencies.activeClient) {
    vscode.window.showWarningMessage(dependencies.serverNotRunningMessage());
    return;
  }

  try {
    const result = await dependencies.activeClient.sendRequest<string | null>('perl/showAst', {
      uri: editor.document.uri.toString(),
    });
    if (!result) {
      vscode.window.showInformationMessage('No AST available for this file');
      return;
    }

    // Reuse the channel instead of creating a new one each invocation. (UX polish)
    if (!parserAstChannel) {
      parserAstChannel = vscode.window.createOutputChannel('Perl Parser AST');
    }
    parserAstChannel.clear();
    parserAstChannel.appendLine(`AST for: ${vscode.workspace.asRelativePath(editor.document.uri)}`);
    parserAstChannel.appendLine('');
    parserAstChannel.appendLine(result);
    parserAstChannel.show();
  } catch {
    vscode.window.showWarningMessage(
      'Show Parser AST is not supported by the current perllsp version',
    );
  }
}
