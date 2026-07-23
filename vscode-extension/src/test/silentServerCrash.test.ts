/**
 * Unit tests for mid-session silent server crash recovery (#4625).
 *
 * Covers the `handleClientStateChange` Running → Stopped branch:
 *   - an unexpected crash surfaces a `showErrorMessage` toast
 *   - the diagnosis hint is captured for `serverNotRunningMessage()`
 *   - auto-restart is attempted up to MAX_AUTO_RESTART_ATTEMPTS (3)
 *   - once the budget is exhausted, a "could not be restarted" toast is shown
 *     and no further auto-restart is attempted
 *   - a user-initiated stop sentinel suppresses the crash notification
 *   - a stable prior run resets the auto-restart budget
 *
 * The real lifecycle controller is not instantiated in this unit harness, so
 * `restartServer()` short-circuits with a warning (the counter is incremented
 * before `restartServer` is called, which is what these tests assert on).
 */

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {},
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
  State: { Stopped: 1, Running: 2, Starting: 3 },
  StateChangeEvent: class {
    constructor(
      public oldState: number,
      public newState: number,
    ) {}
  },
}));

import * as vscode from 'vscode';
import {
  handleClientStateChange,
  serverNotRunningMessage,
  _resetCrashRecoveryStateForTest,
  _autoRestartAttemptsForTest,
  _markStableRunningForTest,
  _setExtensionContextForTest,
  _setUserInitiatedStopPendingForTest,
  _setLastStartupDiagnosisForTest,
} from '../extension';

// vscode-languageclient State numeric values (Stopped=1, Running=2, Starting=3).
const STATE_STOPPED = 1;
const STATE_RUNNING = 2;

interface StateChangeEventLike {
  oldState: number;
  newState: number;
}

function crashEvent(): StateChangeEventLike {
  return { oldState: STATE_RUNNING, newState: STATE_STOPPED };
}

function makeContext(): vscode.ExtensionContext {
  const state = {
    get: jest.fn(() => undefined),
    update: jest.fn(async () => undefined),
  };
  return {
    extension: {
      packageJSON: { publisher: 'EffortlessMetrics', name: 'perl-lsp-rs', version: '0.16.0' },
    },
    extensionMode: vscode.ExtensionMode.Production,
    extensionPath: '/tmp/perl-lsp-test',
    globalState: state,
    subscriptions: [],
    workspaceState: state,
  } as unknown as vscode.ExtensionContext;
}

/** Flush pending microtasks so fire-and-forget async work in the handler runs. */
function flush(): Promise<void> {
  return new Promise((resolve) => {
    setImmediate(resolve);
  });
}

const showErrorMessage = vscode.window.showErrorMessage as jest.Mock;
const showWarningMessage = vscode.window.showWarningMessage as jest.Mock;

describe('mid-session silent server crash recovery (#4625)', () => {
  beforeEach(() => {
    _resetCrashRecoveryStateForTest();
    _setLastStartupDiagnosisForTest(undefined);
    _setExtensionContextForTest(makeContext());
    showErrorMessage.mockReset();
    showErrorMessage.mockResolvedValue(undefined);
    showWarningMessage.mockReset();
    showWarningMessage.mockResolvedValue(undefined);
  });

  test('unexpected Running → Stopped shows an error toast and captures the diagnosis', async () => {
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    const message = showErrorMessage.mock.calls[0][0] as string;
    expect(message).toMatch(/Perl Language Server crashed/i);
    expect(message).toMatch(/restarting automatically/i);

    // The diagnosis hint is now surfaced via serverNotRunningMessage().
    expect(serverNotRunningMessage()).toMatch(/stopped unexpectedly/i);
  });

  test('auto-restart counter increments and caps at MAX_AUTO_RESTART_ATTEMPTS (3)', async () => {
    // Three crashes each consume one retry slot.
    for (let i = 0; i < 3; i++) {
      handleClientStateChange(crashEvent() as never);
      await flush();
    }
    expect(_autoRestartAttemptsForTest()).toBe(3);
    // Each of the three attempts showed a "restarting automatically" toast.
    expect(showErrorMessage).toHaveBeenCalledTimes(3);
    for (const call of showErrorMessage.mock.calls) {
      expect(call[0]).toMatch(/restarting automatically/i);
    }

    // A fourth crash exhausts the budget: a different toast, no further retry.
    showErrorMessage.mockClear();
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    const exhaustedMessage = showErrorMessage.mock.calls[0][0] as string;
    expect(exhaustedMessage).toMatch(/could not be restarted automatically/i);
    // Counter must not exceed 3.
    expect(_autoRestartAttemptsForTest()).toBe(3);
  });

  test('a user-initiated stop suppresses the crash toast and the counter', async () => {
    _setUserInitiatedStopPendingForTest(true);
    handleClientStateChange(crashEvent() as never);
    await flush();

    expect(showErrorMessage).not.toHaveBeenCalled();
    expect(_autoRestartAttemptsForTest()).toBe(0);
  });

  test('a stable prior run resets the auto-restart budget', async () => {
    // First crash consumes a slot (counter 0 → 1).
    handleClientStateChange(crashEvent() as never);
    await flush();
    expect(_autoRestartAttemptsForTest()).toBe(1);

    // Simulate the server having been stably running long enough that the next
    // crash counts as a new episode.
    _markStableRunningForTest();

    handleClientStateChange(crashEvent() as never);
    await flush();

    // Budget was reset to 0 then incremented to 1 — not 2.
    expect(_autoRestartAttemptsForTest()).toBe(1);
  });

  test('a non-crash transition (e.g. Starting) does not surface a toast', async () => {
    handleClientStateChange({ oldState: STATE_STOPPED, newState: 3 } as never);
    await flush();
    expect(showErrorMessage).not.toHaveBeenCalled();
    expect(_autoRestartAttemptsForTest()).toBe(0);
  });
});
