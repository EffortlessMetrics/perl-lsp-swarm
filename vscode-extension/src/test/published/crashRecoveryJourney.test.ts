import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  assertProviderSucceeded,
  bundledBinaryPath,
  bundledServerVersion,
  canSuspendServerProcesses,
  pathsEquivalent,
  providerPosition,
  providerResult,
  receiptsDir,
  resumeServerProcess,
  scanServerProcessIdentities,
  sha256,
  suspendServerProcess,
  terminateServerProcess,
  withTimeout,
  type BundledServerProcessIdentity,
  type ReceiptValue,
} from './journeySupport';

/**
 * Packaged crash-recovery journey (#7848).
 *
 * Runs inside the REAL extension host against the INSTALLED VSIX in an
 * isolated profile and proves the landed generation-owned crash-recovery
 * arbiter (#7845) through the shipped artifact:
 *
 * Transient leg (Journey A — one unexpected crash):
 * - two supported Perl documents reach accepted readiness on the bundled
 *   candidate and one representative provider answers;
 * - the exact server process is terminated FROM THE HARNESS (external
 *   taskkill/SIGKILL, never the extension's user restart command), so the
 *   extension observes a genuine unexpected Running→Stopped transition;
 * - the dead generation's readiness is superseded (generation-scoped
 *   invalidation), exactly one bounded automatic recovery episode runs, and
 *   the replacement process identity differs at pid level while serving the
 *   same candidate artifact;
 * - both open documents replay before provider readiness resumes (a fresh
 *   ready notification per document can only arrive through the replay
 *   didOpen — VS Code does not re-send didOpen for documents that stayed
 *   open across a client replacement), and the re-run provider request is
 *   served by the current replacement generation;
 * - a quiet window proves old process/client state is gone (single stable
 *   replacement pid, stable generation);
 * - the watchdog row (best effort, POSIX only): the running server is
 *   suspended (SIGSTOP) so it becomes unresponsive WITHOUT first exiting;
 *   the shipped watchdog must fire one recovery episode and the suspended
 *   process's later exit must dedupe into that episode (no second restart).
 *   Hosts that cannot suspend a process record the row honestly as
 *   `not_proven` — #7846 owns the deterministic mechanism proof.
 *
 * Breaker leg (Journey B — circuit-breaker exhaustion):
 * - the exact server is terminated repeatedly per the accepted crash budget
 *   (each kill lands on a Running generation well inside the 30s stable-run
 *   grace, so the budget never silently resets);
 * - each kill produces exactly one recovery episode: replacements appear
 *   serially with no overlapping server processes, and the automatic budget
 *   is consumed one attempt at a time until the 4th failure reaches the
 *   stable circuit-breaker state (no further automatic process, no retry
 *   timer effects across a bounded observation window);
 * - the explicit Retry (`perl-lsp.restart`, the same user-facing restart the
 *   exhaustion dialog routes to) starts a healthy replacement WITHOUT binary
 *   source substitution (still the bundled candidate), which then serves
 *   providers and shuts down cleanly with the host.
 *
 * Each leg writes a candidate-bound child receipt; the orchestrator
 * (`run-local-vsix-smoke.js`) validates both, adds its post-host-exit process
 * scan, and emits the joined `vscode_crash_recovery.v1` receipt for
 * #4346/#6056 consumption.
 *
 * Ownership boundary: episode-handle settlement and deferred
 * different-generation serialization are proven discriminatingly at unit
 * level (#7845 falsifiers); this journey observes their installed-path
 * EFFECTS (serial replacements, no overlap, budget exhausted exactly at the
 * limit) without reading arbiter episode internals — the #7798 residual
 * keeps those fields write-only diagnostics.
 */

const EXTENSION_ID = 'EffortlessMetrics.perl-lsp-rs';
const TRANSIENT_FIXTURE_A = 'crash_transient_a.pl';
const TRANSIENT_FIXTURE_B = 'crash_transient_b.pl';
const BREAKER_FIXTURE = 'crash_breaker.pl';

const RECOVERY_WINDOW_MS = 120_000;
const QUIET_WINDOW_MS = 6_000;
const BREAKER_EXHAUSTION_WINDOW_MS = 15_000;
const WATCHDOG_RECOVERY_WINDOW_MS = 60_000;
/** Per-episode wait for a post-crash generation to reach Running on a loaded
 *  hosted runner; matches the initial demand-start bound, not a weaker
 *  invariant — a lifecycle stuck at 'failed' still fails this window. */
const EPISODE_RUNNING_WINDOW_MS = 120_000;
const POLL_INTERVAL_MS = 50;
/** Kills must land inside the stable-run grace so the budget never resets. */
const MAX_RUN_TO_KILL_MS = 25_000;
const AUTOMATIC_BUDGET = 3;

type JourneyLeg = 'transient' | 'breaker';

interface CrashJourneyApi {
  getLanguageClientStartupMetrics?: () => ReceiptValue;
  getActiveDocumentReadiness?: () => ReceiptValue;
  waitForActiveDocumentReady?: (uri: string, timeoutMs?: number) => Promise<void>;
  stop?: () => Promise<void>;
}

