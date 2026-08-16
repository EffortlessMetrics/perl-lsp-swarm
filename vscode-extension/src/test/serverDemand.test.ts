import { describe, expect, test } from '@jest/globals';
import {
  ACTIVATION_TRIGGER_LEDGER,
  SERVER_ENTRY_POINT_LEDGER,
  ServerDemandCoordinator,
  isServerDependentDocument,
  perllspDispositionFor,
  type ActivationTriggerId,
  type ServerDemandSnapshot,
} from '../serverDemand';
import * as fs from 'fs';
import * as path from 'path';

interface ExtensionManifest {
  readonly activationEvents: string[];
  readonly contributes: { readonly commands: { readonly command: string }[] };
}

// Read the manifest at runtime rather than importing it: package.json sits
// outside the compiler's rootDir, and this test's whole purpose is to compare
// the ledger against what actually ships.
const packageJson = JSON.parse(
  fs.readFileSync(path.join(__dirname, '..', '..', 'package.json'), 'utf8'),
) as ExtensionManifest;

class Deferred<T> {
  readonly promise: Promise<T>;
  private resolvePromise!: (value: T) => void;
  private rejectPromise!: (error: unknown) => void;

  constructor() {
    this.promise = new Promise<T>((resolve, reject) => {
      this.resolvePromise = resolve;
      this.rejectPromise = reject;
    });
  }

  resolve(value: T): void {
    this.resolvePromise(value);
  }

  reject(error: unknown): void {
    this.rejectPromise(error);
  }
}

interface Harness {
  readonly coordinator: ServerDemandCoordinator;
  readonly states: ServerDemandSnapshot[];
  starts: number;
  gate: Deferred<void> | undefined;
  failWith: unknown;
}

function createHarness(): Harness {
  const harness: Harness = {
    coordinator: undefined as unknown as ServerDemandCoordinator,
    states: [],
    starts: 0,
    gate: undefined,
    failWith: undefined,
  };

  const coordinator = new ServerDemandCoordinator({
    startServer: async () => {
      harness.starts += 1;
      if (harness.gate) {
        await harness.gate.promise;
      }
      if (harness.failWith !== undefined) {
        throw harness.failWith;
      }
    },
    onStateChange: (snapshot) => {
      harness.states.push(snapshot);
    },
  });

  return Object.assign(harness, { coordinator });
}

const perlFile = { languageId: 'perl', uriScheme: 'file' };
const gherkinFile = { languageId: 'gherkin', uriScheme: 'file' };

describe('activation trigger ledger', () => {
  test('classifies every shipped activation event exactly once', () => {
    const shipped = packageJson.activationEvents;
    const classified = ACTIVATION_TRIGGER_LEDGER.map((row) => row.trigger);

    expect([...classified].sort()).toEqual([...shipped].sort());
    expect(new Set(classified).size).toBe(classified.length);
  });

  test('non-LSP surfaces do not require perllsp immediately', () => {
    // The whole point of #8180: none of these may start a language server.
    expect(perllspDispositionFor('onLanguage:gherkin')).toBe('never');
    expect(perllspDispositionFor('onDebugInitialConfigurations')).toBe('never');
    expect(perllspDispositionFor('onWalkthrough:perl-lsp.gettingStarted')).toBe('on-first-use');
    expect(perllspDispositionFor('onDebugResolve:perl')).toBe('on-first-use');
  });

  test('Perl document triggers require perllsp immediately', () => {
    expect(perllspDispositionFor('onLanguage:perl')).toBe('immediate');
    expect(perllspDispositionFor('onLanguage:perl5')).toBe('immediate');
  });

  test('debug activation never implies an immediate language server', () => {
    // DAP and LSP keep separate process identities.
    const debugRow = ACTIVATION_TRIGGER_LEDGER.find((row) => row.trigger === 'onDebugResolve:perl');
    expect(debugRow?.perlDap).toBe('immediate');
    expect(debugRow?.perllsp).not.toBe('immediate');
  });

  test('an unclassified trigger fails closed', () => {
    expect(perllspDispositionFor('onLanguage:cobol' as ActivationTriggerId)).toBe('never');
  });

  test('status reads never create server demand', () => {
    const statusCommands = ['perl-lsp.showWorkspaceStatus', 'perl-lsp.showStatusMenu'];
    for (const command of statusCommands) {
      const row = SERVER_ENTRY_POINT_LEDGER.find((entry) => entry.command === command);
      expect(row?.perllsp).toBe('never');
    }
  });

  test('every ledgered entry point is a real contributed command', () => {
    const contributed = new Set(packageJson.contributes.commands.map((entry) => entry.command));
    for (const row of SERVER_ENTRY_POINT_LEDGER) {
      expect(contributed.has(row.command)).toBe(true);
    }
  });
});

