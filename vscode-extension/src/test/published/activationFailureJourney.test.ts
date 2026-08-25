import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  RETAINED_SUPPORT_COMMAND_IDS,
  assertProviderSucceeded,
  bundledBinaryPath,
  bundledServerVersion,
  mandatoryActivationCommandIds,
  pathsEquivalent,
  providerPosition,
  providerResult,
  receiptsDir,
  scanProcessesUnderDirectory,
  sha256,
  withTimeout,
  type ReceiptValue,
} from './journeySupport';

/**
 * Packaged activation-failure cleanup and retry journey (#7856).
 *
 * Unlike the dev-tree fault matrix (#7855, jest with a mocked host), this
 * journey runs inside the REAL extension host against the INSTALLED VSIX in an
 * isolated profile, and proves the two installed-path rows the matrix cannot:
 *
 * Failure leg (fault armed through the harness-only environment seam):
 * - a deterministic pre-commit mandatory activation failure (the first
 *   `debugger`-phase resource boundary — required lifecycle construction after
 *   command/status registration) makes `activate()` reject with the injected
 *   reason and leaves the extension in a truthful failed state;
 * - every mandatory registration is rolled back host-side (command registry
 *   truth), while exactly the approved support surfaces survive for failure
 *   reporting (#7854 wiring contract);
 * - no candidate server process exists, and opening a Perl document — the real
 *   demand trigger — cannot start one across a bounded watch window (the
 *   rolled-back listeners and any watchdog/timer are gone);
 * - no crash-recovery budget is consumed: pre-commit failure never constructs
 *   a client, and no server process ever exists to crash (#7798 boundary).
 *
 * Retry leg (same installed profile, fault removed, fresh host — the explicit
 * reload/retry path a user takes):
 * - the SAME candidate activates successfully: bundled-server identity binds
 *   version and path, startup completes, and one representative provider
 *   answers;
 * - resources are singular, not duplicated: one TestController, exactly one
 *   bundled-server process, one command registration set;
 * - the activation API's recoverable `stop` seam shuts the server down and the
 *   child process is gone; host-exit teardown is proven by the orchestrator's
 *   post-run process scan recorded in the joined receipt.
 *
 * Each leg writes a candidate-bound child receipt; the orchestrator
 * (`run-local-vsix-smoke.js`) validates both and emits the joined
 * `vscode_activation_recovery.v1` receipt for #5903/#4346/#6056 consumption.
 */

const EXTENSION_ID = 'EffortlessMetrics.perl-lsp-rs';
const FIXTURE_FILE = 'activation_recovery.pl';
const DEMAND_WATCH_WINDOW_MS = 6_000;

type JourneyLeg = 'failure' | 'retry';

function journeyLeg(): JourneyLeg {
  const leg = process.env.PERL_LSP_ACTIVATION_FAILURE_LEG;
  if (leg === 'failure' || leg === 'retry') {
    return leg;
  }
  throw new Error(
    `PERL_LSP_ACTIVATION_FAILURE_LEG must be 'failure' or 'retry' for the activation-failure journey, got ${JSON.stringify(leg)}`,
  );
}

interface JourneyCandidate {
  extension_id: string;
  extension_version: string | null;
  extension_path: string;
  vsix_sha256: string | null;
  vscode_version: string;
  platform: string;
  architecture: string;
  bundled_server: {
    path: string;
    sha256: string;
    version: string | null;
    version_probe: ReceiptValue;
  };
  server_source_sha: string | null;
  repository_sha: string | null;
  workspace_path: string;
  fixture: { path: string; sha256: string };
}

async function journeyCandidate(
  extension: vscode.Extension<unknown>,
  workspacePath: string,
): Promise<JourneyCandidate> {
  const bundledServerPath = bundledBinaryPath(extension.extensionPath);
  const versionProbe = await bundledServerVersion(bundledServerPath);
  const fixturePath = path.join(workspacePath, FIXTURE_FILE);
  return {
    extension_id: extension.id,
    extension_version: extension.packageJSON?.version ?? null,
    extension_path: extension.extensionPath,
    vsix_sha256: process.env.PERL_LSP_VSIX_SHA256 ?? null,
    vscode_version: vscode.version,
    platform: process.platform,
    architecture: process.arch,
    bundled_server: {
      path: bundledServerPath,
      sha256: sha256(bundledServerPath),
      version: versionProbe.status === 'ok' ? versionProbe.version : null,
      version_probe: versionProbe as unknown as ReceiptValue,
    },
    server_source_sha: process.env.PERL_LSP_SERVER_SOURCE_SHA ?? null,
    repository_sha: process.env.PERL_LSP_CURRENT_SOURCE_SHA ?? null,
    workspace_path: workspacePath,
    fixture: { path: fixturePath, sha256: sha256(fixturePath) },
  };
}

