import { describe, expect, jest, test } from '@jest/globals';
import type { ChildProcess } from 'node:child_process';
import type { MessageTransports, ServerOptions } from 'vscode-languageclient/node';

const childProcess = {
  pid: 1234,
  exitCode: null,
  signalCode: null,
  once: jest.fn(),
  removeListener: jest.fn(),
} as unknown as ChildProcess;
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

    get serverProcess(): ChildProcess | undefined {
      return this._serverProcess;
    }

    protected async createMessageTransports(_encoding: string): Promise<MessageTransports> {
      this._serverProcess = childProcess;
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

const helperServerOptions = (): ServerOptions => ({
  run: { command: 'unused', args: [], transport: 0 },
  debug: { command: 'unused', args: [], transport: 0 },
});

describe('process-bound language client', () => {
  test('retains the spawned child after the library clears its private handle', async () => {
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
    expect(serverProcessOf(client)).toBe(captured);
  });
});
