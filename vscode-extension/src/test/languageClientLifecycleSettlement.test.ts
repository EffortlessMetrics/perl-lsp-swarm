import { describe, expect, jest, test } from '@jest/globals';
import {
  LanguageClientLifecycle,
  type LifecycleClient,
  type LifecycleDisposable,
} from '../languageClientLifecycle';

class DeferredVoid {
  readonly promise: Promise<void>;
  private readonly resolvePromise: () => void;

  constructor() {
    let resolvePromise: (() => void) | undefined;
    this.promise = new Promise<void>((resolve) => {
      resolvePromise = resolve;
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

describe('LanguageClientLifecycle terminal settlement', () => {
  test('does not start a replacement when stop times out', async () => {
    jest.useFakeTimers();
    try {
      const { controller, clients } = makeController();
      const first = await controller.start();
      expect(first).toBe(clients[0]);

      const hungStop = new DeferredVoid();
      first!.stop.mockReturnValue(hungStop.promise);

      const restart = controller.restart();
      await jest.advanceTimersByTimeAsync(10);

      await expect(restart).rejects.toThrow(
        'Language client cleanup is not terminal; replacement startup is blocked',
      );
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

      await expect(controller.start()).rejects.toThrow(
        'Language client cleanup is not terminal; replacement startup is blocked',
      );
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

    await expect(restart).rejects.toThrow(
      'Language client cleanup is not terminal; replacement startup is blocked',
    );
    expect(clients).toHaveLength(1);
    expect(clients[0]!.stop).toHaveBeenCalledTimes(1);
    expect(clients[0]!.dispose).toHaveBeenCalledTimes(1);
    expect(controller.snapshot.state).toBe('failed');
  });
});
