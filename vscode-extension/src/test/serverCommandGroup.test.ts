import * as vscode from 'vscode';
import { registerServerCommandGroup, type ServerCommandContext } from '../serverCommandGroup';
import { HealthCheckStatus, type HealthCheckResult } from '../onboarding';
import { LanguageClientLifecycle, type LifecycleClient } from '../languageClientLifecycle';
import { languageServerRuntimeHealth } from '../languageServerRuntimeHealth';

const outputChannel = {
  appendLine: jest.fn(),
  show: jest.fn(),
} as unknown as vscode.LogOutputChannel;

function makeDependencies(results: HealthCheckResult[] = []): ServerCommandContext & {
  currentServerPath: jest.Mock<string | null, []>;
  resolveServerPath: jest.Mock<Promise<string | null>, []>;
  reinstallServerBinary: jest.Mock;
  restartServer: jest.Mock;
  runHealthCheck: jest.Mock;
  runtimeHealthCheck: jest.Mock;
  showBinaryIdentity: jest.Mock;
} {
  return {
    outputChannel,
    currentServerPath: jest.fn(() => '/configured/perllsp'),
    resolveServerPath: jest.fn(async () => '/configured/perllsp'),
    reinstallServerBinary: jest.fn(async () => ({
      ok: true,
      serverPath: '/installed/perllsp',
      target: 'x86_64-unknown-linux-gnu',
      source: 'existing' as const,
    })),
    restartServer: jest.fn(async () => undefined),
    runHealthCheck: jest.fn(async () => results),
    runtimeHealthCheck: jest.fn(() => ({
      label: 'LSP runtime',
      ok: false,
      status: HealthCheckStatus.Error,
      detail: 'Language server failed to start (generation 4): simulated startup failure',
    })),
    showBinaryIdentity: jest.fn(async () => ({ state: 'ready_exact' })),
  };
}

beforeEach(() => {
  jest.clearAllMocks();
});

