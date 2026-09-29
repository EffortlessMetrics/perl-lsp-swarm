import { probeServerVersion, type ServerVersionProbeBinding } from '../serverVersionProbe';

type VersionCallback = (error: Error | null, stdout: string) => void;

const currentLifecycle = {};

function deferredExecutor() {
  let callback: VersionCallback | undefined;
  const execute = jest.fn(
    (_path: string, _args: string[], _options: { timeout: number }, next: VersionCallback) => {
      callback = next;
    },
  );
  return { execute, callback: () => callback };
}

describe('server-version probe freshness helper', () => {
  test.each([
    [
      'the lifecycle path changes',
      { lifecycle: currentLifecycle, serverPath: '/new/perllsp', generation: 4 },
    ],
    [
      'the lifecycle generation changes for the same path',
      { lifecycle: currentLifecycle, serverPath: '/old/perllsp', generation: 5 },
    ],
    [
      'the lifecycle clears its path during cleanup',
      { lifecycle: currentLifecycle, serverPath: null, generation: 5 },
    ],
  ])('discards a version result when %s', async (_description, nextBinding) => {
    let binding: ServerVersionProbeBinding = {
      lifecycle: currentLifecycle,
      serverPath: '/old/perllsp',
      generation: 4,
    };
    const deferred = deferredExecutor();
    let observed: string | undefined;
    const command = probeServerVersion(() => binding, deferred.execute).then((result) => {
      observed = result;
    });
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
  });

  test('keeps a successful result for the captured lifecycle identity', async () => {
    const binding = { lifecycle: currentLifecycle, serverPath: '/current/perllsp', generation: 7 };
    const deferred = deferredExecutor();
    let observed: string | undefined;
    const command = probeServerVersion(() => binding, deferred.execute).then((result) => {
      observed = result;
    });
    deferred.callback()?.(null, 'perllsp 0.17.0\nperllsp extra\n');
    await command;

    expect(observed).toBe('perllsp 0.17.0');
  });

  test.each([
    ['an execution error', new Error('version failed'), 'perllsp 0.17.0\n'],
    ['empty output', null, ''],
  ])('returns unavailable for %s', async (_description, error, stdout) => {
    const binding: ServerVersionProbeBinding = {
      lifecycle: currentLifecycle,
      serverPath: '/current/perllsp',
      generation: 8,
    };
    const deferred = deferredExecutor();
    const result = probeServerVersion(() => binding, deferred.execute);
    await Promise.resolve();

    deferred.callback()?.(error, stdout);

    await expect(result).resolves.toBe('unavailable');
  });

  test('discards a result when a replacement lifecycle reuses path and generation', async () => {
    let lifecycle: object = {};
    let binding: ServerVersionProbeBinding = {
      lifecycle,
      serverPath: '/same/perllsp',
      generation: 1,
    };
    const deferred = deferredExecutor();
    const result = probeServerVersion(() => binding, deferred.execute);
    lifecycle = {};
    binding = { lifecycle, serverPath: '/same/perllsp', generation: 1 };

    deferred.callback()?.(null, 'perllsp 0.17.0\n');

    await expect(result).resolves.toBe('unavailable');
  });
});