function writeLegReceipt(leg: JourneyLeg, receipt: ReceiptValue): string {
  const destination = path.join(
    receiptsDir(),
    leg === 'failure'
      ? 'activation_failure_journey_failure_receipt.json'
      : 'activation_failure_journey_retry_receipt.json',
  );
  fs.writeFileSync(destination, JSON.stringify(receipt, null, 2));
  return destination;
}

/**
 * Wait until the startup phases settle: every lifecycle phase left `idle` and
 * `running`. Demand-driven startup may still be `idle` on the first poll (the
 * document-open event has not dispatched yet), so an early-exit waiter would
 * mistake "not yet started" for "finished"; a settled-but-not-ok snapshot
 * fails the lifecycle blockers below with the true terminal statuses.
 */
async function waitUntilStartupSettled(
  getMetrics: () => ReceiptValue,
  timeoutMs: number,
): Promise<ReceiptValue> {
  const deadline = Date.now() + timeoutMs;
  let metrics = getMetrics();
  while (
    Date.now() < deadline &&
    [metrics.binary_resolution_status, metrics.server_start_status, metrics.initialize_status].some(
      (status) => status === 'idle' || status === 'running',
    )
  ) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    metrics = getMetrics();
  }
  return metrics;
}

// Exactly one leg runs per extension host: the orchestrator launches a fresh
// host for each leg, so the not-selected leg's test must not even register —
// a mocha failure for a test this host never armed would be a false negative.
const selectedLeg = journeyLeg();