describe('registerServerCommandGroup', () => {
  test('registered health check reports a failed real lifecycle generation', async () => {
    const startupFailure = new Error('simulated client.start failure');
    const client: LifecycleClient = {
      start: jest.fn(async () => {
        throw startupFailure;
      }),
      stop: jest.fn(async () => undefined),
      dispose: jest.fn(async () => undefined),
      onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
    };
    const lifecycle = new LanguageClientLifecycle<LifecycleClient>({
      resolveServerPath: async () => '/failed-start/perllsp',
      createClient: () => client,
    });
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Binary found: /failed-start/perllsp',
      },
    ]);
    dependencies.resolveServerPath.mockImplementation(async () => {
      await lifecycle.start().catch(() => undefined);
      return lifecycle.snapshot.serverPath;
    });
    dependencies.runtimeHealthCheck.mockImplementation(() =>
      languageServerRuntimeHealth(lifecycle.snapshot),
    );
    registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand('perl-lsp.runHealthCheck');

    expect(result).toEqual({
      ok: false,
      checks: [
        {
          label: 'LSP binary',
          status: 'ok',
          detail: 'Binary found: /failed-start/perllsp',
        },
        {
          label: 'LSP runtime',
          status: 'error',
          detail: 'Language server failed to start (generation 1): simulated client.start failure',
        },
      ],
    });
    expect(client.start).toHaveBeenCalledTimes(1);
    expect(lifecycle.snapshot.state).toBe('failed');
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Health check failed: LSP runtime',
      'Show Output',
    );
  });

  test('registers server commands and delegates without owning lifecycle state', async () => {
    const dependencies = makeDependencies();
    const disposables = registerServerCommandGroup(dependencies);

    expect(disposables).toHaveLength(5);
    await vscode.commands.executeCommand('perl-lsp.showOutput');
    await vscode.commands.executeCommand('perl-lsp.reinstall');
    await vscode.commands.executeCommand('perl-lsp.restart');
    await vscode.commands.executeCommand('perl-lsp.showBinaryIdentity');

    expect(outputChannel.show).toHaveBeenCalledTimes(1);
    expect(dependencies.reinstallServerBinary).toHaveBeenCalledTimes(1);
    expect(dependencies.restartServer).toHaveBeenCalledTimes(1);
    expect(dependencies.showBinaryIdentity).toHaveBeenCalledTimes(1);
  });

  test('binary identity command has an honest unsupported result before composition', async () => {
    const { showBinaryIdentity, ...dependencies } = makeDependencies();
    void showBinaryIdentity;
    const disposables = registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand('perl-lsp.showBinaryIdentity');

    expect(result).toEqual({ status: 'unsupported' });
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Binary identity is unavailable until the running server negotiates the identity feature.',
    );
    for (const disposable of disposables) {
      disposable.dispose();
    }
  });

  test('resolves the managed path by default and returns structured health results', async () => {
    const dependencies = makeDependencies([
      {
        label: 'Perl interpreter',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Perl 5.40',
      },
      {
        label: 'LSP binary',
        ok: false,
        status: HealthCheckStatus.Warning,
        detail: 'using a configured fallback',
      },
    ]);
    registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand('perl-lsp.runHealthCheck');

    expect(dependencies.resolveServerPath).toHaveBeenCalledTimes(1);
    expect(dependencies.currentServerPath).not.toHaveBeenCalled();
    expect(dependencies.runHealthCheck).toHaveBeenCalledWith('/configured/perllsp');
    expect(result).toEqual({
      ok: false,
      checks: [
        { label: 'Perl interpreter', status: 'ok', detail: 'Perl 5.40' },
        { label: 'LSP binary', status: 'warning', detail: 'using a configured fallback' },
        {
          label: 'LSP runtime',
          status: 'error',
          detail: 'Language server failed to start (generation 4): simulated startup failure',
        },
      ],
    });
    expect(dependencies.runtimeHealthCheck).toHaveBeenCalledTimes(1);
    expect(outputChannel.appendLine).toHaveBeenCalledWith('[health-check] Results:');
  });

  test('preserves an explicit null path and reports health errors', async () => {
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: false,
        status: HealthCheckStatus.Error,
        detail: 'missing',
      },
    ]);
    registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand('perl-lsp.runHealthCheck', null);

    expect(dependencies.resolveServerPath).not.toHaveBeenCalled();
    expect(dependencies.currentServerPath).not.toHaveBeenCalled();
    expect(dependencies.runHealthCheck).toHaveBeenCalledWith(null);
    expect(dependencies.runtimeHealthCheck).not.toHaveBeenCalled();
    expect(result).toEqual({
      ok: false,
      checks: [{ label: 'LSP binary', status: 'error', detail: 'missing' }],
    });
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Health check failed: LSP binary',
      'Show Output',
    );
  });

  test('resolves the managed path before a first-run health check', async () => {
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Binary found: /managed/perllsp',
      },
    ]);
    dependencies.currentServerPath.mockReturnValue(null);
    dependencies.resolveServerPath.mockResolvedValue('/managed/perllsp');
    registerServerCommandGroup(dependencies);

    await vscode.commands.executeCommand('perl-lsp.runHealthCheck');

    expect(dependencies.resolveServerPath).toHaveBeenCalledTimes(1);
    expect(dependencies.currentServerPath).not.toHaveBeenCalled();
    expect(dependencies.runHealthCheck).toHaveBeenCalledWith('/managed/perllsp');
  });

  test('keeps explicit-path diagnostics setup-only', async () => {
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Binary found: /failed-start/perllsp',
      },
    ]);
    registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand(
      'perl-lsp.runHealthCheck',
      '/failed-start/perllsp',
    );

    expect(result).toEqual({
      ok: true,
      checks: [
        {
          label: 'LSP binary',
          status: 'ok',
          detail: 'Binary found: /failed-start/perllsp',
        },
      ],
    });
    expect(dependencies.runtimeHealthCheck).not.toHaveBeenCalled();
  });
});
