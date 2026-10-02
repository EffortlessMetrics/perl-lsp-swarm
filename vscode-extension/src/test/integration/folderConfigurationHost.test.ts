import * as assert from 'assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import { LanguageClient } from 'vscode-languageclient/node';
import { perlConfigurationMiddleware } from '../../configurationPull';
import {
  syncLanguageClientConfiguration,
  syncLiveLanguageClientConfiguration,
} from '../../languageClientConfiguration';

type WireEvent = { kind: 'push' | 'pull'; value: unknown };

function fixture(directory: string): string {
  const script = path.join(directory, 'configuration-server.js');
  fs.writeFileSync(
    script,
    [
      "const fs = require('fs');",
      'const log = process.env.PERL_LSP_FOLDER_CONFIG_LOG;',
      'const folders = JSON.parse(process.env.PERL_LSP_FOLDER_CONFIG_URIS);',
      'let input = Buffer.alloc(0);',
      'let nextId = 10;',
      'function record(kind, value) { fs.appendFileSync(log, JSON.stringify({ kind, value }) + "\\n"); }',
      'function send(message) {',
      '  const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...message }));',
      '  process.stdout.write(`Content-Length: ${body.length}\\r\\n\\r\\n`);',
      '  process.stdout.write(body);',
      '}',
      'function pull() {',
      '  send({ id: nextId++, method: "workspace/configuration", params: { items: [',
      '    { section: "perl" }, ...folders.map(scopeUri => ({ scopeUri, section: "perl" }))',
      '  ] } });',
      '}',
      'function handle(message) {',
      '  if (message.method === "initialize") {',
      '    send({ id: message.id, result: { capabilities: {} } });',
      '  } else if (message.method === "initialized") {',
      '    pull();',
      '  } else if (message.method === "workspace/didChangeConfiguration") {',
      '    record("push", message.params);',
      '    pull();',
      '  } else if (message.id >= 10 && Object.hasOwn(message, "result")) {',
      '    record("pull", message.result);',
      '  } else if (message.method === "shutdown") {',
      '    send({ id: message.id, result: null });',
      '  } else if (message.method === "exit") {',
      '    process.exit(0);',
      '  }',
      '}',
      "process.stdin.on('data', chunk => {",
      '  input = Buffer.concat([input, chunk]);',
      '  while (true) {',
      '    const headerEnd = input.indexOf("\\r\\n\\r\\n");',
      '    if (headerEnd < 0) break;',
      '    const header = input.subarray(0, headerEnd).toString("ascii");',
      '    const match = /Content-Length: (\\d+)/i.exec(header);',
      '    if (!match) process.exit(2);',
      '    const length = Number(match[1]);',
      '    const start = headerEnd + 4;',
      '    if (input.length < start + length) break;',
      '    const message = JSON.parse(input.subarray(start, start + length).toString("utf8"));',
      '    input = input.subarray(start + length);',
      '    handle(message);',
      '  }',
      '});',
    ].join('\n'),
  );
  return script;
}

function events(log: string): WireEvent[] {
  if (!fs.existsSync(log)) return [];
  return fs
    .readFileSync(log, 'utf8')
    .trim()
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line) as WireEvent);
}

