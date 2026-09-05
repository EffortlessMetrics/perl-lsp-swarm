import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const mockLanguageClientCtor = jest.fn(() => ({
  initializeResult: { capabilities: {} },
  onDidChangeState: jest.fn(() => ({ dispose: jest.fn() })),
  onNotification: jest.fn(() => ({ dispose: jest.fn() })),
  setTrace: jest.fn(async () => undefined),
  start: jest.fn(async () => undefined),
  stop: jest.fn(async () => undefined),
  dispose: jest.fn(async () => undefined),
}));

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: mockLanguageClientCtor,
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import {
  _setActivationPhaseFailureInjectorForTest,
  type ActivationPhaseBoundary,
} from '../activationOwner';
import {
  _activationProjectionsClearedForTest,
  _autoRestartAttemptsForTest,
  _extensionActivationStateForTest,
  activate,
  deactivate,
} from '../extension';
import { fakeDocument, setOpenDocuments } from './serverDemandDocuments';

/**
 * Production-path fault injection for #7855 (built on the #7854 transactional
 * owner wired by #12123 and the substrate matrix from #7955).
 *
 * The substrate matrix proves the rollback mechanism on synthetic
 * `ActivationTransaction`s. This suite injects deterministic failures into the
 * FULL production `activate()` composition — through the test-only
 * phase-boundary injector on `ExtensionActivationOwner`, which fails the
 * attempt immediately after a named production resource boundary completes —
 * and proves, per boundary:
 *
 * - activation rejects and settles as `activation_failed`, never success, and
 *   `perl-lsp.activated` is never claimed (then actively reset);
 * - every resource registered before the boundary is cleaned in exact reverse
 *   registration order, with module projections cleared last;
 * - exactly the approved support surfaces (the output channel plus the four
 *   support commands) survive, undisposed, handed to the host net;
 * - a retry in the same host process starts a fresh attempt from baseline and
 *   can succeed without duplicate resources, while the failed attempt's
 *   callbacks cannot reach the new runtime (the host bus drops disposed
 *   listeners, and disposed command registrations are unregistered);
 * - `deactivate()` on the partial state stays on the pre-transaction fallback
 *   path and is idempotent;
 * - one failing cleanup cannot prevent the remaining rollback, and its failure
 *   is recorded as a bounded receipt entry;
 * - a workspace event delivered mid-rollback cannot corrupt the receipt;
 * - no pre-commit path can enter mid-session crash recovery: no client is
 *   ever constructed or started before commit in the demand-driven
 *   composition, and the failed attempt leaves no live listener behind.
 */

/** Support surfaces intentionally retained after a failed activation (#7854). */
const RETAINED_LABELS = [
  'cmd:perl-lsp.showWhatsNew',
  'cmd:perl-lsp.openConfigurationGuide',
  'cmd:perl-lsp.checkForUpdate',
  'cmd:perl-lsp.reportIssue',
  // Coexistence status works without a server and never mutates state (#7214).
  'cmd:perl-lsp.showCoexistenceStatus',
  'output-channel',
];

/** Named production boundaries the mandatory failure matrix fails after. */
const MATRIX_NAMES = [
  'base context projection',
  'base output channel',
  'base status/health data source',
  'first server command group',
  'all command groups',
  'first workspace/config listener',
  'language client lifecycle',
  'language client demand owner',
  'document/POD/Gherkin providers (optional)',
  'debugger registrations',
  'onboarding support commands (retained prefix)',
  'retained support surfaces',
  'final pre-commit demand listeners',
] as const;

interface TrackedDisposable {
  label: string;
  disposable: { dispose: jest.Mock };
}

interface BusEntry {
  label: string;
  base: string;
  listener: (...args: unknown[]) => void;
  live: boolean;
  invokedCount: number;
  disposable: { dispose: jest.Mock };
}

const tracked: TrackedDisposable[] = [];
const createdOrder: string[] = [];
const disposedOrder: string[] = [];
const busEntries: BusEntry[] = [];
const createdRoots: string[] = [];

function creationOrder(): string[] {
  return [...createdOrder];
}

function disposedLabels(): string[] {
  return [...disposedOrder];
}

/**
 * Wrap one mocked vscode disposable factory so every disposable it returns is
 * tracked in creation order and its dispose calls are recorded globally. The
 * replacement stays a `jest.fn`, so tests can layer `mockImplementationOnce`
 * (for dispose failures) on top of the tracking.
 */
function trackDisposableFactory(
  owner: Record<string, unknown>,
  factory: string,
  labelFor: (...args: unknown[]) => string,
): void {
  const original = owner[factory] as unknown as (...args: unknown[]) => { dispose: jest.Mock };
  owner[factory] = jest.fn((...args: unknown[]) => {
    const disposable = original(...args);
    const label = labelFor(...args);
    // Chain the host's own dispose semantics (for example command
    // unregistration) in front of the tracking, so disposal keeps the real
    // extension-host contract.
    const hostDisposal = disposable.dispose.getMockImplementation() ?? (() => undefined);
    tracked.push({ label, disposable });
    createdOrder.push(label);
    disposable.dispose.mockImplementation(() => {
      hostDisposal();
      disposedOrder.push(label);
    });
    return disposable;
  });
}

