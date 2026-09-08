import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const mockLanguageClientStart = jest.fn(() => new Promise<void>(() => undefined));
const mockLanguageClientStop = jest.fn(async () => undefined);
const mockLanguageClientDispose = jest.fn(async () => undefined);
const mockLanguageClientSetTrace = jest.fn(async () => undefined);
const mockLanguageClientOnDidChangeState = jest.fn(() => ({ dispose: jest.fn() }));
const mockLanguageClientOnNotification = jest.fn(() => ({ dispose: jest.fn() }));

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: jest.fn().mockImplementation(() => ({
    initializeResult: { capabilities: {} },
    onDidChangeState: mockLanguageClientOnDidChangeState,
    onNotification: mockLanguageClientOnNotification,
    setTrace: mockLanguageClientSetTrace,
    start: mockLanguageClientStart,
    stop: mockLanguageClientStop,
    dispose: mockLanguageClientDispose,
  })),
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import {
  _activationProjectionsClearedForTest,
  _extensionActivationStateForTest,
  activate,
  deactivate,
} from '../extension';

/**
 * Production-path proof that `activate()` runs inside the activation
 * transaction (#7854): a mid-activation failure rolls the attempt back
 * through the landed substrate, while a successful activation commits and
 * hands the same cleanup primitives to `deactivate()`.
 */

interface TrackedDisposable {
  label: string;
  disposable: { dispose: jest.Mock };
}

const tracked: TrackedDisposable[] = [];
const disposedOrder: string[] = [];
const disposedEntries: TrackedDisposable[] = [];

function creationOrder(): string[] {
  return tracked.map((entry) => entry.label);
}

function disposedLabels(): string[] {
  return [...disposedOrder];
}

function resetDisposalTracking(): void {
  disposedOrder.length = 0;
  disposedEntries.length = 0;
}

/** Support surfaces intentionally retained after a failed activation (#7854). */
const RETAINED_LABELS = [
  'cmd:perl-lsp.showWhatsNew',
  'cmd:perl-lsp.openConfigurationGuide',
  'cmd:perl-lsp.checkForUpdate',
  'cmd:perl-lsp.reportIssue',
  // Coexistence status works without a server and never mutates state (#7214).
  'cmd:perl-lsp.showCoexistenceStatus',
];

function wrapDisposableFactory<T extends { dispose: jest.Mock }>(
  original: (...args: unknown[]) => T,
  labelFor: (...args: unknown[]) => string,
): (...args: unknown[]) => T {
  return (...args: unknown[]) => {
    const disposable = original(...args);
    const label = labelFor(...args);
    const entry = { label, disposable };
    tracked.push(entry);
    disposable.dispose.mockImplementation(() => {
      disposedOrder.push(label);
      disposedEntries.push(entry);
    });
    return disposable;
  };
}

/**
 * Wraps one mocked vscode factory so every disposable it returns is tracked
 * in creation order and its dispose calls are recorded in global order.
 *
 * The replacement is itself a `jest.fn`, so tests can still layer
 * `mockImplementationOnce` (for failure injection) on top of the tracking.
 */
function trackDisposableFactory(
  owner: Record<string, unknown>,
  factory: string,
  labelFor: (...args: unknown[]) => string,
): void {
  const original = owner[factory] as unknown as (...args: unknown[]) => { dispose: jest.Mock };
  owner[factory] = jest.fn(wrapDisposableFactory(original, labelFor));
}

beforeAll(() => {
  trackDisposableFactory(
    vscode.commands as unknown as Record<string, unknown>,
    'registerCommand',
    (command) => `cmd:${String(command)}`,
  );
  trackDisposableFactory(
    vscode.window as unknown as Record<string, unknown>,
    'createStatusBarItem',
    () => 'status-bar',
  );
  trackDisposableFactory(
    vscode.window as unknown as Record<string, unknown>,
    'createOutputChannel',
    () => 'output-channel',
  );
  trackDisposableFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onWillSaveTextDocument',
    () => 'watcher:format-on-save',
  );
  trackDisposableFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidCreateFiles',
    () => 'watcher:file-creation',
  );
  trackDisposableFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidChangeTextDocument',
    () => 'watcher:arrow-completion',
  );
  trackDisposableFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidChangeConfiguration',
    () => 'watcher:configuration',
  );
  trackDisposableFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidSaveTextDocument',
    () => 'watcher:pod-save',
  );
  trackDisposableFactory(
    vscode.debug as unknown as Record<string, unknown>,
    'registerDebugConfigurationProvider',
    () => 'debug:configuration-provider',
  );
  trackDisposableFactory(
    vscode.debug as unknown as Record<string, unknown>,
    'registerDebugAdapterDescriptorFactory',
    () => 'debug:descriptor-factory',
  );
  trackDisposableFactory(
    vscode.languages as unknown as Record<string, unknown>,
    'registerDocumentSymbolProvider',
    () => 'provider:gherkin-symbols',
  );
  trackDisposableFactory(
    vscode.languages as unknown as Record<string, unknown>,
    'registerFoldingRangeProvider',
    () => 'provider:gherkin-folding',
  );
  trackDisposableFactory(
    vscode.languages as unknown as Record<string, unknown>,
    'registerDefinitionProvider',
    () => 'provider:gherkin-definition',
  );
  trackDisposableFactory(
    vscode.languages as unknown as Record<string, unknown>,
    'registerCodeActionsProvider',
    () => 'provider:gherkin-code-actions',
  );
});

