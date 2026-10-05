import * as vscode from 'vscode';
import {
  syncLanguageClientConfiguration,
  syncLiveLanguageClientConfiguration,
} from '../languageClientConfiguration';

type ConfigurationHandler = (
  params: { items: Array<{ scopeUri?: string; section?: string }> },
  token: unknown,
  next: (...args: unknown[]) => unknown,
) => Promise<unknown[]>;

type CapturedOptions = {
  middleware?: {
    workspace?: {
      configuration?: ConfigurationHandler;
    };
  };
};

const captured: { options?: CapturedOptions | undefined } = {};

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {
    constructor(
      _id: string,
      _name: string,
      _serverOptions: unknown,
      clientOptions: CapturedOptions,
    ) {
      captured.options = clientOptions;
    }

    onNotification() {
      return { dispose: () => undefined };
    }

    setTrace() {
      return Promise.resolve();
    }
  },
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import { createLanguageClient } from '../extension';

/**
 * Install a `perl-lsp` reader whose values genuinely differ per folder.
 *
 * `createLanguageClient` reads configuration while building server options, so
 * this must be in place before the client is constructed.
 */
function installScopedConfiguration(byScope: Record<string, Record<string, unknown>>): void {
  (vscode.workspace.getConfiguration as unknown as jest.Mock).mockImplementation(
    (section?: string, scope?: unknown) => {
      const uri = (scope as { uri?: { toString(): string } } | undefined)?.uri;
      const key = uri ? uri.toString() : scope ? (scope as { toString(): string }).toString() : '';
      const values = section === 'perl-lsp' ? (byScope[key] ?? byScope[''] ?? {}) : {};

      return {
        get: jest.fn((setting: string, defaultValue?: unknown) =>
          setting in values ? values[setting] : defaultValue,
        ),
        has: jest.fn((setting: string) => setting in values),
        inspect: jest.fn((setting: string) =>
          setting in values ? { workspaceFolderValue: values[setting] } : undefined,
        ),
        update: jest.fn(),
      };
    },
  );
}

/**
 * Reachability contract for the folder-owned configuration transport (#14447).
 *
 * `configurationPull.test.ts` proves the resolver answers each folder from its
 * own scope. That proof is worthless if the resolver is never installed on the
 * real client, so this suite executes the actual `createLanguageClient` wiring
 * and drives the captured middleware end to end.
 */
describe('configuration transport wiring (#14447)', () => {
  const FOLDER_A = 'file:///workspace/a';
  const FOLDER_B = 'file:///workspace/b';

  beforeEach(() => {
    captured.options = undefined;
    jest.clearAllMocks();
    installScopedConfiguration({ '': {} });
    Object.assign(vscode.workspace, { workspaceFolders: [{ uri: vscode.Uri.parse(FOLDER_A) }] });
  });

  afterEach(() => {
    Object.assign(vscode.workspace, { workspaceFolders: undefined });
  });

  test('the real client options install a workspace/configuration handler', () => {
    createLanguageClient('/usr/local/bin/perllsp');

    expect(captured.options?.middleware?.workspace?.configuration).toBeInstanceOf(Function);
  });

  test('the installed handler answers folders from the perl-lsp namespace', async () => {
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { includePaths: ['a/lib'] },
      [FOLDER_B]: { includePaths: ['b/lib'] },
    });

    createLanguageClient('/usr/local/bin/perllsp');
    const handler = captured.options?.middleware?.workspace?.configuration;
    expect(handler).toBeInstanceOf(Function);

    const answers = await handler!(
      {
        items: [
          { section: 'perl' },
          { scopeUri: FOLDER_A, section: 'perl' },
          { scopeUri: FOLDER_B, section: 'perl' },
        ],
      },
      undefined,
      jest.fn(),
    );

    // The defect this fixes: every one of these resolved to null, because the
    // client answered `section: "perl"` from the unrelated `perl.*` namespace.
    expect(answers).toHaveLength(3);
    expect(answers[1]).toEqual({ workspace: { includePaths: ['a/lib'] } });
    expect(answers[2]).toEqual({ workspace: { includePaths: ['b/lib'] } });
  });

  test('startup and a live change keep two roots on their own include paths', async () => {
    Object.assign(vscode.workspace, {
      workspaceFolders: [{ uri: vscode.Uri.parse(FOLDER_A) }, { uri: vscode.Uri.parse(FOLDER_B) }],
    });
    installScopedConfiguration({
      '': { includePaths: ['a/lib'] },
      [FOLDER_A]: { includePaths: ['a/lib'] },
      [FOLDER_B]: { includePaths: ['b/lib'] },
    });
    createLanguageClient('/usr/local/bin/perllsp');
    const handler = captured.options?.middleware?.workspace?.configuration;
    expect(handler).toBeInstanceOf(Function);
    const sendNotification = jest.fn(async () => undefined);

    await syncLanguageClientConfiguration({ sendNotification });
    expect(sendNotification).toHaveBeenCalledTimes(1);
    const startupPush = (
      sendNotification.mock.calls as unknown as Array<[string, unknown]>
    )[0]?.[1];
    expect(JSON.stringify(startupPush)).not.toContain('a/lib');
    expect(JSON.stringify(startupPush)).not.toContain('b/lib');

    const items = [
      { section: 'perl' },
      { scopeUri: FOLDER_A, section: 'perl' },
      { scopeUri: FOLDER_B, section: 'perl' },
    ];
    // The unscoped slot is answered from user/global state only (#17334): the
    // workspace-level `''` value must not ride the session-global layer over
    // both folders; each folder slot carries its own value.
    expect(await handler!({ items }, undefined, jest.fn())).toEqual([
      {},
      { workspace: { includePaths: ['a/lib'] } },
      { workspace: { includePaths: ['b/lib'] } },
    ]);

    installScopedConfiguration({
      '': { includePaths: ['a/lib'] },
      [FOLDER_A]: { includePaths: ['a/new-lib'] },
      [FOLDER_B]: { includePaths: ['b/lib'] },
    });
    await syncLiveLanguageClientConfiguration(
      { sendNotification },
      { affectsConfiguration: (key) => key === 'perl-lsp.includePaths' },
    );
    expect(sendNotification).toHaveBeenNthCalledWith(2, 'workspace/didChangeConfiguration', {
      settings: null,
    });
    expect(await handler!({ items }, undefined, jest.fn())).toEqual([
      {},
      { workspace: { includePaths: ['a/new-lib'] } },
      { workspace: { includePaths: ['b/lib'] } },
    ]);
  });

  test('single-root startup and live changes still resolve that root', async () => {
    installScopedConfiguration({ '': {}, [FOLDER_A]: { includePaths: ['a/lib'] } });
    createLanguageClient('/usr/local/bin/perllsp');
    const handler = captured.options?.middleware?.workspace?.configuration;
    const sendNotification = jest.fn(async () => undefined);
    await syncLanguageClientConfiguration({ sendNotification });
    expect(
      await handler!({ items: [{ scopeUri: FOLDER_A, section: 'perl' }] }, undefined, jest.fn()),
    ).toEqual([{ workspace: { includePaths: ['a/lib'] } }]);

    installScopedConfiguration({ '': {}, [FOLDER_A]: { includePaths: ['a/new-lib'] } });
    await syncLiveLanguageClientConfiguration(
      { sendNotification },
      { affectsConfiguration: (key) => key === 'perl-lsp.includePaths' },
    );
    expect(
      await handler!({ items: [{ scopeUri: FOLDER_A, section: 'perl' }] }, undefined, jest.fn()),
    ).toEqual([{ workspace: { includePaths: ['a/new-lib'] } }]);
  });
});