/**
 * Wrap one mocked vscode event factory with a host-faithful bus: registered
 * listeners are recorded, disposal unsubscribes (a real host never delivers an
 * event to a disposed listener), and `fireHostEvent` dispatches only to live
 * listeners while recording which entries ran.
 */
function trackEventFactory(owner: Record<string, unknown>, factory: string, label: string): void {
  owner[factory] = jest.fn((listener: (...args: unknown[]) => void) => {
    // Registrations under one factory get unique ledger labels (a second
    // registration of the same event — for example the demand-created test
    // adapter's save listener — must be distinguishable from the first), while
    // event dispatch and lookups stay addressed by the base label.
    const base = label;
    const occurrence = allEntries(base).length + 1;
    const uniqueLabel = `${base}#${occurrence}`;
    const entry: BusEntry = {
      label: uniqueLabel,
      base,
      listener,
      live: true,
      invokedCount: 0,
      disposable: null as unknown as BusEntry['disposable'],
    };
    busEntries.push(entry);
    createdOrder.push(uniqueLabel);
    entry.disposable = {
      dispose: jest.fn(() => {
        // Host semantics: remove the listener before running dispose effects,
        // so an event fired during rollback cannot reach a listener that is
        // currently being torn down.
        entry.live = false;
        disposedOrder.push(uniqueLabel);
      }),
    };
    return entry.disposable;
  });
}

/** Dispatch one host event to every live listener registered under `label`. */
function fireHostEvent(base: string, ...args: unknown[]): void {
  for (const entry of busEntries.filter((candidate) => candidate.base === base && candidate.live)) {
    entry.invokedCount += 1;
    entry.listener(...args);
  }
}

function liveEntries(base: string): BusEntry[] {
  return busEntries.filter((entry) => entry.base === base && entry.live);
}

function allEntries(base: string): BusEntry[] {
  return busEntries.filter((entry) => entry.base === base);
}

/**
 * Arm one one-shot dispose behavior on the most recent registration under
 * `label`, preserving the host semantics (bus unsubscribe, disposal order)
 * that the real dispose would have performed. `behavior` runs after them;
 * when it throws, the rollback records a bounded cleanup failure.
 */
function armBusDisposeHook(base: string, behavior: () => void): void {
  const entries = allEntries(base);
  const entry = entries[entries.length - 1];
  if (!entry) {
    throw new Error(`no bus entry armed for ${base}`);
  }
  entry.disposable.dispose.mockImplementationOnce(() => {
    entry.live = false;
    disposedOrder.push(entry.label);
    behavior();
  });
}

function registeredCommandIds(): string[] {
  const vscodeMock = vscode as unknown as {
    _registeredCommandsForTest?: () => string[];
  };
  return vscodeMock._registeredCommandsForTest?.() ?? [];
}

function registeredCommandEntries(): Map<string, unknown> {
  const vscodeMock = vscode as unknown as {
    _registeredCommandEntriesForTest?: () => Map<string, unknown>;
  };
  return vscodeMock._registeredCommandEntriesForTest?.() ?? new Map();
}

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
        version: '0.17.0',
      },
    },
    extensionMode: vscode.ExtensionMode.Production,
    extensionPath,
    globalState: state,
    subscriptions: [],
    workspaceState: state,
  } as unknown as vscode.ExtensionContext;
}

function mockConfig(serverPath: string): void {
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((key: string, defaultValue?: unknown) => {
      if (key === 'serverPath') {
        return serverPath;
      }
      if (key === 'autoDownload') {
        return false;
      }
      return defaultValue;
    }),
    has: jest.fn(() => false),
    inspect: jest.fn(),
    update: jest.fn(async () => undefined),
  }));
}

function makeExtensionRoot(): string {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-inject-'));
  createdRoots.push(extensionRoot);
  const serverPath = path.join(
    extensionRoot,
    process.platform === 'win32' ? 'perl-lsp.exe' : 'perl-lsp',
  );
  fs.writeFileSync(serverPath, '');
  mockConfig(serverPath);
  return extensionRoot;
}