describe('server-dependent document eligibility', () => {
  test('accepts Perl file and untitled buffers', () => {
    expect(isServerDependentDocument(perlFile)).toBe(true);
    expect(isServerDependentDocument({ languageId: 'perl', uriScheme: 'untitled' })).toBe(true);
    expect(isServerDependentDocument({ languageId: 'perl5', uriScheme: 'file' })).toBe(true);
  });

  test('rejects non-Perl languages', () => {
    expect(isServerDependentDocument(gherkinFile)).toBe(false);
    expect(isServerDependentDocument({ languageId: 'markdown', uriScheme: 'file' })).toBe(false);
  });

  test('rejects read-only and virtual schemes', () => {
    // A diff view or an output pane rendered as Perl is not a reason to spawn
    // a language server.
    expect(isServerDependentDocument({ languageId: 'perl', uriScheme: 'git' })).toBe(false);
    expect(isServerDependentDocument({ languageId: 'perl', uriScheme: 'output' })).toBe(false);
  });
});

describe('ServerDemandCoordinator', () => {
  test('starts nothing until demand exists', () => {
    const harness = createHarness();
    expect(harness.starts).toBe(0);
    expect(harness.coordinator.snapshot.state).toBe('not_started');
    expect(harness.coordinator.snapshot.reasonCode).toBe('no_server_demand');
  });

  test('first demand starts exactly one server', async () => {
    const harness = createHarness();
    await harness.coordinator.ensureStarted('test');

    expect(harness.starts).toBe(1);
    expect(harness.coordinator.snapshot.state).toBe('running');
  });

  test('a running server is not started again', async () => {
    const harness = createHarness();
    await harness.coordinator.ensureStarted('first');
    await harness.coordinator.ensureStarted('second');

    expect(harness.starts).toBe(1);
  });

  test('concurrent demand joins one start', async () => {
    const harness = createHarness();
    harness.gate = new Deferred<void>();

    const first = harness.coordinator.ensureStarted('first');
    const second = harness.coordinator.ensureStarted('second');
    const third = harness.coordinator.observeDocument(perlFile);

    harness.gate.resolve();
    await Promise.all([first, second, third]);

    // Negative control: two clients for concurrent triggers is a failure.
    expect(harness.starts).toBe(1);
    expect(harness.coordinator.snapshot.state).toBe('running');
  });

  test('Gherkin-only activation does not start the server', async () => {
    const harness = createHarness();
    await harness.coordinator.noteActivationTrigger('onLanguage:gherkin');
    await harness.coordinator.observeDocument(gherkinFile);

    expect(harness.starts).toBe(0);
    expect(harness.coordinator.snapshot.state).toBe('not_started');
  });

  test('walkthrough activation does not start the server', async () => {
    const harness = createHarness();
    await harness.coordinator.noteActivationTrigger('onWalkthrough:perl-lsp.gettingStarted');

    expect(harness.starts).toBe(0);
  });

  test('debug-configuration activation does not start the server', async () => {
    const harness = createHarness();
    await harness.coordinator.noteActivationTrigger('onDebugResolve:perl');
    await harness.coordinator.noteActivationTrigger('onDebugInitialConfigurations');

    expect(harness.starts).toBe(0);
  });

  test('Gherkin first, then a Perl document, starts exactly one server', async () => {
    const harness = createHarness();
    await harness.coordinator.noteActivationTrigger('onLanguage:gherkin');
    expect(harness.starts).toBe(0);

    // Negative control: missing this start is the "needs a reload" bug.
    await harness.coordinator.observeDocument(perlFile);
    expect(harness.starts).toBe(1);
    expect(harness.coordinator.snapshot.state).toBe('running');
  });

  test('walkthrough first, then a Perl document, starts the server', async () => {
    const harness = createHarness();
    await harness.coordinator.noteActivationTrigger('onWalkthrough:perl-lsp.gettingStarted');
    await harness.coordinator.observeDocument(perlFile);

    expect(harness.starts).toBe(1);
  });

  test('a server-dependent command starts a dormant server with no open document', async () => {
    const harness = createHarness();
    await harness.coordinator.ensureStarted('command:runHealthCheck', { retry: true });

    expect(harness.starts).toBe(1);
  });

  test('reports starting before running', async () => {
    const harness = createHarness();
    harness.gate = new Deferred<void>();

    const pending = harness.coordinator.ensureStarted('test');
    expect(harness.coordinator.snapshot.state).toBe('starting');

    harness.gate.resolve();
    await pending;

    expect(harness.states.map((snapshot) => snapshot.state)).toEqual(['starting', 'running']);
  });

  test('never reports starting while no start is intended', async () => {
    const harness = createHarness();
    await harness.coordinator.noteActivationTrigger('onLanguage:gherkin');
    await harness.coordinator.observeDocument(gherkinFile);

    expect(harness.states).toHaveLength(0);
  });
});

