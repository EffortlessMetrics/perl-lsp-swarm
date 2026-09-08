import * as vscode from 'vscode';
import { probeServerVersion, type ServerVersionProbeBinding } from '../serverVersionProbe';
import { registerSupportCommandGroup } from '../supportCommandGroup';

type VersionCallback = (error: Error | null, stdout: string) => void;

function deferredExecutor() {
  let callback: VersionCallback | undefined;
  const execute = jest.fn(
    (_path: string, _args: string[], _options: { timeout: number }, next: VersionCallback) => {
      callback = next;
    },
  );
  return { execute, callback: () => callback };
}

describe('registered Report Issue server-version probe', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  test.each([
    ['the lifecycle path changes', { serverPath: '/new/perllsp', generation: 4 }],
    [
      'the lifecycle generation changes for the same path',
      { serverPath: '/old/perllsp', generation: 5 },
    ],
    ['the lifecycle clears its path during cleanup', { serverPath: null, generation: 5 }],
  ])('discards a version result when %s', async (_description, nextBinding) => {
    let binding: ServerVersionProbeBinding = { serverPath: '/old/perllsp', generation: 4 };
    const deferred = deferredExecutor();
    let observed: string | undefined;
    const disposables = registerSupportCommandGroup({
      reportIssue: async () => {
        observed = await probeServerVersion(() => binding, deferred.execute);
      },
    });

    const command = vscode.commands.executeCommand('perl-lsp.reportIssue');
    await Promise.resolve();
    expect(deferred.execute).toHaveBeenCalledWith(
      '/old/perllsp',
      ['--version'],
      { timeout: 3000 },
      expect.any(Function),
    );

    binding = nextBinding;
    deferred.callback()?.(null, 'perllsp 0.17.0\n');
    await command;

    expect(observed).toBe('unavailable');
    disposables.forEach((disposable) => disposable.dispose());
  });

  test('keeps a successful result for the captured lifecycle identity', async () => {
    const binding = { serverPath: '/current/perllsp', generation: 7 };
    const deferred = deferredExecutor();
    let observed: string | undefined;
    const disposables = registerSupportCommandGroup({
      reportIssue: async () => {
        observed = await probeServerVersion(() => binding, deferred.execute);
      },
    });

    const command = vscode.commands.executeCommand('perl-lsp.reportIssue');
    await Promise.resolve();
    deferred.callback()?.(null, 'perllsp 0.17.0\nperllsp extra\n');
    await command;

    expect(observed).toBe('perllsp 0.17.0');
    disposables.forEach((disposable) => disposable.dispose());
  });

  test.each([
    ['an execution error', new Error('version failed'), 'perllsp 0.17.0\n'],
    ['empty output', null, ''],
  ])('returns unavailable for %s', async (_description, error, stdout) => {
    const binding: ServerVersionProbeBinding = {
      serverPath: '/current/perllsp',
      generation: 8,
    };
    const deferred = deferredExecutor();
    const result = probeServerVersion(() => binding, deferred.execute);
    await Promise.resolve();

    deferred.callback()?.(error, stdout);

    await expect(result).resolves.toBe('unavailable');
  });
});