/** Drain pending microtasks so absence assertions are meaningful. */
async function settle(): Promise<void> {
  for (let index = 0; index < 20; index += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function waitUntil(condition: () => boolean, timeoutMs = 2_000): Promise<void> {
  const startedAt = Date.now();
  while (!condition()) {
    if (Date.now() - startedAt > timeoutMs) {
      throw new Error('condition not met before timeout');
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

/**
 * The number of language clients the mock constructed whose `start` was
 * invoked — one per real server start attempt.
 */
function startedClientCount(): number {
  return mockLanguageClientCtor.mock.results.filter(
    (result) => result.value.start.mock.calls.length > 0,
  ).length;
}

/**
 * Boundaries one clean production activation crosses, recorded once in
 * `beforeAll` so the failure matrix derives its targets from the observed
 * composition instead of hardcoded registration counts.
 */
const recordedBoundaries: ActivationPhaseBoundary[] = [];
/** Command registrations one clean activation performs (duplicate-resource bar). */
let baselineCommandRegistrations = 0;
/** Resource ids the production composition classifies as retained support. */
let retainedResourceIds = new Set<string>();

function boundariesOfPhase(phase: string): ActivationPhaseBoundary[] {
  return recordedBoundaries.filter((boundary) => boundary.phase === phase);
}

function nthOfPhase(phase: string, ordinal: number): ActivationPhaseBoundary {
  const boundary = boundariesOfPhase(phase).find((candidate) => candidate.ordinal === ordinal);
  if (!boundary) {
    throw new Error(`no recorded boundary for phase ${phase} ordinal ${ordinal}`);
  }
  return boundary;
}

function lastOfPhase(phase: string): ActivationPhaseBoundary {
  const boundaries = boundariesOfPhase(phase);
  const boundary = boundaries[boundaries.length - 1];
  if (!boundary) {
    throw new Error(`no recorded boundary for phase ${phase}`);
  }
  return boundary;
}

function lastBoundaryOverall(): ActivationPhaseBoundary {
  const boundary = recordedBoundaries[recordedBoundaries.length - 1];
  if (!boundary) {
    throw new Error('no recorded boundaries');
  }
  return boundary;
}

function boundaryByResourceId(resourceId: string): ActivationPhaseBoundary {
  const boundary = recordedBoundaries.find((candidate) => candidate.resource_id === resourceId);
  if (!boundary) {
    throw new Error(`no recorded boundary with resource id ${resourceId}`);
  }
  return boundary;
}

function resolveMatrixTarget(name: (typeof MATRIX_NAMES)[number]): ActivationPhaseBoundary {
  switch (name) {
    case 'base context projection':
      return boundaryByResourceId('module-projections');
    case 'base output channel':
      // The output channel is the base phase's second registration, right
      // after the module projections.
      return nthOfPhase('base', 2);
    case 'base status/health data source':
      return lastOfPhase('base');
    case 'first server command group':
      // Command groups register per item after the whole group was created,
      // so a real host failure can only strike inside the group's vscode
      // calls — before any item is owned. The matrix therefore fails after
      // batch-complete boundaries; failing mid-batch would observe unowned
      // sibling disposables no production exception can produce. The first
      // server group registers five commands (showOutput, reinstall,
      // restart, showBinaryIdentity, runHealthCheck), so its last item is the
      // commands phase's fifth registration.
      return nthOfPhase('commands', 5);
    case 'all command groups':
      return lastOfPhase('commands');
    case 'first workspace/config listener':
      return nthOfPhase('workspace_listeners', 1);
    case 'language client lifecycle':
      return boundaryByResourceId('language-client-lifecycle');
    case 'language client demand owner':
      return lastOfPhase('language_client');
    case 'document/POD/Gherkin providers (optional)':
      return lastOfPhase('document_providers');
    case 'debugger registrations':
      return lastOfPhase('debugger');
    case 'onboarding support commands (retained prefix)':
      // The onboarding group (showWhatsNew, openConfigurationGuide,
      // checkForUpdate) is the first retained support batch; reportIssue has
      // not registered yet, so exactly the retained prefix crossed.
      return nthOfPhase('support', 3);
    case 'retained support surfaces':
      return lastOfPhase('support');
    case 'final pre-commit demand listeners':
      return lastBoundaryOverall();
  }
  // Exhaustiveness guard without a default case: adding a matrix name
  // without a resolver is a compile-time error, not a runtime one.
  const exhaustive: never = name;
  throw new Error(`unknown matrix target: ${String(exhaustive)}`);
}

interface FailedAttempt {
  context: vscode.ExtensionContext;
  seen: ActivationPhaseBoundary[];
  receipt: NonNullable<ReturnType<typeof _extensionActivationStateForTest>>['lastCleanupReceipt'];
  attemptId: string;
}

/**
 * Run one production activation that fails deterministically right after
 * `target` completes, and return the settled failed attempt. `onBoundary` runs
 * inside the injector before the failure decision, so tests can arm
 * dispose-time behavior on already-created disposables.
 */
async function activateWithInjectedFailure(
  target: ActivationPhaseBoundary,
  onBoundary?: (boundary: ActivationPhaseBoundary) => void,
): Promise<FailedAttempt> {
  const seen: ActivationPhaseBoundary[] = [];
  _setActivationPhaseFailureInjectorForTest((boundary) => {
    seen.push(boundary);
    onBoundary?.(boundary);
    // Match by ledger id, not object identity: targets come from the recorded
    // baseline pass, while each run creates fresh boundary objects.
    return boundary.resource_id === target.resource_id
      ? new Error(`injected failure after ${target.resource_id}`)
      : null;
  });
  const context = makeContext(makeExtensionRoot());
  await expect(activate(context)).rejects.toThrow(`injected failure after ${target.resource_id}`);
  _setActivationPhaseFailureInjectorForTest(null);

  const state = _extensionActivationStateForTest();
  expect(state?.state).toBe('activation_failed');
  return {
    context,
    seen,
    receipt: state?.lastCleanupReceipt ?? null,
    attemptId: state?.attemptId ?? 'unknown',
  };
}

/**
 * Run one production activation whose failure comes from OUTSIDE the
 * injector (for example a vscode factory mock), with a purely recording
 * injector so the boundaries the attempt crossed before the failure are
 * still captured.
 */
async function activateWithRecordingBoundaries(expectedError: string): Promise<FailedAttempt> {
  const seen: ActivationPhaseBoundary[] = [];
  _setActivationPhaseFailureInjectorForTest((boundary) => {
    seen.push(boundary);
    return null;
  });
  const context = makeContext(makeExtensionRoot());
  await expect(activate(context)).rejects.toThrow(expectedError);
  _setActivationPhaseFailureInjectorForTest(null);
  const state = _extensionActivationStateForTest();
  expect(state?.state).toBe('activation_failed');
  return {
    context,
    seen,
    receipt: state?.lastCleanupReceipt ?? null,
    attemptId: state?.attemptId ?? 'unknown',
  };
}

/**
 * The shared mandatory-failure proof: exact reverse-order rollback of every
 * crossed boundary, exact retained support set, context truth, cleared
 * projections, drained host bus, no client/crash surfaces, and the fallback
 * deactivate path.
 */
async function assertRolledBackCleanly(failure: FailedAttempt): Promise<void> {
  const receipt = failure.receipt;
  expect(receipt?.terminal_state).toBe('activation_failed');
  expect(receipt?.attempt_id).toBe(failure.attemptId);

  // Every boundary the attempt crossed before the failure — and nothing else —
  // was cleaned, in exact reverse registration order.
  const registeredIds = failure.seen.map((boundary) => boundary.resource_id);
  const expectedRetained = registeredIds.filter((id) => retainedResourceIds.has(id));
  const expectedCleaned = registeredIds.filter((id) => !retainedResourceIds.has(id)).reverse();
  expect(receipt?.cleaned_resources).toEqual(expectedCleaned);
  expect(receipt?.cleanup_failures).toEqual([]);
  expect([...(receipt?.retained_support_resources ?? [])].sort()).toEqual(
    [...expectedRetained].sort(),
  );

  // Module projections clear last — after the language-client lifecycle
  // teardown, so the teardown cannot observe cleared projections and leak a
  // partially started client (#7854).
  const cleaned = receipt?.cleaned_resources ?? [];
  if (registeredIds.includes('module-projections')) {
    expect(cleaned[cleaned.length - 1]).toBe('module-projections');
  }
  if (registeredIds.includes('language-client-lifecycle')) {
    expect(cleaned.indexOf('module-projections')).toBeGreaterThan(
      cleaned.indexOf('language-client-lifecycle'),
    );
  }
  expect(_activationProjectionsClearedForTest()).toBe(true);

  // Every tracked mandatory disposable was disposed exactly once, in exact
  // reverse creation order; the retained support surfaces were not disposed.
  const mandatoryTracked = creationOrder().filter((label) => !RETAINED_LABELS.includes(label));
  expect(disposedLabels()).toEqual([...mandatoryTracked].reverse());
  for (const label of RETAINED_LABELS) {
    expect(disposedLabels()).not.toContain(label);
  }

  // The host net received exactly the retained support surfaces.
  expect(failure.context.subscriptions).toHaveLength(expectedRetained.length);
  for (const entry of tracked.filter((candidate) => RETAINED_LABELS.includes(candidate.label))) {
    expect(failure.context.subscriptions).toContain(entry.disposable);
    expect(entry.disposable.dispose).not.toHaveBeenCalled();
  }

  // Truth projection: the activation-complete context key was never claimed
  // for the failed attempt and was actively reset.
  expect(vscode.commands.executeCommand).not.toHaveBeenCalledWith(
    'setContext',
    'perl-lsp.activated',
    true,
  );
  expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
    'setContext',
    'perl-lsp.activated',
    false,
  );

  // No listener the failed attempt armed is still live on the host bus.
  for (const entry of busEntries) {
    expect(entry.live).toBe(false);
  }

  // No pre-commit path can enter mid-session crash recovery (#7798): in the
  // demand-driven composition no client exists before commit, and the failed
  // attempt leaves the crash surfaces untouched.
  expect(mockLanguageClientCtor).not.toHaveBeenCalled();
  expect(_autoRestartAttemptsForTest()).toBe(0);

  // Deactivation on the partial state stays on the pre-transaction fallback
  // path: it resolves, is idempotent, and disposes nothing new.
  await expect(deactivate()).resolves.toBeUndefined();
  await expect(deactivate()).resolves.toBeUndefined();
  expect(disposedLabels()).toEqual([...mandatoryTracked].reverse());
}

beforeAll(async () => {
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

  trackEventFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidChangeConfiguration',
    'watcher:configuration',
  );
  trackEventFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onWillSaveTextDocument',
    'watcher:format-on-save',
  );
  trackEventFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidCreateFiles',
    'watcher:file-creation',
  );
  trackEventFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidChangeTextDocument',
    'watcher:arrow-completion',
  );
  trackEventFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidSaveTextDocument',
    'watcher:pod-save',
  );
  trackEventFactory(
    vscode.workspace as unknown as Record<string, unknown>,
    'onDidOpenTextDocument',
    'listener:document-open-demand',
  );
  trackEventFactory(
    vscode.window as unknown as Record<string, unknown>,
    'onDidChangeActiveTextEditor',
    'listener:active-editor-demand',
  );

  // Baseline pass: record the boundaries one clean production activation
  // crosses, then tear it down. The matrix derives its targets from this
  // recording, so a new registration changes the recorded composition rather
  // than silently invalidating a hardcoded count.
  setOpenDocuments([]);
  _setActivationPhaseFailureInjectorForTest((boundary) => {
    recordedBoundaries.push(boundary);
    return null;
  });
  await activate(makeContext(makeExtensionRoot()));
  await deactivate();
  _setActivationPhaseFailureInjectorForTest(null);

  // The retained support set is exactly the output channel (the base phase's
  // second registration, right after the module projections) plus the
  // support-phase command registrations.
  retainedResourceIds = new Set([
    nthOfPhase('base', 2).resource_id,
    ...boundariesOfPhase('support').map((boundary) => boundary.resource_id),
  ]);
  baselineCommandRegistrations = (vscode.commands.registerCommand as jest.Mock).mock.calls.length;
});

