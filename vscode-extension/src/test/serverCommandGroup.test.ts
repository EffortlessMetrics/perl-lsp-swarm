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
  currentRuntimeSnapshot: jest.Mock;
  runtimeFailureCheck: jest.Mock;
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
      detail: 'Language server failed to start: simulated startup failure',
    })),
    currentRuntimeSnapshot: jest.fn(() => ({
      state: 'failed' as const,
      generation: 4,
      error: new Error('simulated startup failure'),
      serverPath: '/configured/perllsp',
    })),
    runtimeFailureCheck: jest.fn(),
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
      languageServerRuntimeHealth(lifecycle.snapshot, '/failed-start/perllsp'),
    );
    dependencies.currentRuntimeSnapshot.mockImplementation(() => lifecycle.snapshot);
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
          detail: 'Language server failed to start: simulated client.start failure',
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

  test('reprobes setup after a successful retry reaches path B', async () => {
    let nextPath = 0;
    const clients: LifecycleClient[] = [];
    const lifecycle = new LanguageClientLifecycle<LifecycleClient>({
      resolveServerPath: async () => `/server-${String.fromCharCode(65 + nextPath++)}`,
      createClient: () => {
        const client: LifecycleClient = {
          start: jest.fn(async () => undefined),
          stop: jest.fn(async () => undefined),
          dispose: jest.fn(async () => undefined),
          onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
        };
        clients.push(client);
        return client;
      },
    });
    let releaseSetup!: () => void;
    const setupReleased = new Promise<void>((resolve) => {
      releaseSetup = resolve;
    });
    let probeEntered!: () => void;
    const probeStarted = new Promise<void>((resolve) => {
      probeEntered = resolve;
    });
    const dependencies = makeDependencies();
    dependencies.resolveServerPath.mockImplementation(async () => {
      await lifecycle.start();
      return lifecycle.snapshot.serverPath;
    });
    dependencies.runHealthCheck.mockImplementation(async (resolvedPath: string | null) => {
      probeEntered();
      await setupReleased;
      return [
        {
          label: 'LSP binary',
          ok: true,
          status: HealthCheckStatus.Ok,
          detail: `Binary found: ${resolvedPath}`,
        },
      ];
    });
    dependencies.runtimeHealthCheck.mockImplementation((resolvedPath: string | null) =>
      languageServerRuntimeHealth(lifecycle.snapshot, resolvedPath),
    );
    dependencies.currentRuntimeSnapshot.mockImplementation(() => lifecycle.snapshot);
    registerServerCommandGroup(dependencies);

    const command = vscode.commands.executeCommand('perl-lsp.runHealthCheck');
    await probeStarted;
    expect(dependencies.runHealthCheck).toHaveBeenCalledWith('/server-A');
    await lifecycle.restart();
    releaseSetup();
    const result = await command;

    expect(clients).toHaveLength(2);
    expect(dependencies.runHealthCheck).toHaveBeenCalledTimes(2);
    expect(result).toEqual({
      ok: true,
      checks: [
        {
          label: 'LSP binary',
          status: 'ok',
          detail: 'Binary found: /server-B',
        },
        {
          label: 'LSP runtime',
          status: 'ok',
          detail: 'Language server is running.',
        },
      ],
    });
  });

  test('reprobes an old setup error after a healthy same-path replacement', async () => {
    let probeCount = 0;
    const lifecycle = new LanguageClientLifecycle<LifecycleClient>({
      resolveServerPath: async () => '/same-server/perllsp',
      createClient: () => ({
        start: jest.fn(async () => {
          return undefined;
        }),
        stop: jest.fn(async () => undefined),
        dispose: jest.fn(async () => undefined),
        onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
      }),
    });
    let releaseSetup!: () => void;
    const setupReleased = new Promise<void>((resolve) => {
      releaseSetup = resolve;
    });
    let probeEntered!: () => void;
    const probeStarted = new Promise<void>((resolve) => {
      probeEntered = resolve;
    });
    const dependencies = makeDependencies();
    dependencies.resolveServerPath.mockImplementation(async () => {
      await lifecycle.start();
      return lifecycle.snapshot.serverPath;
    });
    dependencies.runHealthCheck.mockImplementation(async () => {
      const probeNumber = ++probeCount;
      probeEntered();
      await setupReleased;
      return [
        {
          label: 'LSP binary',
          ok: probeNumber > 1,
          status: probeNumber > 1 ? HealthCheckStatus.Ok : HealthCheckStatus.Error,
          detail: probeNumber > 1 ? 'Binary found: /same-server/perllsp' : 'stale setup failure',
        },
      ];
    });
    dependencies.runtimeHealthCheck.mockImplementation((resolvedPath: string | null) =>
      languageServerRuntimeHealth(lifecycle.snapshot, resolvedPath),
    );
    dependencies.currentRuntimeSnapshot.mockImplementation(() => lifecycle.snapshot);
    registerServerCommandGroup(dependencies);

    const command = vscode.commands.executeCommand('perl-lsp.runHealthCheck');
    await probeStarted;
    await lifecycle.restart();
    releaseSetup();

    const result = await command;
    expect(dependencies.runHealthCheck).toHaveBeenCalledTimes(2);
    expect(result).toMatchObject({
      ok: true,
      checks: [
        { label: 'LSP binary', status: 'ok', detail: 'Binary found: /same-server/perllsp' },
        { label: 'LSP runtime', status: 'ok', detail: 'Language server is running.' },
      ],
    });
  });

  test('reports cleanup-blocked startup as a runtime error', async () => {
    const lifecycle = new LanguageClientLifecycle<LifecycleClient>({
      resolveServerPath: async () => '/cleanup-blocked/perllsp',
      createClient: () => ({
        start: jest.fn(async () => {
          throw new Error('startup failed');
        }),
        stop: jest.fn(async () => {
          throw new Error('process did not exit');
        }),
        dispose: jest.fn(async () => undefined),
        onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
      }),
    });
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Binary found: /cleanup-blocked/perllsp',
      },
    ]);
    dependencies.resolveServerPath.mockImplementation(async () => {
      await lifecycle.start().catch(() => undefined);
      return lifecycle.snapshot.serverPath;
    });
    dependencies.runtimeHealthCheck.mockImplementation((resolvedPath: string | null) =>
      languageServerRuntimeHealth(lifecycle.snapshot, resolvedPath),
    );
    dependencies.currentRuntimeSnapshot.mockImplementation(() => lifecycle.snapshot);
    registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand('perl-lsp.runHealthCheck');

    const checks = (result as { checks: Array<{ label: string; status: string; detail: string }> })
      .checks;
    expect(result).toMatchObject({ ok: false });
    expect(checks.find((check) => check.label === 'LSP runtime')).toMatchObject({
      status: 'error',
    });
    expect(checks.find((check) => check.label === 'LSP runtime')?.detail).toContain(
      'cleanup is incomplete',
    );
  });

  test('reprobes setup after retry path B fails and reports the current failure', async () => {
    let nextPath = 0;
    const lifecycle = new LanguageClientLifecycle<LifecycleClient>({
      resolveServerPath: async () => `/retry-${String.fromCharCode(65 + nextPath++)}`,
      createClient: () => ({
        start: jest.fn(async () => {
          if (nextPath === 2) {
            throw new Error('retry startup refused');
          }
        }),
        stop: jest.fn(async () => undefined),
        dispose: jest.fn(async () => undefined),
        onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
      }),
    });
    await lifecycle.start();
    let releaseSetup!: () => void;
    const setupReleased = new Promise<void>((resolve) => {
      releaseSetup = resolve;
    });
    let probeEntered!: () => void;
    const probeStarted = new Promise<void>((resolve) => {
      probeEntered = resolve;
    });
    const dependencies = makeDependencies();
    dependencies.resolveServerPath.mockResolvedValue('/retry-A');
    dependencies.runHealthCheck.mockImplementation(async () => {
      probeEntered();
      await setupReleased;
      return [
        {
          label: 'LSP binary',
          ok: true,
          status: HealthCheckStatus.Ok,
          detail: 'Binary found: /retry-A',
        },
      ];
    });
    dependencies.runtimeHealthCheck.mockImplementation((resolvedPath: string | null) =>
      languageServerRuntimeHealth(lifecycle.snapshot, resolvedPath),
    );
    dependencies.currentRuntimeSnapshot.mockImplementation(() => lifecycle.snapshot);
    registerServerCommandGroup(dependencies);

    const command = vscode.commands.executeCommand('perl-lsp.runHealthCheck');
    await probeStarted;
    await expect(lifecycle.restart()).rejects.toThrow('retry startup refused');
    releaseSetup();

    const result = await command;
    expect(dependencies.runHealthCheck).toHaveBeenCalledTimes(2);
    expect(result).toMatchObject({ ok: false });
    expect((result as { checks: Array<{ label: string; detail: string }> }).checks.at(-1)).toEqual({
      label: 'LSP runtime',
      status: 'error',
      detail: 'Language server failed to start: retry startup refused',
    });
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
          detail: 'Language server failed to start: simulated startup failure',
        },
      ],
    });
    expect(dependencies.runtimeHealthCheck).toHaveBeenCalledWith('/configured/perllsp');
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
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      'Perl LSP setup check passed.',
      'Show Output',
    );
  });

  test('reports a known matching startup failure for an explicit diagnostic path', async () => {
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Binary found: /failed-start/perllsp',
      },
    ]);
    dependencies.runtimeFailureCheck.mockReturnValue({
      label: 'LSP runtime',
      ok: false,
      status: HealthCheckStatus.Error,
      detail: 'Language server failed to start: simulated startup failure',
    });
    registerServerCommandGroup(dependencies);

    const result = await vscode.commands.executeCommand(
      'perl-lsp.runHealthCheck',
      '/failed-start/perllsp',
    );

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
          detail: 'Language server failed to start: simulated startup failure',
        },
      ],
    });
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      'Health check failed: LSP runtime',
      'Show Output',
    );
  });

  test('fails closed when implicit runtime evidence is unavailable', async () => {
    const dependencies = makeDependencies([
      {
        label: 'LSP binary',
        ok: true,
        status: HealthCheckStatus.Ok,
        detail: 'Binary found: /configured/perllsp',
      },
    ]);
    const incomplete = { ...dependencies } as ServerCommandContext & {
      runtimeHealthCheck?: ServerCommandContext['runtimeHealthCheck'];
    };
    (
      incomplete as unknown as {
        runtimeHealthCheck?: ServerCommandContext['runtimeHealthCheck'] | undefined;
      }
    ).runtimeHealthCheck = undefined;
    registerServerCommandGroup(incomplete as ServerCommandContext);

    const result = await vscode.commands.executeCommand('perl-lsp.runHealthCheck');

    expect(result).toEqual({
      ok: false,
      checks: [
        {
          label: 'LSP binary',
          status: 'ok',
          detail: 'Binary found: /configured/perllsp',
        },
        {
          label: 'LSP runtime',
          status: 'error',
          detail: 'Runtime health evidence is unavailable.',
        },
      ],
    });
  });
});