function makeContext(extensionPath: string): vscode.ExtensionContext {
  const state = {
    get: jest.fn(() => undefined),
    update: jest.fn(async () => undefined),
  };

  return {
    extension: {
      packageJSON: {
        publisher: 'EffortlessMetrics',
        name: 'perl-lsp-rs',
        version: '0.16.0',
      },
    },
    extensionMode: vscode.ExtensionMode.Test,
    extensionPath,
    globalState: state,
    subscriptions: [],
    workspaceState: state,
  } as unknown as vscode.ExtensionContext;
}

describe('transactional production activation (#7854)', () => {
  const originalSkipStartup = process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;

  beforeEach(() => {
    tracked.length = 0;
    resetDisposalTracking();
  });

  afterEach(async () => {
    if (originalSkipStartup === undefined) {
      delete process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP;
    } else {
      process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = originalSkipStartup;
    }

    await deactivate();
    jest.clearAllMocks();
    resetDisposalTracking();
    tracked.length = 0;
  });

  test('a mid-activation failure rolls the attempt back in reverse creation order', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '1';
    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-activation-'));
    (vscode.debug.registerDebugConfigurationProvider as jest.Mock).mockImplementationOnce(() => {
      throw new Error('debugger registration failed');
    });

    const context = makeContext(extensionRoot);
    await expect(activate(context)).rejects.toThrow('debugger registration failed');

    const state = _extensionActivationStateForTest();
    expect(state?.state).toBe('activation_failed');

    // The language-client lifecycle teardown and the demand owner ran through
    // the same rollback, before module projections were cleared.
    expect(state?.lastCleanupReceipt?.cleaned_resources).toContain('language-client-lifecycle');
    expect(state?.lastCleanupReceipt?.terminal_state).toBe('activation_failed');

    // Every mandatory resource created before the failure was disposed, in
    // exact reverse creation order; retained support surfaces (the four
    // support commands and the output channel) were not.
    const disposalOrder = disposedLabels();
    const retained = [...RETAINED_LABELS, 'output-channel'];
    const mandatory = creationOrder().filter((label) => !retained.includes(label));
    expect(disposalOrder).toEqual([...mandatory].reverse());
    for (const label of RETAINED_LABELS) {
      expect(disposalOrder).not.toContain(label);
    }

    // The output channel is a retained support surface: it must stay usable
    // for failure reporting and be handed to the host net instead.
    expect(disposalOrder).not.toContain('output-channel');

    // The host array received exactly the retained surfaces (four support
    // commands plus the output channel); disposed mandatory resources stay
    // out of it.
    expect(context.subscriptions).toHaveLength(RETAINED_LABELS.length + 1);

    // Truth projection: the activation-complete context key was never claimed
    // and was actively reset for the failed attempt.
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      false,
    );
    expect(vscode.commands.executeCommand).not.toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      true,
    );

    // Module-level compatibility projections were cleared from the same
    // authority, so no disposed resource stays reachable and the crash
    // handler cannot adopt an uncommitted attempt. They must clear LAST —
    // after the language-client lifecycle teardown ran — or the teardown
    // would observe cleared projections and leak a partially started client.
    const cleanedResources = state?.lastCleanupReceipt?.cleaned_resources ?? [];
    expect(cleanedResources[cleanedResources.length - 1]).toBe('module-projections');
    expect(cleanedResources.indexOf('module-projections')).toBeGreaterThan(
      cleanedResources.indexOf('language-client-lifecycle'),
    );
    expect(_activationProjectionsClearedForTest()).toBe(true);

    // Deactivation after a rolled-back attempt stays on the fallback path.
    await expect(deactivate()).resolves.toBeUndefined();
  });

  test('the activation API stop seam stays a recoverable language-client shutdown', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '1';
    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-activation-'));
    const context = makeContext(extensionRoot);

    const extensionApi = await activate(context);
    expect(_extensionActivationStateForTest()?.state).toBe('active');

    // The current-source smoke calls api.stop() mid-session ("language client
    // shutdown") and keeps using the extension afterwards, so stop must not
    // perform deactivate()'s terminal teardown of the committed runtime.
    await extensionApi?.stop();

    expect(_extensionActivationStateForTest()?.state).toBe('active');
    expect(disposedLabels()).toEqual([]);
    expect(_activationProjectionsClearedForTest()).toBe(false);
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      true,
    );
    // The shutdown milestone the smoke asserts after stop is recorded by the
    // seam itself, exactly as the pre-transaction deactivate did.
    const metrics = extensionApi?.getLanguageClientStartupMetrics();
    expect((metrics?.milestones as Record<string, unknown> | undefined)?.shutdown).toEqual(
      expect.any(Number),
    );

    // The real terminal path still tears the committed runtime down fully.
    await deactivate();
    expect(disposedLabels()).toEqual([...creationOrder()].reverse());
    expect(_extensionActivationStateForTest()?.lastCleanupReceipt?.terminal_state).toBe(
      'deactivated',
    );
    expect(_activationProjectionsClearedForTest()).toBe(true);
  });

  test('a successful activation commits without extra user-visible effects', async () => {
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '1';
    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-activation-'));
    const context = makeContext(extensionRoot);

    const extensionApi = await activate(context);

    expect(extensionApi).toBeDefined();
    expect(_extensionActivationStateForTest()?.state).toBe('active');

    // Nothing was disposed on the success path: the rollback stack ran zero
    // cleanups.
    expect(disposedLabels()).toEqual([]);

    // Every directly created activation disposable reached the host net at commit —
    // the same array content the pre-transaction code produced by pushing at
    // creation time. HealthWidgetDataSource is an intentional aggregate owner:
    // its child listeners stay out of the host net and are released by its own
    // dispose() path instead.
    const hostArray = context.subscriptions as unknown as { dispose: jest.Mock }[];
    const hostOwned = tracked.filter((entry) => hostArray.includes(entry.disposable));
    const componentOwned = tracked.filter((entry) => !hostArray.includes(entry.disposable));
    for (const entry of hostOwned) {
      expect(hostArray).toContain(entry.disposable);
    }
    expect(hostArray).toHaveLength(hostOwned.length + 3);
    expect(componentOwned.map((entry) => entry.label)).toContain('watcher:file-creation');

    expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      true,
    );
    expect(vscode.commands.executeCommand).not.toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      false,
    );

    // Deactivate releases everything the committed attempt owns, in reverse
    // creation order, through the same cleanup primitives as rollback.
    await deactivate();
    expect(disposedEntries.filter((entry) => hostOwned.includes(entry))).toEqual(
      [...hostOwned].reverse(),
    );
    expect(disposedEntries).toHaveLength(tracked.length);
    for (const entry of tracked) {
      expect(entry.disposable.dispose).toHaveBeenCalledTimes(1);
    }
    expect(_extensionActivationStateForTest()?.lastCleanupReceipt?.terminal_state).toBe(
      'deactivated',
    );
    expect(_extensionActivationStateForTest()?.lastCleanupReceipt?.cleaned_resources).toContain(
      'language-client-lifecycle',
    );
    expect(_activationProjectionsClearedForTest()).toBe(true);

    // A second deactivate is idempotent: the committed runtime guards repeat
    // calls with its receipt.
    await deactivate();
    for (const entry of tracked) {
      expect(entry.disposable.dispose).toHaveBeenCalledTimes(1);
    }
  });

  test('the registered file-creation listener actually populates a created file (#14547)', async () => {
    // The rest of this suite proves the listener was *registered* and is
    // disposed with the attempt. That leaves the callback itself unexercised:
    // replacing the handler body with a no-op kept every other test in the
    // extension suite green, so nothing connected activation to the
    // boilerplate behaviour. This drives the registered callback end to end.
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP = '1';
    const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-activation-'));

    await activate(makeContext(extensionRoot));

    const registrations = (vscode.workspace.onDidCreateFiles as jest.Mock).mock.calls;
    expect(registrations.length).toBeGreaterThan(1);
    const createdFileEvent = { files: [vscode.Uri.file('/ws/lib/Wired.pm')] };
    for (const registration of registrations) {
      const onDidCreateFiles = registration[0] as (
        event: typeof createdFileEvent,
      ) => void | Promise<void>;
      await onDidCreateFiles(createdFileEvent);
    }

    const applyEdit = vscode.workspace.applyEdit as jest.Mock;
    // The exact edit assertion below is also a negative control for replacing
    // the boilerplate handler with a no-op: dispatching every listener must
    // still produce exactly one boilerplate edit.
    expect(applyEdit).toHaveBeenCalledTimes(1);
    const edit = applyEdit.mock.calls[0]?.[0] as {
      inserts: Array<{ uri: { fsPath: string }; newText: string }>;
    };
    expect(edit.inserts.map((insert) => insert.uri.fsPath)).toEqual(['/ws/lib/Wired.pm']);
    expect(edit.inserts[0]?.newText).toContain('package Wired;');
  });
});
