import * as vscode from 'vscode';

export interface NavigationCommandContext {
  readonly openDemoProject: () => Promise<void>;
  readonly organizeImports: () => Promise<void>;
  readonly showVersion: () => Promise<void>;
  readonly showStatusMenu: () => Promise<void>;
}

export function registerNavigationCommandGroup(
  dependencies: NavigationCommandContext,
): vscode.Disposable[] {
  const openDemoProjectCommand = vscode.commands.registerCommand(
    'perl-lsp.openDemoProject',
    async () => {
      await dependencies.openDemoProject();
    },
  );
  const organizeImportsCommand = vscode.commands.registerCommand(
    'perl-lsp.organizeImports',
    async () => {
      await dependencies.organizeImports();
    },
  );
  const showVersionCommand = vscode.commands.registerCommand('perl-lsp.showVersion', async () => {
    await dependencies.showVersion();
  });
  const showStatusMenuCommand = vscode.commands.registerCommand(
    'perl-lsp.showStatusMenu',
    async () => {
      await dependencies.showStatusMenu();
    },
  );

  return [
    openDemoProjectCommand,
    organizeImportsCommand,
    showVersionCommand,
    showStatusMenuCommand,
  ];
}
