/**
 * Unit tests for mid-session silent server crash recovery (#4625).
 *
 * Covers the `handleClientStateChange` Running → Stopped branch:
 *   - an unexpected crash surfaces a `showErrorMessage` toast
 *   - the diagnosis hint is captured for `serverNotRunningMessage()`
 *   - auto-restart is attempted up to MAX_AUTO_RESTART_ATTEMPTS (3)
 *   - once the budget is exhausted, a "could not be restarted" toast is shown
 *     and no further auto-restart is attempted
 *   - a user-initiated stop sentinel suppresses the crash notification
 *   - a stable prior run resets the auto-restart budget
 *
 * Most cases run without a lifecycle controller, so `restartServer()`
 * short-circuits with a warning (the counter is incremented before
 * `restartServer` is called, which is what these tests assert on). The
 * #12724 convergence cases inject a live lifecycle controller with fake
 * clients whose `start()` rejects, reproducing a replacement that fails
 * during startup.
 */

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {},
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
  State: { Stopped: 1, Running: 2, Starting: 3 },
  StateChangeEvent: class {
    constructor(
      public oldState: number,
      public newState: number,
    ) {}
  },
}));

import * as vscode from 'vscode';
import type { LanguageClient, StateChangeEvent } from 'vscode-languageclient/node';
import { ExtensionLanguageClientLifecycle } from '../extensionComposition';
import type { LifecycleDisposable, LifecycleHooks } from '../languageClientLifecycle';
import {
  handleClientStateChange,
  serverNotRunningMessage,
  _resetCrashRecoveryStateForTest,
  _autoRestartAttemptsForTest,
  _markStableRunningForTest,
  _setExtensionContextForTest,
  _setUserInitiatedStopPendingForTest,
  _setLastStartupDiagnosisForTest,
  _watchdogFailureForTest,
  _spawnReplacementCrashGenerationForTest,
  _setLanguageClientLifecycleForTest,
  _handleLifecycleClientStateChangeForTest,
  _languageClientConnectionOptionsForTest,
  _restartServerForTest,
} from '../extension';

// vscode-languageclient State numeric values (Stopped=1, Running=2, Starting=3).
const STATE_STOPPED = 1;
const STATE_RUNNING = 2;

interface StateChangeEventLike {
  oldState: number;
  newState: number;
}

function crashEvent(): StateChangeEventLike {
  return { oldState: STATE_RUNNING, newState: STATE_STOPPED };
}

function makeContext(): vscode.ExtensionContext {
  const state = {
    get: jest.fn(() => undefined),
    update: jest.fn(async () => undefined),
  };
  return {
    extension: {
      packageJSON: { publisher: 'EffortlessMetrics', name: 'perl-lsp-rs', version: '0.16.0' },
    },
    extensionMode: vscode.ExtensionMode.Production,
    extensionPath: '/tmp/perl-lsp-test',
    globalState: state,
    subscriptions: [],
    workspaceState: state,
  } as unknown as vscode.ExtensionContext;
}

/** Flush pending microtasks so fire-and-forget async work in the handler runs. */
function flush(): Promise<void> {
  return new Promise((resolve) => {
    setImmediate(resolve);
  });
}

const showErrorMessage = vscode.window.showErrorMessage as jest.Mock;
const showWarningMessage = vscode.window.showWarningMessage as jest.Mock;

// ---------------------------------------------------------------------------
// #12724 harness: a live lifecycle controller with fake clients whose
// `start()` can reject, modelling replacements that fail during startup on a
// loaded host (the hosted-journey failure shape) without real processes.
// ---------------------------------------------------------------------------

interface FakeLifecycleEvent {
  oldState: number;
  newState: number;
}

class FakeLifecycleClient {
  private readonly listeners = new Set<(event: FakeLifecycleEvent) => void>();
  constructor(
    private readonly startOutcome: Promise<void>,
    readonly id = 0,
    private readonly trace?: string[],
    private readonly emitRunningOnStart = false,
  ) {}
  async start(): Promise<void> {
    this.trace?.push(`start:${this.id}`);
    await this.startOutcome;
    if (this.emitRunningOnStart) {
      this.emit({ oldState: 3, newState: STATE_RUNNING });
    }
  }
  async stop(): Promise<void> {
    this.trace?.push(`stop:${this.id}`);
  }
  dispose(): void | Promise<void> {
    this.trace?.push(`dispose:${this.id}`);
  }
  onDidChangeState(listener: (event: FakeLifecycleEvent) => void): LifecycleDisposable {
    this.listeners.add(listener);
    return {
      dispose: () => {
        this.listeners.delete(listener);
      },
    };
  }
  private emit(event: FakeLifecycleEvent): void {
    for (const listener of this.listeners) {
      listener(event);
    }
  }
}

