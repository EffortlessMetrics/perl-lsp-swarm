/**
 * UI-free lifecycle ownership for the language client.
 *
 * This module deliberately knows nothing about VS Code. The extension supplies
 * path resolution, client construction, and presentation callbacks when it
 * composes the controller.
 */

export type LifecycleState =
  | 'stopped'
  | 'resolving'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'failed';

export type LifecycleCallbackPhase = 'state' | 'client-state' | 'started' | 'stopped' | 'failed';

export interface LifecycleDisposable {
  dispose(): void;
}

export interface LifecycleClient<TEvent = unknown> {
  start(): Promise<void>;
  stop(): Promise<void>;
  dispose(): void | Promise<void>;
  onDidChangeState(listener: (event: TEvent) => void): LifecycleDisposable;
}

export interface LifecycleSnapshot {
  readonly state: LifecycleState;
  readonly generation: number;
  readonly serverPath: string | null;
  readonly error: unknown;
}

export interface LifecycleHooks<TClient extends LifecycleClient<TEvent>, TEvent = unknown> {
  resolveServerPath(): Promise<string | null>;
  createClient(serverPath: string): TClient;
  onStateChange?(snapshot: LifecycleSnapshot): void | Promise<void>;
  onClientStateChange?(client: TClient, event: TEvent): void | Promise<void>;
  /**
   * Invoked once the client has started. Unlike the other hooks, an error
   * thrown or rejected here aborts startup: the client is shut down and the
   * error is surfaced as the rejection of start() or restart().
   */
  onStarted?(client: TClient, serverPath: string): void | Promise<void>;
  onStopped?(snapshot: LifecycleSnapshot): void | Promise<void>;
  onFailed?(snapshot: LifecycleSnapshot): void | Promise<void>;
  onCallbackError?(error: unknown, phase: LifecycleCallbackPhase): void | Promise<void>;
}

export interface LanguageClientLifecycleOptions {
  /** Maximum time allowed for each client stop or dispose operation. */
  stopTimeoutMs?: number;
}

export class LanguageClientLifecycleError extends Error {
  constructor(
    message: string,
    readonly reason: 'server-path-unresolved' | 'cleanup-incomplete' | 'lifecycle',
  ) {
    super(message);
    this.name = 'LanguageClientLifecycleError';
  }
}

interface ActiveClient<TClient extends LifecycleClient<TEvent>, TEvent = unknown> {
  readonly client: TClient;
  readonly serverPath: string;
  readonly generation: number;
  listener: LifecycleDisposable | undefined;
}

interface CleanupResult {
  readonly error: unknown | undefined;
  /** True only when every lifecycle-owned client cleanup call completed successfully. */
  readonly clientCleanupComplete: boolean;
}

interface BoundedOperationResult {
  readonly completed: boolean;
  readonly error: unknown;
}

const DEFAULT_STOP_TIMEOUT_MS = 5_000;

/**
 * Owns the complete lifecycle of one language-client generation.
 *
 * A generation is invalidated before stopping or restarting. Any asynchronous
 * work that belongs to an older generation must finish its cleanup without
 * publishing a stale running state.
 *
 * This owner can establish completion of the client-facing listener/stop/
 * dispose calls only. Exact child-process/resource terminality is a stronger
 * external observation supplied by the process-lifecycle owner.
 */
export class LanguageClientLifecycle<TClient extends LifecycleClient<TEvent>, TEvent = unknown> {
  private readonly stopTimeoutMs: number;
  private state: LifecycleState = 'stopped';
  private generation = 0;
  private serverPath: string | null = null;
  private error: unknown = undefined;
  private activeClient: ActiveClient<TClient, TEvent> | undefined;
  private startPromise: Promise<TClient | undefined> | undefined;
  private restartPromise: Promise<TClient | undefined> | undefined;
  private stopPromise: Promise<void> | undefined;
  private replacementBlockedError: unknown | undefined;
  private readonly cleanupPromises = new WeakMap<TClient, Promise<CleanupResult>>();

  constructor(
    private readonly hooks: LifecycleHooks<TClient, TEvent>,
    options: LanguageClientLifecycleOptions = {},
  ) {
    this.stopTimeoutMs = options.stopTimeoutMs ?? DEFAULT_STOP_TIMEOUT_MS;
  }

  get snapshot(): LifecycleSnapshot {
    return {
      state: this.state,
      generation: this.generation,
      serverPath: this.serverPath,
      error: this.error,
    };
  }

  /** True only while this exact client and generation remain authoritative. */
  isCurrent(client: TClient, generation: number): boolean {
    return (
      this.generation === generation &&
      this.activeClient?.generation === generation &&
      this.activeClient.client === client
    );
  }

