import * as assert from 'assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { ProcessBoundLanguageClient } from '../../processBoundLanguageClient';
import {
  awaitServerProcessExit,
  serverProcessOf,
  type ServerProcessLike,
} from '../../serverProcessTermination';
import { LanguageClientLifecycle } from '../../languageClientLifecycle';

function nodeExecutable(): string {
  const executable = process.env.PERL_LSP_NODE_PATH?.trim();
  assert.ok(executable && path.isAbsolute(executable), 'PERL_LSP_NODE_PATH must be absolute');
  return executable;
}

function writeFixture(directory: string): string {
  const fixture = path.join(directory, 'rejecting-language-server.js');
  fs.writeFileSync(
    fixture,
    [
      "const fs = require('fs');",
      'const control = process.env.PERL_LSP_SETTLEMENT_CONTROL;',
      'let input = Buffer.alloc(0);',
      'function send(message) {',
      '  const body = Buffer.from(JSON.stringify(message));',
      '  process.stdout.write(`Content-Length: ${body.length}\\r\\n\\r\\n`);',
      '  process.stdout.write(body);',
      '}',
      'function handle(message) {',
      "  if (message.method === 'initialize') {",
      '    if (fs.existsSync(`${control}.timeout`)) return;',
      '    if (fs.existsSync(`${control}.allow`)) {',
      "      send({ jsonrpc: '2.0', id: message.id, result: { capabilities: {} } });",
      '      fs.writeFileSync(`${control}.initialized`, String(process.pid));',
      '    } else {',
      "      send({ jsonrpc: '2.0', id: message.id, error: { code: -32000, message: 'intentional initialize rejection' } });",
      '      fs.writeFileSync(`${control}.rejected`, String(process.pid));',
      '    }',
      "  } else if (message.method === 'shutdown') {",
      "    send({ jsonrpc: '2.0', id: message.id, result: null });",
      "  } else if (message.method === 'exit' && fs.existsSync(`${control}.allow`) && !fs.existsSync(`${control}.hold`)) {",
      '    process.exit(0);',
      '  }',
      '}',
      "process.stdin.on('data', (chunk) => {",
      '  input = Buffer.concat([input, chunk]);',
      '  while (true) {',
      "    const headerEnd = input.indexOf('\\r\\n\\r\\n');",
      '    if (headerEnd < 0) break;',
      "    const header = input.subarray(0, headerEnd).toString('ascii');",
      '    const match = /Content-Length: (\\d+)/i.exec(header);',
      '    if (!match) process.exit(2);',
      '    const length = Number(match[1]);',
      '    const start = headerEnd + 4;',
      '    if (input.length < start + length) break;',
      "    const message = JSON.parse(input.subarray(start, start + length).toString('utf8'));",
      '    input = input.subarray(start + length);',
      '    handle(message);',
      '  }',
      '});',
      'setInterval(() => {',
      '  if (fs.existsSync(`${control}.exit`) && !fs.existsSync(`${control}.hold`)) process.exit(0);',
      '}, 10);',
    ].join('\n'),
    'utf8',
  );
  return fixture;
}

async function waitForFile(file: string, timeoutMs = 5_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (fs.existsSync(file)) return;
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  throw new Error(`timed out waiting for ${file}`);
}

function createLifecycle(
  fixture: string,
  control: string,
  startupTimeoutMs = 5_000,
  failAfterStart = false,
): {
  lifecycle: LanguageClientLifecycle<ProcessBoundLanguageClient>;
  created: () => number;
  clients: ProcessBoundLanguageClient[];
  observedExit: (client: ProcessBoundLanguageClient) => Promise<void>;
} {
  let createdCount = 0;
  const clients: ProcessBoundLanguageClient[] = [];
  const observedExit = new Map<ProcessBoundLanguageClient, Promise<void>>();
  const lifecycle = new LanguageClientLifecycle<ProcessBoundLanguageClient>(
    {
      resolveServerPath: async () => fixture,
      createClient: (serverPath) => {
        createdCount += 1;
        const client = new ProcessBoundLanguageClient(
          `settlement-${createdCount}`,
          'Settlement Test',
          {
            run: {
              command: nodeExecutable(),
              args: [serverPath],
              options: {
                env: { ...process.env, PERL_LSP_SETTLEMENT_CONTROL: control },
                detached: true,
              },
            },
            debug: {
              command: nodeExecutable(),
              args: [serverPath],
              options: {
                env: { ...process.env, PERL_LSP_SETTLEMENT_CONTROL: control },
                detached: true,
              },
            },
          },
          {
            documentSelector: [{ scheme: 'file', language: 'perl' }],
            initializationOptions: {},
          },
        );
        clients.push(client);
        return client;
      },
      captureStopWitness: (client) => {
        const witness = serverProcessOf(client);
        if (witness !== undefined) {
          observedExit.set(
            client,
            new Promise<void>((resolve, reject) => {
              if (witness.exitCode !== null || witness.signalCode !== null) {
                resolve();
                return;
              }
              const timer = setTimeout(() => {
                witness.removeListener('exit', onExit);
                reject(
                  new Error(`timed out waiting for child ${String(witness.pid)} to emit exit`),
                );
              }, 5_000);
              const onExit = (): void => {
                clearTimeout(timer);
                resolve();
              };
              witness.once('exit', onExit);
            }),
          );
        }
        return witness;
      },
      isClientTerminal: async (_client, witness) =>
        await awaitServerProcessExit(witness as ServerProcessLike | undefined, 100),
      ...(failAfterStart
        ? {
            onStarted: async () => {
              if (createdCount === 1) {
                throw new Error('intentional onStarted failure');
              }
            },
          }
        : {}),
    },
    { startupTimeoutMs, stopTimeoutMs: 1_000 },
  );
  return {
    lifecycle,
    created: () => createdCount,
    clients,
    observedExit: (client: ProcessBoundLanguageClient): Promise<void> => {
      const promise = observedExit.get(client);
      assert.ok(promise, 'lifecycle must capture an independent child-exit observation');
      return promise;
    },
  };
}

