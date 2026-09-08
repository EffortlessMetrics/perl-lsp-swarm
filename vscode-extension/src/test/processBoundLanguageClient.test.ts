import { describe, expect, jest, test } from '@jest/globals';
import type { ChildProcess } from 'node:child_process';
import type { MessageTransports, ServerOptions } from 'vscode-languageclient/node';

class FakeChild {
  pid = 1234;
  exitCode: number | null = null;
  signalCode: string | null = null;
  private readonly exitListeners: Array<(code: number, signal: string | null) => void> = [];

  once(_event: 'exit', listener: (code: number, signal: string | null) => void): this {
    this.exitListeners.push(listener);
    return this;
  }

  removeListener(_event: 'exit', listener: (code: number, signal: string | null) => void): this {
    const index = this.exitListeners.indexOf(listener);
    if (index >= 0) {
      this.exitListeners.splice(index, 1);
    }
    return this;
  }

  exit(): void {
    this.exitCode = 0;
    for (const listener of [...this.exitListeners]) {
      listener(0, null);
    }
  }
}

const transportsForTest = {
  reader: { dispose: jest.fn() },
  writer: { dispose: jest.fn() },
} as unknown as MessageTransports;

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class MockLanguageClient {
    private _serverProcess: ChildProcess | undefined;

    constructor(
      _id: string,
      _name: string,
      _serverOptions: ServerOptions,
      _clientOptions: unknown,
    ) {}

    stopResult: Promise<void> = Promise.resolve();

    stop(_timeout?: number): Promise<void> {
      return this.stopResult;
    }

    get serverProcess(): ChildProcess | undefined {
      return this._serverProcess;
    }

    protected async createMessageTransports(_encoding: string): Promise<MessageTransports> {
      this._serverProcess = (this as MockLanguageClient & { skipChild?: boolean }).skipChild
        ? undefined
        : (new FakeChild() as unknown as ChildProcess);
      if ((this as MockLanguageClient & { rejectAfterSpawn?: boolean }).rejectAfterSpawn) {
        throw new Error('transport creation failed after spawn');
      }
      return transportsForTest;
    }
  },
}));

import { ProcessBoundLanguageClient } from '../processBoundLanguageClient';
import { awaitServerProcessExit, serverProcessOf } from '../serverProcessTermination';

class TestableProcessBoundLanguageClient extends ProcessBoundLanguageClient {
  async createTransportsForTest(): Promise<MessageTransports> {
    return this.createMessageTransports('utf8');
  }
}

class RejectingProcessBoundLanguageClient extends TestableProcessBoundLanguageClient {
  rejectAfterSpawn = true;
}

class EmptyProcessBoundLanguageClient extends TestableProcessBoundLanguageClient {
  skipChild = true;
  rejectAfterSpawn = true;
}

const helperServerOptions = (): ServerOptions => ({
  run: { command: 'unused', args: [], transport: 0 },
  debug: { command: 'unused', args: [], transport: 0 },
});

describe('process-bound language client', () => {
  test('retains a live child after the library clears its private handle', async () => {
    const client = new TestableProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );

    await client.createTransportsForTest();
    const captured = client.serverProcess;
    expect(captured).toBeDefined();

    (client as unknown as { _serverProcess: ChildProcess | undefined })._serverProcess = undefined;

    expect(serverProcessOf(client)).toBe(captured);
    await expect(awaitServerProcessExit(captured, 10, () => true)).resolves.toBe(false);
  });

  test('the same retained child becomes terminal after its exit event', async () => {
    const client = new TestableProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );
    await client.createTransportsForTest();
    const captured = client.serverProcess;
    expect(captured).toBeDefined();

    (client as unknown as { _serverProcess: ChildProcess | undefined })._serverProcess = undefined;
    (captured as unknown as FakeChild).exit();

    await expect(awaitServerProcessExit(captured, 10, () => true)).resolves.toBe(true);
    expect(serverProcessOf(client)).toBe(captured);
  });

  test('a missing capture remains unavailable after failed transport creation', async () => {
    const client = new EmptyProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );
    await expect(client.createTransportsForTest()).rejects.toThrow(
      'transport creation failed after spawn',
    );

    expect(client.serverProcess).toBeUndefined();
    expect(serverProcessOf(client)).toBeUndefined();
    await expect(awaitServerProcessExit(undefined, 10, () => true)).resolves.toBe(false);
  });

  test('captures the child even when transport creation rejects after spawn', async () => {
    const client = new RejectingProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );

    await expect(client.createTransportsForTest()).rejects.toThrow(
      'transport creation failed after spawn',
    );
    const captured = client.serverProcess;
    expect(captured).toBeDefined();
    (client as unknown as { _serverProcess: ChildProcess | undefined })._serverProcess = undefined;
    expect(serverProcessOf(client)).toBe(captured);
  });

  test('preserves the prior witness when a later transport attempt has no child', async () => {
    const client = new TestableProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );
    await client.createTransportsForTest();
    const captured = client.serverProcess;
    expect(captured).toBeDefined();

    const testState = client as unknown as { skipChild?: boolean; rejectAfterSpawn?: boolean };
    testState.skipChild = true;
    testState.rejectAfterSpawn = true;
    await expect(client.createTransportsForTest()).rejects.toThrow(
      'transport creation failed after spawn',
    );
    expect(serverProcessOf(client)).toBe(captured);
  });

  test('returns the exact stop promise and preserves successful completion', async () => {
    const client = new TestableProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );
    const operation = Promise.resolve();
    (client as unknown as { stopResult: Promise<void> }).stopResult = operation;

    expect(client.stop(17)).toBe(operation);
    await expect(operation).resolves.toBeUndefined();
  });

  test('returns the exact rejected stop promise to the lifecycle caller', async () => {
    const client = new TestableProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );
    const operation = Promise.reject(new Error('cleanup failed'));
    (client as unknown as { stopResult: Promise<void> }).stopResult = operation;

    expect(client.stop()).toBe(operation);
    await expect(operation).rejects.toThrow('cleanup failed');
  });

  test('attaches a fire-and-forget rejection handler without swallowing the returned failure', async () => {
    const client = new TestableProcessBoundLanguageClient(
      'process-bound-test',
      'Process-bound test',
      helperServerOptions(),
      { documentSelector: [] },
    );
    const operation = Promise.reject(new Error('unhandled cleanup failure'));
    (client as unknown as { stopResult: Promise<void> }).stopResult = operation;
    const unhandled: unknown[] = [];
    const onUnhandled = (reason: unknown): void => {
      unhandled.push(reason);
    };
    process.on('unhandledRejection', onUnhandled);

    try {
      void client.stop();
      await new Promise<void>((resolve) => setImmediate(() => resolve()));
      expect(unhandled).toEqual([]);
    } finally {
      process.off('unhandledRejection', onUnhandled);
    }
  });
});