  /** Start the current generation, coalescing concurrent callers. */
  start(): Promise<TClient | undefined> {
    if (this.restartPromise) {
      return this.restartPromise;
    }
    if (this.startPromise) {
      return this.startPromise;
    }
    if (this.replacementBlockedError !== undefined) {
      return Promise.reject(this.replacementBlockedFailure());
    }
    if (this.state === 'running' && this.activeClient) {
      return Promise.resolve(this.activeClient.client);
    }
    return this.beginStart();
  }

  /** Stop the current generation and invalidate all pending startup work. */
  stop(): Promise<void> {
    if (this.stopPromise) {
      return this.stopPromise;
    }

    const invalidatedStartPromise = this.startPromise;
    const promise = this.runStop();
    this.stopPromise = promise;
    const clearStopState = (): void => {
      if (this.startPromise === invalidatedStartPromise) {
        this.startPromise = undefined;
      }
      this.clearStopPromise(promise);
    };
    promise.then(clearStopState, clearStopState);
    return promise;
  }

  /** Stop the current generation and start a fresh one, coalescing callers. */
  restart(): Promise<TClient | undefined> {
    if (this.restartPromise) {
      return this.restartPromise;
    }

    const promise = this.runRestart();
    this.restartPromise = promise;
    promise.then(
      () => this.clearRestartPromise(promise),
      () => this.clearRestartPromise(promise),
    );
    return promise;
  }

  private beginStart(): Promise<TClient | undefined> {
    if (this.replacementBlockedError !== undefined) {
      return Promise.reject(this.replacementBlockedFailure());
    }
    if (this.startPromise) {
      return this.startPromise;
    }

    if (this.stopPromise) {
      const queued = this.stopPromise.then(() => {
        if (this.startPromise === queued) {
          this.startPromise = undefined;
        }
        return this.beginStart();
      });
      this.startPromise = queued;
      return queued;
    }

    const startGeneration = ++this.generation;
    const promise = this.runStart(startGeneration);
    this.startPromise = promise;
    promise.then(
      () => this.clearStartPromise(promise),
      () => this.clearStartPromise(promise),
    );
    return promise;
  }

  private async runStart(startGeneration: number): Promise<TClient | undefined> {
    this.error = undefined;
    this.serverPath = null;
    this.transition('resolving', startGeneration);

    let active: ActiveClient<TClient, TEvent> | undefined;
    try {
      const serverPath = await this.hooks.resolveServerPath();
      if (!this.isCurrentGeneration(startGeneration)) {
        return undefined;
      }
      if (!serverPath) {
        throw new LanguageClientLifecycleError(
          'Language server path could not be resolved.',
          'server-path-unresolved',
        );
      }

      this.serverPath = serverPath;
      this.transition('starting', startGeneration);
      const client = this.hooks.createClient(serverPath);
      active = {
        client,
        serverPath,
        generation: startGeneration,
        listener: undefined,
      };
      this.activeClient = active;
      active.listener = client.onDidChangeState((event) => {
        this.notifyClientState(active as ActiveClient<TClient, TEvent>, event);
      });

      await client.start();
      if (!this.isCurrentActive(active)) {
        this.recordCleanupResult(await this.shutdown(active));
        return undefined;
      }

      if (this.hooks.onStarted) {
        await this.hooks.onStarted(client, serverPath);
      }
      if (!this.isCurrentActive(active)) {
        this.recordCleanupResult(await this.shutdown(active));
        return undefined;
      }

      this.transition('running', startGeneration);
      return client;
    } catch (error: unknown) {
      if (active) {
        this.recordCleanupResult(await this.shutdown(active));
      }
      if (!this.isCurrentGeneration(startGeneration)) {
        return undefined;
      }

      this.error = error;
      this.transition('failed', startGeneration);
      this.notifyCallback('failed', this.hooks.onFailed, this.snapshot);
      throw error;
    }
  }

  private async runStop(): Promise<void> {
    const stopGeneration = ++this.generation;
    const active = this.activeClient;
    this.activeClient = undefined;
    this.serverPath = null;
    this.transition('stopping', stopGeneration);

    const cleanup = active
      ? await this.shutdown(active)
      : this.replacementBlockedError !== undefined
        ? { error: this.replacementBlockedError, clientCleanupComplete: false }
        : { error: undefined, clientCleanupComplete: true };
    if (!cleanup.clientCleanupComplete) {
      this.recordCleanupResult(cleanup);
      this.error = cleanup.error ?? this.replacementBlockedFailure();
      this.transition('failed', stopGeneration);
      this.notifyCallback('failed', this.hooks.onFailed, this.snapshot);
    } else {
      this.replacementBlockedError = undefined;
      this.error = undefined;
      this.transition('stopped', stopGeneration);
      this.notifyCallback('stopped', this.hooks.onStopped, this.snapshot);
    }
  }

