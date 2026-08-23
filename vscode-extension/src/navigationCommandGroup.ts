import * as vscode from 'vscode';

export interface NavigationCommandContext {
  readonly openDemoProject: () => Promise<void>;
  readonly showVersion: () => Promise<void>;
  readonly showWorkspaceStatus: () => Promise<void>;
  readonly showStatusMenu: () => Promise<void>;
}

export function registerNavigationCommandGroup(
  dependencies: NavigationCommandContext,
): vscode.Disposable[] {
  const openDemoProjectCommand = vscode.commands.registerCommand(
    'perl-lsp.openDemoProject',
    dependencies.openDemoProject,
  );
  const showVersionCommand = vscode.commands.registerCommand(
    'perl-lsp.showVersion',
    dependencies.showVersion,
  );
  const showStatusMenuCommand = vscode.commands.registerCommand(
    'perl-lsp.showStatusMenu',
    dependencies.showStatusMenu,
  );
  const showWorkspaceStatusCommand = vscode.commands.registerCommand(
    'perl-lsp.showWorkspaceStatus',
    dependencies.showWorkspaceStatus,
  );

  return [
    openDemoProjectCommand,
    showVersionCommand,
    showStatusMenuCommand,
    showWorkspaceStatusCommand,
  ];
}