function makeStartupFailingLifecycle(failuresBeforeSuccess: number): {
  lifecycle: ExtensionLanguageClientLifecycle<FakeLifecycleClient, FakeLifecycleEvent>;
  createdClients: () => number;
} {
  // The initial (pre-crash) generation starts successfully; only replacement
  // generations after the crash fail during startup.
  let created = 0;
  const hooks: LifecycleHooks<FakeLifecycleClient, FakeLifecycleEvent> = {
    resolveServerPath: async () => '/server/perllsp',
    createClient: () => {
      const isInitialGeneration = created === 0;
      created += 1;
      const willFailDuringStartup = !isInitialGeneration && created - 1 <= failuresBeforeSuccess;
      return new FakeLifecycleClient(
        willFailDuringStartup
          ? Promise.reject(new Error('simulated slow-host startup failure'))
          : Promise.resolve(),
      );
    },
  };
  return {
    lifecycle: new ExtensionLanguageClientLifecycle(hooks),
    createdClients: () => created,
  };
}

/**
 * #14448 harness: a live lifecycle whose client fails `stop()`, so cleanup
 * stays incomplete and the lifecycle blocks replacement startup until reload.
 */
class StopFailingLifecycleClient extends FakeLifecycleClient {
  override async stop(): Promise<void> {
    throw new Error('simulated client cleanup failure');
  }
}

function makeCleanupBlockingLifecycle(): ExtensionLanguageClientLifecycle<
  FakeLifecycleClient,
  FakeLifecycleEvent
> {
  const hooks: LifecycleHooks<FakeLifecycleClient, FakeLifecycleEvent> = {
    resolveServerPath: async () => '/server/perllsp',
    createClient: () => new StopFailingLifecycleClient(Promise.resolve()),
  };
  return new ExtensionLanguageClientLifecycle(hooks);
}

function makeFinalizationFailingLifecycle(finalizationDelayMs: number): {
  lifecycle: ExtensionLanguageClientLifecycle<FakeLifecycleClient, FakeLifecycleEvent>;
  trace: string[];
  states: Array<{ generation: number; state: string }>;
} {
  let created = 0;
  const trace: string[] = [];
  const states: Array<{ generation: number; state: string }> = [];
  const hooks: LifecycleHooks<FakeLifecycleClient, FakeLifecycleEvent> = {
    resolveServerPath: async () => '/server/perllsp',
    createClient: () => {
      created += 1;
      trace.push(`create:${created}`);
      return new FakeLifecycleClient(Promise.resolve(), created, trace, created > 1);
    },
    onStateChange: (snapshot) => {
      states.push({ generation: snapshot.generation, state: snapshot.state });
    },
    onClientStateChange: (_client, event) => {
      _handleLifecycleClientStateChangeForTest(event as unknown as StateChangeEvent);
    },
    onStarted: async (startedClient) => {
      if (startedClient.id === 1) {
        return;
      }
      if (finalizationDelayMs === 0) {
        throw new Error('simulated startup finalization failure');
      }
      await new Promise<void>((_, reject) => {
        setTimeout(
          () => reject(new Error('simulated slow startup finalization failure')),
          finalizationDelayMs,
        );
      });
    },
  };
  return {
    lifecycle: new ExtensionLanguageClientLifecycle(hooks),
    trace,
    states,
  };
}

function injectedLifecycle(
  lifecycle: ExtensionLanguageClientLifecycle<FakeLifecycleClient, FakeLifecycleEvent>,
): ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent> {
  return lifecycle as unknown as ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent>;
}

