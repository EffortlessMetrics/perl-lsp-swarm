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

/** Wall-clock budget for `perl -c`, and the single source for the timeout message. */
const CHECK_SYNTAX_TIMEOUT_MS = 10_000;

/**
 * Node attaches spawn metadata (`code`, `killed`) to the `execFile` error, but
 * the injectable seam only promises a bare `Error`. Read the extra fields
 * structurally so classification never depends on the concrete error class.
 */
type ExecFileFailure = Error & {
  code?: string | number | null;
  killed?: boolean;
};

/**
 * Failures that name the environment rather than the document: perl never
 * spawned (ENOENT), or was killed by the wall-clock budget (timeout). Node
 * reports a timeout as a kill (`killed: true`); some platforms surface
 * ETIMEDOUT. Their diagnosis is never derived from the child's output.
 */
function isSpawnEnvironmentFailure(error: Error): boolean {
  const failure: ExecFileFailure = error;
  const code = typeof failure.code === 'string' ? failure.code : undefined;
  return code === 'ENOENT' || code === 'ETIMEDOUT' || failure.killed === true;
}

/**
 * Separate "perl never ran" from "perl ran and rejected the file" (#16574).
 *
 * On ENOENT the child never spawns, so stdout and stderr are both empty and the
 * previous code rendered that as a bare `Syntax error: ` — a false verdict with
 * no diagnosis for a file that was never checked.
 */
function classifyCheckSyntaxFailure(error: Error, output: string): string {
  const failure: ExecFileFailure = error;
  if (isSpawnEnvironmentFailure(error)) {
    if (failure.code === 'ENOENT') {
      return (
        'Perl was not found on PATH. Install Perl (strawberryperl.com, Homebrew, or your ' +
        'package manager) — the language server works without it, but syntax checking does not.'
      );
    }
    return `Perl syntax check exceeded the ${CHECK_SYNTAX_TIMEOUT_MS / 1000}s timeout.`;
  }

  const detail = output.trim();
  // Fall back to the spawn message so an unusual failure still says something.
  return detail.length > 0 ? `Syntax error: ${detail}` : `Syntax error: ${failure.message}`;
}

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
    run('perl', perlArgs, { timeout: CHECK_SYNTAX_TIMEOUT_MS }, (error, stdout, stderr) => {
      const output = (stdout + stderr).trim();
      if (error) {
        const failure = classifyCheckSyntaxFailure(error, output);
        // Never write a bare `[check-syntax] ` line when the child produced no text (#16574).
        // A genuine syntax verdict keeps perl's own text as the detail, but an
        // environment failure keeps its diagnosis even when perl produced
        // partial output before being killed — otherwise the channel hides the
        // real cause behind a truncated fragment.
        const channelDetail =
          output.length > 0 && !isSpawnEnvironmentFailure(error)
            ? output
            : [failure, output].filter((part) => part.length > 0).join('\n');
        vscode.window.showErrorMessage(failure, 'Show Output').then((selection) => {
          if (selection === 'Show Output') {
            dependencies.outputChannel.appendLine(`[check-syntax] ${channelDetail}`);
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

/**
 * Show the local Perl interpreter's @INC paths in a dedicated output channel.
 *
 * What this actually reports: the raw `@INC` of whatever `perl` is first on
 * `PATH` (see the `run('perl', ...)` call below). It is *not* the set of roots
 * `perl-lsp` searches, and it may well be a different interpreter than the one
 * the server resolved. The header says so, because a user who runs this while
 * asking "why can't the LSP find my module?" is precisely the reader for whom
 * an unqualified list is actively misleading — it omits the roots in dispute
 * and reports no caveat (#16581).
 */
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
      // Header first, and it states the scope of the listing before the paths
      // rather than after them: a reader who scrolls past the list has already
      // been misled. Command titles are quoted exactly as `package.nls.json`
      // registers them, so the pointer is clickable-by-name rather than
      // approximate.
      incPathsChannel.appendLine('Perl @INC of the `perl` found on PATH:');
      incPathsChannel.appendLine('');
      for (const line of lines) {
        incPathsChannel.appendLine(`  ${line}`);
      }
      incPathsChannel.appendLine('');
      incPathsChannel.appendLine(
        'This is NOT the set of roots perl-lsp uses for a file. Workspace ' +
          "`includePaths`, PERL5LIB policy, lexical use lib, and the server's " +
          'Perl probe can change module lookup. For workspace configuration and ' +
          'policy, run "Perl: Show Workspace Trust Report". For a specific module ' +
          'lookup, open a Perl file and run "Perl: Explain Missing Module Lookup".',
      );
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

  const uri = editor.document.uri.toString();
  try {
    const result = await dependencies.activeClient.sendRequest<string | null>('perl/showAst', {
      uri,
    });
    if (!result) {
      vscode.window.showInformationMessage(
        'No current AST is available for this file. Try again after parsing completes; check Problems if this persists.',
      );
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
  } catch (error) {
    if (typeof error === 'object' && error !== null && 'code' in error) {
      if (error.code === -32601) {
        vscode.window.showWarningMessage(
          'Show Parser AST is not supported by the current perllsp version',
        );
        return;
      }
      if (
        error.code === -32602 &&
        'message' in error &&
        error.message === `Document not found: ${uri}`
      ) {
        vscode.window.showWarningMessage(
          'The language server has not opened this file yet. Try again shortly, or reload the window if this persists.',
        );
        return;
      }
    }
    vscode.window.showWarningMessage(
      'Could not show Parser AST. Try again or check the Perl Language Server output.',
    );
  }
}