async function releaseClient(
  client: ProcessBoundLanguageClient,
  control: string,
  observedExit: (client: ProcessBoundLanguageClient) => Promise<void>,
): Promise<void> {
  const witness = serverProcessOf(client);
  assert.ok(witness, 'real client must expose its captured server process');
  if (witness.exitCode !== null || witness.signalCode !== null) return;
  const exit = observedExit(client);
  fs.writeFileSync(`${control}.exit`, 'exit', 'utf8');
  await exit;
}

suite('Real language-client process settlement', function () {
  this.timeout(30_000);

  test('initialize rejection remains blocked when upstream cleanup is non-retryable', async function () {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-settlement-'));
    const control = path.join(directory, 'control');
    let clients: ProcessBoundLanguageClient[] = [];
    let observedExit = async (_client: ProcessBoundLanguageClient): Promise<void> => {
      throw new Error('child-exit observer was not initialized');
    };
    try {
      const fixture = writeFixture(directory);
      const createdLifecycle = createLifecycle(fixture, control);
      const { lifecycle, created } = createdLifecycle;
      observedExit = createdLifecycle.observedExit;
      clients = createdLifecycle.clients;
      await assert.rejects(lifecycle.start(), /replacement startup is blocked/);
      await waitForFile(`${control}.rejected`);
      assert.equal(created(), 1);
      await assert.rejects(lifecycle.restart(), /replacement startup is blocked/);
      const firstClient = clients[0];
      assert.ok(firstClient);
      await releaseClient(firstClient, control, observedExit);
      await assert.rejects(lifecycle.restart(), /replacement startup is blocked/);
      assert.equal(created(), 1);
    } finally {
      for (const client of clients) {
        await releaseClient(client, control, observedExit);
      }
      fs.rmSync(directory, { recursive: true, force: true });
    }
  });

  test('a timed-out startup remains blocked after its child later exits', async function () {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-settlement-timeout-'));
    const control = path.join(directory, 'control');
    let clients: ProcessBoundLanguageClient[] = [];
    let observedExit = async (_client: ProcessBoundLanguageClient): Promise<void> => {
      throw new Error('child-exit observer was not initialized');
    };
    try {
      const fixture = writeFixture(directory);
      fs.writeFileSync(`${control}.timeout`, 'timeout', 'utf8');
      const createdLifecycle = createLifecycle(fixture, control, 100);
      const { lifecycle, created } = createdLifecycle;
      observedExit = createdLifecycle.observedExit;
      clients = createdLifecycle.clients;
      await assert.rejects(lifecycle.start(), /replacement startup is blocked/);
      assert.equal(created(), 1);
      await assert.rejects(lifecycle.restart(), /replacement startup is blocked/);
      const firstClient = clients[0];
      assert.ok(firstClient);
      await releaseClient(firstClient, control, observedExit);
      await assert.rejects(lifecycle.restart(), /replacement startup is blocked/);
      assert.equal(created(), 1);
    } finally {
      for (const client of clients) {
        await releaseClient(client, control, observedExit);
      }
      fs.rmSync(directory, { recursive: true, force: true });
    }
  });

  test('an onStarted failure admits one replacement after the real child exits', async function () {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-settlement-on-started-'));
    const control = path.join(directory, 'control');
    let clients: ProcessBoundLanguageClient[] = [];
    let observedExit = async (_client: ProcessBoundLanguageClient): Promise<void> => {
      throw new Error('child-exit observer was not initialized');
    };
    try {
      const fixture = writeFixture(directory);
      fs.writeFileSync(`${control}.allow`, 'allow', 'utf8');
      fs.writeFileSync(`${control}.hold`, 'hold', 'utf8');
      const createdLifecycle = createLifecycle(fixture, control, 5_000, true);
      const { lifecycle, created } = createdLifecycle;
      observedExit = createdLifecycle.observedExit;
      clients = createdLifecycle.clients;
      await assert.rejects(lifecycle.start(), /replacement startup is blocked/);
      await waitForFile(`${control}.initialized`);
      assert.equal(created(), 1);
      await assert.rejects(lifecycle.restart(), /replacement startup is blocked/);
      assert.equal(created(), 1);

      const firstClient = clients[0];
      assert.ok(firstClient);
      const firstWitness = serverProcessOf(firstClient);
      assert.ok(firstWitness);
      const exit = observedExit(firstClient);
      fs.rmSync(`${control}.hold`, { force: true });
      fs.writeFileSync(`${control}.exit`, 'exit', 'utf8');
      await exit;
      fs.rmSync(`${control}.exit`, { force: true });
      const replacement = await lifecycle.restart();
      assert.ok(replacement);
      await waitForFile(`${control}.initialized`);
      assert.equal(created(), 2);
      await lifecycle.stop();
      assert.equal(lifecycle.snapshot.state, 'stopped');
    } finally {
      fs.rmSync(`${control}.hold`, { force: true });
      for (const client of clients) {
        await releaseClient(client, control, observedExit);
      }
      fs.rmSync(directory, { recursive: true, force: true });
    }
  });
});