async function waitForEvents(log: string, count: number): Promise<WireEvent[]> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const observed = events(log);
    if (observed.length >= count) return observed;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Expected ${count} wire events, saw ${JSON.stringify(events(log))}`);
}

suite('Folder configuration in the VS Code host', function () {
  this.timeout(40_000);

  test('real client keeps contradictory roots separate through startup and live pull', async function () {
    const folders = vscode.workspace.workspaceFolders;
    if (folders?.length !== 2) this.skip();
    const [a, b] = folders!;
    assert.ok(a && b);
    const aSettings = vscode.workspace.getConfiguration('perl-lsp', a.uri);
    const bSettings = vscode.workspace.getConfiguration('perl-lsp', b.uri);
    const previousA = aSettings.inspect<string[]>('includePaths')?.workspaceFolderValue;
    const previousB = bSettings.inspect<string[]>('includePaths')?.workspaceFolderValue;
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-folder-config-'));
    const log = path.join(temp, 'wire.jsonl');
    const script = fixture(temp);
    const node = process.env.PERL_LSP_NODE_PATH;
    assert.ok(node && path.isAbsolute(node), 'explicit Node executable required');
    const client = new LanguageClient(
      'perl-lsp-folder-config-host',
      'Perl folder configuration host test',
      {
        command: node,
        args: [script],
        options: {
          env: {
            ...process.env,
            PERL_LSP_FOLDER_CONFIG_LOG: log,
            PERL_LSP_FOLDER_CONFIG_URIS: JSON.stringify([a.uri.toString(), b.uri.toString()]),
          },
        },
      },
      {
        documentSelector: [{ scheme: 'file', language: 'perl' }],
        middleware: { workspace: { configuration: perlConfigurationMiddleware() } },
      },
    );

    try {
      await aSettings.update('includePaths', ['a/lib'], vscode.ConfigurationTarget.WorkspaceFolder);
      await bSettings.update('includePaths', ['b/lib'], vscode.ConfigurationTarget.WorkspaceFolder);
      await client.start();
      const initial = await waitForEvents(log, 1);
      assert.deepEqual(initial[0], {
        kind: 'pull',
        value: [
          {},
          { workspace: { includePaths: ['a/lib'] } },
          { workspace: { includePaths: ['b/lib'] } },
        ],
      });

      await syncLanguageClientConfiguration(client);
      const startup = await waitForEvents(log, 3);
      assert.equal(startup[1]?.kind, 'push');
      assert.doesNotMatch(JSON.stringify(startup[1]?.value), /a\/lib|b\/lib/);
      assert.deepEqual(startup[2], initial[0]);

      await aSettings.update(
        'includePaths',
        ['a/new-lib'],
        vscode.ConfigurationTarget.WorkspaceFolder,
      );
      await syncLiveLanguageClientConfiguration(client, {
        affectsConfiguration: (key) => key === 'perl-lsp.includePaths',
      });
      const changed = await waitForEvents(log, 5);
      assert.deepEqual(changed[3], { kind: 'push', value: { settings: null } });
      assert.deepEqual(changed[4], {
        kind: 'pull',
        value: [
          {},
          { workspace: { includePaths: ['a/new-lib'] } },
          { workspace: { includePaths: ['b/lib'] } },
        ],
      });
    } finally {
      try {
        await client.stop();
      } finally {
        try {
          await Promise.all([
            aSettings.update('includePaths', previousA, vscode.ConfigurationTarget.WorkspaceFolder),
            bSettings.update('includePaths', previousB, vscode.ConfigurationTarget.WorkspaceFolder),
          ]);
        } finally {
          fs.rmSync(temp, { recursive: true, force: true });
        }
      }
    }
  });

  test('real client keeps one root configured through startup and live pull', async function () {
    const folders = vscode.workspace.workspaceFolders;
    if (folders?.length !== 1) this.skip();
    const root = folders?.[0];
    assert.ok(root);
    const settings = vscode.workspace.getConfiguration('perl-lsp', root.uri);
    const previous = settings.inspect<string[]>('includePaths')?.workspaceFolderValue;
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-one-folder-config-'));
    const log = path.join(temp, 'wire.jsonl');
    const script = fixture(temp);
    const node = process.env.PERL_LSP_NODE_PATH;
    assert.ok(node && path.isAbsolute(node));
    const client = new LanguageClient(
      'perl-lsp-one-folder-config-host',
      'Perl one-folder configuration host test',
      {
        command: node,
        args: [script],
        options: {
          env: {
            ...process.env,
            PERL_LSP_FOLDER_CONFIG_LOG: log,
            PERL_LSP_FOLDER_CONFIG_URIS: JSON.stringify([root.uri.toString()]),
          },
        },
      },
      {
        documentSelector: [{ scheme: 'file', language: 'perl' }],
        middleware: { workspace: { configuration: perlConfigurationMiddleware() } },
      },
    );
    try {
      await settings.update(
        'includePaths',
        ['one/lib'],
        vscode.ConfigurationTarget.WorkspaceFolder,
      );
      await client.start();
      assert.deepEqual((await waitForEvents(log, 1))[0], {
        kind: 'pull',
        value: [{}, { workspace: { includePaths: ['one/lib'] } }],
      });
      await syncLanguageClientConfiguration(client);
      const startup = await waitForEvents(log, 3);
      assert.doesNotMatch(JSON.stringify(startup[1]?.value), /one\/lib/);
      assert.deepEqual(startup[2], startup[0]);
      await settings.update(
        'includePaths',
        ['one/new-lib'],
        vscode.ConfigurationTarget.WorkspaceFolder,
      );
      await syncLiveLanguageClientConfiguration(client, {
        affectsConfiguration: (key) => key === 'perl-lsp.includePaths',
      });
      const changed = await waitForEvents(log, 5);
      assert.deepEqual(changed[3], { kind: 'push', value: { settings: null } });
      assert.deepEqual(changed[4], {
        kind: 'pull',
        value: [{}, { workspace: { includePaths: ['one/new-lib'] } }],
      });
    } finally {
      try {
        await client.stop();
      } finally {
        try {
          await settings.update(
            'includePaths',
            previous,
            vscode.ConfigurationTarget.WorkspaceFolder,
          );
        } finally {
          fs.rmSync(temp, { recursive: true, force: true });
        }
      }
    }
  });

  test('standalone-file mode retains unscoped startup and live include paths', async function () {
    if (vscode.workspace.workspaceFolders?.length) this.skip();
    const settings = vscode.workspace.getConfiguration('perl-lsp');
    const previous = settings.inspect<string[]>('includePaths')?.globalValue;
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-standalone-config-'));
    const log = path.join(temp, 'wire.jsonl');
    const script = fixture(temp);
    const node = process.env.PERL_LSP_NODE_PATH;
    assert.ok(node && path.isAbsolute(node));
    const client = new LanguageClient(
      'perl-lsp-standalone-config-host',
      'Perl standalone configuration host test',
      {
        command: node,
        args: [script],
        options: {
          env: {
            ...process.env,
            PERL_LSP_FOLDER_CONFIG_LOG: log,
            PERL_LSP_FOLDER_CONFIG_URIS: '[]',
          },
        },
      },
      {
        documentSelector: [{ scheme: 'file', language: 'perl' }],
        middleware: { workspace: { configuration: perlConfigurationMiddleware() } },
      },
    );
    try {
      await settings.update('includePaths', ['standalone/lib'], vscode.ConfigurationTarget.Global);
      await client.start();
      await waitForEvents(log, 1);
      await syncLanguageClientConfiguration(client);
      const startup = await waitForEvents(log, 3);
      assert.deepEqual(
        (startup[1]?.value as { settings: { perl: { workspace: unknown } } }).settings.perl
          .workspace,
        { includePaths: ['standalone/lib'] },
      );

      await settings.update(
        'includePaths',
        ['standalone/new-lib'],
        vscode.ConfigurationTarget.Global,
      );
      await syncLiveLanguageClientConfiguration(client, {
        affectsConfiguration: (key) => key === 'perl-lsp.includePaths',
      });
      const changed = await waitForEvents(log, 5);
      assert.deepEqual(
        (changed[3]?.value as { settings: { perl: { workspace: unknown } } }).settings.perl
          .workspace,
        { includePaths: ['standalone/new-lib'] },
      );
    } finally {
      try {
        await client.stop();
      } finally {
        try {
          await settings.update('includePaths', previous, vscode.ConfigurationTarget.Global);
        } finally {
          fs.rmSync(temp, { recursive: true, force: true });
        }
      }
    }
  });
});
