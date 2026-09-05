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

/** Stands in for the server child process the real node client owns. */
class FakeServerProcess {
  exited = false;
}

class FakeClient implements LifecycleClient {
  /** Stands in for the real client's own terminal state (State.Stopped). */
  terminal = false;
  /**
   * Mirrors `LanguageClient.serverProcess`: present while the client owns a
   * child, cleared by the client itself inside `stop()`.
   */
  serverProcess: FakeServerProcess | undefined = new FakeServerProcess();
  start = jest.fn(async () => undefined);
  stop = jest.fn(async () => undefined);
  dispose = jest.fn(async () => undefined);

  onDidChangeState(_listener: (event: unknown) => void): LifecycleDisposable {
    return { dispose: jest.fn() };
  }
}

function makeController(
  stopTimeoutMs = 10,
  isClientTerminal?: (client: FakeClient, witness: unknown) => boolean | Promise<boolean>,
  captureStopWitness?: (client: FakeClient) => unknown,
): {
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
      ...(isClientTerminal ? { isClientTerminal } : {}),
      ...(captureStopWitness ? { captureStopWitness } : {}),
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

  test('a settled stop rejection from a terminal client admits the replacement (hung server, #14155)', async () => {
    // vscode-languageclient rejects stop() with a shutdown timeout after its
    // own cleanup finished and it terminated the server process; a hung
    // server (the watchdog case) always produces this shape.
    const { controller, clients } = makeController(10, (client) => client.terminal);
    const first = await controller.start();
    first!.terminal = true;
    first!.stop.mockRejectedValue(new Error('Stopping the server timed out'));

    const replacement = await controller.restart();

    expect(replacement).toBe(clients[1]);
    expect(clients).toHaveLength(2);
    expect(first!.stop).toHaveBeenCalledTimes(1);
    expect(first!.dispose).toHaveBeenCalledTimes(1);
    expect(controller.snapshot.state).toBe('running');
    expect(controller.snapshot.error).toBeUndefined();
  });

  test('a settled stop rejection from a client that is not terminal still blocks replacement', async () => {
    const { controller, clients } = makeController(10, (client) => client.terminal);
    const first = await controller.start();
    first!.terminal = false;
    first!.stop.mockRejectedValue(new Error('Stopping the server timed out'));

    await expect(controller.restart()).rejects.toMatchObject({ reason: 'cleanup-incomplete' });
    expect(clients).toHaveLength(1);
    expect(controller.snapshot.state).toBe('failed');
  });

  /**
   * Production shape: vscode-languageclient moves to State.Stopped and clears
   * `serverProcess` inside `stop()` before it schedules process termination,
   * so the handle must be captured before `stop()` and the exit observed on
   * that captured handle, not inferred from the client state.
   */
  function makeProcessBoundController(): ReturnType<typeof makeController> {
    return makeController(
      50,
      async (client, witness) => {
        if (!client.terminal) {
          return false;
        }
        const child = witness as FakeServerProcess | undefined;
        if (child === undefined) {
          return true;
        }
        // Bounded wait for the exit, like awaitServerProcessExit.
        for (let i = 0; i < 3 && !child.exited; i += 1) {
          await Promise.resolve();
        }
        return child.exited;
      },
      (client) => client.serverProcess,
    );
  }

  function rejectStopLikeLanguageClient(client: FakeClient): void {
    client.stop.mockImplementation(async () => {
      client.terminal = true;
      client.serverProcess = undefined;
      throw new Error('Stopping the server timed out');
    });
  }

  test('a Stopped client whose server process is still alive blocks the replacement (#14155)', async () => {
    const { controller, clients } = makeProcessBoundController();
    const first = await controller.start();
    const child = first!.serverProcess!;
    rejectStopLikeLanguageClient(first!);

    await expect(controller.restart()).rejects.toMatchObject({ reason: 'cleanup-incomplete' });

    expect(child.exited).toBe(false);
    expect(first!.terminal).toBe(true);
    expect(first!.serverProcess).toBeUndefined();
    expect(clients).toHaveLength(1);
    expect(controller.snapshot.state).toBe('failed');
  });

  test('a Stopped client whose captured server process has exited admits the replacement (#14155)', async () => {
    const { controller, clients } = makeProcessBoundController();
    const first = await controller.start();
    const child = first!.serverProcess!;
    first!.stop.mockImplementation(async () => {
      first!.terminal = true;
      first!.serverProcess = undefined;
      child.exited = true;
      throw new Error('Stopping the server timed out');
    });

    const replacement = await controller.restart();

    expect(replacement).toBe(clients[1]);
    expect(clients).toHaveLength(2);
    expect(controller.snapshot.state).toBe('running');
  });

  test('the stop witness is captured before stop() runs', async () => {
    const seen: unknown[] = [];
    const { controller } = makeController(
      10,
      (_client, witness) => {
        seen.push(witness);
        return true;
      },
      (client) => client.serverProcess,
    );
    const first = await controller.start();
    const child = first!.serverProcess;
    rejectStopLikeLanguageClient(first!);

    await controller.restart();

    expect(seen).toEqual([child]);
  });

  test('a terminal check that outlives the stop bound blocks the replacement', async () => {
    jest.useFakeTimers();
    try {
      const { controller, clients } = makeController(10, () => new Promise<boolean>(() => undefined));
      const first = await controller.start();
      first!.stop.mockRejectedValue(new Error('Stopping the server timed out'));

      const restart = controller.restart();
      await jest.advanceTimersByTimeAsync(10);

      await expect(restart).rejects.toMatchObject({ reason: 'cleanup-incomplete' });
      expect(clients).toHaveLength(1);
      expect(controller.snapshot.state).toBe('failed');
    } finally {
      jest.useRealTimers();
    }
  });

  test('a stop that outlives the bound blocks replacement even when the client reports terminal', async () => {
    jest.useFakeTimers();
    try {
      const { controller, clients } = makeController(10, (client) => client.terminal);
      const first = await controller.start();
      first!.terminal = true;
      first!.stop.mockReturnValue(new DeferredVoid().promise);

      const restart = controller.restart();
      await jest.advanceTimersByTimeAsync(10);

      await expect(restart).rejects.toMatchObject({ reason: 'cleanup-incomplete' });
      expect(clients).toHaveLength(1);
      expect(controller.snapshot.state).toBe('failed');
    } finally {
      jest.useRealTimers();
    }
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