describe('failure and retry', () => {
  test('a failed start is not retried automatically', async () => {
    const harness = createHarness();
    harness.failWith = new Error('boom');

    await harness.coordinator.ensureStarted('first');
    expect(harness.coordinator.snapshot.state).toBe('failed');
    expect(harness.coordinator.snapshot.reasonCode).toBe('startup_failure');

    // An open editor must not re-spawn a server that just failed.
    await harness.coordinator.observeDocument(perlFile);
    expect(harness.starts).toBe(1);
  });

  test('an explicit retry starts again', async () => {
    const harness = createHarness();
    harness.failWith = new Error('boom');
    await harness.coordinator.ensureStarted('first');

    harness.failWith = undefined;
    await harness.coordinator.ensureStarted('command:restartServer', { retry: true });

    expect(harness.starts).toBe(2);
    expect(harness.coordinator.snapshot.state).toBe('running');
  });

  test('ensureStarted never rejects', async () => {
    const harness = createHarness();
    harness.failWith = new Error('boom');

    // Callers are UI paths; a rejected promise here becomes an unhandled
    // rejection in the extension host.
    await expect(harness.coordinator.ensureStarted('test')).resolves.toBeUndefined();
  });
});

describe('workspace trust gate', () => {
  test('demand raised while gated does not start the server', async () => {
    const harness = createHarness();
    harness.coordinator.closeGate('workspace_untrusted');

    await harness.coordinator.observeDocument(perlFile);

    expect(harness.starts).toBe(0);
    expect(harness.coordinator.snapshot.state).toBe('action_required');
    expect(harness.coordinator.snapshot.reasonCode).toBe('workspace_untrusted');
  });

  test('granting trust honours demand recorded while gated', async () => {
    const harness = createHarness();
    harness.coordinator.closeGate('workspace_untrusted');
    await harness.coordinator.observeDocument(perlFile);

    await harness.coordinator.openGate();

    // The user should not have to re-open the file to get language features.
    expect(harness.starts).toBe(1);
    expect(harness.coordinator.snapshot.state).toBe('running');
  });

  test('granting trust with no demand leaves the server dormant', async () => {
    const harness = createHarness();
    harness.coordinator.closeGate('workspace_untrusted');
    await harness.coordinator.observeDocument(gherkinFile);

    await harness.coordinator.openGate();

    expect(harness.starts).toBe(0);
    expect(harness.coordinator.snapshot.state).toBe('not_started');
  });

  test('repeated demand while gated still starts only once', async () => {
    const harness = createHarness();
    harness.coordinator.closeGate('workspace_untrusted');
    await harness.coordinator.observeDocument(perlFile);
    await harness.coordinator.observeDocument(perlFile);
    await harness.coordinator.ensureStarted('command:runHealthCheck');

    await harness.coordinator.openGate();

    expect(harness.starts).toBe(1);
  });
});

describe('generation correctness', () => {
  test('a stop invalidates the running generation so later demand restarts', async () => {
    const harness = createHarness();
    await harness.coordinator.ensureStarted('first');
    expect(harness.starts).toBe(1);

    harness.coordinator.noteStopped();
    expect(harness.coordinator.snapshot.state).toBe('not_started');

    await harness.coordinator.observeDocument(perlFile);
    expect(harness.starts).toBe(2);
  });

  test('noteRunning clears a stale failed state', async () => {
    const harness = createHarness();
    harness.failWith = new Error('boom');
    await harness.coordinator.ensureStarted('first');
    expect(harness.coordinator.snapshot.state).toBe('failed');

    // A successful restart owns its own start sequence.
    harness.coordinator.noteRunning();
    expect(harness.coordinator.snapshot.state).toBe('running');
    expect(harness.coordinator.snapshot.error).toBeUndefined();
  });

  test('demand after dispose does not start a server', async () => {
    const harness = createHarness();
    harness.coordinator.dispose();

    await harness.coordinator.observeDocument(perlFile);
    await harness.coordinator.ensureStarted('test', { retry: true });

    expect(harness.starts).toBe(0);
  });

  test('an in-flight start cannot publish running into a disposed coordinator', async () => {
    const harness = createHarness();
    harness.gate = new Deferred<void>();

    const pending = harness.coordinator.ensureStarted('test');
    harness.coordinator.dispose();
    harness.gate.resolve();
    await pending;

    expect(harness.coordinator.snapshot.state).not.toBe('running');
    expect(harness.states.some((snapshot) => snapshot.state === 'running')).toBe(false);
  });

  test('a stale failed generation cannot overwrite a newer state', async () => {
    const harness = createHarness();
    harness.gate = new Deferred<void>();
    harness.failWith = new Error('stale');

    const pending = harness.coordinator.ensureStarted('first');
    // The server is replaced out from under the in-flight start.
    harness.coordinator.noteRunning();
    harness.gate.resolve();
    await pending;

    expect(harness.coordinator.snapshot.state).toBe('running');
  });
});
