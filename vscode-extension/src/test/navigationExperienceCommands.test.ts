import * as vscode from 'vscode';
import { showStatusMenuCommand } from '../navigationCommands';

describe('workspace experience status-menu routes', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    Object.assign(vscode.window, { activeTextEditor: undefined });
    (vscode.window.showQuickPick as jest.Mock).mockResolvedValue(undefined);
  });

  test('makes canonical workspace status and provider explanations discoverable', async () => {
    await showStatusMenuCommand();

    const items = (vscode.window.showQuickPick as jest.Mock).mock.calls[0]?.[0] as Array<{
      label: string;
      command?: string;
    }>;

    expect(items).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          label: expect.stringContaining('Show Workspace Status'),
          command: 'perl-lsp.showWorkspaceStatus',
        }),
        expect.objectContaining({
          label: expect.stringContaining('Explain Provider Result'),
          command: 'perl-lsp.explainProviderDecision',
        }),
      ]),
    );
  });
});