  private async runRestart(): Promise<TClient | undefined> {
    await this.stop();
    if (this.replacementBlockedError !== undefined) {
      throw this.replacementBlockedFailure();
    }
    return this.beginStart();
  }

  private isCurrentGeneration(generation: number): boolean {
    return this.generation === generation;
  }

  private isCurrentActive(active: ActiveClient<TClient, TEvent>): boolean {
    return this.isCurrentGeneration(active.generation) && this.activeClient === active;
  }

  private transition(nextState: LifecycleState, generation: number): void {
    this.state = nextState;
    this.generation = generation;
    this.notifyCallback('state', this.hooks.onStateChange, this.snapshot);
  }

  private notifyClientState(active: ActiveClient<TClient, TEvent>, event: TEvent): void {
    if (!this.isCurrentActive(active) || !this.hooks.onClientStateChange) {
      return;
    }
    this.notifyCallback('client-state', this.hooks.onClientStateChange, active.client, event);
  }

  private notifyCallback<TArgs extends readonly unknown[]>(
    phase: LifecycleCallbackPhase,
    callback: ((...args: TArgs) => void | Promise<void>) | undefined,
    ...args: TArgs
  ): void {
    if (!callback) {
      return;
    }
    try {
      Promise.resolve(callback(...args)).catch((error: unknown) => {
        this.reportCallbackError(error, phase);
      });
    } catch (error: unknown) {
      this.reportCallbackError(error, phase);
    }
  }

  private reportCallbackError(error: unknown, phase: LifecycleCallbackPhase): void {
    if (!this.hooks.onCallbackError) {
      return;
    }
    try {
      Promise.resolve(this.hooks.onCallbackError(error, phase)).catch(() => undefined);
    } catch {
      // A presentation error must never become an unhandled rejection.
    }
  }

  private shutdown(active: ActiveClient<TClient, TEvent>): Promise<CleanupResult> {
    const existing = this.cleanupPromises.get(active.client);
    if (existing) {
      return existing;
    }

    const promise = this.performShutdown(active);
    this.cleanupPromises.set(active.client, promise);
    return promise;
  }

  private async performShutdown(active: ActiveClient<TClient, TEvent>): Promise<CleanupResult> {
    if (this.activeClient === active) {
      this.activeClient = undefined;
    }

    let firstError: unknown = undefined;
    let clientCleanupComplete = true;
    if (active.listener) {
      try {
        active.listener.dispose();
      } catch (error: unknown) {
        firstError = error;
        clientCleanupComplete = false;
      }
      active.listener = undefined;
    }

    const stopResult = await this.runBounded('stop', () => active.client.stop());
    if (!stopResult.completed) {
      firstError ??= stopResult.error;
      clientCleanupComplete = false;
    }

    const disposeResult = await this.runBounded('dispose', () => active.client.dispose());
    if (!disposeResult.completed) {
      firstError ??= disposeResult.error;
      clientCleanupComplete = false;
    }

    return { error: firstError, clientCleanupComplete };
  }

  private recordCleanupResult(cleanup: CleanupResult): void {
    if (cleanup.clientCleanupComplete) {
      return;
    }
    this.replacementBlockedError =
      cleanup.error ??
      new LanguageClientLifecycleError(
        'Language client cleanup calls did not complete successfully.',
        'cleanup-incomplete',
      );
  }

  private replacementBlockedFailure(): LanguageClientLifecycleError {
    const detail =
      this.replacementBlockedError instanceof Error && this.replacementBlockedError.message
        ? `: ${this.replacementBlockedError.message}`
        : '';
    return new LanguageClientLifecycleError(
      `Language client cleanup is incomplete; replacement startup is blocked${detail}`,
      'cleanup-incomplete',
    );
  }

  private async runBounded(
    operation: string,
    callback: () => void | Promise<void>,
  ): Promise<BoundedOperationResult> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const operationPromise = Promise.resolve().then(callback);
      const timeoutPromise = new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          reject(
            new LanguageClientLifecycleError(
              `Language client ${operation} timed out after ${this.stopTimeoutMs}ms.`,
              'lifecycle',
            ),
          );
        }, this.stopTimeoutMs);
      });
      await Promise.race([operationPromise, timeoutPromise]);
      return { completed: true, error: undefined };
    } catch (error: unknown) {
      return { completed: false, error };
    } finally {
      if (timer) {
        clearTimeout(timer);
      }
    }
  }

  private clearStartPromise(promise: Promise<TClient | undefined>): void {
    if (this.startPromise === promise) {
      this.startPromise = undefined;
    }
  }

  private clearStopPromise(promise: Promise<void>): void {
    if (this.stopPromise === promise) {
      this.stopPromise = undefined;
    }
  }

  private clearRestartPromise(promise: Promise<TClient | undefined>): void {
    if (this.restartPromise === promise) {
      this.restartPromise = undefined;
    }
  }
}
