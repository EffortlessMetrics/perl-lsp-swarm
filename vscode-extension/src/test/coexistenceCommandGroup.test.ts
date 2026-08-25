import * as vscode from 'vscode';
import { registerCoexistenceCommandGroup } from '../coexistenceCommandGroup';

describe('coexistence command group (#7214)', () => {
  test('registers the status command and dispatches to its dependency', async () => {
    const showCoexistenceStatus = jest.fn(async () => undefined);
    registerCoexistenceCommandGroup({ showCoexistenceStatus });

    await vscode.commands.executeCommand('perl-lsp.showCoexistenceStatus');
    expect(showCoexistenceStatus).toHaveBeenCalledTimes(1);
  });
});
