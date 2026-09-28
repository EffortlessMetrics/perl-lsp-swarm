import * as vscode from 'vscode';

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
});
