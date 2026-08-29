import * as vscode from 'vscode';

/**
 * Explicit dependencies for the coexistence explanation command (#7214).
 *
 * The group owns command registration only. Detection, collection, and the
 * advisory flow stay owned by the composition layer that supplies this
 * callback.
 */
export interface CoexistenceCommandContext {
  readonly showCoexistenceStatus: () => Promise<void>;
}

/** Register commands owned by the coexistence group. */
export function registerCoexistenceCommandGroup(
  dependencies: CoexistenceCommandContext,
): vscode.Disposable[] {
  const showCoexistenceStatusCommand = vscode.commands.registerCommand(
    'perl-lsp.showCoexistenceStatus',
    dependencies.showCoexistenceStatus,
  );

  return [showCoexistenceStatusCommand];
}
