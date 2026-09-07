import * as vscode from 'vscode';
import {
  registerWorkspaceConfigurationEvents,
  type WorkspaceEventHandlers,
} from '../extensionWorkspaceEvents';

describe('workspace configuration event routing', () => {
  test('dispatches each configuration class once', () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const handlers: WorkspaceEventHandlers = {
      onLiveConfigurationChanged: jest.fn(),
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
    };
    const disposable = registerWorkspaceConfigurationEvents(handlers);

    listener?.({
      affectsConfiguration: (setting: string) =>
        new Set([
          'perl-lsp.includePaths',
          'perl-lsp.aiCompletion.enabled',
          'perl-lsp.featureProfile',
        ]).has(setting),
    } as vscode.ConfigurationChangeEvent);

    expect(handlers.onLiveConfigurationChanged).toHaveBeenCalledTimes(1);
    expect(handlers.onReconstructConfigurationChanged).toHaveBeenCalledTimes(1);
    expect(handlers.onRestartRequired).toHaveBeenCalledTimes(1);
    expect(disposable).toBeDefined();
  });

  test('routes unrelated changes nowhere', () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const handlers: WorkspaceEventHandlers = {
      onLiveConfigurationChanged: jest.fn(),
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
    };
    registerWorkspaceConfigurationEvents(handlers);

    listener?.({
      affectsConfiguration: (setting: string) => setting === 'perl-lsp.autoPopulateNewFiles',
    } as vscode.ConfigurationChangeEvent);

    expect(handlers.onLiveConfigurationChanged).not.toHaveBeenCalled();
    expect(handlers.onReconstructConfigurationChanged).not.toHaveBeenCalled();
    expect(handlers.onRestartRequired).not.toHaveBeenCalled();
  });

  test('reports synchronous handler failures without escaping the event listener', async () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const onError = jest.fn();
    registerWorkspaceConfigurationEvents({
      onLiveConfigurationChanged: () => {
        throw new Error('sync handler failed');
      },
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
      onError,
    });

    listener?.({
      affectsConfiguration: (setting: string) => setting === 'perl-lsp.includePaths',
    } as vscode.ConfigurationChangeEvent);

    expect(onError).toHaveBeenCalledWith(expect.any(Error));
  });

  test('uses stderr when no error handler is supplied', () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });
    const stderrWrite = jest.spyOn(process.stderr, 'write').mockImplementation(() => true);

    registerWorkspaceConfigurationEvents({
      onLiveConfigurationChanged: () => {
        throw new Error('unreported handler failed');
      },
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
    });

    listener?.({
      affectsConfiguration: (setting: string) => setting === 'perl-lsp.includePaths',
    } as vscode.ConfigurationChangeEvent);

    expect(stderrWrite).toHaveBeenCalledWith(
      expect.stringContaining(
        '[workspace-configuration] handler failed: Error: unreported handler failed',
      ),
    );
    stderrWrite.mockRestore();
  });

  test('reports rejected async handlers', async () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const onError = jest.fn();
    registerWorkspaceConfigurationEvents({
      onLiveConfigurationChanged: async () => {
        throw new Error('async handler failed');
      },
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
      onError,
    });

    listener?.({
      affectsConfiguration: (setting: string) => setting === 'perl-lsp.includePaths',
    } as vscode.ConfigurationChangeEvent);
    await Promise.resolve();

    expect(onError).toHaveBeenCalledWith(expect.any(Error));
  });

  // The whole reason the hook exists (#14966): a removed setting drives no subsystem, so
  // `classifyConfigurationChange` puts it in no class and none of the three classified
  // handlers ever fires for it. Without an unclassified observer the legacy-setting reader
  // would never be re-read.
  test('delivers a change that classifies as nothing to the unclassified observer', () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const handlers: WorkspaceEventHandlers = {
      onAnyConfigurationChanged: jest.fn(),
      onLiveConfigurationChanged: jest.fn(),
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
    };
    registerWorkspaceConfigurationEvents(handlers);

    listener?.({
      affectsConfiguration: (setting: string) => setting === 'perl-lsp.mcp.servers',
    } as vscode.ConfigurationChangeEvent);

    expect(handlers.onAnyConfigurationChanged).toHaveBeenCalledTimes(1);
    expect(handlers.onLiveConfigurationChanged).not.toHaveBeenCalled();
    expect(handlers.onReconstructConfigurationChanged).not.toHaveBeenCalled();
    expect(handlers.onRestartRequired).not.toHaveBeenCalled();
  });

  // The observer runs before classification, so a handler that reads state the
  // classified handlers then mutate sees the pre-change value. Recording the call order
  // fails if the hook is moved after the classification block rather than removed.
  test('delivers to the unclassified observer before any classified handler', () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const calls: string[] = [];
    registerWorkspaceConfigurationEvents({
      onAnyConfigurationChanged: () => {
        calls.push('any');
      },
      onLiveConfigurationChanged: () => {
        calls.push('live');
      },
      onReconstructConfigurationChanged: () => {
        calls.push('reconstruct');
      },
      onRestartRequired: () => {
        calls.push('restart');
      },
    });

    listener?.({
      affectsConfiguration: (setting: string) =>
        new Set(['perl-lsp.includePaths', 'perl-lsp.featureProfile']).has(setting),
    } as vscode.ConfigurationChangeEvent);

    expect(calls).toEqual(['any', 'live', 'restart']);
  });

  test('a throwing unclassified observer cannot stop the classified handlers', () => {
    let listener: ((event: vscode.ConfigurationChangeEvent) => void) | undefined;
    (vscode.workspace.onDidChangeConfiguration as jest.Mock).mockImplementation((callback) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    const onError = jest.fn();
    const handlers: WorkspaceEventHandlers = {
      onAnyConfigurationChanged: () => {
        throw new Error('observer failed');
      },
      onLiveConfigurationChanged: jest.fn(),
      onReconstructConfigurationChanged: jest.fn(),
      onRestartRequired: jest.fn(),
      onError,
    };
    registerWorkspaceConfigurationEvents(handlers);

    listener?.({
      affectsConfiguration: (setting: string) => setting === 'perl-lsp.includePaths',
    } as vscode.ConfigurationChangeEvent);

    expect(onError).toHaveBeenCalledWith(expect.any(Error));
    expect(handlers.onLiveConfigurationChanged).toHaveBeenCalledTimes(1);
  });
});