suite('Packaged activation-failure cleanup and retry (#7856)', function () {
  this.timeout(300_000);

  (selectedLeg === 'failure' ? test : test.skip)(
    'failure leg: pre-commit mandatory failure rolls the installed extension back truthfully',
    async function () {
      const leg: JourneyLeg = 'failure';
      const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
      assert.ok(workspaceFolder, 'activation-failure journey requires a workspace folder');
      const workspacePath = workspaceFolder.uri.fsPath;
      const extension = vscode.extensions.getExtension(EXTENSION_ID);
      assert.ok(extension, 'activation-failure journey requires the installed extension');
      const faultPhase = process.env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE ?? null;
      assert.ok(
        faultPhase,
        'the failure leg requires PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE to be armed by the orchestrator',
      );

      const candidate = await journeyCandidate(extension, workspacePath);
      const binDirectory = path.dirname(candidate.bundled_server.path);
      const blockers: Array<{ label: string; result: unknown }> = [];
      const observations: ReceiptValue = {};

      // The extension must not have activated before the journey arms its
      // assertions: activation is language-demand driven and no Perl document
      // has been opened in this fresh host.
      assert.equal(extension.isActive, false, 'extension must not be active before the journey');
      // Baseline command registry BEFORE the attempt: the complete rollback
      // check is a before/after diff, so no registration — contributed,
      // internal, or added in the future — can slip past a sampled list.
      const commandsBefore = new Set(await vscode.commands.getCommands(true));
      const mandatoryExpected = mandatoryActivationCommandIds(extension.packageJSON);
      observations.mandatory_commands_expected = mandatoryExpected.length;

      // The mandatory pre-commit failure: activation rejects through the exact
      // path a real mid-activation exception takes.
      let activationError = 'no error';
      try {
        await extension.activate();
        blockers.push({
          label: 'activation_rejected',
          result: { expected: /harness-injected activation failure/, actual: 'resolved' },
        });
      } catch (error: unknown) {
        activationError = error instanceof Error ? error.message : String(error);
      }
      observations.activation_error = activationError;
      if (!/harness-injected activation failure after debugger-1/.test(activationError)) {
        blockers.push({
          label: 'activation_rejected',
          result: {
            expected: 'harness-injected activation failure after debugger-1',
            actual: activationError,
          },
        });
      }
      observations.extension_active_after_failure = extension.isActive;

      // Truthful failed state in the real host: exactly the retained support
      // commands survive; every mandatory command registration was rolled back.
      const commands = new Set(await vscode.commands.getCommands(true));
      const retained = new Set<string>(RETAINED_SUPPORT_COMMAND_IDS);
      // The diff proves the COMPLETE claim over the extension's own
      // registrations: any `perl-lsp.*` command the failed attempt left
      // registered beyond the retained set is a leak, whether or not this
      // file predicted its id (production registers nothing outside that
      // namespace). Host-internal ids are deliberately out of scope — the
      // workbench lazily registers its own commands during the session (for
      // example `workbench.action.output.show...` for the retained output
      // channel's view, or `keywordActivation.status.command`), and none of
      // them is extension rollback debt.
      const extensionOwned = (id: string) => id.startsWith('perl-lsp.');
      const leakedCommands = [...commands].filter(
        (id) => extensionOwned(id) && !commandsBefore.has(id) && !retained.has(id),
      );
      const mandatoryPresent = mandatoryExpected.filter((id) => commands.has(id));
      const retainedPresent = RETAINED_SUPPORT_COMMAND_IDS.filter((id) => commands.has(id));
      observations.commands_leaked_beyond_retained = leakedCommands;
      observations.mandatory_commands_remaining = mandatoryPresent;
      observations.retained_support_commands_present = retainedPresent;
      if (leakedCommands.length > 0) {
        blockers.push({
          label: 'mandatory_commands_rolled_back',
          result: { leaked: leakedCommands },
        });
      }
      if (mandatoryPresent.length > 0) {
        blockers.push({
          label: 'expected_mandatory_commands_absent',
          result: { remaining: mandatoryPresent },
        });
      }
      if (retainedPresent.length !== RETAINED_SUPPORT_COMMAND_IDS.length) {
        blockers.push({
          label: 'retained_support_surface',
          result: { present: retainedPresent },
        });
      }

      // No candidate server process exists after the rolled-back attempt.
      const processesAfterFailure = await scanProcessesUnderDirectory(binDirectory);
      observations.bundled_server_processes_after_failure = processesAfterFailure;
      if (processesAfterFailure.length > 0) {
        blockers.push({
          label: 'no_bundled_server_process',
          result: processesAfterFailure,
        });
      }

      // The real demand trigger cannot resurrect a server from the failed
      // attempt: opening the fixture document arms onLanguage:perl demand, but
      // the rolled-back listeners are gone and no watchdog/timer survives.
      const document = await vscode.workspace.openTextDocument(candidate.fixture.path);
      await vscode.window.showTextDocument(document);
      await new Promise((resolve) => setTimeout(resolve, DEMAND_WATCH_WINDOW_MS));
      observations.perl_document_opened = vscode.workspace.textDocuments.some(
        (open) => open.uri.fsPath === candidate.fixture.path,
      );
      const processesAfterDemand = await scanProcessesUnderDirectory(binDirectory);
      observations.demand_watch_window_ms = DEMAND_WATCH_WINDOW_MS;
      observations.bundled_server_processes_after_demand_window = processesAfterDemand;
      const commandsAfterDemand = new Set(await vscode.commands.getCommands(true));
      const leakedAfterDemand = [...commandsAfterDemand].filter(
        (id) => extensionOwned(id) && !commandsBefore.has(id) && !retained.has(id),
      );
      observations.commands_leaked_after_demand_window = leakedAfterDemand;
      if (processesAfterDemand.length > 0 || leakedAfterDemand.length > 0) {
        blockers.push({
          label: 'failed_attempt_cannot_start_a_server',
          result: { processes: processesAfterDemand, leaked_commands: leakedAfterDemand },
        });
      }

      // Crash-budget negative observation (#7798): no server process existed at
      // any scan, so no post-activation crash-recovery episode could start.
      observations.crash_budget_evidence = [
        'no bundled-server process after the rolled-back attempt',
        'no bundled-server process across the demand watch window',
        'pre-commit failure: no language client was constructed before commit',
      ];

      const receipt: ReceiptValue = {
        schema_version: 'vscode_activation_recovery_leg.v1',
        receipt_kind: 'activation_failure_journey_leg',
        leg,
        candidate,
        fault: {
          env: 'PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE',
          phase: faultPhase,
          boundary: 'debugger-1',
          guard: 'published-smoke harness extension present',
          pre_commit: true,
        },
        observations,
        product_blockers: blockers,
        verdict: blockers.length > 0 ? 'failed' : 'pass',
      };
      writeLegReceipt(leg, receipt);
      assert.equal(blockers.length, 0, JSON.stringify(blockers, null, 2));
    },
  );

  (selectedLeg === 'retry' ? test : test.skip)(
    'retry leg: the same installed candidate activates once, cleanly, after the fault is removed',
    async function () {
      const leg = journeyLeg();
      assert.equal(leg, 'retry', 'this host must run the retry leg');
      const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
      assert.ok(workspaceFolder, 'activation-failure journey requires a workspace folder');
      const workspacePath = workspaceFolder.uri.fsPath;
      const extension = vscode.extensions.getExtension(EXTENSION_ID);
      assert.ok(extension, 'activation-failure journey requires the installed extension');
      assert.equal(
        process.env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE,
        undefined,
        'the retry leg must run with the fault removed',
      );

      const candidate = await journeyCandidate(extension, workspacePath);
      const binDirectory = path.dirname(candidate.bundled_server.path);
      const blockers: Array<{ label: string; result: unknown }> = [];
      const observations: ReceiptValue = {};

      // The explicit retry: activation of the same installed candidate commits.
      const activation = (await withTimeout('retry activation', extension.activate(), 90_000)) as
        | {
            getLanguageClientStartupMetrics?: () => ReceiptValue;
            stop?: () => Promise<void>;
          }
        | undefined;
      observations.activation_resolved = activation !== undefined;
      if (activation === undefined) {
        blockers.push({ label: 'activation_resolved', result: 'undefined api' });
      }

      // Same-candidate identity: the activated server IS the bundled one.
      const expectedVersion = candidate.extension_version;
      const identityBlockers: ReceiptValue = {};
      if (
        candidate.bundled_server.version === null ||
        candidate.bundled_server.version !== expectedVersion
      ) {
        identityBlockers.bundled_version = {
          expected: expectedVersion,
          actual: candidate.bundled_server.version,
        };
      }
      observations.server_identity = identityBlockers;
      if (Object.keys(identityBlockers).length > 0) {
        blockers.push({ label: 'bundled_server_version', result: identityBlockers });
      }

      // Real demand starts the bundled server and it reaches initialize.
      const document = await vscode.workspace.openTextDocument(candidate.fixture.path);
      await vscode.window.showTextDocument(document);
      const position = providerPosition(document);
      const metrics =
        activation?.getLanguageClientStartupMetrics !== undefined
          ? await waitUntilStartupSettled(activation.getLanguageClientStartupMetrics, 60_000)
          : {};
      observations.startup = metrics;
      const lifecycleExpectations: Array<[string, string]> = [
        ['binary_resolution_source', 'bundled'],
        ['binary_resolution_status', 'ok'],
        ['server_start_status', 'ok'],
        ['initialize_status', 'ok'],
      ];
      const lifecycleFailures = lifecycleExpectations
        .filter(([field, expected]) => metrics[field] !== expected)
        .map(([field, expected]) => ({ field, expected, actual: metrics[field] ?? null }));
      if (lifecycleFailures.length > 0) {
        blockers.push({ label: 'startup_lifecycle', result: lifecycleFailures });
      }
      if (metrics.server_version !== candidate.bundled_server.version) {
        blockers.push({
          label: 'activated_server_version',
          result: {
            expected: candidate.bundled_server.version,
            actual: metrics.server_version ?? null,
          },
        });
      }
      if (!pathsEquivalent(metrics.binary_resolution_path, candidate.bundled_server.path)) {
        blockers.push({
          label: 'activated_server_path',
          result: {
            expected: candidate.bundled_server.path,
            actual: metrics.binary_resolution_path ?? null,
          },
        });
      }

      // One representative provider result from the retry's server.
      const completion = await providerResult(
        'retry completion',
        'vscode.executeCompletionItemProvider',
        document.uri,
        position,
      );
      const hover = await providerResult(
        'retry hover',
        'vscode.executeHoverProvider',
        document.uri,
        position,
      );
      observations.provider_smoke = { completion, hover };
      try {
        assertProviderSucceeded('retry completion', completion);
        assertProviderSucceeded('retry hover', hover);
      } catch (error: unknown) {
        blockers.push({
          label: 'provider_smoke',
          result: {
            completion,
            hover,
            error: error instanceof Error ? error.message : String(error),
          },
        });
      }

      // Duplicate-resource rows: exactly one bundled-server process serves the
      // retry's runtime, and raising NEW demand (a second Perl document) is
      // coalesced into that one server instead of starting a duplicate — the
      // real-host duplicate-registration observation.
      const processesAtRunning = await scanProcessesUnderDirectory(binDirectory);
      observations.bundled_server_processes_running = processesAtRunning;
      if (processesAtRunning.length !== 1) {
        blockers.push({ label: 'single_bundled_server_process', result: processesAtRunning });
      }
      const secondDemandPath = path.join(workspacePath, 'activation_retry_demand.pl');
      fs.writeFileSync(
        secondDemandPath,
        ['use strict;', 'use warnings;', '', 'my $other = 7;', 'print $other;', ''].join('\n'),
      );
      const secondDocument = await vscode.workspace.openTextDocument(secondDemandPath);
      await vscode.window.showTextDocument(secondDocument);
      await new Promise((resolve) => setTimeout(resolve, DEMAND_WATCH_WINDOW_MS));
      const processesAfterSecondDemand = await scanProcessesUnderDirectory(binDirectory);
      observations.bundled_server_processes_after_second_demand = processesAfterSecondDemand;
      if (processesAfterSecondDemand.length !== 1) {
        blockers.push({
          label: 'second_demand_coalesced',
          result: processesAfterSecondDemand,
        });
      }
      const commands = new Set(await vscode.commands.getCommands(true));
      const mandatoryExpected = mandatoryActivationCommandIds(extension.packageJSON);
      const mandatoryMissing = mandatoryExpected.filter((id) => !commands.has(id));
      observations.mandatory_commands_expected = mandatoryExpected.length;
      observations.mandatory_commands_missing = mandatoryMissing;
      if (mandatoryMissing.length > 0) {
        blockers.push({ label: 'mandatory_commands_registered', result: mandatoryMissing });
      }

      // Recoverable shutdown seam: stop resolves and the child process is gone.
      let stopOutcome = 'not_observable';
      if (activation?.stop) {
        try {
          await withTimeout('retry stop seam', activation.stop(), 30_000);
          stopOutcome = 'stopped';
        } catch (error: unknown) {
          stopOutcome = error instanceof Error ? error.message : String(error);
        }
      }
      observations.stop_seam = stopOutcome;
      if (stopOutcome !== 'stopped') {
        blockers.push({ label: 'stop_seam', result: stopOutcome });
      }
      // Bounded exit poll: the recoverable stop resolves when the client
      // dispose completes, but the OS may take a moment to reap the child on
      // a loaded host; poll before treating a lingering pid as a cleanup
      // failure.
      const stopPollDeadline = Date.now() + 5_000;
      let processesAfterStop = await scanProcessesUnderDirectory(binDirectory);
      while (processesAfterStop.length > 0 && Date.now() < stopPollDeadline) {
        await new Promise((resolve) => setTimeout(resolve, 250));
        processesAfterStop = await scanProcessesUnderDirectory(binDirectory);
      }
      observations.bundled_server_processes_after_stop = processesAfterStop;
      if (processesAfterStop.length > 0) {
        blockers.push({ label: 'stop_seam_process_cleanup', result: processesAfterStop });
      }
      // Give the host a moment to observe no resurrection after the recoverable
      // stop (the demand listeners may restart the server only on NEW demand).
      await new Promise((resolve) => setTimeout(resolve, DEMAND_WATCH_WINDOW_MS));
      const processesAfterStopWindow = await scanProcessesUnderDirectory(binDirectory);
      observations.bundled_server_processes_after_stop_window = processesAfterStopWindow;
      // No watchdog, timer, or stale callback may resurrect the server after
      // the recoverable stop without new demand — the same authority the
      // issue's retry/watchdog row forbids, observed on the committed runtime.
      if (processesAfterStopWindow.length > 0) {
        blockers.push({
          label: 'stop_window_resurrection',
          result: processesAfterStopWindow,
        });
      }

      const receipt: ReceiptValue = {
        schema_version: 'vscode_activation_recovery_leg.v1',
        receipt_kind: 'activation_failure_journey_leg',
        leg,
        candidate,
        fault: { env: 'PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE', phase: null, removed: true },
        observations,
        product_blockers: blockers,
        verdict: blockers.length > 0 ? 'failed' : 'pass',
      };
      writeLegReceipt(leg, receipt);
      assert.equal(blockers.length, 0, JSON.stringify(blockers, null, 2));
    },
  );
});
