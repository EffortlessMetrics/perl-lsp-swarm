interface PendingWaiter {
  readonly resolve: () => void;
  readonly reject: (error: Error) => void;
  timer?: ReturnType<typeof setTimeout>;
}

function clearPendingTimer(waiter: PendingWaiter): void {
  if (waiter.timer !== undefined) {
    clearTimeout(waiter.timer);
  }
}

/**
 * Tracks the active-document readiness notification for the current client
 * generation. A restart clears prior readiness so callers cannot accidentally
 * use an event from the previous language-server process.
 */
export class ActiveDocumentReadiness {
  private generation = 0;
  private indexReady = false;
  private readyUris = new Set<string>();
  private waiters = new Map<string, Set<PendingWaiter>>();

  public beginGeneration(): number {
    this.generation += 1;
    for (const waiters of this.waiters.values()) {
      for (const waiter of waiters) {
        clearPendingTimer(waiter);
        waiter.reject(new Error('Active-document readiness was superseded by a restart.'));
      }
    }
    this.waiters.clear();
    this.indexReady = false;
    this.readyUris.clear();
    return this.generation;
  }

  public markReady(uri: string, generation = this.generation): void {
    if (generation !== this.generation) {
      return;
    }
    if (!uri) {
      return;
    }

    this.readyUris.add(uri);
    this.resolveWaiters(uri);
  }

  public markIndexReady(generation = this.generation): void {
    if (generation !== this.generation) {
      return;
    }

    this.indexReady = true;
    for (const uri of [...this.waiters.keys()]) {
      this.resolveWaiters(uri);
    }
  }

  private resolveWaiters(uri: string): void {
    const waiters = this.waiters.get(uri);
    if (!waiters) {
      return;
    }

    this.waiters.delete(uri);
    for (const waiter of waiters) {
      clearPendingTimer(waiter);
      waiter.resolve();
    }
  }

  public waitFor(uri: string, timeoutMs: number): Promise<void> {
    if (this.indexReady || this.readyUris.has(uri)) {
      return Promise.resolve();
    }

    return new Promise<void>((resolve, reject) => {
      const waiter: PendingWaiter = { resolve, reject };
      waiter.timer = setTimeout(() => {
        const waiters = this.waiters.get(uri);
        waiters?.delete(waiter);
        if (waiters?.size === 0) {
          this.waiters.delete(uri);
        }
        reject(new Error(`Active document ${uri} was not ready after ${timeoutMs}ms.`));
      }, timeoutMs);
      const waiters = this.waiters.get(uri) ?? new Set<PendingWaiter>();
      waiters.add(waiter);
      this.waiters.set(uri, waiters);
    });
  }
}