/** Drain the recovery continuation chain (pure microtask/immediate cascades). */
async function drain(rounds = 40): Promise<void> {
  for (let round = 0; round < rounds; round += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

async function drainMicrotasks(rounds = 80): Promise<void> {
  for (let round = 0; round < rounds; round += 1) {
    await Promise.resolve();
  }
}

describe('mid-session silent server crash recovery (#4625)', () => {
  beforeEach(() => {
    _resetCrashRecoveryStateForTest();
    _setLastStartupDiagnosisForTest(undefined);
    _setExtensionContextForTest(makeContext());
    _setLanguageClientLifecycleForTest(undefined);
    showErrorMessage.mockReset();
    showErrorMessage.mockResolvedValue(undefined);
    showWarningMessage.mockReset();
    showWarningMessage.mockResolvedValue(undefined);
  });

  test('delegates connection-close restart ownership exclusively to the lifecycle arbiter', () => {
    expect(_languageClientConnectionOptionsForTest()).toEqual({ maxRestartCount: 0 });
  });

  test('unexpected Running → Stopped shows an error toast and captures the diagnosis', async () => {
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    const message = showErrorMessage.mock.calls[0][0] as string;
    expect(message).toMatch(/Perl Language Server crashed/i);
    expect(message).toMatch(/restarting automatically/i);

    // The diagnosis hint is now surfaced via serverNotRunningMessage().
    expect(serverNotRunningMessage()).toMatch(/stopped unexpectedly/i);
  });

  test('auto-restart counter increments and caps at MAX_AUTO_RESTART_ATTEMPTS (3)', async () => {
    // Three crashes each consume one retry slot.
    for (let i = 0; i < 3; i++) {
      handleClientStateChange(crashEvent() as never);
      await flush();
    }
    expect(_autoRestartAttemptsForTest()).toBe(3);
    // Each of the three attempts showed a "restarting automatically" toast.
    expect(showErrorMessage).toHaveBeenCalledTimes(3);
    for (const call of showErrorMessage.mock.calls) {
      expect(call[0]).toMatch(/restarting automatically/i);
    }

    // A fourth crash exhausts the budget: a different toast, no further retry.
    showErrorMessage.mockClear();
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    const exhaustedMessage = showErrorMessage.mock.calls[0][0] as string;
    expect(exhaustedMessage).toMatch(/could not be restarted automatically/i);
    // Counter must not exceed 3.
    expect(_autoRestartAttemptsForTest()).toBe(3);
  });

  test('budget exhaustion retires the live client after exactly three replacements', async () => {
    let created = 0;
    const lifecycle = new ExtensionLanguageClientLifecycle<FakeLifecycleClient, FakeLifecycleEvent>(
      {
        resolveServerPath: async () => '/server/perllsp',
        createClient: () => {
          created += 1;
          return new FakeLifecycleClient(Promise.resolve(), created);
        },
      },
    );
    _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
    await lifecycle.start();

    for (let crash = 0; crash < 3; crash += 1) {
      handleClientStateChange(crashEvent() as never);
      await drain();
    }

    expect(created).toBe(4); // initial client plus exactly three replacements
    expect(_autoRestartAttemptsForTest()).toBe(3);
    showErrorMessage.mockResolvedValue('Restart Server');
    handleClientStateChange(crashEvent() as never);
    await drain();

    expect(created).toBe(5); // explicit retry creates exactly one fresh client
    expect(_autoRestartAttemptsForTest()).toBe(0);
    expect(lifecycle.snapshot.state).toBe('running');
    expect(lifecycle.client).toBeDefined();
  });

  test('budget exhaustion without explicit retry leaves the client unavailable', async () => {
    let created = 0;
    const lifecycle = new ExtensionLanguageClientLifecycle<FakeLifecycleClient, FakeLifecycleEvent>(
      {
        resolveServerPath: async () => '/server/perllsp',
        createClient: () => {
          created += 1;
          return new FakeLifecycleClient(Promise.resolve(), created);
        },
      },
    );
    _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
    await lifecycle.start();
    for (let crash = 0; crash < 4; crash += 1) {
      handleClientStateChange(crashEvent() as never);
      await drain();
    }

    expect(created).toBe(4);
    expect(lifecycle.snapshot.state).toBe('stopped');
    expect(lifecycle.client).toBeUndefined();
  });

  test('a user-initiated stop suppresses the crash toast and the counter', async () => {
    _setUserInitiatedStopPendingForTest(true);
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(showErrorMessage).not.toHaveBeenCalled();
    expect(_autoRestartAttemptsForTest()).toBe(0);
  });

  test('a stable prior run resets the auto-restart budget', async () => {
    // First crash consumes a slot (counter 0 → 1).
    handleClientStateChange(crashEvent() as never);
    await flush();
    expect(_autoRestartAttemptsForTest()).toBe(1);

    // Simulate the server having been stably running long enough that the next
    // crash counts as a new episode.
    _markStableRunningForTest();

    handleClientStateChange(crashEvent() as never);
    await flush();

    // Budget was reset to 0 then incremented to 1 — not 2.
    expect(_autoRestartAttemptsForTest()).toBe(1);
  });

  test('a non-crash transition (e.g. Starting) does not surface a toast', async () => {
    handleClientStateChange({ oldState: STATE_STOPPED, newState: 3 } as never);
    await flush();
    expect(showErrorMessage).not.toHaveBeenCalled();
    expect(_autoRestartAttemptsForTest()).toBe(0);
  });

  // ------------------------------------------------------------------
  // #7845: convergence wiring through the generation-owned arbiter.
  // ------------------------------------------------------------------

  test('a duplicate stopped callback for the same failed generation arbitrates exactly once', async () => {
    // Two Running → Stopped callbacks arrive in the same tick for the same
    // failed generation: one recovery operation, one toast, one budget slot.
    handleClientStateChange(crashEvent() as never);
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(_autoRestartAttemptsForTest()).toBe(1);
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toMatch(/restarting automatically/i);
  });

  test('a watchdog observation racing a process exit deduplicates into one episode', async () => {
    // Watchdog fires first, then the process exits for the same generation.
    void _watchdogFailureForTest();
    handleClientStateChange(crashEvent() as never);
    await flush();

    // Watchdog and process exit must not increment the budget twice.
    expect(_autoRestartAttemptsForTest()).toBe(1);
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
  });

  test('a process exit followed by an in-flight watchdog observation deduplicates', async () => {
    // Process exits first; the watchdog timeout for the same generation is
    // still in flight (recovery has not yet replaced the generation). Both
    // observations must land in one episode before the restart completes.
    handleClientStateChange(crashEvent() as never);
    void _watchdogFailureForTest();
    await flush();

    expect(_autoRestartAttemptsForTest()).toBe(1);
    const restartToasts = showErrorMessage.mock.calls.filter((call) =>
      /restarting automatically/i.test(String(call[0])),
    );
    expect(restartToasts).toHaveLength(1);
  });

  test('a stale watchdog result for a superseded generation is dropped, not arbitrated', async () => {
    // Generation 0 crashes and is recovered (the replacement generation 1
    // is now current). A watchdog probe that started before the crash and
    // timed out after the replacement started must not open a second
    // episode or restart the healthy replacement.
    handleClientStateChange(crashEvent() as never);
    await flush();
    expect(_autoRestartAttemptsForTest()).toBe(1);

    void _watchdogFailureForTest(0);
    await flush();

    expect(_autoRestartAttemptsForTest()).toBe(1);
    const restartToasts = showErrorMessage.mock.calls.filter((call) =>
      /restarting automatically/i.test(String(call[0])),
    );
    expect(restartToasts).toHaveLength(1);
  });

  test('a manual restart from the exhausted toast never consumes crash budget', async () => {
    for (let i = 0; i < 3; i++) {
      handleClientStateChange(crashEvent() as never);
      await flush();
    }
    expect(_autoRestartAttemptsForTest()).toBe(3);

    // The fourth crash exhausts the budget; the user picks Restart Server.
    showErrorMessage.mockReset();
    showErrorMessage.mockResolvedValue('Restart Server');
    handleClientStateChange(crashEvent() as never);
    await flush();

    const exhaustedMessage = showErrorMessage.mock.calls[0][0] as string;
    expect(exhaustedMessage).toMatch(/could not be restarted automatically/i);
    // The explicit user restart reset the budget without consuming a slot.
    expect(_autoRestartAttemptsForTest()).toBe(0);
  });

  test('deactivation clears episode state so no armed recovery survives the session', async () => {
    handleClientStateChange(crashEvent() as never);
    await flush();
    expect(_autoRestartAttemptsForTest()).toBe(1);

    // disposeLanguageClient performs the same explicit-recovery reset used
    // by deactivation and managed updates; model it through the reset entry.
    _resetCrashRecoveryStateForTest();
    expect(_autoRestartAttemptsForTest()).toBe(0);

    // A fresh session's first crash starts a brand-new episode at attempt 1.
    handleClientStateChange(crashEvent() as never);
    await flush();
    expect(_autoRestartAttemptsForTest()).toBe(1);
  });

  // ------------------------------------------------------------------
  // #7845 review falsifier: generation G+1 fails before generation G's
  // `restartServer` promise resolves. The G+1 failure must be serialized
  // behind G's active episode (no second concurrent restart), and G's
  // continuation must settle exactly its own episode handle — never the
  // newer episode — before the deferred G+1 failure re-arbitrates.
  // ------------------------------------------------------------------
  test('a replacement generation failing before the pending restart resolves is serialized behind the active episode', async () => {
    // Generation 0 crashes: episode recovery-0-1 opens and its continuation
    // begins awaiting restartServer.
    handleClientStateChange(crashEvent() as never);
    expect(showErrorMessage).toHaveBeenCalledTimes(1);

    // The pending restart has already spawned generation 1, and generation
    // 1 fails BEFORE generation 0's restartServer promise resolves.
    _spawnReplacementCrashGenerationForTest();
    handleClientStateChange(crashEvent() as never);

    // No second concurrent restart may start while generation 0's restart
    // promise is still pending: exactly one restart toast so far and one
    // consumed budget slot.
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(_autoRestartAttemptsForTest()).toBe(1);

    await flush();

    // Generation 0's continuation settled exactly its own episode handle,
    // then the deferred generation-1 failure re-arbitrated serially into
    // the next episode: attempt 2, one more restart, in order.
    expect(_autoRestartAttemptsForTest()).toBe(2);
    const restartToasts = showErrorMessage.mock.calls.filter((call) =>
      /restarting automatically/i.test(String(call[0])),
    );
    expect(restartToasts).toHaveLength(2);
    expect(String(restartToasts[0][0])).toMatch(/attempt 1\/3/);
    expect(String(restartToasts[1][0])).toMatch(/attempt 2\/3/);
  });

  // ------------------------------------------------------------------
  // #12724: a replacement that fails DURING STARTUP (never reaches
  // Running) produces no further Running→Stopped crash event and no
  // watchdog observation, so settling `recovery_failed` without re-arming
  // deadlocks convergence: budget remains, but no restart ever happens
  // again and documents are never re-marked ready. The recovery must
  // re-arm by arbitrating the failed replacement's own recorded failure
  // through the same entry point, while genuinely repeated failures still
  // exhaust fail-closed into the exhaustion dialog.
  // ------------------------------------------------------------------

  test('a replacement failing during startup re-arms recovery until a replacement runs (#12724)', async () => {
    // Attempts 1 and 2 reject during client.start(); attempt 3 succeeds.
    const { lifecycle } = makeStartupFailingLifecycle(2);
    _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
    await lifecycle.start();
    expect(lifecycle.snapshot.state).toBe('running');

    handleClientStateChange(crashEvent() as never);
    await drain();

    expect(lifecycle.snapshot.state).toBe('running');
    expect(_autoRestartAttemptsForTest()).toBe(3);
    const restartToasts = showErrorMessage.mock.calls.filter((call) =>
      /restarting automatically/i.test(String(call[0])),
    );
    expect(restartToasts).toHaveLength(3);
    expect(String(restartToasts[0][0])).toMatch(/attempt 1\/3/);
    expect(String(restartToasts[1][0])).toMatch(/attempt 2\/3/);
    expect(String(restartToasts[2][0])).toMatch(/attempt 3\/3/);
  });

  test('genuinely repeated startup failures still exhaust the budget fail-closed (#12724)', async () => {
    const { lifecycle } = makeStartupFailingLifecycle(Number.POSITIVE_INFINITY);
    _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
    await lifecycle.start();

    handleClientStateChange(crashEvent() as never);
    await drain();

    expect(_autoRestartAttemptsForTest()).toBe(3);
    expect(lifecycle.snapshot.state).toBe('stopped');
    const exhaustedToasts = showErrorMessage.mock.calls.filter((call) =>
      /could not be restarted automatically/i.test(String(call[0])),
    );
    expect(exhaustedToasts).toHaveLength(1);
    // The exhaustion dialog is terminal: it never offers an automatic retry.
    expect(String(exhaustedToasts[0][0])).not.toMatch(/restarting automatically/i);
  });

  test('raw Running cannot reset retry budget before slow startup finalization rejects (#12724)', async () => {
    jest.useFakeTimers();
    try {
      const finalizationDelayMs = 31_000;
      const { lifecycle, states } = makeFinalizationFailingLifecycle(finalizationDelayMs);
      _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
      await lifecycle.start();

      handleClientStateChange(crashEvent() as never);
      for (let attempt = 0; attempt < 3; attempt += 1) {
        await jest.advanceTimersByTimeAsync(finalizationDelayMs);
      }
      await jest.advanceTimersByTimeAsync(0);

      expect(_autoRestartAttemptsForTest()).toBe(3);
      const restartToasts = showErrorMessage.mock.calls.filter((call) =>
        /restarting automatically/i.test(String(call[0])),
      );
      expect(restartToasts).toHaveLength(3);
      expect(restartToasts.map((call) => String(call[0]))).toEqual([
        expect.stringMatching(/attempt 1\/3/),
        expect.stringMatching(/attempt 2\/3/),
        expect.stringMatching(/attempt 3\/3/),
      ]);
      const exhaustedToasts = showErrorMessage.mock.calls.filter((call) =>
        /could not be restarted automatically/i.test(String(call[0])),
      );
      expect(exhaustedToasts).toHaveLength(1);
      expect(
        states.filter(({ generation, state }) => generation > 1 && state === 'running'),
      ).toEqual([]);
    } finally {
      jest.clearAllTimers();
      jest.useRealTimers();
    }
  });

  test('failed-start client is stopped and disposed before the next generation is created (#12724)', async () => {
    jest.useFakeTimers();
    try {
      const { lifecycle, trace } = makeFinalizationFailingLifecycle(0);
      _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
      await lifecycle.start();

      handleClientStateChange(crashEvent() as never);
      await drainMicrotasks();

      const stopped = trace.indexOf('stop:2');
      const disposed = trace.indexOf('dispose:2');
      const nextCreated = trace.indexOf('create:3');
      expect(stopped).toBeGreaterThan(-1);
      expect(disposed).toBeGreaterThan(stopped);
      expect(nextCreated).toBeGreaterThan(disposed);
    } finally {
      jest.clearAllTimers();
      jest.useRealTimers();
    }
  });

  // ------------------------------------------------------------------
  // #14448: a client whose stop() fails leaves cleanup incomplete; the
  // lifecycle then refuses replacement construction until the window
  // reloads. Automatic recovery must surface the reload remediation once
  // and must not re-arm a permanently blocked lifecycle through the
  // remaining retry budget, and an explicit restart must offer the same
  // reload remediation instead of a generic failure.
  // ------------------------------------------------------------------

  test('auto-recovery against a cleanup-blocked lifecycle presents reload remediation without re-arming (#14448)', async () => {
    const lifecycle = makeCleanupBlockingLifecycle();
    _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
    await lifecycle.start();
    expect(lifecycle.snapshot.state).toBe('running');

    handleClientStateChange(crashEvent() as never);
    await drain();

    expect(lifecycle.snapshot.state).toBe('failed');
    // One observation consumed exactly one budget slot; the blocked
    // lifecycle was not re-arbitrated through the remaining budget.
    expect(_autoRestartAttemptsForTest()).toBe(1);
    const reloadToasts = showErrorMessage.mock.calls.filter(
      (call) => /did not finish cleaning up/i.test(String(call[0])) && call.includes('Reload Window'),
    );
    expect(reloadToasts).toHaveLength(1);
  });

  test('explicit restart blocked by incomplete cleanup offers window reload instead of a generic failure (#14448)', async () => {
    const lifecycle = makeCleanupBlockingLifecycle();
    _setLanguageClientLifecycleForTest(injectedLifecycle(lifecycle));
    await lifecycle.start();
    expect(lifecycle.snapshot.state).toBe('running');

    const blocked = await _restartServerForTest(makeContext());

    expect(blocked).toBe(true);
    expect(lifecycle.snapshot.state).toBe('failed');
    const reloadToasts = showErrorMessage.mock.calls.filter(
      (call) => /did not finish cleaning up/i.test(String(call[0])) && call.includes('Reload Window'),
    );
    expect(reloadToasts).toHaveLength(1);
    const genericFailures = showErrorMessage.mock.calls.filter((call) =>
      /^Failed to restart Perl Language Server/i.test(String(call[0])),
    );
    expect(genericFailures).toHaveLength(0);
  });
});