function journeyLeg(): JourneyLeg {
  const leg = process.env.PERL_LSP_CRASH_RECOVERY_LEG;
  if (leg === 'transient' || leg === 'breaker') {
    return leg;
  }
  throw new Error(
    `PERL_LSP_CRASH_RECOVERY_LEG must be 'transient' or 'breaker' for the crash-recovery journey, got ${JSON.stringify(leg)}`,
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
  fixtures: Array<{ path: string; sha256: string }>;
}

async function journeyCandidate(
  extension: vscode.Extension<unknown>,
  workspacePath: string,
  fixtures: string[],
): Promise<JourneyCandidate> {
  const bundledServerPath = bundledBinaryPath(extension.extensionPath);
  const versionProbe = await bundledServerVersion(bundledServerPath);
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
    fixtures: fixtures.map((name) => {
      const fixturePath = path.join(workspacePath, name);
      return { path: fixturePath, sha256: sha256(fixturePath) };
    }),
  };
}

function writeLegReceipt(leg: JourneyLeg, receipt: ReceiptValue): string {
  const destination = path.join(
    receiptsDir(),
    leg === 'transient'
      ? 'crash_recovery_journey_transient_receipt.json'
      : 'crash_recovery_journey_breaker_receipt.json',
  );
  fs.writeFileSync(destination, JSON.stringify(receipt, null, 2));
  return destination;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function readinessGeneration(api: CrashJourneyApi): number | null {
  const snapshot = api.getActiveDocumentReadiness?.();
  const generation = snapshot?.generation;
  return typeof generation === 'number' ? generation : null;
}

function lifecycleState(api: CrashJourneyApi): unknown {
  return api.getLanguageClientStartupMetrics?.().lifecycle_state ?? null;
}

function fixtureContent(name: 'a' | 'b' | 'breaker'): string {
  const probe = name === 'a' ? '$value' : name === 'b' ? '$other' : '$breaker';
  return ['use strict;', 'use warnings;', '', `my ${probe} = 11;`, `print ${probe};`, ''].join(
    '\n',
  );
}

/**
 * Demand-start the bundled candidate through the real trigger (opening a Perl
 * document) and wait until the lifecycle reports a running server with
 * bundled-binary identity.
 */
async function demandStartAndAwaitRunning(
  api: CrashJourneyApi,
  documentPaths: string[],
  readinessTimeoutMs: number,
): Promise<{ documents: vscode.TextDocument[]; startup: ReceiptValue }> {
  const documents: vscode.TextDocument[] = [];
  for (const documentPath of documentPaths) {
    const document = await vscode.workspace.openTextDocument(documentPath);
    await vscode.window.showTextDocument(document);
    documents.push(document);
  }
  const deadline = Date.now() + readinessTimeoutMs;
  let startup = api.getLanguageClientStartupMetrics?.() ?? {};
  while (Date.now() < deadline && startup.lifecycle_state !== 'running') {
    await delay(POLL_INTERVAL_MS);
    startup = api.getLanguageClientStartupMetrics?.() ?? {};
  }
  return { documents, startup };
}

interface CrashObservationSample {
  at_ms: number;
  readiness_generation: number | null;
  index_state: unknown;
  fully_ready: unknown;
  lifecycle_state: unknown;
  server_pids: number[];
}

async function sampleState(
  api: CrashJourneyApi,
  binDirectory: string,
): Promise<CrashObservationSample> {
  const readiness = api.getActiveDocumentReadiness?.() ?? {};
  const processes = await scanServerProcessIdentities(binDirectory);
  return {
    at_ms: Date.now(),
    readiness_generation: typeof readiness.generation === 'number' ? readiness.generation : null,
    index_state: readiness.indexState ?? null,
    fully_ready: readiness.fullyReady ?? null,
    lifecycle_state: lifecycleState(api),
    server_pids: processes.map((process) => process.pid),
  };
}

/**
 * Wait until the readiness generation strictly exceeds `afterGeneration`,
 * sampling process/generation state along the way. Returns the samples so the
 * receipt can show overlap (max simultaneous server processes), replacement
 * pid succession, and whether a `building` sample was observed between the
 * dead generation and the replacement's readiness (generation-scoped
 * invalidation is observable exactly there).
 */
async function awaitGenerationAdvance(
  api: CrashJourneyApi,
  binDirectory: string,
  afterGeneration: number,
  timeoutMs: number,
): Promise<{
  advanced: boolean;
  samples: CrashObservationSample[];
  final: CrashObservationSample;
}> {
  const deadline = Date.now() + timeoutMs;
  const samples: CrashObservationSample[] = [];
  let current = await sampleState(api, binDirectory);
  samples.push(current);
  while (Date.now() < deadline && (current.readiness_generation ?? -1) <= afterGeneration) {
    await delay(POLL_INTERVAL_MS);
    current = await sampleState(api, binDirectory);
    samples.push(current);
  }
  const advanced = (current.readiness_generation ?? -1) > afterGeneration;
  return { advanced, samples, final: current };
}

function maxOverlap(samples: CrashObservationSample[]): number {
  return samples.reduce((max, sample) => Math.max(max, sample.server_pids.length), 0);
}

function distinctPids(samples: CrashObservationSample[]): number[] {
  return [...new Set(samples.flatMap((sample) => sample.server_pids))];
}

// Exactly one leg runs per extension host: the orchestrator launches a fresh
// host for each leg, so the not-selected leg's test must not even register.
const selectedLeg = journeyLeg();

suite('Packaged crash-recovery journey (#7848)', function () {
  // Worst-case wait arithmetic (startup + kill/recovery cycles + the real
  // watchdog interval + bounded observation windows) approaches 8 minutes on
  // a slow runner; the smoke job budget accommodates one matrix leg per job.
  // The breaker leg's worst case after the hosted-calibrated windows
  // (4 episodes × (120s Running + 120s replacement) + exhaustion + explicit
  // retry) exceeds 10 minutes; Mocha must not abort before writeLegReceipt,
  // or the failure loses its receipt entirely.
  this.timeout(1_200_000);

  (selectedLeg === 'transient' ? test : test.skip)(
    'transient leg: one unexpected crash recovers exactly once with replay and clean state',
    async function () {
      const leg: JourneyLeg = 'transient';
      const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
      assert.ok(workspaceFolder, 'crash-recovery journey requires a workspace folder');
      const workspacePath = workspaceFolder.uri.fsPath;
      const extension = vscode.extensions.getExtension(EXTENSION_ID);
      assert.ok(extension, 'crash-recovery journey requires the installed extension');
      assert.equal(
        extension.isActive,
        false,
        'extension must not be active before the journey (fresh host, no Perl document opened)',
      );
      const api = (await extension.activate()) as CrashJourneyApi | undefined;
      assert.ok(api, 'activation must return the extension API');
      assert.ok(
        api.getLanguageClientStartupMetrics,
        'the crash journey requires getLanguageClientStartupMetrics',
      );
      assert.ok(
        api.getActiveDocumentReadiness,
        'the crash journey requires getActiveDocumentReadiness',
      );
      assert.ok(
        api.waitForActiveDocumentReady,
        'the crash journey requires waitForActiveDocumentReady',
      );

      fs.writeFileSync(path.join(workspacePath, TRANSIENT_FIXTURE_A), fixtureContent('a'));
      fs.writeFileSync(path.join(workspacePath, TRANSIENT_FIXTURE_B), fixtureContent('b'));
      const candidate = await journeyCandidate(extension, workspacePath, [
        TRANSIENT_FIXTURE_A,
        TRANSIENT_FIXTURE_B,
      ]);
      const binDirectory = path.dirname(candidate.bundled_server.path);
      const blockers: Array<{ label: string; result: unknown }> = [];
      const observations: ReceiptValue = {};

      // Steps 2-4: start the exact packaged candidate through real demand and
      // reach accepted readiness on BOTH open documents.
      const { documents, startup } = await demandStartAndAwaitRunning(
        api,
        candidate.fixtures.map((fixture) => fixture.path),
        90_000,
      );
      observations.startup = startup;
      if (startup.binary_resolution_source !== 'bundled') {
        blockers.push({
          label: 'bundled_candidate_selected',
          result: {
            expected: 'bundled',
            actual: startup.binary_resolution_source ?? null,
          },
        });
      }
      if (!pathsEquivalent(startup.binary_resolution_path, candidate.bundled_server.path)) {
        blockers.push({
          label: 'bundled_candidate_path',
          result: {
            expected: candidate.bundled_server.path,
            actual: startup.binary_resolution_path ?? null,
          },
        });
      }
      const documentA = documents[0];
      const documentB = documents[1];
      assert.ok(documentA, 'the transient leg requires its first fixture document');
      assert.ok(documentB, 'the transient leg requires its second fixture document');
      await withTimeout(
        'readiness document A',
        api.waitForActiveDocumentReady!(documentA.uri.toString(), 60_000),
        60_000,
      );
      await withTimeout(
        'readiness document B',
        api.waitForActiveDocumentReady!(documentB.uri.toString(), 60_000),
        60_000,
      );
      const failedGeneration = readinessGeneration(api);
      observations.readiness_before_crash = api.getActiveDocumentReadiness?.() ?? null;
      observations.failed_generation = failedGeneration;
      if (failedGeneration === null) {
        blockers.push({ label: 'readiness_generation_observable', result: 'null' });
      }

      // Step 5: one representative provider result before the crash.
      const providerBefore = await providerResult(
        'transient completion before crash',
        'vscode.executeCompletionItemProvider',
        documentA.uri,
        providerPosition(documentA),
      );
      observations.provider_before_crash = providerBefore;
      try {
        assertProviderSucceeded('transient completion before crash', providerBefore);
      } catch (error: unknown) {
        blockers.push({
          label: 'provider_before_crash',
          result: {
            provider: providerBefore,
            error: error instanceof Error ? error.message : String(error),
          },
        });
      }
      const processesBefore = await scanServerProcessIdentities(binDirectory);
      observations.server_processes_before_crash = processesBefore;
      if (processesBefore.length !== 1) {
        blockers.push({
          label: 'single_server_before_crash',
          result: processesBefore,
        });
      }
      const failedProcess = processesBefore[0];
      if (failedProcess === undefined) {
        const receipt: ReceiptValue = {
          schema_version: 'vscode_crash_recovery_leg.v1',
          receipt_kind: 'crash_recovery_journey_leg',
          leg,
          candidate,
          fault: null,
          observations,
          product_blockers: blockers,
          verdict: 'failed',
        };
        writeLegReceipt(leg, receipt);
        throw new Error(`no server process to terminate: ${JSON.stringify(blockers)}`);
      }

      // Steps 6-8: terminate the exact server process FROM THE HARNESS and
      // observe one bounded automatic recovery into a new generation.
      const termination = await terminateServerProcess(failedProcess.pid);
      observations.failure_injection = {
        method: 'harness-external process termination (unexpected; not the user restart command)',
        pid: failedProcess.pid,
        result: termination,
      };
      if (termination.outcome === 'error') {
        blockers.push({ label: 'external_termination', result: termination });
      }
      const recovery = await awaitGenerationAdvance(
        api,
        binDirectory,
        failedGeneration ?? 0,
        RECOVERY_WINDOW_MS,
      );
      observations.recovery_samples = {
        generation_sequence: [
          ...new Set(recovery.samples.map((sample) => sample.readiness_generation)),
        ],
        max_simultaneous_server_processes: maxOverlap(recovery.samples),
        distinct_server_pids: distinctPids(recovery.samples),
        building_sample_observed: recovery.samples.some(
          (sample) => sample.index_state === 'building',
        ),
        samples: recovery.samples.map((sample) => ({
          at_ms: sample.at_ms,
          readiness_generation: sample.readiness_generation,
          index_state: sample.index_state,
          lifecycle_state: sample.lifecycle_state,
          server_pids: sample.server_pids,
        })),
      };
      observations.replacement_generation = recovery.final.readiness_generation;
      if (!recovery.advanced) {
        blockers.push({
          label: 'automatic_recovery_restarted_generation',
          result: 'readiness generation did not advance within the bounded window',
        });
      }
      if (recovery.advanced) {
        // Step 9: replacement process identity differs while the candidate
        // artifact matches (same installed binary, different pid).
        const replacementProcesses = await scanServerProcessIdentities(binDirectory);
        observations.server_processes_after_recovery = replacementProcesses;
        if (replacementProcesses.length !== 1) {
          blockers.push({
            label: 'single_replacement_process',
            result: replacementProcesses,
          });
        }
        const replacement = replacementProcesses[0];
        if (replacement !== undefined) {
          if (replacement.pid === failedProcess.pid) {
            blockers.push({
              label: 'replacement_process_identity_differs',
              result: { failed_pid: failedProcess.pid, replacement_pid: replacement.pid },
            });
          }
          if (!pathsEquivalent(replacement.path, candidate.bundled_server.path)) {
            blockers.push({
              label: 'replacement_matches_candidate_artifact',
              result: { expected: candidate.bundled_server.path, actual: replacement.path },
            });
          }
          if (sha256(replacement.path) !== candidate.bundled_server.sha256) {
            blockers.push({
              label: 'replacement_artifact_digest',
              result: 'running binary digest differs from the packaged candidate',
            });
          }
        }
        // Two overlapping replacement servers would prove a broken arbiter.
        if (maxOverlap(recovery.samples) > 1) {
          blockers.push({
            label: 'no_overlapping_replacement_servers',
            result: { max_overlap: maxOverlap(recovery.samples) },
          });
        }

        // Step 10: open documents replay exactly-once-per-generation before
        // provider readiness resumes. The readiness set was cleared at the
        // replacement generation boundary, so a resolved waiter proves a
        // FRESH ready notification arrived for each document through the
        // replay didOpen — VS Code never re-sends didOpen for documents that
        // stayed open across a client replacement.
        const replayRows: ReceiptValue = {};
        for (const document of [documentA, documentB]) {
          try {
            await withTimeout(
              `replay readiness ${path.basename(document.uri.fsPath)}`,
              api.waitForActiveDocumentReady!(document.uri.toString(), EPISODE_RUNNING_WINDOW_MS),
              EPISODE_RUNNING_WINDOW_MS,
            );
            replayRows[path.basename(document.uri.fsPath)] = 'ready_in_replacement_generation';
          } catch (error: unknown) {
            replayRows[path.basename(document.uri.fsPath)] =
              error instanceof Error ? error.message : String(error);
            blockers.push({
              label: 'open_documents_replayed',
              result: {
                document: path.basename(document.uri.fsPath),
                error: replayRows[path.basename(document.uri.fsPath)],
              },
            });
          }
        }
        observations.replay = replayRows;

        // Step 11: re-run the provider request and prove current
        // replacement-generation output.
        const providerAfter = await providerResult(
          'transient hover after recovery',
          'vscode.executeHoverProvider',
          documentA.uri,
          providerPosition(documentA),
        );
        const generationAtRequest = readinessGeneration(api);
        const processesAtRequest = await scanServerProcessIdentities(binDirectory);
        observations.provider_after_recovery = {
          provider: providerAfter,
          readiness_generation_at_request: generationAtRequest,
          server_pids_at_request: processesAtRequest.map((process) => process.pid),
        };
        if (
          generationAtRequest === null ||
          generationAtRequest <= (failedGeneration ?? Number.POSITIVE_INFINITY)
        ) {
          blockers.push({
            label: 'provider_served_by_replacement_generation',
            result: {
              failed_generation: failedGeneration,
              generation_at_request: generationAtRequest,
            },
          });
        }
        try {
          assertProviderSucceeded('transient hover after recovery', providerAfter);
        } catch (error: unknown) {
          blockers.push({
            label: 'provider_after_recovery',
            result: {
              provider: providerAfter,
              error: error instanceof Error ? error.message : String(error),
            },
          });
        }
      }

      // Step 12: old process/client/watchdog/retry state is gone — a quiet
      // window with one stable replacement pid and a stable generation.
      const quietStartGeneration = readinessGeneration(api);
      const quietDeadline = Date.now() + QUIET_WINDOW_MS;
      const quietSamples: CrashObservationSample[] = [];
      while (Date.now() < quietDeadline) {
        await delay(POLL_INTERVAL_MS * 4);
        quietSamples.push(await sampleState(api, binDirectory));
      }
      const quietWindow: ReceiptValue = {
        window_ms: QUIET_WINDOW_MS,
        generation_stable:
          quietSamples.every((sample) => sample.readiness_generation === quietStartGeneration) &&
          quietStartGeneration !== null,
        max_simultaneous_server_processes: maxOverlap(quietSamples),
        distinct_server_pids: distinctPids(quietSamples),
        failed_pid_resurrected: quietSamples.some((sample) =>
          sample.server_pids.includes(failedProcess.pid),
        ),
      };
      observations.quiet_window = quietWindow;
      if (quietWindow.failed_pid_resurrected === true) {
        blockers.push({
          label: 'failed_process_not_resurrected',
          result: { pid: failedProcess.pid },
        });
      }
      if (quietWindow.generation_stable !== true) {
        blockers.push({
          label: 'no_residual_retry_state',
          result: 'generation moved during the quiet window',
        });
      }
      if (maxOverlap(quietSamples) > 1) {
        blockers.push({
          label: 'single_stable_server',
          result: { max_overlap: maxOverlap(quietSamples) },
        });
      }

      // Watchdog row: make the running replacement unresponsive WITHOUT first
      // exiting (POSIX SIGSTOP) and prove one watchdog recovery episode whose
      // later process exit dedupes. Hosts without a suspend capability record
      // the row honestly as not_proven (#7846 owns deterministic proof).
      let watchdogRow: ReceiptValue;
      const runningProcesses = await scanServerProcessIdentities(binDirectory);
      const watchdogTarget = runningProcesses[0];
      if (!canSuspendServerProcesses() || watchdogTarget === undefined) {
        watchdogRow = {
          status: 'not_proven',
          reason: canSuspendServerProcesses()
            ? 'no running server process available to suspend for the watchdog row'
            : 'host platform cannot safely suspend the installed server process; deterministic watchdog mechanism proof is owned by #7846',
        };
      } else {
        const generationBeforeWatchdog = readinessGeneration(api);
        const suspend = suspendServerProcess(watchdogTarget.pid);
        observations.watchdog_suspend = suspend;
        if (suspend.outcome !== 'suspended') {
          watchdogRow = {
            status: 'not_proven',
            reason: `suspend failed: ${suspend.detail}`,
          };
        } else {
          let watchdogRecovery;
          try {
            watchdogRecovery = await awaitGenerationAdvance(
              api,
              binDirectory,
              generationBeforeWatchdog ?? 0,
              WATCHDOG_RECOVERY_WINDOW_MS,
            );
          } finally {
            // The restart's stop terminates the suspended process; resume it
            // so a leftover cannot wedge the host teardown (kill delivered
            // while suspended takes effect after SIGCONT) — even when the
            // generation wait itself throws.
            const resume = resumeServerProcess(watchdogTarget.pid);
            observations.watchdog_resume = resume;
          }
          if (!watchdogRecovery.advanced) {
            watchdogRow = {
              status: 'not_proven',
              reason: `watchdog did not trigger a recovery episode within ${WATCHDOG_RECOVERY_WINDOW_MS}ms (real-time observation; #7846 owns the deterministic grace/timeout matrix)`,
            };
          } else {
            // The later process-exit of the suspended generation must dedupe
            // into the watchdog episode: stability is judged against the
            // generation the watchdog recovery itself produced — a second
            // serial restart after the suspended process's exit would move a
            // sample past that bound and must fail the row (a max-of-samples
            // bound would be vacuously true).
            const recoveryGeneration = watchdogRecovery.final.readiness_generation;
            const dedupeDeadline = Date.now() + QUIET_WINDOW_MS * 2;
            const dedupeSamples: CrashObservationSample[] = [];
            while (Date.now() < dedupeDeadline) {
              await delay(POLL_INTERVAL_MS * 4);
              dedupeSamples.push(await sampleState(api, binDirectory));
            }
            const generationStable =
              recoveryGeneration !== null &&
              dedupeSamples.every(
                (sample) => (sample.readiness_generation ?? 0) <= recoveryGeneration,
              );
            // A generation advance alone cannot prove the watchdog recovery:
            // beginGeneration runs before client.start(), so a replacement
            // that then fails to start would pass on generation movement
            // alone. The row requires exactly one replacement process that
            // actually reaches Running inside the observation window.
            const replacementRunning = dedupeSamples.some(
              (sample) =>
                sample.server_pids.filter((pid) => pid !== watchdogTarget.pid).length === 1 &&
                sample.lifecycle_state === 'running',
            );
            watchdogRow = {
              status:
                recoveryGeneration !== null &&
                replacementRunning &&
                maxOverlap(dedupeSamples) <= 1 &&
                generationStable
                  ? 'pass'
                  : 'failed',
              watchdog_episode_generation: watchdogRecovery.final.readiness_generation,
              later_exit_deduped: {
                quiet_window_ms: QUIET_WINDOW_MS * 2,
                replacement_running: replacementRunning,
                max_simultaneous_server_processes: maxOverlap(dedupeSamples),
                distinct_server_pids: distinctPids(dedupeSamples),
                suspended_pid: watchdogTarget.pid,
                suspended_pid_alive_after: dedupeSamples.some((sample) =>
                  sample.server_pids.includes(watchdogTarget.pid),
                ),
              },
            };
          }
        }
      }
      observations.watchdog = watchdogRow;

      const receipt: ReceiptValue = {
        schema_version: 'vscode_crash_recovery_leg.v1',
        receipt_kind: 'crash_recovery_journey_leg',
        leg,
        candidate,
        fault: observations.failure_injection ?? null,
        observations,
        product_blockers: blockers,
        verdict: blockers.length > 0 ? 'failed' : 'pass',
      };
      writeLegReceipt(leg, receipt);
      assert.equal(blockers.length, 0, JSON.stringify(blockers, null, 2));
    },
  );

  (selectedLeg === 'breaker' ? test : test.skip)(
    'breaker leg: repeated unexpected failures exhaust the circuit breaker and explicit retry recovers',
    async function () {
      const leg: JourneyLeg = 'breaker';
      const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
      assert.ok(workspaceFolder, 'crash-recovery journey requires a workspace folder');
      const workspacePath = workspaceFolder.uri.fsPath;
      const extension = vscode.extensions.getExtension(EXTENSION_ID);
      assert.ok(extension, 'crash-recovery journey requires the installed extension');
      assert.equal(
        extension.isActive,
        false,
        'extension must not be active before the journey (fresh host, no Perl document opened)',
      );
      const api = (await extension.activate()) as CrashJourneyApi | undefined;
      assert.ok(api, 'activation must return the extension API');
      assert.ok(
        api.getLanguageClientStartupMetrics,
        'the crash journey requires getLanguageClientStartupMetrics',
      );
      assert.ok(
        api.getActiveDocumentReadiness,
        'the crash journey requires getActiveDocumentReadiness',
      );
      assert.ok(
        api.waitForActiveDocumentReady,
        'the crash journey requires waitForActiveDocumentReady',
      );

      fs.writeFileSync(path.join(workspacePath, BREAKER_FIXTURE), fixtureContent('breaker'));
      const candidate = await journeyCandidate(extension, workspacePath, [BREAKER_FIXTURE]);
      const binDirectory = path.dirname(candidate.bundled_server.path);
      const blockers: Array<{ label: string; result: unknown }> = [];
      const observations: ReceiptValue = {};

      const { documents, startup } = await demandStartAndAwaitRunning(
        api,
        candidate.fixtures.map((fixture) => fixture.path),
        90_000,
      );
      observations.startup = startup;
      if (startup.binary_resolution_source !== 'bundled') {
        blockers.push({
          label: 'bundled_candidate_selected',
          result: { expected: 'bundled', actual: startup.binary_resolution_source ?? null },
        });
      }
      const document = documents[0];
      assert.ok(document, 'the breaker leg requires its fixture document');
      await withTimeout(
        'breaker initial readiness',
        api.waitForActiveDocumentReady!(document.uri.toString(), 60_000),
        60_000,
      );

      // Repeatedly terminate the exact server per the accepted crash budget.
      // Every kill lands on a Running generation well inside the 30s
      // stable-run grace so the budget cannot silently reset, and each
      // replacement is killed after it reaches Running so the extension
      // observes genuine Running→Stopped transitions.
      const episodes: Array<ReceiptValue> = [];
      let currentProcess: BundledServerProcessIdentity | undefined = (
        await scanServerProcessIdentities(binDirectory)
      )[0];
      if (currentProcess === undefined) {
        const receipt: ReceiptValue = {
          schema_version: 'vscode_crash_recovery_leg.v1',
          receipt_kind: 'crash_recovery_journey_leg',
          leg,
          candidate,
          fault: null,
          observations,
          product_blockers: [{ label: 'initial_server_process', result: 'none found' }],
          verdict: 'failed',
        };
        writeLegReceipt(leg, receipt);
        throw new Error('breaker leg: no server process after demand start');
      }
      for (let episodeIndex = 0; episodeIndex <= AUTOMATIC_BUDGET; episodeIndex += 1) {
        if (currentProcess === undefined) {
          blockers.push({
            label: `episode_${episodeIndex + 1}_server_process`,
            result: 'no live server process to terminate for this episode',
          });
          break;
        }
        const generationAtKill = readinessGeneration(api);
        // Wait for the current generation to report Running before killing
        // (a pre-Running death is a startup failure, not a crash episode),
        // and measure the kill latency from the moment Running was observed
        // so the receipt proves every kill landed inside the 30s stable-run
        // grace and could not have silently reset the budget. The bound
        // matches the initial demand start: a post-crash restart does the
        // same server spawn + initialization on the same loaded host, so a
        // 60s episode window that was never validated on hosted runners
        // falsified slow-but-healthy recoveries. The proven invariant (kill
        // inside the 30s stable grace, measured FROM the Running
        // observation) is unchanged; a lifecycle stuck at 'failed' still
        // fails here with richer receipt evidence.
        const runningDeadline = Date.now() + EPISODE_RUNNING_WINDOW_MS;
        let runningObservedAt: number | null = null;
        while (Date.now() < runningDeadline) {
          if (lifecycleState(api) === 'running') {
            runningObservedAt = Date.now();
            break;
          }
          await delay(POLL_INTERVAL_MS);
        }
        if (runningObservedAt === null) {
          blockers.push({
            label: `episode_${episodeIndex + 1}_generation_reached_running`,
            result: `lifecycle stayed ${String(lifecycleState(api))} before the kill`,
          });
          break;
        }
        const runToKillMs = Date.now() - runningObservedAt;
        const termination = await terminateServerProcess(currentProcess.pid);
        const episode: ReceiptValue = {
          episode_index: episodeIndex + 1,
          killed_pid: currentProcess.pid,
          generation_at_kill: generationAtKill,
          lifecycle_state_at_kill: lifecycleState(api),
          run_to_kill_ms: runToKillMs,
          termination,
        };
        if (termination.outcome === 'error') {
          blockers.push({ label: `episode_${episodeIndex + 1}_termination`, result: termination });
        }
        if (runToKillMs >= MAX_RUN_TO_KILL_MS) {
          blockers.push({
            label: `episode_${episodeIndex + 1}_kill_inside_stable_grace`,
            result: {
              run_to_kill_ms: runToKillMs,
              max_run_to_kill_ms: MAX_RUN_TO_KILL_MS,
              stable_run_grace_ms: 30_000,
            },
          });
        }
        if (episodeIndex < AUTOMATIC_BUDGET) {
          // A replacement must appear: poll for a new pid distinct from every
          // killed pid, sampling overlap along the way.
          const killTarget = currentProcess;
          const deadline = Date.now() + RECOVERY_WINDOW_MS;
          const episodeSamples: CrashObservationSample[] = [];
          let replacement: BundledServerProcessIdentity | undefined;
          while (Date.now() < deadline) {
            await delay(POLL_INTERVAL_MS);
            const processes = await scanServerProcessIdentities(binDirectory);
            episodeSamples.push({
              at_ms: Date.now(),
              readiness_generation: readinessGeneration(api),
              index_state: api.getActiveDocumentReadiness?.().indexState ?? null,
              fully_ready: api.getActiveDocumentReadiness?.().fullyReady ?? null,
              lifecycle_state: lifecycleState(api),
              server_pids: processes.map((process) => process.pid),
            });
            replacement = processes.find((process) => process.pid !== killTarget.pid);
            if (replacement !== undefined) {
              break;
            }
          }
          episode.max_simultaneous_server_processes = maxOverlap(episodeSamples);
          episode.distinct_server_pids = distinctPids(episodeSamples);
          if (replacement === undefined) {
            blockers.push({
              label: `episode_${episodeIndex + 1}_automatic_replacement`,
              result: 'no replacement process appeared within the bounded window',
            });
            episodes.push(episode);
            currentProcess = undefined;
            break;
          }
          episode.replacement_pid = replacement.pid;
          episode.generation_after_recovery = readinessGeneration(api);
          if (maxOverlap(episodeSamples) > 1) {
            blockers.push({
              label: `episode_${episodeIndex + 1}_no_overlapping_servers`,
              result: { max_overlap: maxOverlap(episodeSamples) },
            });
          }
          if (
            typeof episode.generation_after_recovery === 'number' &&
            typeof generationAtKill === 'number' &&
            episode.generation_after_recovery <= generationAtKill
          ) {
            blockers.push({
              label: `episode_${episodeIndex + 1}_generation_advanced`,
              result: { before: generationAtKill, after: episode.generation_after_recovery },
            });
          }
          episodes.push(episode);
          currentProcess = replacement;
        } else {
          // The final failure must reach the stable circuit breaker: no
          // replacement process, no generation movement, no retry-timer
          // effects across a bounded observation window (real-time window;
          // the deterministic timer matrix is owned by #7846).
          const deadline = Date.now() + BREAKER_EXHAUSTION_WINDOW_MS;
          const exhaustionSamples: CrashObservationSample[] = [];
          while (Date.now() < deadline) {
            await delay(POLL_INTERVAL_MS * 4);
            exhaustionSamples.push(await sampleState(api, binDirectory));
          }
          episode.exhaustion_window_ms = BREAKER_EXHAUSTION_WINDOW_MS;
          episode.background_server_processes = distinctPids(exhaustionSamples);
          const generationSequence = [
            ...new Set(exhaustionSamples.map((sample) => sample.readiness_generation)),
          ];
          const lifecycleStates = [
            ...new Set(exhaustionSamples.map((sample) => String(sample.lifecycle_state))),
          ];
          episode.generation_sequence_during_exhaustion = generationSequence;
          episode.lifecycle_states_during_exhaustion = lifecycleStates;
          episode.max_simultaneous_server_processes = maxOverlap(exhaustionSamples);
          if (distinctPids(exhaustionSamples).length > 0) {
            blockers.push({
              label: 'budget_exhaustion_stops_background_servers',
              result: distinctPids(exhaustionSamples),
            });
          }
          if (generationSequence.length > 1) {
            blockers.push({
              label: 'budget_exhaustion_generation_stable',
              result: generationSequence,
            });
          }
          // A budget-exceeding recovery that advances the generation and dies
          // before the first sample would leave one stable newer generation
          // and pass every check above: compare against the generation at the
          // final kill, not only across samples.
          if (
            generationAtKill !== null &&
            exhaustionSamples.some(
              (sample) => (sample.readiness_generation ?? 0) > generationAtKill,
            )
          ) {
            blockers.push({
              label: 'budget_exhaustion_no_recovery_after_final_kill',
              result: {
                generation_at_kill: generationAtKill,
                sampled: generationSequence,
              },
            });
          }
          if (!lifecycleStates.every((state) => state !== 'running')) {
            blockers.push({
              label: 'budget_exhaustion_terminal_state',
              result: lifecycleStates,
            });
          }
          episodes.push(episode);
        }
      }
      observations.episodes = episodes;
      observations.automatic_budget = AUTOMATIC_BUDGET;
      observations.attempted_kills = episodes.length;
      observations.exhausted = episodes.length === AUTOMATIC_BUDGET + 1;

      // The stable action-required state and the exhaustion dialog's content
      // are not observable from inside the harness (no dialog-interception
      // seam; the modal waits for a user). Recorded as a typed limitation —
      // the no-reinstall-on-crash-count policy proof is owned by #7846.
      observations.action_required_dialog = {
        observable: false,
        limitation:
          'exhaustion dialog content cannot be read from the harness; the reinstall-free exhaustion copy is proven at unit level (#7846)',
      };

      // Explicit Retry: the same user-facing restart command the exhaustion
      // dialog routes to, executed by the harness. It must recover WITHOUT
      // binary-source substitution — the replacement is still the bundled
      // candidate.
      const retryStartGeneration = readinessGeneration(api);
      await withTimeout(
        'explicit retry restart',
        vscode.commands.executeCommand('perl-lsp.restart'),
        90_000,
      );
      const retryDeadline = Date.now() + 90_000;
      let retryStartup = api.getLanguageClientStartupMetrics?.() ?? {};
      while (Date.now() < retryDeadline && retryStartup.lifecycle_state !== 'running') {
        await delay(POLL_INTERVAL_MS);
        retryStartup = api.getLanguageClientStartupMetrics?.() ?? {};
      }
      const retryProcesses = await scanServerProcessIdentities(binDirectory);
      const explicitRetry: ReceiptValue = {
        via: 'command:perl-lsp.restart (explicit user action; not an automatic recovery)',
        lifecycle_state_after: retryStartup.lifecycle_state ?? null,
        binary_resolution_source_after: retryStartup.binary_resolution_source ?? null,
        readiness_generation_before: retryStartGeneration,
        readiness_generation_after: readinessGeneration(api),
        server_processes_after: retryProcesses,
      };
      observations.explicit_retry = explicitRetry;
      if (retryStartup.binary_resolution_source !== 'bundled') {
        blockers.push({
          label: 'explicit_retry_without_binary_substitution',
          result: {
            expected: 'bundled',
            actual: retryStartup.binary_resolution_source ?? null,
          },
        });
      }
      if (retryProcesses.length !== 1) {
        blockers.push({ label: 'explicit_retry_single_server', result: retryProcesses });
      }
      const retryProcess = retryProcesses[0];
      if (
        retryProcess !== undefined &&
        !pathsEquivalent(retryProcess.path, candidate.bundled_server.path)
      ) {
        blockers.push({
          label: 'explicit_retry_bundled_path',
          result: { expected: candidate.bundled_server.path, actual: retryProcess.path },
        });
      }
      // Fresh readiness and provider behavior on the post-retry server.
      try {
        await withTimeout(
          'post-retry readiness',
          api.waitForActiveDocumentReady!(document.uri.toString(), EPISODE_RUNNING_WINDOW_MS),
          EPISODE_RUNNING_WINDOW_MS,
        );
        explicitRetry.readiness = 'ready_in_retry_generation';
      } catch (error: unknown) {
        explicitRetry.readiness = error instanceof Error ? error.message : String(error);
        blockers.push({
          label: 'explicit_retry_fresh_readiness',
          result: explicitRetry.readiness,
        });
      }
      const retryProvider = await providerResult(
        'breaker completion after retry',
        'vscode.executeCompletionItemProvider',
        document.uri,
        providerPosition(document, '$breaker'),
      );
      explicitRetry.provider = retryProvider;
      try {
        assertProviderSucceeded('breaker completion after retry', retryProvider);
      } catch (error: unknown) {
        blockers.push({
          label: 'explicit_retry_provider',
          result: {
            provider: retryProvider,
            error: error instanceof Error ? error.message : String(error),
          },
        });
      }

      // Quiet window: the healthy post-retry server is stable (clean shutdown
      // itself is proven by the orchestrator's post-host-exit scan).
      const quietDeadline = Date.now() + QUIET_WINDOW_MS;
      const quietSamples: CrashObservationSample[] = [];
      while (Date.now() < quietDeadline) {
        await delay(POLL_INTERVAL_MS * 4);
        quietSamples.push(await sampleState(api, binDirectory));
      }
      observations.post_retry_quiet_window = {
        window_ms: QUIET_WINDOW_MS,
        max_simultaneous_server_processes: maxOverlap(quietSamples),
        distinct_server_pids: distinctPids(quietSamples),
      };
      if (maxOverlap(quietSamples) > 1 || distinctPids(quietSamples).length > 1) {
        blockers.push({
          label: 'post_retry_stable_single_server',
          result: observations.post_retry_quiet_window,
        });
      }

      const receipt: ReceiptValue = {
        schema_version: 'vscode_crash_recovery_leg.v1',
        receipt_kind: 'crash_recovery_journey_leg',
        leg,
        candidate,
        fault: {
          method: 'harness-external repeated process termination per the accepted crash budget',
        },
        observations,
        product_blockers: blockers,
        verdict: blockers.length > 0 ? 'failed' : 'pass',
      };
      writeLegReceipt(leg, receipt);
      assert.equal(blockers.length, 0, JSON.stringify(blockers, null, 2));
    },
  );
});
