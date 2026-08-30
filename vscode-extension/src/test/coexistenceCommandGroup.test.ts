import * as vscode from 'vscode';
import { registerCoexistenceCommandGroup } from '../coexistenceCommandGroup';

describe('coexistence command group (#7214)', () => {
  test('registers the status command and dispatches to its dependency', async () => {
    const showCoexistenceStatus = jest.fn(async () => undefined);
    const disposables = registerCoexistenceCommandGroup({ showCoexistenceStatus });
    // An empty registration would leak nothing at dispose time but also
    // register nothing; the returned collection is what activation pushes
    // into context.subscriptions, so its shape is part of the contract.
    expect(disposables).toHaveLength(1);

    await vscode.commands.executeCommand('perl-lsp.showCoexistenceStatus');
    expect(showCoexistenceStatus).toHaveBeenCalledTimes(1);

    for (const disposable of disposables) {
      disposable.dispose();
    }
    await vscode.commands.executeCommand('perl-lsp.showCoexistenceStatus');
    expect(showCoexistenceStatus).toHaveBeenCalledTimes(1);
  });
});
