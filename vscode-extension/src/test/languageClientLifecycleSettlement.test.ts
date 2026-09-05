import { describe, expect, jest, test } from '@jest/globals';
import {
  LanguageClientLifecycle,
  LanguageClientLifecycleError,
  type LifecycleClient,
  type LifecycleDisposable,
} from '../languageClientLifecycle';

class DeferredVoid {
  readonly promise: Promise<undefined>;
  private readonly resolvePromise: () => void;

  constructor() {
    let resolvePromise: (() => void) | undefined;
    this.promise = new Promise<undefined>((resolve) => {
      resolvePromise = () => resolve(undefined);
    });
    if (!resolvePromise) {
      throw new Error('Deferred resolver was not initialized');
    }
    this.resolvePromise = resolvePromise;
  }

  resolve(): void {
    this.resolvePromise();
  }
}

class FakeClient implements LifecycleClient {
  start = jest.fn(async () => undefined);
  stop = jest.fn(async () => undefined);
  dispose = jest.fn(async () => undefined);

  onDidChangeState(_listener: (event: unknown) => void): LifecycleDisposable {
    return { dispose: jest.fn() };
  }
}

function makeController(stopTimeoutMs = 10): {
  controller: LanguageClientLifecycle<FakeClient>;
  clients: FakeClient[];
} {
  const clients: FakeClient[] = [];
  const controller = new LanguageClientLifecycle<FakeClient>(
    {
      resolveServerPath: async () => '/server/perllsp',
      createClient: () => {
        const client = new FakeClient();
        clients.push(client);
        return client;
      },
    },
    { stopTimeoutMs },
  );
  return { controller, clients };
}

describe('LanguageClientLifecycle client cleanup admission', () => {
  test('does not start a replacement when stop does not complete', async () => {
    jest.useFakeTimers();
    try {
      const { controller, clients } = makeController();
      const first = await controller.start();
      expect(first).toBe(clients[0]);

      const hungStop = new DeferredVoid();
      first!.stop.mockReturnValue(hungStop.promise);

      const restart = controller.restart();
      await jest.advanceTimersByTimeAsync(10);

      await expect(restart).rejects.toBeInstanceOf(LanguageClientLifecycleError);
      await expect(restart).rejects.toMatchObject({ reason: 'cleanup-incomplete' });
      expect(clients).toHaveLength(1);
      expect(first!.dispose).toHaveBeenCalledTimes(1);
      expect(controller.snapshot.state).toBe('failed');
    } finally {
      jest.useRealTimers();
    }
  });

  test('a late old stop completion cannot unblock replacement startup', async () => {
    jest.useFakeTimers();
    try {
      const { controller, clients } = makeController();
      const first = await controller.start();
      const hungStop = new DeferredVoid();
      first!.stop.mockReturnValue(hungStop.promise);

      const stop = controller.stop();
      await jest.advanceTimersByTimeAsync(10);
      await expect(stop).resolves.toBeUndefined();
      expect(controller.snapshot.state).toBe('failed');

      hungStop.resolve();
      await Promise.resolve();
      await Promise.resolve();

      await expect(controller.start()).rejects.toBeInstanceOf(LanguageClientLifecycleError);
      expect(clients).toHaveLength(1);
    } finally {
      jest.useRealTimers();
    }
  });

  test('listener cleanup failure blocks a replacement even when stop and dispose resolve', async () => {
    const clients: FakeClient[] = [];
    const listenerError = new Error('listener cleanup failed');
    const controller = new LanguageClientLifecycle<FakeClient>({
      resolveServerPath: async () => '/server/perllsp',
      createClient: () => {
        const client = new FakeClient();
        client.onDidChangeState = () => ({
          dispose: () => {
            throw listenerError;
          },
        });
        clients.push(client);
        return client;
      },
    });

    await controller.start();
    const restart = controller.restart();

    await expect(restart).rejects.toBeInstanceOf(LanguageClientLifecycleError);
    expect(clients).toHaveLength(1);
    expect(clients[0]!.stop).toHaveBeenCalledTimes(1);
    expect(clients[0]!.dispose).toHaveBeenCalledTimes(1);
    expect(controller.snapshot.state).toBe('failed');
  });

  test.each(['stop', 'dispose'] as const)(
    '%s rejecting without a reason blocks replacement startup',
    async (operation) => {
      const { controller, clients } = makeController();
      await controller.start();
      clients[0]![operation].mockRejectedValue(undefined);

      await expect(controller.restart()).rejects.toBeInstanceOf(LanguageClientLifecycleError);
      expect(clients).toHaveLength(1);
      expect(controller.snapshot.state).toBe('failed');
    },
  );

  test('a startup failure whose teardown also fails surfaces the cleanup block, not the startup error', async () => {
    const startupError = new Error('simulated startup failure');
    const cleanupError = new Error('simulated cleanup failure');
    const client = new FakeClient();
    client.start.mockRejectedValue(startupError);
    client.stop.mockRejectedValue(cleanupError);
    const clients: FakeClient[] = [];
    const controller = new LanguageClientLifecycle<FakeClient>({
      resolveServerPath: async () => '/server/perllsp',
      createClient: () => {
        clients.push(client);
        return client;
      },
    });

    const rejection = await controller.start().then(
      () => {
        throw new Error('start unexpectedly succeeded');
      },
      (error: unknown) => error,
    );

    // The lifecycle is replacement-blocked, so the surfaced rejection is the
    // cleanup block; the startup error remains as diagnostic cause.
    expect(rejection).toBeInstanceOf(LanguageClientLifecycleError);
    expect((rejection as LanguageClientLifecycleError).reason).toBe('cleanup-incomplete');
    expect((rejection as Error).cause).toBe(startupError);
    expect(controller.snapshot.state).toBe('failed');

    // The block is sticky: no later start or restart may construct a client.
    await expect(controller.start()).rejects.toMatchObject({ reason: 'cleanup-incomplete' });
    await expect(controller.restart()).rejects.toMatchObject({ reason: 'cleanup-incomplete' });
    expect(clients).toHaveLength(1);
  });
});