afterAll(() => {
  _setActivationPhaseFailureInjectorForTest(null);
});

describe('production activation failure injection (#7855)', () => {
  beforeEach(() => {
    setOpenDocuments([]);
    tracked.length = 0;
    createdOrder.length = 0;
    disposedOrder.length = 0;
    busEntries.length = 0;
    jest.clearAllMocks();
  });

  afterEach(async () => {
    _setActivationPhaseFailureInjectorForTest(null);
    await deactivate();
    setOpenDocuments([]);
    jest.clearAllMocks();
    disposedOrder.length = 0;
    createdOrder.length = 0;
    tracked.length = 0;
    busEntries.length = 0;
    while (createdRoots.length > 0) {
      fs.rmSync(createdRoots.pop() as string, { recursive: true, force: true });
    }
  });

  test('the baseline composition records every named phase', () => {
    // The recorded composition is the injection contract: each production
    // phase the issue names must be observable as at least one boundary, and
    // the retained support set must be exactly five approved surfaces.
    for (const phase of [
      'base',
      'commands',
      'workspace_listeners',
      'language_client',
      'document_providers',
      'debugger',
      'support',
    ]) {
      expect(boundariesOfPhase(phase).length).toBeGreaterThan(0);
    }
    expect(retainedResourceIds.size).toBe(RETAINED_LABELS.length);
    expect(baselineCommandRegistrations).toBeGreaterThan(0);
  });

  test.each([...MATRIX_NAMES])(
    'failure after %s rolls the production attempt back completely',
    async (name) => {
      const failure = await activateWithInjectedFailure(resolveMatrixTarget(name));
      await assertRolledBackCleanly(failure);
    },
  );

  test('a command-factory failure mid-batch rolls back every owned resource', async () => {
    // The real host failure shape INSIDE a command group: the first command
    // registers, then a later registration in the same group throws before
    // the group returns, so the whole batch never reaches ownDisposables.
    const registerMock = vscode.commands.registerCommand as jest.Mock;
    const originalImpl = registerMock.getMockImplementation();
    let commandRegistrations = 0;
    registerMock.mockImplementation((...args: unknown[]) => {
      commandRegistrations += 1;
      if (commandRegistrations === 2) {
        throw new Error('command factory refused');
      }
      return originalImpl?.(...args);
    });
    let failure: FailedAttempt;
    try {
      failure = await activateWithRecordingBoundaries('command factory refused');
    } finally {
      registerMock.mockImplementation(originalImpl);
    }

    // Activation rejects, settles failed, and rolls back every resource the
    // attempt actually owned — in exact reverse order, projections last, the
    // output channel retained to the host net.
    const state = _extensionActivationStateForTest();
    expect(state?.state).toBe('activation_failed');
    const receipt = state?.lastCleanupReceipt;
    const ownedIds = failure.seen.map((boundary) => boundary.resource_id);
    expect(receipt?.cleaned_resources).toEqual(
      ownedIds.filter((id) => !retainedResourceIds.has(id)).reverse(),
    );
    expect(receipt?.retained_support_resources).toEqual(['base-2']);
    expect(failure.context.subscriptions).toHaveLength(1);
    const cleanedIds = receipt?.cleaned_resources ?? [];
    expect(cleanedIds[cleanedIds.length - 1]).toBe('module-projections');
    expect(cleanedIds.indexOf('module-projections')).toBeGreaterThan(
      cleanedIds.indexOf('language-client-lifecycle'),
    );
    expect(_activationProjectionsClearedForTest()).toBe(true);
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      false,
    );

    // Pinned #7854 wiring boundary, recorded against the wiring owner: the
    // first command of the failed group was created and registered host-side
    // but never entered the attempt ledger — command groups own their
    // disposables only after the whole group returns, so a mid-batch factory
    // failure leaves the earlier siblings as an UNAPPROVED surviving surface
    // (undisposed, still registered, absent from the host net). Repairing the
    // batch ownership is the activation-wiring owner's slice, not this test
    // slice's; this assertion makes the gap visible and falsifiable instead
    // of hiding it behind batch-complete injection points.
    const showOutput = tracked.find((entry) => entry.label === 'cmd:perl-lsp.showOutput');
    expect(showOutput).toBeDefined();
    expect(showOutput?.disposable.dispose).not.toHaveBeenCalled();
    expect(registeredCommandIds()).toContain('perl-lsp.showOutput');
    expect(failure.context.subscriptions).not.toContain(showOutput?.disposable);
    expect(disposedLabels()).not.toContain('cmd:perl-lsp.showOutput');

    // The partial state still deactivates through the fallback path,
    // idempotently, without touching the leaked sibling.
    await expect(deactivate()).resolves.toBeUndefined();
    await expect(deactivate()).resolves.toBeUndefined();
    expect(showOutput?.disposable.dispose).not.toHaveBeenCalled();
  });

  test('a failed activation retries cleanly in the same host process', async () => {
    // Fail after the final pre-commit boundary so every production resource
    // exists when the attempt rolls back.
    const firstFailure = await activateWithInjectedFailure(lastBoundaryOverall());
    const firstRetainedCount = firstFailure.receipt?.retained_support_resources.length ?? 0;
    await assertRolledBackCleanly(firstFailure);

    // The failed attempt's mandatory commands are unregistered by their
    // disposal, while the retained support commands stay usable for failure
    // reporting — the state an explicit Retry is offered from.
    const commandsAfterFailure = registeredCommandIds();
    expect(commandsAfterFailure).not.toContain('perl-lsp.restart');
    for (const supportCommand of [
      'perl-lsp.showWhatsNew',
      'perl-lsp.openConfigurationGuide',
      'perl-lsp.checkForUpdate',
      'perl-lsp.reportIssue',
    ]) {
      expect(commandsAfterFailure).toContain(supportCommand);
    }
    // Identity of the live retained registrations, so the retry can be proved
    // to REPLACE them rather than register duplicates alongside them.
    const retainedCallbacksBeforeRetry = registeredCommandEntries();
    const retainedFromFailedAttempt = [...firstFailure.context.subscriptions] as unknown as {
      dispose: jest.Mock;
    }[];

    // Retry: a fresh attempt in the same host process starts from baseline and
    // completes. Attempt ids are unique generations, not a resumed attempt.
    const commandsBeforeRetry = (vscode.commands.registerCommand as jest.Mock).mock.calls.length;
    const createdBeforeRetry = createdOrder.length;
    const retryContext = makeContext(makeExtensionRoot());
    const extensionApi = await activate(retryContext);
    const createdAtCommit = createdOrder.length;
    expect(extensionApi).toBeDefined();
    const retryState = _extensionActivationStateForTest();
    expect(retryState?.state).toBe('active');
    expect(retryState?.attemptId).not.toBe(firstFailure.attemptId);
    expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
      'setContext',
      'perl-lsp.activated',
      true,
    );

    // No duplicate resources: the retry registered exactly one fresh set of
    // commands — the baseline count, not double — and every retained command
    // the failed attempt left behind was REPLACED by the retry's callback
    // (one live registration per command id), never left as a simultaneous
    // stale resource. The retry's host net carries only retry-created
    // disposables.
    expect(
      (vscode.commands.registerCommand as jest.Mock).mock.calls.length - commandsBeforeRetry,
    ).toBe(baselineCommandRegistrations);
    const retainedCallbacksAfterRetry = registeredCommandEntries();
    for (const supportCommand of [
      'perl-lsp.showWhatsNew',
      'perl-lsp.openConfigurationGuide',
      'perl-lsp.checkForUpdate',
      'perl-lsp.reportIssue',
    ]) {
      expect(retainedCallbacksAfterRetry.get(supportCommand)).toBeDefined();
      expect(retainedCallbacksAfterRetry.get(supportCommand)).not.toBe(
        retainedCallbacksBeforeRetry.get(supportCommand),
      );
    }
    for (const retainedDisposable of retainedFromFailedAttempt) {
      expect(retryContext.subscriptions).not.toContain(retainedDisposable);
    }

    // The failed attempt's demand listeners cannot reach the new runtime: the
    // host bus holds exactly the retry's live listener, and dispatching a
    // Perl document reaches only that one.
    const staleEntries = allEntries('listener:document-open-demand').filter((entry) => !entry.live);
    expect(staleEntries.length).toBeGreaterThan(0);
    expect(liveEntries('listener:document-open-demand')).toHaveLength(1);
    fireHostEvent('listener:document-open-demand', fakeDocument('perl'));
    for (const stale of staleEntries) {
      expect(stale.invokedCount).toBe(0);
    }
    await waitUntil(() => startedClientCount() === 1);
    expect(startedClientCount()).toBe(1);

    // Deactivation is the terminal path: it disposes EVERYTHING the retry's
    // committed runtime owns — including the retry's own support commands —
    // exactly once, in reverse registration order, and is idempotent. The
    // demand-started client's post-commit resources (the test adapter's save
    // listener) are torn down inside the same teardown by the lifecycle
    // cleanup, not by the activation ledger. The first attempt's retained
    // support surfaces are the host's to dispose, untouched by the retry's
    // teardown.
    const retryOwned = creationOrder().slice(createdBeforeRetry, createdAtCommit);
    const postCommitCreated = creationOrder().slice(createdAtCommit);
    disposedOrder.length = 0;
    await deactivate();
    const terminalDisposal = disposedLabels();
    expect([...terminalDisposal].sort()).toEqual([...retryOwned, ...postCommitCreated].sort());
    expect(terminalDisposal.filter((label) => retryOwned.includes(label))).toEqual(
      [...retryOwned].reverse(),
    );
    expect(_extensionActivationStateForTest()?.lastCleanupReceipt?.terminal_state).toBe(
      'deactivated',
    );
    await deactivate();
    expect(disposedLabels()).toHaveLength(retryOwned.length + postCommitCreated.length);
    for (const label of RETAINED_LABELS) {
      // The FIRST registration under each retained label is the failed
      // attempt's; the host net it was handed to still owns it.
      const fromFailedAttempt = tracked.find((entry) => entry.label === label);
      expect(fromFailedAttempt?.disposable.dispose).not.toHaveBeenCalled();
    }
    expect(firstFailure.context.subscriptions).toHaveLength(firstRetainedCount);
  });

  test('one cleanup failure cannot prevent the remaining rollback', async () => {
    // The configuration watcher is the second workspace listener production
    // registers (after format-on-save); its dispose runs mid-rollback when a
    // later phase fails.
    const failure = await activateWithInjectedFailure(lastOfPhase('debugger'), (boundary) => {
      if (boundary.phase === 'workspace_listeners' && boundary.ordinal === 2) {
        armBusDisposeHook('watcher:configuration', () => {
          throw new Error('configuration cleanup refused');
        });
      }
    });
    const receipt = failure.receipt;

    // The failing cleanup is recorded as one bounded receipt entry; every
    // other owned resource was still attempted, in the same deterministic
    // order.
    expect(receipt?.cleanup_failures).toEqual([
      {
        resource_id: 'workspace_listeners-2',
        phase: 'workspace_listeners',
        reason: 'configuration cleanup refused',
      },
    ]);
    const registeredIds = failure.seen.map((boundary) => boundary.resource_id);
    expect(receipt?.cleaned_resources).toEqual(
      registeredIds
        .filter((id) => id !== 'workspace_listeners-2' && !retainedResourceIds.has(id))
        .reverse(),
    );

    // Disposal was attempted for every mandatory tracked disposable, the
    // failing one included, in exact reverse creation order.
    const mandatoryTracked = creationOrder().filter((label) => !RETAINED_LABELS.includes(label));
    expect(disposedLabels()).toEqual([...mandatoryTracked].reverse());

    // The attempt still settles as failed, and the partial state deactivates
    // via the fallback path.
    expect(_extensionActivationStateForTest()?.state).toBe('activation_failed');
    await expect(deactivate()).resolves.toBeUndefined();
  });

  test('a workspace event delivered during rollback cannot corrupt the receipt', async () => {
    // Fail after the debugger phase. In reverse order the arrow-completion
    // watcher is disposed before the configuration watcher, so firing a
    // configuration change while the arrow watcher's dispose runs reaches the
    // still-live configuration listener — the realistic mid-rollback
    // interleaving.
    const failure = await activateWithInjectedFailure(lastOfPhase('debugger'), (boundary) => {
      if (boundary.phase === 'document_providers' && boundary.ordinal === 1) {
        armBusDisposeHook('watcher:arrow-completion', () => {
          fireHostEvent('watcher:configuration', { affectsConfiguration: () => false });
        });
      }
    });

    // The mid-rollback dispatch reached the still-live configuration listener
    // (host semantics), ran its handler without throwing into the rollback,
    // and left the receipt exact: every non-retained resource cleaned in
    // reverse order with zero recorded cleanup failures.
    const configEntries = allEntries('watcher:configuration');
    expect(configEntries).toHaveLength(1);
    expect(configEntries[0]?.invokedCount).toBe(1);
    expect(failure.receipt?.cleanup_failures).toEqual([]);
    await assertRolledBackCleanly(failure);
  });

  test('a stale demand callback dispatched after rollback finds no runtime to act on', async () => {
    // Defense-in-depth beyond the host bus barrier: even a host that
    // dispatched a stale callback after rollback would find the module
    // projections cleared by the same authority, so the callback is inert.
    const failure = await activateWithInjectedFailure(lastBoundaryOverall());
    await assertRolledBackCleanly(failure);

    const stale = allEntries('listener:document-open-demand')[0];
    if (!stale) {
      throw new Error('no demand listener was armed by the failed attempt');
    }
    expect(stale.live).toBe(false);
    stale.listener(fakeDocument('perl'));
    await settle();

    // No server was started by the stale callback: there is no coordinator or
    // client projection left to route demand into, and no crash-recovery
    // surface was entered.
    expect(startedClientCount()).toBe(0);
    expect(mockLanguageClientCtor).not.toHaveBeenCalled();
    expect(_autoRestartAttemptsForTest()).toBe(0);
  });

  describe('packaged-journey harness failure seam (#7856)', () => {
    const SEAM_ENV = 'PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE';
    let savedEnv: string | undefined;

    beforeEach(() => {
      savedEnv = process.env[SEAM_ENV];
      (vscode.extensions.getExtension as jest.Mock).mockReturnValue(undefined);
    });

    afterEach(() => {
      if (savedEnv === undefined) {
        delete process.env[SEAM_ENV];
      } else {
        process.env[SEAM_ENV] = savedEnv;
      }
      (vscode.extensions.getExtension as jest.Mock).mockReturnValue(undefined);
    });

    test('the seam is inert without the harness extension, even with the env set', async () => {
      process.env[SEAM_ENV] = 'debugger';
      // No harness extension installed: getExtension returns undefined.
      await activate(makeContext(makeExtensionRoot()));
      expect(_extensionActivationStateForTest()?.state).toBe('active');
      await deactivate();
    });

    test('the seam is inert for an unknown phase name', async () => {
      process.env[SEAM_ENV] = 'not-a-real-phase';
      (vscode.extensions.getExtension as jest.Mock).mockReturnValue({});
      await activate(makeContext(makeExtensionRoot()));
      expect(_extensionActivationStateForTest()?.state).toBe('active');
      await deactivate();
    });

    test('the seam fails the first boundary of the named phase and clears itself', async () => {
      process.env[SEAM_ENV] = 'debugger';
      (vscode.extensions.getExtension as jest.Mock).mockReturnValue({ id: 'harness' });

      // The packaged failure leg: activation rejects at the named production
      // boundary through the same rollback any real mid-activation exception
      // takes, with the retained support set intact.
      const context = makeContext(makeExtensionRoot());
      await expect(activate(context)).rejects.toThrow(
        'harness-injected activation failure after debugger-1 (#7856 packaged journey)',
      );
      const state = _extensionActivationStateForTest();
      expect(state?.state).toBe('activation_failed');
      expect(state?.lastCleanupReceipt?.terminal_state).toBe('activation_failed');
      expect(state?.lastCleanupReceipt?.cleanup_failures).toEqual([]);
      expect(registeredCommandIds()).toContain('perl-lsp.reportIssue');
      expect(registeredCommandIds()).not.toContain('perl-lsp.restart');
      expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
        'setContext',
        'perl-lsp.activated',
        false,
      );

      // The retry leg: with the fault removed, a fresh attempt in the same
      // host process cannot inherit the seam — the injector was cleared when
      // the failed attempt ended.
      delete process.env[SEAM_ENV];
      const retryApi = await activate(makeContext(makeExtensionRoot()));
      expect(retryApi).toBeDefined();
      expect(_extensionActivationStateForTest()?.state).toBe('active');
      expect(vscode.commands.executeCommand).toHaveBeenCalledWith(
        'setContext',
        'perl-lsp.activated',
        true,
      );
      await deactivate();
    });
  });
});
