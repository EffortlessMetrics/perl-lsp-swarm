import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { execFile } from 'child_process';
import {
  LanguageClient,
  State as LanguageClientState,
  TransportKind,
  Trace,
} from 'vscode-languageclient/node';
import type {
  LanguageClientOptions,
  ServerOptions,
  StateChangeEvent,
} from 'vscode-languageclient/node';
import { PerlTestAdapter } from './testAdapter';
import { activateDebugger, rewriteTestLensCommand } from './debugAdapter';
import { BinaryDownloader, parseLocalVersion } from './downloader';
import {
  acquireLaunchManagedCandidateReference,
  mayReleaseManagedCandidateReferences,
  releaseManagedCandidateSessionReferences,
} from './managedCandidateRuntime';
import { runLanguageServerHealthCheck } from './languageServerHealth';
import { OnboardingManager } from './onboarding';
import {
  openDemoProjectCommand,
  suggestAiCompletionIfSupported,
  suggestDiscoveredIncludePaths,
  validateIncludePaths,
} from './extensionWorkspaceGuidance';
export {
  openDemoProjectCommand,
  suggestAiCompletionIfSupported,
  suggestDiscoveredIncludePaths,
  validateIncludePaths,
} from './extensionWorkspaceGuidance';
import {
  coexistenceReevaluationRequested,
  runCoexistenceAdvisory,
  showCoexistenceStatusCommand,
} from './coexistenceAdvisory';
import { registerCoexistenceCommandGroup } from './coexistenceCommandGroup';
import { WhatsNewManager } from './whatsNew';
import { generateBoilerplate } from './fileCreation';
import { handleFormattingError } from './formattingErrors';
import { HealthWidget, ClientState } from './healthWidget';
import { HealthWidgetDataSource } from './healthWidgetDataSource';
import { projectWorkspaceLifecycle } from './workspaceExperienceState';
import { registerPodPreview } from './podPreview';
import { registerGherkinProviders } from './gherkinProviders';
import { registerGherkinStepDefinitionSupport } from './gherkinStepDefinitions';
import { registerDocumentFeatureGroup } from './documentFeatureGroup';
import { StreamingCompletionController } from './streamingCompletion';
import { InlineCompletionOwner } from './inlineCompletionRouting';
import {
  runAllTestsWithProve,
  runCurrentTestWithProve,
  runTestAtCursorCommand,
  runTestsCommand,
} from './testCommands';
import { registerMcpSupport } from './mcpSupport';
import { registerServerCommandGroup } from './serverCommandGroup';
import {
  showBinaryIdentityStatus,
  type BinaryIdentityCommandHost,
  type BinaryIdentityRequestClient,
} from './binaryIdentityCommand';
import type { SelectedBinaryRole } from './binaryIdentityStatus';
import { registerCriticCommandGroup } from './criticCommandGroup';
import { registerTestCommandGroup } from './testCommandGroup';
import { registerOnboardingCommandGroup } from './onboardingCommandGroup';
import { registerNavigationCommandGroup } from './navigationCommandGroup';
import {
  showStatusMenuCommand,
  showWorkspaceStatusCommand,
  showVersionCommand,
} from './navigationCommands';
import { registerDiagnosticCommandGroup } from './diagnosticCommandGroup';
import {
  copyProviderDecisionReceiptCommand as copyProviderDecisionReceipt,
  explainDiagnosticCommand as explainDiagnostic,
  explainMissingModuleLookupCommand as explainMissingModuleLookup,
  explainProviderDecisionCommand as explainProviderDecision,
  previewPackageRenameCommand as previewPackageRename,
  previewSafeDeleteCommand as previewSafeDelete,
  showWorkspaceTrustReportCommand as showWorkspaceTrustReport,
} from './diagnosticCommands';
import type { DiagnosticCommandOptions, LspExecuteCommandClient } from './diagnosticCommands';
import { registerDocumentCommandGroup } from './documentCommandGroup';
import {
  formatDocumentCommand,
  openPerlModuleCommand,
  runCheckSyntaxCommand,
  showIncPathsCommand,
  showParserAstCommand,
} from './documentCommands';
import { registerRefactoringCommandGroup } from './refactoringCommandGroup';
import {
  extractMethodCommand,
  extractVariableCommand,
  showRefactoringOptionsCommand,
} from './refactoringCommands';
import { registerSupportCommandGroup } from './supportCommandGroup';
import { reportIssueCommand } from './supportCommands';
export { formatIssueDiagnosticInfo } from './supportCommands';
import { ExtensionLanguageClientLifecycle } from './extensionComposition';
import { LanguageClientLifecycleError } from './languageClientLifecycle';
import type { LifecycleState } from './languageClientLifecycle';
import {
  StaleDocumentReplayError,
  replayOpenPerlDocumentsWhenReady,
} from './languageClientDocumentSync';
import { settleLspProviderCallWithDisposition } from './lspProviderCall';
import {
  CrashRecoveryArbiter,
  type CrashObservationSource,
  type CrashRecoveryDecision,
  type RecoveryTerminalDisposition,
} from './crashRecoveryArbiter';
import {
  ServerDemandCoordinator,
  isServerDependentDocument,
  type ServerDemandSnapshot,
} from './serverDemand';
import type {
  BinaryResolutionSource,
  LanguageClientStartupMetricsSnapshot,
  LanguageClientStartupMilestone,
} from './languageClientStartupMetrics';
import { LanguageClientStartupMetrics } from './languageClientStartupMetrics';
import {
  FeatureActivationMetrics,
  type FeatureActivationMetricsSnapshot,
} from './featureActivationMetrics';
import { registerWorkspaceConfigurationEvents } from './extensionWorkspaceEvents';
import { workspaceTrustClientRuntimeState } from './workspaceTrustRuntimeState';
export { workspaceTrustClientRuntimeState } from './workspaceTrustRuntimeState';
import {
  buildDisabledFeaturesFromConfig,
  buildPerlCriticConfiguration as buildPerlCriticConfigurationPayload,
  CRITIC_SETTINGS,
  hasExplicitPerlCriticOverrides,
  syncLanguageClientConfiguration,
  syncUserAiCompletionConfiguration,
  syncPerlCriticConfiguration as syncPerlCriticConfigurationFromConfig,
} from './languageClientConfiguration';
export { buildDisabledFeaturesFromConfig } from './languageClientConfiguration';
import { perlConfigurationMiddleware } from './configurationPull';
import {
  classifyStartupError,
  formatStartupFailureDialog,
  StartupErrorKind,
} from './startupDiagnosis';
import type { StartupErrorDiagnosis } from './startupDiagnosis';
import type { ManagedBinarySource, ReinstallCommandResult } from './commandResults';
import {
  ActiveDocumentReadiness,
  type ActiveDocumentReadinessSnapshot,
} from './activeDocumentReadiness';
import type {
  ActivationAttemptState,
  ActivationCleanupReceipt,
  ActivationPhase,
} from './activationTransaction';
import { ACTIVATION_PHASES } from './activationTransaction';
import {
  ExtensionActivationOwner,
  _setActivationPhaseFailureInjectorForTest,
} from './activationOwner';
import type { ClientResourceMeasurement } from './clientMeasurement';
import { extensionOwnedResourceMeasurements } from './extensionOwnedResourceCensus';
import type { LegacyMigrationState } from './configurationMigrationLive';
import {
  LegacyMigrationSurface,
  refreshLegacyMigrationOnConfigurationChange,
  registerLegacyMigrationFolderWatcher,
} from './configurationMigrationHost';

// Compatibility projections for existing command/provider code. Lifecycle
// ownership lives in `languageClientLifecycle`; these values are synchronized
// from its authoritative snapshot and never drive start/stop transitions.
let client: LanguageClient | undefined;
let outputChannel: vscode.LogOutputChannel;
let testAdapter: PerlTestAdapter | undefined;
let currentServerPath: string | null = null;
// Set by getServerPath() when perl-lsp.serverPath is configured but the file
// does not exist, so the "not found" error can name the broken setting instead
// of failing silently.
let configuredServerPathMissing: string | null = null;
let statusBarItem: vscode.StatusBarItem | undefined;
let healthWidget: HealthWidget | undefined;
let healthWidgetDataSource: HealthWidgetDataSource | undefined;
let streamingController: StreamingCompletionController | undefined;

/**
 * The single owner for Perl inline completion in VS Code (#8282).
 *
 * Consults the current streaming adapter through a getter rather than holding
 * it, so a controller disposed by configuration change, restart, or extension
 * disposal stops being routed to without rebuilding the owner.
 */
const inlineCompletionOwner = new InlineCompletionOwner(() => streamingController);
let languageClientLifecycle:
  | ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent>
  | undefined;
// The extension lifecycle and CrashRecoveryArbiter are the sole restart owners.
// Disable vscode-languageclient's independent connection-close restart loop so
// one server crash cannot create overlapping replacement clients/processes.
const LANGUAGE_CLIENT_CONNECTION_OPTIONS = Object.freeze({ maxRestartCount: 0 });
/**
 * The single owner of "should perllsp be running?" (#8180). Extension
 * activation composes it; nothing else may start the language client directly.
 */
let serverDemand: ServerDemandCoordinator | undefined;
/**
 * The transactional owner of the current activation attempt (#7854). Every
 * activation-created resource registers with it; a failed attempt rolls back
 * through it in reverse registration order, and a committed attempt hands its
 * runtime to `deactivate()` for ordinary shutdown.
 */
let extensionActivation: ExtensionActivationOwner | null = null;

export function createBinaryIdentityCommand(
  getClient: () => BinaryIdentityRequestClient | undefined,
  extensionVersion: string,
  selectedRole: SelectedBinaryRole,
  host: BinaryIdentityCommandHost,
  reportError: (message: string) => void = () => undefined,
): () => Promise<unknown> {
  return async () => {
    const activeClient = getClient();
    if (!activeClient) {
      return { status: 'unavailable' as const };
    }

    try {
      return await showBinaryIdentityStatus(activeClient, host, {
        extensionVersion,
        selectedRole,
      });
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      reportError(message);
      return { status: 'error' as const, message };
    }
  };
}

const languageClientStartupMetrics = new LanguageClientStartupMetrics();
const activeDocumentReadiness = new ActiveDocumentReadiness();
const featureActivationMetrics = new FeatureActivationMetrics();

export function getLanguageClientStartupMetrics(): LanguageClientStartupMetricsSnapshot {
  return languageClientStartupMetrics.snapshot();
}

export function getFeatureActivationMetrics(): FeatureActivationMetricsSnapshot {
  return featureActivationMetrics.snapshot();
}

export function getActiveDocumentReadiness(): ActiveDocumentReadinessSnapshot {
  return activeDocumentReadiness.snapshot();
}

/**
 * Production producer for the `extension_owned_*` counters of
 * `vscode_client_measurement.v1` (#14678, parent #7866).
 *
 * Sourced from the activation ownership registry, so it reports only resources
 * this extension registered. Counters the registry cannot distinguish, and
 * shared extension-host memory, stay `not_proven` rather than `0`. This is a
 * separate authority from {@link getLanguageClientStartupMetrics}, which owns
 * startup milestone and server timing and carries no resource counts.
 *
 * Scope: the census is **attempt-scoped**. `extensionActivation` is replaced on
 * each activation, so this reports the current attempt's ownership only and
 * cannot see a resource a previous attempt failed to release. Within one
 * attempt a failed release stays visible; detecting retention *across* a reload
 * needs terminal censuses aggregated outside the attempt, which is part of
 * #7866's restart/reload work, not this claim.
 */
export function getExtensionOwnedResourceMeasurements(): ClientResourceMeasurement[] {
  return extensionOwnedResourceMeasurements(extensionActivation?.resourceCensus() ?? null);
}

export function markLanguageClientStartupMilestone(
  milestone: LanguageClientStartupMilestone,
): void {
  languageClientStartupMetrics.markMilestone(milestone);
}

export function waitForActiveDocumentReady(uri: string, timeoutMs = 30_000): Promise<void> {
  return activeDocumentReadiness.waitFor(uri, timeoutMs);
}
/**
 * Cached startup diagnosis from the last server failure.
 *
 * Set when the LSP fails to start (`initializeLanguageClient`) or when the
 * server stops unexpectedly mid-session (the lifecycle client-state hook). Read by
 * `serverNotRunningMessage()` so every "server not running" surface shows the
 * specific root cause (e.g. "glibc mismatch") instead of a generic hint.
 *
 * Cleared to `undefined` when the server starts successfully so a stale
 * failure from a previous session is not shown after a successful restart.
 */
let lastStartupDiagnosis: StartupErrorDiagnosis | undefined;

/**
 * Cached extension context so the mid-session crash handler (which is wired
 * through a lifecycle callback and has no parameter of its own) can drive an
 * auto-restart via `restartServer(context)`. Set in `activate()`.
 */
let extensionContext: vscode.ExtensionContext | undefined;

/**
 * Live compatibility reader for registered legacy settings (#14966, under #7838).
 *
 * Set in `runExtensionActivation`; cleared with the other module projections when an
 * attempt rolls back. Its state is redacted by construction, so it is safe to hand to
 * the activation API.
 */
let legacyMigrationSurface: LegacyMigrationSurface | undefined;

/**
 * Mid-session crash recovery state (#4625, #7845).
 *
 * `crashRecoveryArbiter` is the single generation-owned recovery arbiter
 * (#7845): every post-activation crash observation (process exit, watchdog
 * timeout, or both) is routed through `observeFailure` so one failed
 * language-client generation authorizes at most one recovery operation. The
 * arbiter owns the automatic-restart budget and the stable-run grace reset,
 * consuming the pre-existing constants below rather than minting new policy.
 *
 * `userInitiatedStopPending` is set before a user-driven stop/restart so the
 * crash handler can distinguish an expected `Running → Stopped` transition
 * from an unexpected one. The lifecycle controller also disposes its
 * state-change listener before stopping, so this is defense-in-depth against
 * any future code path that stops the client without going through the
 * controller.
 */
const MAX_AUTO_RESTART_ATTEMPTS = 3;
const STABLE_RUN_GRACE_MS = 30_000;
const WATCHDOG_INTERVAL_MS = 30_000;
const WATCHDOG_TIMEOUT_MS = 10_000;
const crashRecoveryArbiter = new CrashRecoveryArbiter(
  MAX_AUTO_RESTART_ATTEMPTS,
  STABLE_RUN_GRACE_MS,
);
let watchdogTimer: NodeJS.Timeout | undefined;
let userInitiatedStopPending = false;

/**
 * Fallback failed-generation identity used only when the real lifecycle
 * controller is absent (unit-test harness). With a live lifecycle the
 * generation comes from `snapshot.generation`, which increments on every
 * start/stop and therefore identifies exactly one server process run.
 */
let fallbackCrashGeneration = 0;

function currentCrashGeneration(): number {
  return languageClientLifecycle?.snapshot.generation ?? fallbackCrashGeneration;
}

function crashProcessIdentity(generation: number): string {
  return `perl-lsp-generation-${generation}`;
}

/**
 * Return the best available "server not running" message to show the user.
 *
 * When a startup failure has been diagnosed (`lastStartupDiagnosis` is set),
 * formats and returns the specific root cause so the user sees an actionable
 * message (e.g. "glibc mismatch — install from source") instead of a generic
 * restart prompt.
 *
 * Exported so unit tests can verify the message without a full extension host.
 */
export function serverNotRunningMessage(): string {
  if (lastStartupDiagnosis) {
    return formatStartupFailureDialog(lastStartupDiagnosis, undefined);
  }
  const availability = languageClientLifecycle?.availability;
  if (availability?.kind === 'starting') {
    return 'Perl Language Server is still starting. Try again in a moment.';
  }
  if (availability?.kind === 'failed') {
    const error =
      availability.error instanceof Error ? availability.error.message : String(availability.error);
    return `Perl Language Server failed to start${error ? `: ${error}` : '.'}`;
  }
  return 'Perl Language Server is not running. Run the Health Check (Command Palette: "Perl: Run Health Check") to diagnose the issue.';
}

function syncLifecycleProjection(): void {
  client = languageClientLifecycle?.client;
  currentServerPath = languageClientLifecycle?.serverPath ?? null;
}

/**
 * Render a demand-start failure for a user-facing message.
 *
 * `ServerDemandCoordinator` reports failure through its state rather than by
 * rejecting, so callers read the captured error instead of catching one.
 */
function describeDemandError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return error === undefined ? 'unknown error' : String(error);
}

/**
 * Test helper — inject a cached diagnosis without going through the full
 * startup path.  Only exported for use in unit tests.
 * @internal
 */
export function _setLastStartupDiagnosisForTest(
  diagnosis: StartupErrorDiagnosis | undefined,
): void {
  lastStartupDiagnosis = diagnosis;
}

/**
 * Test helper — reset mid-session crash-recovery state between cases.
 * @internal
 */
export function _resetCrashRecoveryStateForTest(): void {
  crashRecoveryArbiter.resetAllEpisodeMemory();
  fallbackCrashGeneration = 0;
  userInitiatedStopPending = false;
}

/**
 * Test helper — read the current automatic crash-recovery attempt counter.
 * @internal
 */
export function _autoRestartAttemptsForTest(): number {
  return crashRecoveryArbiter.automaticAttemptCount();
}

/**
 * Test helper — mark the server as having been stably running since the given
 * timestamp (defaults to long enough ago to satisfy the stable-run grace).
 * @internal
 */
export function _markStableRunningForTest(since?: number): void {
  crashRecoveryArbiter.markRunning(
    currentCrashGeneration(),
    since ?? Date.now() - (STABLE_RUN_GRACE_MS + 1_000),
  );
}

/**
 * Test helper — inject the extension context required by the crash handler.
 * @internal
 */
export function _setExtensionContextForTest(context: vscode.ExtensionContext): void {
  extensionContext = context;
}

/**
 * Test helper — inject (or clear) the live lifecycle controller so unit tests
 * can drive `restartServer` through real start/stop transitions with fake
 * clients. Pass `undefined` to restore the no-controller fallback harness.
 * @internal
 */
export function _setLanguageClientLifecycleForTest(
  lifecycle: ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent> | undefined,
): void {
  languageClientLifecycle = lifecycle;
}

/** Test helper exposing the production connection-close ownership policy. */
export function _languageClientConnectionOptionsForTest(): Readonly<{ maxRestartCount: number }> {
  return LANGUAGE_CLIENT_CONNECTION_OPTIONS;
}

/**
 * Test helper — deliver a watchdog-sourced failure observation through the
 * same production arbiter entry point the watchdog interval calls (#7845).
 * The optional generation mirrors the probe-binding the interval captures.
 * @internal
 */
export function _watchdogFailureForTest(generation?: number): Promise<void> {
  return recoverFromObservedCrash('watchdog', generation);
}

/**
 * Test helper — simulate the lifecycle spawning one replacement generation
 * while a recovery continuation is still awaiting its restart promise.
 * With a live lifecycle the crash-generation identity is owned by the
 * lifecycle controller (it increments on every start); the unit-test
 * harness has no controller, so tests drive the increment explicitly to
 * model a replacement generation that exists (and can fail) before the
 * older continuation's `restartServer` promise resolves (#7845).
 * @internal
 */
export function _spawnReplacementCrashGenerationForTest(): number {
  fallbackCrashGeneration += 1;
  return fallbackCrashGeneration;
}

/**
 * Test helper — set the user-initiated-stop sentinel.
 * @internal
 */
export function _setUserInitiatedStopPendingForTest(value: boolean): void {
  userInitiatedStopPending = value;
}

/**
 * Test helper — drive the explicit restart path with an injected lifecycle so
 * cleanup-blocked restart admission can be asserted without a command
 * registry (#14448).
 * @internal
 */
export function _restartServerForTest(context: vscode.ExtensionContext): Promise<boolean> {
  return restartServer(context);
}

export async function syncPerlCriticConfiguration(
  activeClient: Pick<LanguageClient, 'sendNotification'> | undefined = client,
  documentUri?: vscode.Uri,
): Promise<void> {
  await syncPerlCriticConfigurationFromConfig(activeClient, documentUri);
}

export async function runPerlCriticOnActiveFile(
  activeClient: Pick<LanguageClient, 'sendRequest' | 'sendNotification'> | undefined = client,
): Promise<void> {
  // Use the module-level outputChannel so critic output is not fragmented
  // into a separate channel instance. activate() creates the channel before
  // any command can be invoked, so the fallback should never fire in
  // practice; assigning to the module-level variable (rather than a local)
  // ensures subsequent calls reuse the same channel (#4630).
  if (!outputChannel) {
    outputChannel = vscode.window.createOutputChannel('Perl Language Server', { log: true });
  }
  const channel = outputChannel;
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'perl') {
    vscode.window.showErrorMessage('No active Perl file to run Critic on');
    return;
  }

  if (editor.document.isDirty) {
    await editor.document.save();
  }

  if (!activeClient) {
    vscode.window.showWarningMessage(serverNotRunningMessage());
    return;
  }

  if (hasExplicitPerlCriticOverrides(editor.document.uri)) {
    await syncPerlCriticConfiguration(activeClient, editor.document.uri);
  }

  let result: unknown;
  try {
    result = await activeClient.sendRequest('workspace/executeCommand', {
      command: 'perl.runCritic',
      arguments: [editor.document.uri.toString()],
    });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    vscode.window.showErrorMessage(`Failed to run Critic: ${message}`);
    return;
  }

  const response = result && typeof result === 'object' ? (result as Record<string, unknown>) : {};
  const status = typeof response.status === 'string' ? response.status : 'unknown';
  const violationCount =
    typeof response.violationCount === 'number'
      ? response.violationCount
      : Array.isArray(response.violations)
        ? response.violations.length
        : 0;
  const analyzerUsed =
    typeof response.analyzerUsed === 'string' ? response.analyzerUsed : 'unknown';
  const fileName = path.basename(editor.document.uri.fsPath);

  channel.info(
    `[critic] ${fileName}: status=${status} violations=${violationCount} analyzer=${analyzerUsed}`,
  );

  if (status === 'error' || typeof response.error === 'string') {
    const message =
      typeof response.error === 'string' ? response.error : 'Critic returned an error';
    vscode.window.showErrorMessage(message, 'Show Output').then((selection) => {
      if (selection === 'Show Output') {
        channel.show();
      }
    });
    return;
  }

  if (violationCount > 0) {
    vscode.window
      .showWarningMessage(
        `Critic found ${violationCount} issue${violationCount === 1 ? '' : 's'} in ${fileName}.`,
        'Show Output',
      )
      .then((selection) => {
        if (selection === 'Show Output') {
          channel.show();
        }
      });
    return;
  }

  vscode.window
    .showInformationMessage(`Critic passed for ${fileName} using ${analyzerUsed}.`, 'Show Output')
    .then((selection) => {
      if (selection === 'Show Output') {
        channel.show();
      }
    });
}

export async function setPerlCriticSeverity(
  activeClient: Pick<LanguageClient, 'sendNotification'> | undefined = client,
): Promise<void> {
  const resourceUri = vscode.window.activeTextEditor?.document.uri;
  const selection = await vscode.window.showQuickPick(
    [
      { label: '1', description: 'Brutal — report everything (Error→Hint)' },
      { label: '2', description: 'Strict — report most issues' },
      { label: '3', description: 'Balanced default' },
      { label: '4', description: 'Permissive — fewer issues' },
      { label: '5', description: 'Gentle — only severe (Error only)' },
    ],
    {
      placeHolder: 'Choose a Critic severity level',
    },
  );

  if (!selection) {
    return;
  }

  const severity = Number(selection.label);
  const config = vscode.workspace.getConfiguration('perl-lsp', resourceUri);
  // `critic.severity` is declared `resource`, but the server keeps one
  // session-global Critic state and only learns it through the unscoped
  // `didChangeConfiguration` push (#8253; see CRITIC_SESSION_STATE_DEFECT in
  // configurationOwnership.ts). Startup calls syncLanguageClientConfiguration
  // with no scope, and an unscoped read cannot see a workspaceFolderValue — so
  // writing the owning folder here would make the chosen severity work for the
  // current session and then silently vanish on restart. Keep the write at a
  // scope the session-global push can actually read until Critic becomes
  // folder-owned server-side.
  const target =
    vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0
      ? vscode.ConfigurationTarget.Workspace
      : vscode.ConfigurationTarget.Global;
  // Write the native `critic.severity` key — the product-surface setting.
  await config.update('critic.severity', severity, target);
  const payload = buildPerlCriticConfigurationPayload(resourceUri, severity);
  if (activeClient && payload) {
    await activeClient.sendNotification('workspace/didChangeConfiguration', payload);
  }

  vscode.window.showInformationMessage(`Critic severity set to ${severity}.`);
}

type DiagnosticCommandOptionsWithDefaults = DiagnosticCommandOptions;

function diagnosticCommandOptions(): DiagnosticCommandOptionsWithDefaults {
  return {
    outputChannel,
    serverNotRunningMessage,
  };
}

export async function showWorkspaceTrustReportCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
  clientRuntimeState: () => Record<string, unknown> = workspaceTrustClientRuntimeState,
): Promise<void> {
  return showWorkspaceTrustReport(activeClient, clientRuntimeState, diagnosticCommandOptions());
}

export async function explainMissingModuleLookupCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
  moduleOverride?: string,
): Promise<unknown | undefined> {
  return explainMissingModuleLookup(activeClient, moduleOverride, diagnosticCommandOptions());
}

export async function explainProviderDecisionCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
  providerOverride?: string,
): Promise<void> {
  return explainProviderDecision(activeClient, providerOverride, diagnosticCommandOptions());
}

export async function explainDiagnosticCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
  request?: unknown,
): Promise<void> {
  return explainDiagnostic(activeClient, request, diagnosticCommandOptions());
}

export async function previewSafeDeleteCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
): Promise<void> {
  return previewSafeDelete(activeClient, diagnosticCommandOptions());
}

export async function previewPackageRenameCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
): Promise<void> {
  return previewPackageRename(activeClient, diagnosticCommandOptions());
}

export async function copyProviderDecisionReceiptCommand(
  activeClient: LspExecuteCommandClient | undefined = client,
  providerOverride?: string,
): Promise<void> {
  return copyProviderDecisionReceipt(activeClient, providerOverride, diagnosticCommandOptions());
}

export async function activate(context: vscode.ExtensionContext) {
  const activation = new ExtensionActivationOwner(context, (message) => {
    outputChannel?.error(message);
  });
  extensionActivation = activation;
  const harnessFailureArmed = armHarnessActivationFailureInjection();
  try {
    const extensionApi = await runExtensionActivation(context, activation);
    // Commit before publishing the activation-complete context key: the
    // commandPalette/walkthrough `perl-lsp.activated` gate must not claim a
    // committed runtime while the attempt is still rolling forward (#7854).
    activation.commit();
    vscode.commands.executeCommand('setContext', 'perl-lsp.activated', true);
    return extensionApi;
  } catch (error: unknown) {
    const reason = error instanceof Error ? error.message : String(error);
    const receipt = await activation.rollback();
    vscode.commands.executeCommand('setContext', 'perl-lsp.activated', false);
    outputChannel?.error(
      `[activation] Attempt ${receipt.attempt_id} failed and was rolled back: ${reason}`,
    );
    throw error;
  } finally {
    if (harnessFailureArmed) {
      _setActivationPhaseFailureInjectorForTest(null);
    }
  }
}

/**
 * The extension id of the private published-smoke harness that ships only in
 * this repository (`src/test/published/harness`) and is never published: its
 * presence in a host is the discriminator that makes the packaged-journey
 * failure seam (#7856) available only in that harness.
 */
const PUBLISHED_SMOKE_HARNESS_EXTENSION_ID = 'EffortlessMetrics.perl-lsp-published-smoke-harness';

/**
 * Test-only packaged-journey seam (#7856): arm the #7855 phase-boundary failure
 * injector from the extension-test environment so the published-smoke harness
 * can fail one deterministic pre-commit resource boundary of the INSTALLED
 * extension — the exact shape a mid-activation host failure takes — without
 * patching package bytes.
 *
 * Available only in the harness: it requires BOTH the namespaced test
 * environment variable naming a real activation phase AND the private
 * published-smoke harness extension to be present in the host. Real
 * installations never satisfy both, and the guard is checked before any
 * injector is installed, so the seam cannot affect a normal activation. The
 * injector fires once, at the first boundary of the named phase, and is cleared
 * when the attempt ends, so a later explicit retry cannot inherit the fault.
 * @internal
 */
function armHarnessActivationFailureInjection(): boolean {
  const phase = process.env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE;
  if (!phase || !isActivationPhase(phase)) {
    return false;
  }
  if (!vscode.extensions.getExtension(PUBLISHED_SMOKE_HARNESS_EXTENSION_ID)) {
    return false;
  }
  let injected = false;
  _setActivationPhaseFailureInjectorForTest((boundary) => {
    if (injected || boundary.phase !== phase) {
      return null;
    }
    injected = true;
    return new Error(
      `harness-injected activation failure after ${boundary.resource_id} (#7856 packaged journey)`,
    );
  });
  return true;
}

function isActivationPhase(value: string): value is ActivationPhase {
  return (ACTIVATION_PHASES as readonly string[]).includes(value);
}

async function runExtensionActivation(
  context: vscode.ExtensionContext,
  activation: ExtensionActivationOwner,
) {
  languageClientStartupMetrics.markMilestone('activate_entered');
  featureActivationMetrics.beginActivation();
  // Module-level compatibility projections are owned by the attempt from its
  // first tick (#7854). Registered first so reverse-order cleanup clears them
  // last, after every owned resource — including the language client — was
  // torn down. The output channel is deliberately NOT cleared here: it is a
  // retained support surface, so it stays reachable for failure reporting
  // after a rolled-back attempt.
  activation.ownCleanup('module-projections', 'base', 'mandatory_for_activation', () => {
    clearActivationProjections();
  });
  // Cache the context so the mid-session crash handler (#4625) can drive an
  // auto-restart without a parameter of its own.
  extensionContext = context;
  // LogOutputChannel is required by vscode-languageclient for outputChannel and
  // traceOutputChannel. Messages are routed through level-aware methods
  // (debug/info/warn/error) so the VS Code Output panel level filter works.
  outputChannel = vscode.window.createOutputChannel('Perl Language Server', { log: true });
  activation.own('base', 'support_surface_allowed_after_failure', outputChannel);
  // Registered legacy settings are read before any feature domain runs, so the reasons a
  // stale setting is being ignored are already in the output channel when the domain it
  // used to configure reports itself inert (#14966). The reader interprets configuration
  // and never writes it.
  const migrationSurface = new LegacyMigrationSurface(
    outputChannel,
    (context.extension.packageJSON.version as string) ?? 'unknown',
  );
  legacyMigrationSurface = migrationSurface;
  try {
    migrationSurface.refresh();
  } catch (error: unknown) {
    // A support surface must not decide whether activation succeeds. The failure is
    // reported rather than swallowed, and the published state stays empty.
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.error(`[configuration-migration] initial read failed: ${message}`);
  }
  // The generic MCP passthrough is runtime-inert (#7119), so this domain is no
  // longer activation-critical: it registers nothing and returns no disposable.
  const mcpDisposable = featureActivationMetrics.measure('mcp', false, () =>
    registerMcpSupport(outputChannel),
  );
  if (mcpDisposable) {
    activation.own('support', 'optional_degradable', mcpDisposable);
  }
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusBarItem.command = 'perl-lsp.showWorkspaceStatus';
  statusBarItem.accessibilityInformation = {
    label: 'Perl Language Server',
    role: 'button',
  };
  statusBarItem.show();
  activation.own('base', 'mandatory_for_activation', statusBarItem);
  healthWidget = new HealthWidget(statusBarItem);
  // Extension activation is not language-server startup (#8180). Until a
  // server-dependent trigger exists the widget reports the truthful dormant
  // state instead of an indefinite `starting` spinner.
  healthWidget.setWorkspaceLifecycleState('dormant', {
    detail: 'Perl language features start when you open a Perl file.',
    reasonCode: 'no_server_demand',
  });
  // Wire the file/error-count setters to client-side telemetry (#4620).
  // Without this, the running-state status bar never shows the
  // `perl-lsp v<x>: <N> files | <M> errors` indicator the widget promises.
  healthWidgetDataSource = HealthWidgetDataSource.fromDeps(healthWidget, {
    languages: vscode.languages,
    workspace: vscode.workspace,
  });
  healthWidgetDataSource.start();
  activation.own('base', 'mandatory_for_activation', healthWidgetDataSource);
  languageClientLifecycle = createLanguageClientLifecycle(context);
  // The language client lifecycle is attempt-owned until commit: a failed
  // activation tears down any partially constructed client and timer state
  // through the same primitive ordinary deactivation uses (#7854).
  activation.ownCleanup(
    'language-client-lifecycle',
    'language_client',
    'mandatory_for_activation',
    () => disposeLanguageClient(),
  );
  syncLifecycleProjection();
  // One owner for every server-dependent path (#8180). Command helpers and
  // document listeners route demand through this object; none of them call
  // the lifecycle's start() directly.
  serverDemand = new ServerDemandCoordinator({
    startServer: () => startLanguageServerOnDemand(context),
    onStateChange: (snapshot) => {
      presentServerDemandState(snapshot);
    },
    log: (message) => {
      outputChannel.info(message);
    },
  });
  activation.own('language_client', 'mandatory_for_activation', {
    dispose: () => {
      serverDemand?.dispose();
      serverDemand = undefined;
    },
  });

  // Register server-facing commands through an explicit dependency context.
  // Lifecycle transitions remain owned by the authoritative composition.
  const serverCommandDisposables = registerServerCommandGroup({
    outputChannel,
    currentServerPath: () => currentServerPath,
    resolveServerPath: async () => {
      const lifecycle = languageClientLifecycle;
      if (!lifecycle) {
        return currentServerPath;
      }

      // An explicit health check is a server-dependent entry point: it may
      // start a dormant server, and it retries a previously failed start
      // because the user asked for it directly (#8180). Routing through the
      // demand owner also coalesces with activation's in-flight startup, so a
      // first-run health check never observes the transient null projection
      // while the managed binary is being resolved.
      //
      // ensureStarted reports failure through its state rather than rejecting,
      // so the outcome is read from the snapshot; a try/catch here would never
      // run and would silently drop this warning.
      await serverDemand?.ensureStarted('command:runHealthCheck', { retry: true });
      const demand = serverDemand?.snapshot;
      if (demand?.state === 'failed') {
        outputChannel.warn(
          `[health-check] Server startup did not complete: ${describeDemandError(demand.error)}`,
        );
      }
      syncLifecycleProjection();
      return lifecycle.serverPath;
    },
    reinstallServerBinary: () => reinstallServerBinary(context),
    restartServer: () => restartServerFromExplicitRecovery(context),
    showBinaryIdentity: createBinaryIdentityCommand(
      () => client ?? languageClientLifecycle?.client,
      (context.extension.packageJSON.version as string) ?? 'unknown',
      'managed',
      {
        show: async (presentation) => {
          await vscode.window.showInformationMessage(
            `${presentation.label}\n${presentation.detail}`,
          );
          return undefined;
        },
        refreshIdentity: async () => undefined,
        repairManagedPair: async () => undefined,
        inspectConfiguredBinary: async () => undefined,
        copySupportPacket: async (packet) => {
          await vscode.env.clipboard.writeText(packet);
        },
      },
      (message) => {
        outputChannel.error(`[binary-identity] ${message}`);
        void vscode.window.showErrorMessage(`Failed to read Perl LSP binary identity: ${message}`);
      },
    ),
    runHealthCheck: async (serverPath) => {
      const onboarding = new OnboardingManager(context, outputChannel);
      return onboarding.runSetupHealthCheck(serverPath);
    },
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', serverCommandDisposables);

  const openDemoProject = async () => {
    await openDemoProjectCommand(context);
  };

  const criticCommandDisposables = registerCriticCommandGroup({
    runPerlCriticOnActiveFile: () => runPerlCriticOnActiveFile(),
    setPerlCriticSeverity: () => setPerlCriticSeverity(),
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', criticCommandDisposables);

  const testCommandDisposables = registerTestCommandGroup({
    runTests: (test) =>
      runTestsCommand(test, {
        activeClient: client,
        testAdapter,
        statusBarItem,
        serverNotRunningMessage,
      }),
    runCurrentTest: () => runCurrentTestWithProve(),
    runTestAtCursor: () =>
      runTestAtCursorCommand({
        activeClient: client,
        serverNotRunningMessage,
      }),
    runAllTests: () => runAllTestsWithProve(),
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', testCommandDisposables);

  const navigationCommandDisposables = registerNavigationCommandGroup({
    openDemoProject,
    showVersion: async () => {
      // Reporting the server version needs a resolved binary, so this is a
      // server-dependent entry point and must honour its `on-first-use` ledger
      // row. Without this a dormant session answers an explicit version request
      // with "server is not running".
      await serverDemand?.ensureStarted('command:showVersion', { retry: true });
      syncLifecycleProjection();
      return showVersionCommand({
        currentServerPath: () => currentServerPath,
        outputChannel,
        serverNotRunningMessage,
        getServerVersion: (serverPath) =>
          new Promise((resolve, reject) => {
            execFile(serverPath, ['--version'], (error: Error | null, stdout: string) => {
              if (error) {
                reject(error);
                return;
              }
              resolve(stdout.trim());
            });
          }),
      });
    },
    showStatusMenu: showStatusMenuCommand,
    showWorkspaceStatus: () =>
      showWorkspaceStatusCommand({
        getWorkspaceStatus: () => {
          const widget = healthWidget;
          const mode = widget?.mode ?? 'starting';
          const hasLiveServer = mode === 'running' || mode === 'indexing';
          const activeEditor = vscode.window.activeTextEditor;
          const activePerlDocument = activeEditor?.document.languageId === 'perl';
          return {
            mode,
            ...(hasLiveServer && widget?.version !== undefined ? { version: widget.version } : {}),
            ...(widget?.fileCount === undefined ? {} : { fileCount: widget.fileCount }),
            ...(mode === 'stopped' ? {} : { errorCount: widget?.errorCount ?? 0 }),
            ...(widget?.lifecycleState ? { lifecycle: widget.lifecycleState } : {}),
            ...(widget?.experienceDetail ? { lifecycleDetail: widget.experienceDetail } : {}),
            ...(hasLiveServer
              ? {
                  readinessState: widget?.enhancedReadinessAvailable
                    ? widget.readinessState
                    : ('legacy' as const),
                }
              : {}),
            ...(widget?.readinessReason ? { readinessReason: widget.readinessReason } : {}),
            ...(widget?.experienceAction ? { nextAction: widget.experienceAction } : {}),
            ...(activePerlDocument && activeEditor
              ? {
                  activeDocumentReady: activeDocumentReadiness.isReady(
                    activeEditor.document.uri.toString(),
                  ),
                }
              : {}),
          };
        },
      }),
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', navigationCommandDisposables);

  const documentCommandDisposables = registerDocumentCommandGroup({
    checkSyntax: () =>
      runCheckSyntaxCommand({
        activeClient: client,
        outputChannel,
        serverNotRunningMessage,
      }),
    formatDocument: formatDocumentCommand,
    showIncPaths: showIncPathsCommand,
    openModule: openPerlModuleCommand,
    showParserAst: () =>
      showParserAstCommand({
        activeClient: client,
        outputChannel,
        serverNotRunningMessage,
      }),
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', documentCommandDisposables);

  const diagnosticCommandDisposables = registerDiagnosticCommandGroup({
    explainProviderDecision: (provider) =>
      explainProviderDecisionCommand(client, typeof provider === 'string' ? provider : undefined),
    previewSafeDelete: () => previewSafeDeleteCommand(client),
    previewPackageRename: () => previewPackageRenameCommand(client),
    copyProviderDecisionReceipt: (provider) =>
      copyProviderDecisionReceiptCommand(
        client,
        typeof provider === 'string' ? provider : undefined,
      ),
    showWorkspaceTrustReport: () =>
      showWorkspaceTrustReportCommand(client, () => workspaceTrustClientRuntimeState(context)),
    explainMissingModuleLookup: (moduleName) =>
      explainMissingModuleLookupCommand(
        client,
        typeof moduleName === 'string' ? moduleName : undefined,
      ),
    explainDiagnostic: (request) => explainDiagnosticCommand(client, request),
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', diagnosticCommandDisposables);

  const whatsNewManager = featureActivationMetrics.measure(
    'whats_new',
    true,
    () => new WhatsNewManager(context, outputChannel),
  );
  const onboardingCommandDisposables = registerOnboardingCommandGroup({
    showWhatsNew: async () => {
      featureActivationMetrics.markFirstUse('whats_new');
      await whatsNewManager.showWhatsNew();
    },
    openConfigurationGuide: () => {
      void vscode.commands.executeCommand(
        'workbench.action.openSettings',
        '@ext:EffortlessMetrics.perl-lsp-rs',
      );
    },
    checkForUpdate: async () => {
      if (!vscode.workspace.isTrusted) {
        vscode.window.showWarningMessage(
          'Cannot check for binary updates in an untrusted workspace. Grant workspace trust first.',
        );
        return;
      }
      const downloader = new BinaryDownloader(context, outputChannel);
      await context.globalState.update('perl-lsp.lastUpdateCheck', 0);
      await downloader.checkForUpdateSilent();
    },
  });
  // Onboarding/What's New and support surfaces are intentionally usable after
  // a failed activation attempt (the user may need to report the failure), so
  // they register as retained support surfaces rather than mandatory ones
  // (#7854).
  activation.ownDisposables(
    'support',
    'support_surface_allowed_after_failure',
    onboardingCommandDisposables,
  );

  const refactoringCommandDisposables = registerRefactoringCommandGroup({
    extractVariable: () =>
      extractVariableCommand({ activeClient: client, serverNotRunningMessage }),
    extractMethod: () => extractMethodCommand({ activeClient: client, serverNotRunningMessage }),
    showRefactoringOptions: showRefactoringOptionsCommand,
  });
  activation.ownDisposables('commands', 'mandatory_for_activation', refactoringCommandDisposables);

  const supportCommandDisposables = registerSupportCommandGroup({
    reportIssue: () =>
      reportIssueCommand({
        getServerVersion: () =>
          new Promise((resolve) => {
            if (!currentServerPath) {
              resolve('unavailable');
              return;
            }
            execFile(
              currentServerPath,
              ['--version'],
              { timeout: 3000 },
              (error: Error | null, stdout: string) => {
                if (error) {
                  resolve('unavailable');
                  return;
                }
                const firstLine = stdout.trim().split('\n')[0] ?? '';
                resolve(firstLine.trim() || 'unavailable');
              },
            );
          }),
        extensionVersion: (context.extension.packageJSON.version as string) ?? 'unknown',
        editorVersion: vscode.version,
        platform: process.platform,
        arch: process.arch,
        editorName: (vscode.env as unknown as { appName?: string }).appName,
      }),
  });
  activation.ownDisposables(
    'support',
    'support_surface_allowed_after_failure',
    supportCommandDisposables,
  );

  // Coexistence status is usable without a running server: it explains host
  // observations and never mutates other tools (#7214). Registered last in
  // the retained support prefix so failure-injection ordinals above stay
  // stable.
  const coexistenceCommandDisposables = registerCoexistenceCommandGroup({
    showCoexistenceStatus: () => showCoexistenceStatusCommand(context),
  });
  activation.ownDisposables(
    'support',
    'support_surface_allowed_after_failure',
    coexistenceCommandDisposables,
  );

  const formatOnSaveDisposable = vscode.workspace.onWillSaveTextDocument((event) => {
    if (!shouldFormatOnSave(event.document)) {
      return;
    }

    event.waitUntil(formatDocumentOnSave(event.document));
  });
  activation.own('workspace_listeners', 'mandatory_for_activation', formatOnSaveDisposable);

  const configurationWatcher = featureActivationMetrics.measure('configuration', true, () =>
    registerWorkspaceConfigurationEvents({
      // Registered legacy keys were removed and drive no subsystem, so no
      // configuration class classifies them; they are observed unclassified
      // (#14966) instead of through a second host listener.
      onAnyConfigurationChanged: (event) => {
        if (legacyMigrationSurface) {
          refreshLegacyMigrationOnConfigurationChange(legacyMigrationSurface, event);
        }
      },
      onLiveConfigurationChanged: async (event) => {
        if (event.affectsConfiguration('perl-lsp.trace.server') && client) {
          const newTrace = getTraceLevel();
          await client.setTrace(newTrace);
          outputChannel.info(`Trace level changed to: ${newTrace}`);
        }

        if (event.affectsConfiguration('perl-lsp.includePaths')) {
          await validateIncludePaths(context);
        }

        const criticChanged = CRITIC_SETTINGS.some((setting) =>
          event.affectsConfiguration(setting),
        );
        if (event.affectsConfiguration('perl-lsp.includePaths') || criticChanged) {
          await syncLanguageClientConfiguration(client);
        }

        // Advisory coexistence findings re-evaluate when an owned input
        // changes; every collected input is classified live, so this block is
        // reachable for all of them. Dedupe keeps this silent unless the
        // finding set changed (#7214 clear/restore semantics).
        if (coexistenceReevaluationRequested((setting) => event.affectsConfiguration(setting))) {
          await runCoexistenceAdvisory(context);
        }
      },
      onReconstructConfigurationChanged: async (event) => {
        if (event.affectsConfiguration('perl-lsp.enableTestIntegration')) {
          await refreshTestAdapter(context);
        }

        if (
          event.affectsConfiguration('perl-lsp.aiCompletion.enabled') ||
          event.affectsConfiguration('perl-lsp.aiCompletion.streaming.enabled')
        ) {
          refreshStreamingController(client);
          await syncUserAiCompletionConfiguration(client);
        }
      },
      onRestartRequired: () => promptForClientRefresh(context),
      onError: (error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        outputChannel.error(`[configuration] change handling failed: ${message}`);
      },
    }),
  );
  activation.own('workspace_listeners', 'mandatory_for_activation', configurationWatcher);

  // Registered after the configuration watcher so the existing workspace_listeners
  // ordinals keep their meaning. The migration surface depends on the folder set as well
  // as configuration content, and VS Code reports folder changes on their own event.
  const legacyMigrationFolderWatcher = registerLegacyMigrationFolderWatcher(
    migrationSurface,
    (error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      outputChannel.error(`[configuration-migration] folder-change read failed: ${message}`);
    },
  );
  activation.own('workspace_listeners', 'optional_degradable', legacyMigrationFolderWatcher);

  const fileCreationWatcher = vscode.workspace.onDidCreateFiles(async (event) => {
    try {
      await populateCreatedFiles(event);
    } catch (e) {
      outputChannel.error('File creation handler error', e);
    }
  });
  activation.own('workspace_listeners', 'mandatory_for_activation', fileCreationWatcher);

  const arrowCompletionWatcher = vscode.workspace.onDidChangeTextDocument((event) => {
    maybeNudgeArrowCompletion(event);
  });
  activation.own('workspace_listeners', 'mandatory_for_activation', arrowCompletionWatcher);

  // The document feature group receives a scoped facade context: registrations
  // it pushes during activation join the attempt, while lazily created
  // resources (a POD preview webview panel's onDidDispose hook) fall through
  // to ordinary host disposal after the attempt closed (#7854).
  const providerDisposables = featureActivationMetrics.measure('providers', true, () => [
    ...registerDocumentFeatureGroup({
      extensionContext: activation.scopedContext('document_providers', 'optional_degradable'),
      registerGherkinProviders,
      registerGherkinStepDefinitionSupport,
      registerPodPreview,
    }),
  ]);
  activation.ownDisposables('document_providers', 'optional_degradable', providerDisposables);

  languageClientStartupMetrics.markMilestone('commands_registered');

  // Initialize debug adapter. The debugger owns its registrations through the
  // scoped facade context, which routes them into the attempt (#7854).
  featureActivationMetrics.measure('debugger', true, () =>
    activateDebugger(activation.scopedContext('debugger', 'mandatory_for_activation')),
  );

  if (
    context.extensionMode === vscode.ExtensionMode.Test &&
    process.env.PERL_LSP_EXTENSION_TEST_SKIP_STARTUP === '1'
  ) {
    outputChannel.warn('[extension-test] Skipping automatic server startup.');
    languageClientStartupMetrics.markMilestone('activate_returned');
    return {
      getLanguageClientStartupMetrics,
      getFeatureActivationMetrics,
      getActiveDocumentReadiness,
      getExtensionOwnedResourceMeasurements,
      getLegacyConfigurationMigrationState,
      markLanguageClientStartupMilestone,
      waitForActiveDocumentReady,
      stop: stopLanguageClientForActivationApi,
    };
  }

  // Workspace Trust gate: do not download binaries or spawn the language
  // server in an untrusted workspace. Demand raised while untrusted is
  // remembered by the coordinator and honoured when trust is granted, so the
  // user does not have to re-open the file to get language features.
  if (!vscode.workspace.isTrusted) {
    outputChannel.info(
      '[startup] Workspace is not trusted — deferring language server startup until trust is granted.',
    );
    // Deferred startup is a configuration decision, not a slow start. Without
    // this the widget stays on 'starting' indefinitely, which is
    // indistinguishable from a server that hung — the exact conflation the
    // #5900 experience contract forbids.
    serverDemand?.closeGate('workspace_untrusted');
    healthWidget?.setWorkspaceLifecycleState('configuration_action_required', {
      detail: 'Perl language features are paused because this workspace is not trusted.',
      action: 'Trust this workspace to start the Perl language server.',
      reasonCode: 'workspace_untrusted',
    });
    const trustDisposable = vscode.workspace.onDidGrantWorkspaceTrust(() => {
      outputChannel.info('[startup] Workspace trust granted — re-evaluating server demand.');
      void serverDemand?.openGate().catch((error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        outputChannel.error(`[startup] Post-trust startup failed: ${message}`);
      });
    });
    activation.own('workspace_listeners', 'mandatory_for_activation', trustDisposable);
  }

  // Extension activation is complete. Language-server startup is a separate
  // transition driven by real demand (#8180): an eligible Perl document that
  // is already open, one that opens later, or an explicit server command.
  const documentDemandDisposables = registerServerDemandListeners();
  activation.ownDisposables(
    'workspace_listeners',
    'mandatory_for_activation',
    documentDemandDisposables,
  );
  scheduleServerDemandEvaluation(context, whatsNewManager);
  languageClientStartupMetrics.markMilestone('activate_returned');
  return {
    getLanguageClientStartupMetrics,
    getFeatureActivationMetrics,
    getActiveDocumentReadiness,
    getExtensionOwnedResourceMeasurements,
    getLegacyConfigurationMigrationState,
    markLanguageClientStartupMilestone,
    waitForActiveDocumentReady,
    stop: stopLanguageClientForActivationApi,
  };
}

export async function deactivate() {
  try {
    // A committed activation owns shutdown through the same cleanup
    // primitives rollback uses (#7854). Without a committed runtime
    // (activation never ran to commit, or the attempt was rolled back) the
    // pre-transaction shutdown path stays authoritative.
    const receipt = (await extensionActivation?.deactivate()) ?? null;
    if (receipt === null) {
      await disposeLanguageClient();
    }
  } finally {
    languageClientStartupMetrics.markMilestone('shutdown');
  }
}

/**
 * The activation API's `stop` seam: a recoverable language-client shutdown,
 * not the terminal teardown `deactivate()` performs (#7854).
 *
 * Historically `stop` was literally `deactivate`, and `deactivate()` only
 * stopped the language client. The current-source smoke exercises this seam
 * mid-session ("language client shutdown") and keeps using the extension
 * afterwards — diagnostics arrive because the demand listeners survive and
 * restart the server — so it must stay light: the committed activation
 * runtime, its registrations, and the output channel stay live, and only the
 * language client plus the shutdown milestone are touched.
 */
async function stopLanguageClientForActivationApi(): Promise<void> {
  await disposeLanguageClient();
  languageClientStartupMetrics.markMilestone('shutdown');
}

/**
 * Clears the module-level compatibility projections from the activation
 * transaction's authority (#7854). Registered as the attempt's first
 * resource, so reverse-order cleanup runs it last — after every owned
 * resource, including the language client lifecycle, was torn down. The
 * output channel is deliberately retained: it is a support surface that must
 * stay usable for failure reporting after a rolled-back attempt.
 */
function clearActivationProjections(): void {
  stopWatchdog();
  client = undefined;
  currentServerPath = null;
  configuredServerPathMissing = null;
  testAdapter = undefined;
  streamingController = undefined;
  statusBarItem = undefined;
  healthWidget = undefined;
  healthWidgetDataSource = undefined;
  serverDemand = undefined;
  languageClientLifecycle = undefined;
  lastStartupDiagnosis = undefined;
  extensionContext = undefined;
  legacyMigrationSurface = undefined;
}

/**
 * Redacted legacy-setting migration state for status, doctor, and installed transition
 * tests (#14966, under #7838).
 *
 * Exposed through the activation API so those surfaces observe migration state without
 * reaching into extension internals, and without any raw configuration value.
 */
function getLegacyConfigurationMigrationState(): LegacyMigrationState | undefined {
  return legacyMigrationSurface?.snapshot();
}

/**
 * Test helper — expose the production activation owner's state (#7854).
 * @internal
 */
export function _extensionActivationStateForTest(): {
  state: ActivationAttemptState;
  attemptId: string;
  resourceIds: string[];
  lastCleanupReceipt: ActivationCleanupReceipt | null;
} | null {
  if (extensionActivation === null) {
    return null;
  }
  return {
    state: extensionActivation.currentState(),
    attemptId: extensionActivation.attemptId,
    resourceIds: extensionActivation.resourceIds(),
    lastCleanupReceipt: extensionActivation.lastCleanupReceipt(),
  };
}

/**
 * Test helper — whether the module-level compatibility projections were
 * cleared by the activation authority (#7854).
 * @internal
 */
export function _activationProjectionsClearedForTest(): boolean {
  return (
    extensionContext === undefined &&
    languageClientLifecycle === undefined &&
    serverDemand === undefined &&
    statusBarItem === undefined &&
    healthWidget === undefined &&
    healthWidgetDataSource === undefined &&
    testAdapter === undefined &&
    streamingController === undefined
  );
}

/**
 * Convert one VS Code document into demand.
 *
 * The listener is intentionally the only place that turns "a Perl buffer
 * exists" into a server start, so the decision cannot drift between the
 * open-document and active-editor paths.
 */
function observeDocumentDemand(document: vscode.TextDocument): Promise<void> {
  return (
    serverDemand?.observeDocument({
      languageId: document.languageId,
      uriScheme: document.uri.scheme,
    }) ?? Promise.resolve()
  );
}

/**
 * Arm the bounded listeners that start the server for a Perl document opened
 * *after* a non-LSP surface already activated the extension.
 *
 * `activate()` runs once per session. Without these listeners a user who
 * activated the extension through Gherkin, the walkthrough, or a debug
 * configuration would need a window reload before Perl language features
 * worked at all.
 */
function registerServerDemandListeners(): vscode.Disposable[] {
  return [
    vscode.workspace.onDidOpenTextDocument((document) => {
      void observeDocumentDemand(document);
    }),
    // A document restored with the window is already open when the extension
    // activates, so it never fires onDidOpenTextDocument. Becoming the active
    // editor is the second, independent way real demand appears.
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor) {
        void observeDocumentDemand(editor.document);
      }
    }),
  ];
}

/**
 * Decide whether this activation already carries server demand.
 *
 * Housekeeping that is *not* server-dependent (first-run welcome, What's New)
 * runs on every activation. It is sequenced after a start attempt when one is
 * made, so the welcome notification still reports the resolved server path.
 */
function scheduleServerDemandEvaluation(
  context: vscode.ExtensionContext,
  whatsNewManager: WhatsNewManager,
): void {
  const hasOpenPerlDocument = vscode.workspace.textDocuments.some((document) =>
    isServerDependentDocument({
      languageId: document.languageId,
      uriScheme: document.uri.scheme,
    }),
  );

  if (!hasOpenPerlDocument) {
    outputChannel.info(
      '[server-demand] Activated without an open Perl document — the language server stays dormant until one is opened or a server command runs.',
    );
    finishActivationHousekeeping(context, whatsNewManager);
    return;
  }

  const started = serverDemand?.ensureStarted('activation:open-perl-document') ?? Promise.resolve();
  void started.finally(() => {
    finishActivationHousekeeping(context, whatsNewManager);
  });
}

/**
 * Start the language server for a demand that has already been authorized.
 *
 * Only {@link ServerDemandCoordinator} calls this, which is what keeps
 * "exactly one client generation per demand" true.
 */
async function startLanguageServerOnDemand(context: vscode.ExtensionContext): Promise<void> {
  const initialized = await initializeLanguageClient(context);
  if (!initialized) {
    // initializeLanguageClient reports its own actionable diagnosis and returns
    // false rather than throwing. Fail here, before the post-start work: none of
    // it helps without a server, an update check would download a binary that
    // just failed to launch, and a rejection from any of those calls would
    // replace the real startup error with a misleading one.
    throw new Error(
      'Language server did not start; see the Perl Language Server output for details.',
    );
  }
  languageClientStartupMetrics.markMilestone('workspace_ready');
  await validateIncludePaths(context);
  await suggestDiscoveredIncludePaths(context);
  await runCoexistenceAdvisory(context);

  // Background update check — fire-and-forget after startup completes.
  // Runs at most once per updateCheckInterval hours; no-ops when serverPath
  // is user-managed, channel='tag', or updateCheckInterval=0.
  // Skipped in untrusted workspaces as a defense-in-depth measure.
  // This now runs only once the server is actually wanted and running, so a
  // Gherkin-only session no longer downloads a server it will never launch.
  if (vscode.workspace.isTrusted) {
    const updateDownloader = new BinaryDownloader(context, outputChannel);
    updateDownloader.checkForUpdateSilent().catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      outputChannel.error(`[update-check] Error: ${msg}`);
    });
  } else {
    outputChannel.info('[update-check] Skipped background update check in untrusted workspace.');
  }
}

/** Present typed demand states on the status widget. */
function presentServerDemandState(snapshot: ServerDemandSnapshot): void {
  const widget = healthWidget;
  if (!widget) {
    return;
  }

  switch (snapshot.state) {
    case 'not_started':
      // A mid-session crash publishes `failed` with an actionable diagnosis
      // that is still true; returning to dormant must not blur it. The trust
      // gate is deliberately *not* protected here: demand goes back to
      // not_started only when trust was granted, and an action-required state
      // that outlives its cause is its own defect.
      if (widget.lifecycleState === 'failed') {
        break;
      }
      widget.setWorkspaceLifecycleState('dormant', {
        detail: 'Perl language features start when you open a Perl file.',
        reasonCode: 'no_server_demand',
      });
      break;
    case 'starting':
      widget.setWorkspaceLifecycleState('starting');
      break;
    case 'running':
    case 'action_required':
      // Both states already have a more specific owner: the client-state
      // projection renders indexing/ready, and the trust gate publishes its own
      // actionable message. Overwriting either here would lose information.
      break;
    case 'failed': {
      if (widget.lifecycleState === 'failed') {
        // initializeLanguageClient already published a specific root cause.
        break;
      }
      const message = describeDemandError(snapshot.error);
      widget.setWorkspaceLifecycleState('failed', {
        detail: `Perl Language Server failed to start: ${message}`,
        action: 'Run the Health Check or fix the server configuration.',
        reasonCode: 'startup_failure',
      });
      break;
    }
  }
}

/**
 * Activation work that does not depend on the language server.
 *
 * This must run whether or not the server was started, otherwise a user whose
 * first contact with the extension is Gherkin or the walkthrough would never
 * see the first-run welcome or What's New.
 */
function finishActivationHousekeeping(
  context: vscode.ExtensionContext,
  whatsNewManager: WhatsNewManager,
): void {
  try {
    runActivationHousekeeping(context, whatsNewManager);
  } catch (error: unknown) {
    // This runs both directly and inside a .finally() on a floating promise, so
    // a synchronous throw would either escape activate() or become an unhandled
    // rejection in the extension host. Optional welcome UI is never worth that.
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.error(`[activation] Post-activation housekeeping failed: ${message}`);
  }
}

function runActivationHousekeeping(
  context: vscode.ExtensionContext,
  whatsNewManager: WhatsNewManager,
): void {
  // First-run onboarding: show welcome notification once per installation.
  const onboarding = featureActivationMetrics.measure(
    'onboarding',
    false,
    () => new OnboardingManager(context, outputChannel),
  );
  if (onboarding.shouldShowWelcome()) {
    featureActivationMetrics.markFirstUse('onboarding');
    // Fire-and-forget; failures must not block extension startup.
    onboarding.showWelcomeNotification(currentServerPath).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      outputChannel.error(`[onboarding] Error showing welcome: ${msg}`);
    });
    // Mark the version seen on first install so the next activation
    // (after an update) triggers the What's New panel instead of welcome.
    whatsNewManager.markVersionSeen().catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      outputChannel.error(`[whats-new] Error marking version seen: ${msg}`);
    });
  } else if (whatsNewManager.shouldShowWhatsNew()) {
    // Extension was updated — show What's New panel.
    // Fire-and-forget; failures must not block extension startup.
    whatsNewManager
      .markVersionSeen()
      .then(() => {
        return whatsNewManager.showWhatsNew();
      })
      .catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        outputChannel.error(`[whats-new] Error showing What's New: ${msg}`);
      });
  }
}

/**
 * If `userPath` (the configured perl-lsp.serverPath) is set but does not exist,
 * log a diagnostic and return it so callers can surface an actionable error —
 * instead of silently falling through to PATH/bundled and leaving the user with
 * a generic "not found" that gives no hint their setting was the cause. Returns
 * null when no configured path was rejected. Exported for testing.
 */
export function diagnoseConfiguredServerPath(
  userPath: string | undefined,
  pathExists: boolean,
  channel: vscode.LogOutputChannel,
): string | null {
  if (!userPath || pathExists) {
    return null;
  }
  channel.info(
    `[startup] perl-lsp.serverPath is configured but does not exist: ${userPath}. ` +
      `Falling back to PATH/bundled binary (or auto-download).`,
  );
  return userPath;
}

async function getServerPath(
  context: vscode.ExtensionContext,
): Promise<{ path: string | null; source: BinaryResolutionSource }> {
  // First check user settings
  const config = vscode.workspace.getConfiguration('perl-lsp');
  const userPath = config.get<string>('serverPath');
  const userPathExists = userPath ? fs.existsSync(userPath) : false;

  if (userPath && userPathExists) {
    outputChannel.info(`Using user-configured Perl LSP binary: ${userPath}`);
    configuredServerPathMissing = null;
    return { path: userPath, source: 'configured' };
  }

  configuredServerPathMissing = diagnoseConfiguredServerPath(
    userPath,
    userPathExists,
    outputChannel,
  );

  const platform = process.platform;
  const arch = process.arch;
  const binaryNames =
    platform === 'win32' ? ['perllsp.exe', 'perl-lsp.exe'] : ['perllsp', 'perl-lsp'];

  const findInPath = (): string | null => {
    const pathDirs = process.env.PATH?.split(path.delimiter) || [];
    for (const dir of pathDirs) {
      for (const binaryName of binaryNames) {
        const fullPath = path.join(dir, binaryName);
        if (fs.existsSync(fullPath)) {
          outputChannel.info(`Found Perl LSP binary in PATH: ${fullPath}`);
          return fullPath;
        }
      }
    }
    return null;
  };

  const findBundled = (): string | null => {
    for (const binaryName of binaryNames) {
      const bundledPath = path.join(
        context.extensionPath,
        'bin',
        `${platform}-${arch}`,
        binaryName,
      );

      if (!fs.existsSync(bundledPath)) {
        continue;
      }

      outputChannel.info(`Using bundled Perl LSP binary: ${bundledPath}`);
      if (platform !== 'win32') {
        try {
          fs.chmodSync(bundledPath, 0o755);
        } catch (chmodError: unknown) {
          const msg = chmodError instanceof Error ? chmodError.message : String(chmodError);
          outputChannel.info(
            `[startup] Could not update executable permissions for bundled binary: ${msg}`,
          );
        }
      }
      return bundledPath;
    }
    return null;
  };

  // Firebase Studio (and IDX) run in remote containers where extension install
  // paths may be mounted read-only or noexec. Prefer PATH there so users can
  // provide perllsp from their workspace/toolchain.
  const remoteName = vscode.env.remoteName?.toLowerCase() ?? '';
  const preferPathBeforeBundled = remoteName.includes('firebase') || remoteName.includes('idx');
  if (preferPathBeforeBundled) {
    const pathCandidate = findInPath();
    if (pathCandidate) {
      return { path: pathCandidate, source: 'path' };
    }
  }

  const bundledCandidate = findBundled();
  if (bundledCandidate) {
    return { path: bundledCandidate, source: 'bundled' };
  }

  const pathCandidate = findInPath();
  if (pathCandidate) {
    return { path: pathCandidate, source: 'path' };
  }

  // Check if auto-download is enabled
  const autoDownload = config.get<boolean>('autoDownload', true);

  if (autoDownload && !vscode.workspace.isTrusted) {
    // Defense-in-depth: the activate() trust gate already prevents server
    // startup in untrusted workspaces, but getServerPath can also be reached
    // via reinstall/restart paths. Block binary download here too.
    outputChannel.info(
      'Perl LSP binary not found, but auto-download is skipped in untrusted workspaces.',
    );
    return { path: null, source: 'unavailable' };
  }

  if (autoDownload) {
    outputChannel.info('Perl LSP binary not found, attempting to download...');
    const downloader = new BinaryDownloader(context, outputChannel);
    const downloadedPath = await downloader.ensureBinary();

    if (downloadedPath) {
      outputChannel.info(`Downloaded Perl LSP binary to: ${downloadedPath}`);
      return { path: downloadedPath, source: 'downloaded' };
    }
  } else {
    outputChannel.warn('Perl LSP binary not found and auto-download is disabled');
  }

  outputChannel.error('Failed to obtain a Perl LSP binary');
  return { path: null, source: 'unavailable' };
}

function createLanguageClientLifecycle(
  context: vscode.ExtensionContext,
): ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent> {
  return new ExtensionLanguageClientLifecycle({
    resolveServerPath: async () => {
      languageClientStartupMetrics.beginBinaryResolution();
      try {
        const resolution = await getServerPath(context);
        languageClientStartupMetrics.finishBinaryResolution(
          resolution.path ? 'ok' : 'unavailable',
          resolution.source,
          resolution.path,
        );
        return resolution.path;
      } catch (error: unknown) {
        languageClientStartupMetrics.finishBinaryResolution('error');
        throw error;
      }
    },
    createClient: (serverPath) => {
      // Bind this extension-host session to the exact managed candidate
      // before the server process can spawn (#10083), so no collector can
      // delete the candidate between selection and reference establishment.
      // No-op for user-managed or pre-policy installs, which are never
      // deletion subjects.
      const boundCandidateId = acquireLaunchManagedCandidateReference(
        serverPath,
        vscode.env.sessionId,
        (message) => outputChannel.info(`[managed-candidate] ${message}`),
      );
      if (boundCandidateId !== null) {
        outputChannel.info(`[managed-candidate] session bound to ${boundCandidateId}`);
      }
      languageClientStartupMetrics.beginServerStart();
      languageClientStartupMetrics.beginInitialize();
      return createLanguageClient(serverPath);
    },
    onStarted: async (startedClient) => {
      languageClientStartupMetrics.finishServerStart('ok');
      languageClientStartupMetrics.finishInitialize('ok');
      languageClientStartupMetrics.setServerVersion(
        startedClient.initializeResult?.serverInfo?.version,
      );
      const generation = languageClientLifecycle?.snapshot.generation;
      if (generation === undefined) {
        throw new Error('Language-client lifecycle is unavailable during startup finalization.');
      }
      await finalizeStartedLanguageClient(context, startedClient, generation);
    },
    onFailed: (snapshot) => {
      languageClientStartupMetrics.finishServerStart('error');
      languageClientStartupMetrics.finishInitialize('error');
      const message =
        snapshot.error instanceof Error ? snapshot.error.message : String(snapshot.error);
      outputChannel.error(
        `[lifecycle] Language client failed to start (generation ${snapshot.generation}): ${message}`,
      );
    },
    onStateChange: (snapshot) => {
      languageClientStartupMetrics.setLifecycleState(snapshot.state);
      syncLifecycleProjection();
      healthWidget?.onStateChange(clientStateForLifecycle(snapshot.state));
      if (snapshot.state === 'running') {
        // A language client can emit Running before onStarted finishes. The
        // lifecycle's running transition is the authoritative post-finalization
        // signal, so only it may start the stable-run grace window and reset
        // the automatic-restart budget after a genuinely healthy replacement.
        crashRecoveryArbiter.markRunning(snapshot.generation, Date.now());
      }
      // Only `resolving` needs an explicit projection: onStateChange maps it to
      // generic Starting, and every other lifecycle state is already owned by
      // onStateChange (including active indexing tokens and client_stopped detail).
      if (snapshot.state === 'resolving') {
        healthWidget?.setWorkspaceLifecycleState(projectWorkspaceLifecycle(snapshot.state));
      }
    },
    onClientStateChange: (_activeClient, event) => handleLifecycleClientStateChange(event),
    onCallbackError: (error, phase) => {
      const message = error instanceof Error ? error.message : String(error);
      outputChannel.error(`[lifecycle] ${phase} callback failed: ${message}`);
    },
  });
}

function clientStateForLifecycle(state: LifecycleState): ClientState {
  if (state === 'resolving' || state === 'starting') {
    return ClientState.Starting;
  }
  if (state === 'running') {
    return ClientState.Running;
  }
  return ClientState.Stopped;
}

async function finalizeStartedLanguageClient(
  context: vscode.ExtensionContext,
  startedClient: LanguageClient,
  generation: number,
): Promise<void> {
  const isCurrent = (): boolean =>
    languageClientLifecycle?.controller.isCurrent(startedClient, generation) === true;
  const assertCurrent = (): void => {
    if (!isCurrent()) {
      throw new StaleDocumentReplayError();
    }
  };

  assertCurrent();
  // A LanguageClient restart does not replay didOpen for documents that VS Code
  // kept open while the previous client was stopped. Rehydrate those documents
  // before providers issue requests against the new server.
  if (generation > 1) {
    const openPerlDocuments = vscode.workspace.textDocuments
      .filter(
        (document) =>
          document.languageId === 'perl' &&
          (document.uri.scheme === 'file' || document.uri.scheme === 'untitled'),
      )
      .map((document) => ({
        uri: document.uri.toString(),
        languageId: document.languageId,
        version: document.version,
        text: document.getText(),
      }));
    await replayOpenPerlDocumentsWhenReady(
      startedClient,
      openPerlDocuments,
      LanguageClientState.Running,
      isCurrent,
      2000,
    );
    assertCurrent();
  }

  // This hook is part of the lifecycle controller so initial startup and
  // restart/reinstall generations rebuild the same client integrations.
  // Refresh both self-reported identity fields on every generation (#12705):
  // assigning and clearing here keeps the reused status widget honest across
  // replacement startups instead of retaining the prior server's identity,
  // while a custom/wrapper server's name survives alongside its version.
  const serverInfo = startedClient.initializeResult?.serverInfo;
  healthWidget?.setName(serverInfo?.name);
  healthWidget?.setVersion(serverInfo?.version);

  // Offer AI inline completion once if the server advertises support (#1634).
  // Fire-and-forget; failures must not block lifecycle finalization.
  suggestAiCompletionIfSupported(context, startedClient).catch((err: unknown) => {
    const msg = err instanceof Error ? err.message : String(err);
    outputChannel.error(`[ai-completion] Error suggesting AI completion: ${msg}`);
  });

  await refreshTestAdapter(context);
  assertCurrent();
  refreshStreamingController(startedClient);
  try {
    await syncLanguageClientConfiguration(startedClient);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.error(`[configuration] initial synchronization failed: ${message}`);
  }
  assertCurrent();
  lastStartupDiagnosis = undefined;
  outputChannel.info('Perl Language Server started successfully');
}

/**
 * Present the remediation for replacement startup blocked by incomplete
 * client cleanup (#14448): the lifecycle refuses to construct a replacement
 * client until the window reloads, so generic start/restart guidance would
 * mislead the user into retrying a permanently blocked lifecycle.
 */
async function presentCleanupIncompleteBlockedRecovery(): Promise<void> {
  healthWidget?.onStateChange(ClientState.Stopped);
  const choice = await vscode.window.showErrorMessage(
    'The previous Perl language client did not finish cleaning up, so replacement startup is blocked. Reload the window before trying again.',
    'Reload Window',
    'View Logs',
  );
  if (choice === 'Reload Window') {
    void vscode.commands.executeCommand('workbench.action.reloadWindow');
  } else if (choice === 'View Logs') {
    outputChannel.show();
  }
}

async function initializeLanguageClient(context: vscode.ExtensionContext): Promise<boolean> {
  healthWidget?.onStateChange(ClientState.Starting);

  const lifecycle = languageClientLifecycle;
  if (!lifecycle) {
    outputChannel.debug('[startup] Language client lifecycle was not composed');
    healthWidget?.onStateChange(ClientState.Stopped);
    return false;
  }

  try {
    const startedClient = await lifecycle.start();
    if (!startedClient) {
      return false;
    }

    const serverPath = lifecycle.serverPath;
    if (!serverPath) {
      outputChannel.debug('[startup] Lifecycle started without a server path');
      return false;
    }

    return true;
  } catch (startError: unknown) {
    const msg = startError instanceof Error ? startError.message : String(startError);
    outputChannel.error(`[startup] Language client failed to start: ${msg}`);

    if (
      startError instanceof LanguageClientLifecycleError &&
      startError.reason === 'cleanup-incomplete'
    ) {
      await presentCleanupIncompleteBlockedRecovery();
      return false;
    }

    if (!lifecycle.serverPath) {
      healthWidget?.onStateChange(ClientState.Stopped);
      const notFoundMessage = configuredServerPathMissing
        ? `Perl Language Server not found: your perl-lsp.serverPath points to "${configuredServerPathMissing}", which does not exist. Fix the path or clear the setting to auto-download.`
        : 'Perl Language Server (perllsp) not found.';
      const choice = await vscode.window.showErrorMessage(
        notFoundMessage,
        'Install (cargo install perllsp)',
        'Open Settings',
      );

      if (choice === 'Install (cargo install perllsp)') {
        void vscode.window.showInformationMessage(
          'Run in your terminal: cargo install perllsp\nThen reload VS Code.',
        );
      } else if (choice === 'Open Settings') {
        void vscode.commands.executeCommand('workbench.action.openSettings', 'perl-lsp.serverPath');
      }
      return false;
    }

    // Probe the binary to get an actionable OS-level diagnosis (#3280).
    // If the probe result is Unknown (binary gave no useful output), fall
    // back to the health check (#3312) which can detect missing Perl etc.
    // lastStartupDiagnosis is updated so that serverNotRunningMessage() in
    // command handlers surfaces the specific root cause rather than a generic prompt.
    const probeResult = await probeStartupFailure(lifecycle.serverPath);
    let healthMsg: string | undefined;
    if (probeResult.kind === StartupErrorKind.Unknown) {
      const onboarding = new OnboardingManager(context, outputChannel);
      healthMsg = await onboarding.runStartupDiagnostics(lifecycle.serverPath);
    }
    // Cache the structured diagnosis so serverNotRunningMessage() can format
    // it; when healthMsg overrides the hint, wrap it as a synthetic diagnosis.
    lastStartupDiagnosis =
      healthMsg && probeResult.kind === StartupErrorKind.Unknown
        ? { kind: StartupErrorKind.Unknown, hint: healthMsg, remediation: probeResult.remediation }
        : probeResult;
    const dialogMessage = formatStartupFailureDialog(probeResult, healthMsg);

    const choice = await vscode.window.showErrorMessage(
      dialogMessage,
      'View Logs',
      'Run Health Check',
      'Reinstall',
      'Check serverPath Setting',
    );
    if (choice === 'View Logs') {
      outputChannel.show();
    } else if (choice === 'Run Health Check') {
      if (lifecycle.serverPath) {
        await vscode.commands.executeCommand('perl-lsp.runHealthCheck', lifecycle.serverPath);
      } else {
        await vscode.commands.executeCommand('perl-lsp.runHealthCheck');
      }
    } else if (choice === 'Reinstall') {
      await reinstallServerBinary(context);
    } else if (choice === 'Check serverPath Setting') {
      void vscode.commands.executeCommand('workbench.action.openSettings', 'perl-lsp.serverPath');
    }
    return false;
  }
}

/**
 * Exported so the configuration-transport wiring contract can execute the real
 * client options rather than asserting on source text (#14447).
 */
export function createLanguageClient(serverPath: string): LanguageClient {
  const generation = activeDocumentReadiness.beginGeneration();
  healthWidget?.seedIndexReadinessState('building');
  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      args: getLanguageServerLaunchArgs(false),
      transport: TransportKind.stdio,
    },
    debug: {
      command: serverPath,
      args: getLanguageServerLaunchArgs(true),
      transport: TransportKind.stdio,
    },
  };

  const disabledFeatures = buildDisabledFeaturesFromConfig(
    vscode.workspace.getConfiguration('perl-lsp'),
  );

  const clientOptions: LanguageClientOptions = {
    connectionOptions: LANGUAGE_CLIENT_CONNECTION_OPTIONS,
    documentSelector: [
      { scheme: 'file', language: 'perl' },
      { scheme: 'untitled', language: 'perl' },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/.perltidyrc'),
    },
    outputChannel,
    traceOutputChannel: outputChannel,
    middleware: {
      // The server pulls `section: "perl"` once unscoped and once per workspace
      // folder. Without this adapter the language client would resolve those
      // against the `perl.*` namespace, which this extension does not
      // contribute, and every folder item would come back null (#14447).
      workspace: {
        configuration: perlConfigurationMiddleware(),
      },
      provideCompletionItem: async (document, position, context, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleMiddlewareProviderCall(
          'Completion',
          document,
          async () => next(document, position, context, token),
          null,
        );
      },
      // The one authoritative provider for Perl inline completion (#8282).
      // Installing the owner as middleware keeps it on the language client's
      // own provider registration, so no second provider competes for the same
      // document selector.
      provideInlineCompletionItems: (document, position, context, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return inlineCompletionOwner.provideInlineCompletionItems(
          document,
          position,
          context,
          token,
          next,
        );
      },
      provideDefinition: async (document, position, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleMiddlewareProviderCall(
          'Definition',
          document,
          async () => next(document, position, token),
          null,
        );
      },
      provideHover: async (document, position, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleMiddlewareProviderCall(
          'Hover',
          document,
          async () => next(document, position, token),
          null,
        );
      },
      provideReferences: async (document, position, options, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleMiddlewareProviderCall(
          'References',
          document,
          async () => next(document, position, options, token),
          null,
        );
      },
      provideDocumentSymbols: async (document, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleMiddlewareProviderCall(
          'Symbols',
          document,
          async () => next(document, token),
          null,
        );
      },
      provideRenameEdits: async (document, position, newName, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleMiddlewareProviderCall(
          'Rename',
          document,
          async () => next(document, position, newName, token),
          null,
          'safe_refusal',
        );
      },
      provideCodeLenses: async (document, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return [];
        }
        try {
          const lenses = await next(document, token);
          return lenses?.map(rewriteTestLensCommand);
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return [];
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return [];
          }
          outputChannel?.warn(`[provider] CodeLens failed: ${message}`);
          return [];
        }
      },
      resolveCodeLens: async (codeLens, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return rewriteTestLensCommand(codeLens);
        }
        try {
          const resolved = await next(codeLens, token);
          return rewriteTestLensCommand(resolved ?? codeLens);
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return rewriteTestLensCommand(codeLens);
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return rewriteTestLensCommand(codeLens);
          }
          outputChannel?.warn(`[provider] CodeLens resolve failed: ${message}`);
          return rewriteTestLensCommand(codeLens);
        }
      },
      provideDocumentFormattingEdits: async (document, options, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleFormattingProviderCall(async () => next(document, options, token), null);
      },
      provideDocumentRangeFormattingEdits: async (document, range, options, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        return settleFormattingProviderCall(
          async () => next(document, range, options, token),
          null,
          true,
        );
      },
      provideFoldingRanges: async (document, context, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return [];
        }
        try {
          const result = await next(document, context, token);
          return result ?? [];
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return [];
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return [];
          }
          outputChannel?.warn(`[provider] FoldingRanges failed: ${message}`);
          return [];
        }
      },
      provideInlayHints: async (document, range, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return [];
        }
        try {
          const result = await next(document, range, token);
          return result ?? [];
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return [];
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return [];
          }
          outputChannel?.warn(`[provider] InlayHints failed: ${message}`);
          return [];
        }
      },
      provideDocumentSemanticTokens: async (document, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        try {
          return (await next(document, token)) ?? null;
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return null;
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return null;
          }
          outputChannel?.warn(`[provider] SemanticTokens failed: ${message}`);
          return null;
        }
      },
      provideDocumentRangeSemanticTokens: async (document, range, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        try {
          return (await next(document, range, token)) ?? null;
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return null;
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return null;
          }
          outputChannel?.warn(`[provider] RangeSemanticTokens failed: ${message}`);
          return null;
        }
      },
      provideDocumentSemanticTokensEdits: async (document, previousResultId, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return null;
        }
        try {
          return (await next(document, previousResultId, token)) ?? null;
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return null;
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return null;
          }
          outputChannel?.warn(`[provider] SemanticTokensEdits failed: ${message}`);
          return null;
        }
      },
      provideCodeActions: async (document, range, context, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return [];
        }
        try {
          const result = await next(document, range, context, token);
          return result ?? [];
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return [];
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return [];
          }
          outputChannel?.warn(`[provider] CodeActions failed: ${message}`);
          return [];
        }
      },
      resolveCodeAction: async (item, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return item;
        }
        try {
          return (await next(item, token)) ?? item;
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return item;
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return item;
          }
          outputChannel?.warn(`[provider] CodeAction resolve failed: ${message}`);
          return item;
        }
      },
      provideDocumentLinks: async (document, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return [];
        }
        try {
          const result = await next(document, token);
          return result ?? [];
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return [];
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return [];
          }
          outputChannel?.warn(`[provider] DocumentLinks failed: ${message}`);
          return [];
        }
      },
      resolveDocumentLink: async (link, token, next) => {
        if (languageClientLifecycle?.snapshot.state !== 'running') {
          return link;
        }
        try {
          return (await next(link, token)) ?? link;
        } catch (error: unknown) {
          if (isRequestCancellation(error)) {
            return link;
          }
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes('Client got disposed')) {
            return link;
          }
          outputChannel?.warn(`[provider] DocumentLink resolve failed: ${message}`);
          return link;
        }
      },
      handleWorkDoneProgress: (token, params, next) => {
        healthWidget?.onProgress(token, params);
        next(token, params);
      },
    },
    initializationOptions: {
      disabledFeatures,
    },
  };

  const lc = new LanguageClient(
    'perl-language-server',
    'Perl Language Server',
    serverOptions,
    clientOptions,
  );
  lc.onNotification('perl-lsp/active-document-ready', (params: { uri?: string }) => {
    if (params?.uri) {
      activeDocumentReadiness.markReady(params.uri, generation);
    }
  });
  lc.onNotification(
    'perl-lsp/index-ready',
    (params: {
      ready?: boolean;
      state?: 'building' | 'ready' | 'ready_limited';
      reason?: string | null;
    }) => {
      const state = params?.state ?? (params?.ready === true ? 'ready' : undefined);
      if (state !== undefined) {
        activeDocumentReadiness.markIndexReady(generation, state, params.reason ?? undefined);
        healthWidget?.onIndexReadinessState(state, params.reason ?? undefined);
      }
    },
  );
  void lc.setTrace(getTraceLevel());
  return lc;
}

function recordLspProviderOutcome(
  label: string,
  document: vscode.TextDocument,
  result: unknown,
  emptyOutcome: 'legitimate_empty' | 'safe_refusal' = 'legitimate_empty',
): void {
  try {
    const presentation = presentLspProviderOutcome(
      label,
      result,
      activeDocumentReadiness.isReady(document.uri.toString()),
      emptyOutcome,
    );
    healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
  } catch {
    // Provider status projection must never replace the settled wire result.
  }
}

async function settleMiddlewareProviderCall<T>(
  label: string,
  document: vscode.TextDocument,
  call: () => Promise<T>,
  fallback: T,
  emptyOutcome: 'legitimate_empty' | 'safe_refusal' = 'legitimate_empty',
): Promise<T> {
  const settlement = await settleLspProviderCallWithDisposition(call, fallback);
  if (settlement.kind === 'returned') {
    safelyObserveProviderOutcome(() =>
      recordLspProviderOutcome(label, document, settlement.value, emptyOutcome),
    );
  } else if (settlement.kind === 'failed') {
    safelyObserveProviderOutcome(() => handleLspProviderError(label, settlement.error));
  }
  return settlement.wireValue;
}

export async function settleFormattingProviderCall<T>(
  call: () => Promise<T>,
  fallback: T,
  range: boolean = false,
): Promise<T> {
  const settlement = await settleLspProviderCallWithDisposition(call, fallback);
  if (settlement.kind === 'returned') {
    safelyObserveProviderOutcome(() => {
      const presentation = presentFormattingProviderOutcome(
        Array.isArray(settlement.value) ? settlement.value.length : 0,
        range,
      );
      healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
    });
  } else if (settlement.kind === 'failed') {
    const message = describeLspProviderError(settlement.error);
    safelyObserveProviderOutcome(() => handleFormattingError(message, outputChannel));
    safelyObserveProviderOutcome(() => {
      const presentation = presentFormattingProviderError(message, range);
      healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
    });
  }
  return settlement.wireValue;
}

function safelyObserveProviderOutcome(observer: () => void): void {
  try {
    observer();
  } catch {
    // Provider status and diagnostics must never replace the wire fallback.
  }
}

function describeLspProviderError(error: unknown): string {
  try {
    return error instanceof Error ? error.message : String(error);
  } catch {
    return 'unavailable provider error';
  }
}

function handleLspProviderError(label: string, error: unknown): void {
  const message = describeLspProviderError(error);
  try {
    const presentation = presentLspProviderError(label, message);
    healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
  } catch {
    // A failing status projection must not suppress the diagnostic warning.
  }
  try {
    outputChannel?.warn(`[provider] ${label} failed: ${message}`);
  } catch {
    // Logging is best effort after the wire fallback has been selected.
  }
}

function isRequestCancellation(error: unknown): boolean {
  return (
    error !== null &&
    typeof error === 'object' &&
    'code' in error &&
    (error as { code?: unknown }).code === -32800
  );
}

function providerResultHasValue(result: unknown): boolean {
  if (result === undefined || result === null) {
    return false;
  }
  if (Array.isArray(result)) {
    return result.length > 0;
  }
  if (typeof result === 'object' && 'items' in result) {
    const items = (result as { items?: unknown }).items;
    return Array.isArray(items) ? items.length > 0 : Boolean(items);
  }
  return true;
}

export function presentLspProviderOutcome(
  label: string,
  result: unknown,
  ready: boolean,
  emptyOutcome: 'legitimate_empty' | 'safe_refusal' = 'legitimate_empty',
): {
  providerOutcome: 'exact_current' | 'legitimate_empty' | 'safe_refusal' | 'not_ready';
  detail: string;
  action?: string;
  reasonCode: string;
} {
  if (!ready) {
    return {
      providerOutcome: 'not_ready',
      detail: `${label} is waiting for the active Perl document and workspace index to become ready.`,
      action: 'Wait for workspace readiness, then retry the request.',
      reasonCode: `${label.toLowerCase()}_before_readiness`,
    };
  }
  if (providerResultHasValue(result)) {
    return {
      providerOutcome: 'exact_current',
      detail: `${label} returned a current source-backed result.`,
      reasonCode: `${label.toLowerCase()}_result_available`,
    };
  }
  return emptyOutcome === 'safe_refusal'
    ? {
        providerOutcome: 'safe_refusal',
        detail: `${label} declined to produce an edit for this request.`,
        action: 'Review the provider decision before applying changes.',
        reasonCode: `${label.toLowerCase()}_safe_refusal`,
      }
    : {
        providerOutcome: 'legitimate_empty',
        detail: `${label} returned no result for the current source.`,
        reasonCode: `${label.toLowerCase()}_legitimate_empty`,
      };
}

export function presentLspProviderError(
  label: string,
  message: string,
): {
  providerOutcome: 'product_or_instrument_error';
  detail: string;
  action: string;
  reasonCode: string;
} {
  return {
    providerOutcome: 'product_or_instrument_error',
    detail: `${label} failed: ${message}`,
    action: 'Run the Health Check or inspect the provider decision explanation.',
    reasonCode: `${label.toLowerCase()}_provider_error`,
  };
}

export function shouldNudgeArrowCompletion(linePrefix: string): boolean {
  if (!linePrefix.endsWith('-')) {
    return false;
  }

  const beforeDash = linePrefix.slice(0, -1);
  if (beforeDash.length === 0 || /\s$/.test(beforeDash) || beforeDash.endsWith(':')) {
    return false;
  }

  return /(?:\$[\w:]+|[@%][\w:]+|[A-Z]\w*)$/.test(beforeDash);
}

/** Map an observed formatting result to the canonical provider presentation. */
export function presentFormattingProviderOutcome(
  editCount: number,
  range: boolean = false,
): {
  providerOutcome: 'exact_current' | 'legitimate_empty';
  detail: string;
  reasonCode: string;
} {
  if (editCount > 0) {
    return {
      providerOutcome: 'exact_current',
      detail: `Formatter produced ${editCount} ${range ? 'range ' : 'document '}edit${editCount === 1 ? '' : 's'}.`,
      reasonCode: range ? 'range_formatting_edits_available' : 'formatting_edits_available',
    };
  }
  return {
    providerOutcome: 'legitimate_empty',
    detail: range
      ? 'Formatter reported no range edits; the selected range is already formatted.'
      : 'Formatter reported no edits; the document is already formatted.',
    reasonCode: range ? 'range_formatting_already_current' : 'formatting_already_current',
  };
}

/** Map an observed formatting failure to an actionable provider presentation. */
export function presentFormattingProviderError(
  message: string,
  range: boolean = false,
): {
  providerOutcome: 'product_or_instrument_error';
  detail: string;
  action: string;
  reasonCode: string;
} {
  return {
    providerOutcome: 'product_or_instrument_error',
    detail: `${range ? 'Range formatting' : 'Formatting'} failed: ${message}`,
    action: 'Check the formatter configuration or run the Health Check.',
    reasonCode: range ? 'range_formatting_error' : 'formatting_error',
  };
}

export function maybeNudgeArrowCompletion(event: vscode.TextDocumentChangeEvent): void {
  const editor = vscode.window.activeTextEditor;
  if (!editor || event.document !== editor.document || event.document.languageId !== 'perl') {
    return;
  }

  if (event.contentChanges.length !== 1) {
    return;
  }

  const change = event.contentChanges[0];
  if (!change) {
    return;
  }

  if (change.rangeLength !== 0 || change.text !== '-') {
    return;
  }

  const lineText = event.document.lineAt(change.range.start.line).text;
  const linePrefix = lineText.slice(0, change.range.start.character + change.text.length);
  if (!shouldNudgeArrowCompletion(linePrefix)) {
    return;
  }

  void vscode.commands.executeCommand('editor.action.triggerSuggest');
}

/**
 * Insert boilerplate into newly created Perl files that are still empty.
 *
 * `perl-lsp.autoPopulateNewFiles` is contributed `scope: "resource"`, so the
 * gate is resolved against each created URI rather than once for the whole
 * event (#14547). An unscoped `getConfiguration('perl-lsp')` cannot observe a
 * `workspaceFolderValue` at all, so a multi-root workspace where one folder
 * turns population off previously took the global value for every folder. The
 * read must stay inside the loop for the declared scope to mean anything.
 *
 * A URI outside every workspace folder resolves to the global/workspace value,
 * which is the same answer the hoisted read gave, as does an unset value. A
 * workspace opened as a single folder has no workspace-folder layer to select,
 * so it is unaffected too — but note that a `.code-workspace` listing exactly
 * one folder is mechanically multi-root and does have that layer, so a value
 * set on that folder now wins where it previously could not be seen.
 */
export async function populateCreatedFiles(event: vscode.FileCreateEvent): Promise<void> {
  for (const uri of event.files) {
    const scoped = vscode.workspace.getConfiguration('perl-lsp', uri);
    if (!scoped.get<boolean>('autoPopulateNewFiles', true)) {
      continue;
    }

    const boilerplate = generateBoilerplate(uri.fsPath);
    if (!boilerplate) {
      continue;
    }

    const doc = await vscode.workspace.openTextDocument(uri);
    if (doc.getText().length > 0) {
      // File already has content — don't overwrite
      continue;
    }

    const edit = new vscode.WorkspaceEdit();
    edit.insert(uri, new vscode.Position(0, 0), boilerplate.content);
    await vscode.workspace.applyEdit(edit);
  }
}

/**
 * Probe the LSP binary directly and return diagnostic information.
 *
 * Runs the binary with `--version` (fast probe, 3s timeout). On failure,
 * classifies the stderr output into an actionable diagnosis.
 *
 * When execFile fails with no stderr (e.g., ENOEXEC for wrong-arch or EACCES
 * for permission denied), the OS never writes to stderr — the error code lives
 * in err.code instead.  We synthesize a recognisable string so that
 * classifyStartupError() returns the right kind rather than Unknown.
 */
async function probeStartupFailure(serverPath: string): Promise<StartupErrorDiagnosis> {
  return new Promise((resolve) => {
    execFile(
      serverPath,
      ['--version'],
      { timeout: 3000 },
      (err: Error | null, stdout: string, stderr: string) => {
        const combined = [stderr, stdout].filter(Boolean).join('\n').trim();
        if (err) {
          outputChannel.error(`[startup-probe] Binary probe failed: ${err.message}`);
          if (combined) {
            outputChannel.debug(`[startup-probe] stderr: ${combined}`);
          }

          // When stderr is empty, infer from the OS error code so the
          // classifier returns an actionable kind instead of Unknown.
          const errCode = (err as NodeJS.ErrnoException).code;
          let diagInput = combined;
          if (!diagInput) {
            if (errCode === 'ENOEXEC') {
              // Kernel refused execve — wrong ELF machine type (arch mismatch)
              diagInput = 'cannot execute binary file: Exec format error';
            } else if (errCode === 'EACCES') {
              // Kernel refused execve — execute bit not set
              diagInput = 'Permission denied';
            } else {
              diagInput = err.message;
            }
          }
          resolve(classifyStartupError(diagInput));
        } else {
          // Binary responded fine — classify as unknown (client-level issue)
          resolve(classifyStartupError(''));
        }
      },
    );
  });
}

function getTraceLevel(): Trace {
  const traceSetting = vscode.workspace
    .getConfiguration('perl-lsp')
    .get<string>('trace.server', 'off');

  switch ((traceSetting || 'off').toLowerCase()) {
    case 'messages':
      return Trace.Messages;
    case 'verbose':
      return Trace.Verbose;
    default:
      return Trace.Off;
  }
}

function getServerArgs(baseArgs: string[]): string[] {
  const config = vscode.workspace.getConfiguration('perl-lsp');
  const featureProfile = config.get<string>('featureProfile', 'auto');
  const canonicalProfile = normalizeFeatureProfile(featureProfile || 'auto');

  if (!canonicalProfile || canonicalProfile === 'auto') {
    return baseArgs;
  }

  return [...baseArgs, `--feature-profile=${canonicalProfile}`];
}

export function getLanguageServerLaunchArgs(enableLogging: boolean): string[] {
  const baseArgs = enableLogging ? ['--log'] : [];
  return getServerArgs(baseArgs);
}

function normalizeFeatureProfile(rawProfile: string): string | null {
  const normalized = rawProfile.trim().toLowerCase();
  if (!normalized) {
    return 'auto';
  }

  const normalizedProfile = normalized.replace(/_/g, '-');
  const knownProfiles = getSupportedFeatureProfiles();

  if (!knownProfiles.includes(normalizedProfile)) {
    outputChannel.warn(`Unsupported featureProfile '${rawProfile}'. Falling back to 'auto'.`);
    return null;
  }

  return normalizedProfile;
}

function getSupportedFeatureProfiles(): string[] {
  const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
  const schemaEnum =
    extension?.packageJSON?.contributes?.configuration?.properties?.['perl-lsp.featureProfile']
      ?.enum;

  if (Array.isArray(schemaEnum)) {
    return schemaEnum
      .map((value: unknown) => `${value}`)
      .map((profile) => profile.toLowerCase().replace(/_/g, '-'));
  }

  return ['auto', 'ga-lock', 'ga', 'prod', 'production', 'all'];
}

/**
 * Restart the language server through the authoritative lifecycle.
 *
 * Returns true only when restart was refused because the lifecycle's client
 * cleanup is incomplete (#14448): that lifecycle cannot admit a replacement
 * until the window reloads, so automatic crash recovery must not spend
 * further retry slots on it.
 */
async function restartServer(_context: vscode.ExtensionContext): Promise<boolean> {
  const lifecycle = languageClientLifecycle;
  if (!lifecycle) {
    vscode.window.showWarningMessage('Perl Language Server is not initialized yet.');
    return false;
  }

  // A dormant server has nothing to restart: only a lifecycle that never
  // left its initial stopped state is dormant. A `failed` lifecycle owns a
  // half-built generation that `restart()` must stop before starting (#12724),
  // so it must fall through to the restart path even when the compatibility
  // projections are stale. An explicit restart request is itself a
  // server-dependent entry point (#8180), so a dormant request is honoured by
  // starting the server rather than reporting the extension as "not
  // initialized".
  if (
    !client &&
    !currentServerPath &&
    !lifecycle.hasPendingServerPathOverride &&
    lifecycle.snapshot.generation === 0 &&
    lifecycle.snapshot.state === 'stopped'
  ) {
    if (!serverDemand) {
      vscode.window.showWarningMessage('Perl Language Server is not initialized yet.');
      return false;
    }
    await serverDemand.ensureStarted('command:restart', { retry: true });
    syncLifecycleProjection();
    const demand = serverDemand.snapshot;
    if (demand.state === 'failed') {
      // ensureStarted reports failure through its state rather than rejecting.
      // Staying silent here would be worse than the old "not initialized"
      // warning: the user asked for a server and would get no answer at all.
      const message = describeDemandError(demand.error);
      outputChannel?.error(`Failed to start perl-lsp: ${message}`);
      vscode.window
        .showErrorMessage(`Failed to start Perl Language Server: ${message}`, 'Show Output')
        .then((selection) => {
          if (selection === 'Show Output') {
            outputChannel.show();
          }
        });
      return false;
    }
    vscode.window
      .showInformationMessage('Perl Language Server started', 'Show Output')
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
      });
    return false;
  }

  // Mark this as a user-driven (or auto-recovery-driven) restart so the
  // mid-session crash handler (#4625) does not treat the resulting
  // Running → Stopped transition as an unexpected crash. The lifecycle
  // controller also disposes its state-change listener before stopping, so
  // this is defense-in-depth. Cleared in `finally` so a subsequent genuine
  // crash is never accidentally suppressed.
  userInitiatedStopPending = true;
  try {
    disposeClientIntegrations();
    const started = await lifecycle.restart();
    if (!started) {
      return false;
    }
    languageClientStartupMetrics.markMilestone('restart');
    syncLifecycleProjection();
    // Restart owns its own stop-then-start sequence, so tell the demand owner a
    // running generation exists. Without this a prior `failed` demand state
    // would survive a successful restart and suppress later document demand.
    serverDemand?.noteRunning();
    vscode.window
      .showInformationMessage('Perl Language Server restarted', 'Show Output')
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
      });
    return false;
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel?.error(`Failed to restart perl-lsp: ${message}`);
    // A rejected restart() means the running generation was stopped and its
    // replacement failed: the server is stopped. Tell the demand owner,
    // otherwise its stale `running`/in-flight belief suppresses all later
    // document demand, and even an explicit health-check retry no-ops because
    // `retry` only overrides `failed`.
    serverDemand?.noteStopped();
    if (error instanceof LanguageClientLifecycleError && error.reason === 'cleanup-incomplete') {
      // Incomplete cleanup blocks this lifecycle until the window reloads
      // (#14448): present that remediation instead of a bare restart failure,
      // and report the block so automatic crash recovery stops retrying.
      await presentCleanupIncompleteBlockedRecovery();
      return true;
    }
    vscode.window
      .showErrorMessage(`Failed to restart Perl Language Server: ${message}`, 'Show Output')
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
      });
    return false;
  } finally {
    userInitiatedStopPending = false;
  }
}

async function restartServerFromExplicitRecovery(context: vscode.ExtensionContext): Promise<void> {
  crashRecoveryArbiter.resetForExplicitRecovery();
  await restartServer(context);
}

function shouldFormatOnSave(document: vscode.TextDocument): boolean {
  if (document.languageId !== 'perl') {
    return false;
  }

  const config = vscode.workspace.getConfiguration('perl-lsp', document.uri);
  if (!config.get<boolean>('formatOnSave', false)) {
    return false;
  }

  // Warn if formatting is attempted while server is still indexing. (UX_GAP_2.4)
  // The format will likely fail silently — log it so users see why.
  if (healthWidget?.mode === 'indexing') {
    outputChannel.debug('[formatOnSave] Server is still indexing — formatting may fail');
  }

  return true;
}

async function formatDocumentOnSave(document: vscode.TextDocument): Promise<vscode.TextEdit[]> {
  const edits = await vscode.commands.executeCommand<vscode.TextEdit[]>(
    'vscode.executeFormatDocumentProvider',
    document.uri,
  );

  return edits ?? [];
}

async function refreshTestAdapter(context: vscode.ExtensionContext) {
  if (testAdapter) {
    testAdapter.dispose();
    testAdapter = undefined;
  }

  const config = vscode.workspace.getConfiguration('perl-lsp');
  if (!config.get<boolean>('enableTestIntegration', true)) {
    outputChannel.warn('Perl test integration disabled.');
    return;
  }

  testAdapter = new PerlTestAdapter();
  context.subscriptions.push(testAdapter);
  outputChannel.info('Perl test integration enabled.');
}

/**
 * Create or dispose the streaming inline completion controller based on config.
 *
 * The controller is only active when both `aiCompletion.enabled` and
 * `aiCompletion.streaming.enabled` are true and a language client is running.
 */
function refreshStreamingController(activeClient: LanguageClient | undefined): void {
  // Always dispose any existing controller first
  if (streamingController) {
    streamingController.dispose();
    streamingController = undefined;
  }

  if (!activeClient) {
    return;
  }

  const config = vscode.workspace.getConfiguration('perl-lsp');
  const aiEnabled = config.get<boolean>('aiCompletion.enabled', false);
  const streamingEnabled = config.get<boolean>('aiCompletion.streaming.enabled', true);

  if (aiEnabled && streamingEnabled) {
    streamingController = new StreamingCompletionController(activeClient);
    outputChannel.info('Streaming inline completion controller enabled.');
  }
}

function getManagedBinarySource(): ManagedBinarySource {
  const downloadBaseUrl = vscode.workspace
    .getConfiguration('perl-lsp')
    .get<string>('downloadBaseUrl', '');
  return downloadBaseUrl ? 'internal-base-url' : 'github-release';
}

async function readInstalledServerVersion(serverPath: string): Promise<string | undefined> {
  return new Promise((resolve) => {
    execFile(
      serverPath,
      ['--version'],
      { timeout: 3000 },
      (error: Error | null, stdout: string) => {
        if (error) {
          resolve(undefined);
          return;
        }
        resolve(parseLocalVersion(stdout) ?? undefined);
      },
    );
  });
}

async function reinstallServerBinary(
  context: vscode.ExtensionContext,
): Promise<ReinstallCommandResult> {
  if (!vscode.workspace.isTrusted) {
    vscode.window.showErrorMessage(
      'Cannot reinstall the perl-lsp binary in an untrusted workspace. Grant workspace trust first.',
    );
    return {
      ok: false,
      serverPath: currentServerPath ?? '',
      target: '',
      source: getManagedBinarySource(),
      error: 'workspace is not trusted',
    };
  }

  outputChannel.show(true);
  outputChannel.info('Reinstalling perllsp binary...');

  const downloader = new BinaryDownloader(context, outputChannel);
  const target = downloader.getTargetTriple();
  const source = getManagedBinarySource();

  // Lifecycle snapshot: stop a running language client before install so
  // Windows releases its handle on the existing perllsp.exe. On failure
  // we restart with the previous binary so the user is never left worse
  // off than before they invoked Reinstall.
  const lifecycleState = languageClientLifecycle?.snapshot.state;
  const wasRunning =
    lifecycleState !== undefined && lifecycleState !== 'stopped' && lifecycleState !== 'failed';
  const previousServerPath = languageClientLifecycle?.serverPath ?? null;

  if (wasRunning) {
    outputChannel.debug('[reinstall] stopping language client to release the running binary');
    try {
      await disposeLanguageClient();
    } catch (stopErr: unknown) {
      const msg = stopErr instanceof Error ? stopErr.message : String(stopErr);
      outputChannel.debug(`[reinstall] language client stop reported: ${msg}`);
    }
    // Brief grace period: Windows can lag a few ms releasing the
    // executable file handle after the LSP child process exits.
    await new Promise((resolve) => setTimeout(resolve, 250));
  }

  const downloadedPath = await downloader.ensureBinary(true);

  if (!downloadedPath) {
    vscode.window
      .showErrorMessage(
        'Could not reinstall perl-lsp. Check your internet connection and proxy settings, then try again.',
        'Show Output',
        'Open Settings',
      )
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
        if (selection === 'Open Settings') {
          void vscode.commands.executeCommand('workbench.action.openSettings', 'http.proxy');
        }
      });
    if (wasRunning && previousServerPath) {
      outputChannel.error('[reinstall] restoring previous binary after failed download');
      languageClientLifecycle?.setServerPathOverride(previousServerPath);
      try {
        await restartServer(context);
      } catch {
        // restartServer surfaces its own dialog/log; nothing to add here.
      }
    }
    return {
      ok: false,
      serverPath: previousServerPath ?? '',
      target,
      source,
      error: downloader.getLastErrorMessage() ?? 'download failed',
    };
  }

  const healthOk = await runLanguageServerHealthCheck(downloadedPath, outputChannel);
  const version = await readInstalledServerVersion(downloadedPath);
  if (!healthOk) {
    vscode.window
      .showErrorMessage(
        'The downloaded perl-lsp binary failed its health check — it may be corrupted or incompatible with your platform.',
        'Show Output',
        'Report Issue',
      )
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
        if (selection === 'Report Issue') {
          void vscode.env.openExternal(
            vscode.Uri.parse('https://github.com/EffortlessMetrics/perl-lsp/issues'),
          );
        }
      });
    if (wasRunning && previousServerPath) {
      outputChannel.error('[reinstall] restoring previous binary after failed health check');
      languageClientLifecycle?.setServerPathOverride(previousServerPath);
      try {
        await restartServer(context);
      } catch {
        // restartServer surfaces its own dialog/log.
      }
    }
    return {
      ok: false,
      serverPath: downloadedPath,
      target,
      source,
      version,
      checksumVerified: true,
      error: 'health check failed',
    };
  }

  languageClientLifecycle?.setServerPathOverride(downloadedPath);

  if (wasRunning) {
    outputChannel.info('[reinstall] restarting language client with the freshly installed binary');
    try {
      await restartServer(context);
    } catch {
      // restartServer surfaces its own dialog/log.
    }
  } else {
    vscode.window.showInformationMessage('perl-lsp was reinstalled successfully.', 'OK');
  }

  return {
    ok: true,
    serverPath: downloadedPath,
    target,
    source,
    version,
    checksumVerified: true,
  };
}

export function handleClientStateChange(event: StateChangeEvent): void {
  // Preserve the status-bar signal for an unexpected language-client crash;
  // the controller state changes only on explicit lifecycle operations.
  healthWidget?.onStateChange(event.newState as unknown as ClientState);

  // Note: event.newState / oldState are vscode-languageclient's `State` enum
  // which shares numeric values with ClientState (Stopped=1, Running=2) but
  // is a distinct nominal type, so we cast to number for comparison.
  const newStateNum = event.newState as unknown as number;
  const oldStateNum = event.oldState as unknown as number;

  // Start/stop the watchdog based on client state (#5092)
  if (newStateNum === ClientState.Running) {
    startWatchdog();
  } else if (newStateNum === ClientState.Stopped) {
    stopWatchdog();
  }

  if (newStateNum !== ClientState.Stopped || oldStateNum !== ClientState.Running) {
    return;
  }

  // A user-initiated stop/restart disposes the lifecycle's state-change
  // listener before the client stops, so this branch is only reached for
  // *unexpected* mid-session crashes. The sentinel below is defense-in-depth
  // for any future path that stops the client outside the controller.
  if (userInitiatedStopPending) {
    userInitiatedStopPending = false;
    return;
  }

  // One authoritative crash-recovery entry point (#7845): the unexpected
  // process-exit observation is arbitrated by generation + process identity,
  // so a late duplicate callback for the same failed generation cannot start
  // a second recovery.
  void recoverFromObservedCrash('process_exit');
}

/**
 * Testable adapter for the raw language-client state callback. A raw Running
 * event is presentation/process evidence only; lifecycle stability is not
 * recorded until startup finalization publishes the lifecycle `running`
 * transition (#12724).
 * @internal
 */
export function _handleLifecycleClientStateChangeForTest(event: StateChangeEvent): void {
  handleLifecycleClientStateChange(event);
}

function handleLifecycleClientStateChange(event: StateChangeEvent): void {
  if (event.newState === LanguageClientState.Starting) {
    languageClientStartupMetrics.markMilestone('process_started');
    languageClientStartupMetrics.finishServerStart('ok');
  }
  handleClientStateChange(event);
}

/**
 * Capture the mid-session failure diagnosis, invalidate the failed
 * generation's demand state, and surface the failure on the health widget.
 * Shared by every non-deduped arbiter decision (#7845). Returns the captured
 * hint for user-facing messages.
 */
function recordUnexpectedFailure(): string {
  const hint = 'The Perl Language Server stopped unexpectedly. Check the Output panel for details.';
  lastStartupDiagnosis = {
    kind: StartupErrorKind.Unknown,
    hint,
    remediation:
      'Try restarting the server (Command Palette: "Perl: Restart Server") or run the Health Check.',
  };
  serverDemand?.noteStopped();
  healthWidget?.setWorkspaceLifecycleState('failed', {
    detail: 'The Perl Language Server stopped unexpectedly.',
    action: 'Restart the server or run the Health Check.',
    reasonCode: 'unexpected_server_stop',
  });
  return hint;
}

/**
 * Surface an unexpected mid-session server failure observed through
 * `source` (process exit or watchdog) and arbitrate its recovery (#4625,
 * #7845).
 *
 * The observation is routed through the generation-owned
 * `crashRecoveryArbiter` first: a duplicate observation for a generation
 * that already has an active or recently settled recovery episode is
 * deduplicated and performs no recovery work, and a different-generation
 * failure that arrives while an episode's restart promise is still pending
 * is deferred behind that episode (the active continuation drains and
 * re-arbitrates it after settling its own episode handle). Only a
 * `start_recovery` decision captures the crash diagnosis, invalidates the
 * failed generation's demand state, surfaces the failure, and consumes one
 * automatic-restart slot; a `crash_budget_exhausted` decision stops looping
 * and asks the user to intervene.
 */
async function recoverFromObservedCrash(
  source: CrashObservationSource,
  observedGeneration: number = currentCrashGeneration(),
): Promise<void> {
  // A stale observation (e.g. a watchdog probe that started before the
  // failed generation was replaced) must never arbitrate against the
  // replacement generation: if the observed generation has been superseded,
  // drop the observation instead of restarting the healthy replacement.
  if (observedGeneration !== currentCrashGeneration()) {
    outputChannel?.warn(
      `[lifecycle] Stale ${source} observation for superseded generation ${observedGeneration} ignored (current generation ${currentCrashGeneration()}).`,
    );
    return;
  }
  const failedGeneration = observedGeneration;
  const decision = crashRecoveryArbiter.observeFailure({
    failed_generation: failedGeneration,
    process_identity: crashProcessIdentity(failedGeneration),
    source,
    observed_at_ms: Date.now(),
  });

  if (
    decision.disposition === 'deduped_existing_episode' ||
    decision.disposition === 'deduped_previous_episode'
  ) {
    outputChannel?.info(
      `[lifecycle] ${source} observation for generation ${failedGeneration} deduplicated by recovery episode ${decision.episode_id} (${decision.disposition}); no second arbitration.`,
    );
    return;
  }

  if (decision.disposition === 'deferred_active_episode') {
    // A different generation failed while this episode's restart promise is
    // still pending. It must not open a second concurrent restart nor
    // overwrite the active episode: it is queued in the arbiter and
    // re-arbitrated by the active continuation's settle (#7845).
    outputChannel?.info(
      `[lifecycle] ${source} observation for generation ${failedGeneration} deferred behind active recovery episode ${decision.episode_id}; it will be re-arbitrated when that episode settles.`,
    );
    return;
  }

  const context = extensionContext;
  const hint = recordUnexpectedFailure();

  outputChannel?.info(
    `[lifecycle] Perl Language Server failed mid-session (source: ${decision.observation_source}, episode ${decision.episode_id}).`,
  );

  if (decision.disposition === 'crash_budget_exhausted') {
    // The raw client has stopped, but the lifecycle snapshot otherwise still
    // advertises its last accepted `running` generation. Retire that dead
    // client before presenting the manual-recovery boundary so exhaustion is
    // observably terminal and an explicit retry starts from `stopped`.
    disposeClientIntegrations();
    await languageClientLifecycle?.stop();
    await reportCrashBudgetExhausted();
    return;
  }

  const attempt = decision.automatic_attempt;
  outputChannel?.info(
    `[lifecycle] Auto-restarting Perl Language Server (attempt ${attempt}/${MAX_AUTO_RESTART_ATTEMPTS})…`,
  );
  const message = `Perl Language Server crashed and is restarting automatically (attempt ${attempt}/${MAX_AUTO_RESTART_ATTEMPTS}). ${hint}`;
  void vscode.window.showErrorMessage(message, 'Show Output').then((selection) => {
    if (selection === 'Show Output') {
      outputChannel?.show();
    }
  });
  if (!context) {
    outputChannel?.info('[lifecycle] Cannot auto-restart: extension context is not available.');
    await settleRecoveryEpisode(decision, 'recovery_failed', null);
    return;
  }
  // restartServer surfaces its own dialogs/logs; its boolean result reports
  // the one terminal refusal automatic recovery must not retry: a lifecycle
  // whose client cleanup is incomplete stays blocked until the window
  // reloads, and re-arming it would only burn the remaining retry budget.
  const restartBlockedByIncompleteCleanup = await restartServer(context);
  // The replacement run now owns the next failed-generation identity (in
  // the unit-test harness the lifecycle controller is absent, so the
  // fallback generation advances here — after arbitration began — so that
  // a duplicate observation for the failed generation still dedupes).
  if (languageClientLifecycle === undefined) {
    // The fallback models "this restart spawned one newer generation":
    // advance monotonically past the failed generation, but never past a
    // replacement that already spawned and failed while the restart
    // promise was pending — that failed replacement stays current so its
    // deferred observation re-arbitrates after this settle.
    fallbackCrashGeneration = Math.max(fallbackCrashGeneration, failedGeneration + 1);
    await settleRecoveryEpisode(decision, 'recovered', currentCrashGeneration());
    return;
  }
  // With a live lifecycle, restartServer resolves only after the
  // replacement generation finished its startup finalization (initialize +
  // document replay + readiness rebound), so the episode settles as
  // recovered only on an accepted running replacement — a spawned process
  // alone is not "recovered" (#7845). A restart that did not reach the
  // running state settles as failed and the next crash (if any) consumes
  // another retry slot.
  const replacementSnapshot = languageClientLifecycle.snapshot;
  if (replacementSnapshot.state === 'running') {
    await settleRecoveryEpisode(decision, 'recovered', replacementSnapshot.generation);
    return;
  }
  outputChannel?.error(
    `[lifecycle] Auto-restart attempt ${attempt} did not reach the running state (state: ${replacementSnapshot.state}).`,
  );
  await settleRecoveryEpisode(decision, 'recovery_failed', null);
  // A replacement that failed during STARTUP (#12724) can never be observed
  // again by the existing failure surfaces: it never reached Running, so no
  // Running→Stopped crash event fires, and the watchdog only arms while
  // running. Settling `recovery_failed` here would therefore deadlock
  // convergence — readiness stays cleared and no later restart ever happens
  // even though automatic budget remains. Re-arm instead: arbitrate the
  // failed replacement generation's own recorded failure through this same
  // entry point, so the arbiter keeps sole ownership of dedupe, budget, and
  // exhaustion (fail-closed: genuinely repeated failures still end at the
  // exhaustion dialog). An explicit recovery that superseded the failed
  // generation meanwhile is handled by the stale-generation guard above.
  if (replacementSnapshot.state === 'failed' && !restartBlockedByIncompleteCleanup) {
    await recoverFromObservedCrash('startup_failure', replacementSnapshot.generation);
  }
}

/**
 * Settle exactly the episode that authorized the current recovery
 * continuation — the episode handle carried by `decision` — and then drain
 * the oldest deferred different-generation failure, if any (#7845).
 *
 * Binding settlement to the handle means a continuation whose restart
 * promise resolved late can never settle a newer episode that became
 * active in the meantime; the deferred drain serializes a
 * different-generation failure that arrived while this episode was active
 * into its own recovery instead of running a second restart concurrently.
 */
async function settleRecoveryEpisode(
  decision: CrashRecoveryDecision,
  terminal: RecoveryTerminalDisposition,
  replacementGeneration: number | null,
): Promise<void> {
  const settledActive = crashRecoveryArbiter.settleEpisode(
    decision,
    terminal,
    replacementGeneration,
  );
  if (!settledActive) {
    outputChannel?.warn(
      `[lifecycle] Settling recovery episode ${decision.episode_id} as ${terminal} skipped: the episode is no longer active (superseded by an explicit recovery or an earlier settle).`,
    );
  }
  const pending = crashRecoveryArbiter.takePendingFailureObservation();
  if (pending === null) {
    return;
  }
  outputChannel?.info(
    `[lifecycle] Re-arbitrating deferred ${pending.source} observation for generation ${pending.failed_generation} after recovery episode ${decision.episode_id} settled (${terminal}).`,
  );
  await recoverFromObservedCrash(pending.source, pending.failed_generation);
}

/**
 * Report the arbiter's crash-budget exhaustion to the user (#7845). Called
 * from `recoverFromObservedCrash` when the arbiter returns
 * `crash_budget_exhausted`; a manual restart is an explicit user action and
 * does not consume crash budget.
 */
async function reportCrashBudgetExhausted(): Promise<void> {
  const context = extensionContext;
  const hint =
    lastStartupDiagnosis?.hint ??
    'The Perl Language Server stopped unexpectedly. Check the Output panel for details.';

  // Retry budget exhausted — do not loop. Ask the user to intervene.
  outputChannel?.info(
    `[lifecycle] Auto-restart limit reached (${MAX_AUTO_RESTART_ATTEMPTS} attempts). Awaiting manual restart.`,
  );
  const exhausted = `Perl Language Server crashed and could not be restarted automatically after ${MAX_AUTO_RESTART_ATTEMPTS} attempts. ${hint}`;
  const selection = await vscode.window.showErrorMessage(
    exhausted,
    'Restart Server',
    'Run Health Check',
    'Show Output',
  );
  if (selection === 'Restart Server' && context) {
    // A manual restart is an explicit user restart (#7845): it resets the
    // automatic crash-recovery budget without ever having consumed it.
    // restartServer never rejects; it surfaces its own failure dialogs.
    await restartServerFromExplicitRecovery(context);
  } else if (selection === 'Run Health Check') {
    const serverPath = currentServerPath ?? undefined;
    await vscode.commands.executeCommand('perl-lsp.runHealthCheck', serverPath);
  } else if (selection === 'Show Output') {
    outputChannel?.show();
  }
}

/**
 * Start a periodic liveness watchdog. Every WATCHDOG_INTERVAL_MS, sends a
 * lightweight workspace/symbol request. If it doesn't respond within
 * WATCHDOG_TIMEOUT_MS, the server is considered hung and restarted. (#5092)
 */
function startWatchdog(): void {
  stopWatchdog();
  watchdogTimer = setInterval(async () => {
    if (languageClientLifecycle?.snapshot.state !== 'running') {
      return;
    }
    // Bind the probe to the generation it interrogates: if the generation is
    // replaced while the request is in flight, the late result is stale and
    // must not arbitrate against the replacement (#7845).
    const probedGeneration = currentCrashGeneration();
    let watchdogTimeout: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        languageClientLifecycle.client?.sendRequest('$/perl-lsp/watchdog'),
        new Promise((_, reject) => {
          watchdogTimeout = setTimeout(
            () => reject(new Error('watchdog timeout')),
            WATCHDOG_TIMEOUT_MS,
          );
        }),
      ]);
    } catch {
      outputChannel.warn('[watchdog] Server unresponsive — triggering restart');
      // Watchdog observations route through the same arbiter (#7845): a
      // hung generation is a failure episode keyed by generation + process
      // identity, so a watchdog timeout and the process exit that follows
      // deduplicate into one recovery operation, while a stale probe for a
      // superseded generation is dropped. Unlike the legacy path, the
      // watchdog no longer resets the crash budget before recovering.
      await recoverFromObservedCrash('watchdog', probedGeneration);
    } finally {
      if (watchdogTimeout !== undefined) {
        clearTimeout(watchdogTimeout);
      }
    }
  }, WATCHDOG_INTERVAL_MS);
}

/** Stop the watchdog timer. */
function stopWatchdog(): void {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = undefined;
  }
}

async function promptForClientRefresh(context: vscode.ExtensionContext) {
  const choice = await vscode.window.showInformationMessage(
    'Perl LSP settings changed. Restart the language server to apply the new configuration.',
    'Restart Now',
    'Later',
  );

  if (choice === 'Restart Now') {
    await restartServer(context);
  }
}

function disposeClientIntegrations(): void {
  if (streamingController) {
    streamingController.dispose();
    streamingController = undefined;
  }

  if (testAdapter) {
    testAdapter.dispose();
    testAdapter = undefined;
  }
}

async function disposeLanguageClient(): Promise<void> {
  // Extension shutdown is user-initiated; suppress the crash handler and
  // reset crash-recovery state so a re-activation starts with a fresh budget.
  // Deactivation is terminal for the session (#7845): the episode dedupe
  // history is cleared too, so a re-activated lifecycle's restarted
  // generation counter cannot collide with a previous session's episode
  // identities and suppress a genuine new crash.
  userInitiatedStopPending = true;
  crashRecoveryArbiter.resetAllEpisodeMemory();
  disposeClientIntegrations();
  let shutdownProvedTerminal = false;
  if (languageClientLifecycle) {
    await languageClientLifecycle.stop();
    // stop() resolves — never rejects — even when stop/dispose timed out and
    // the lifecycle transitioned to `failed`. Only the clean `stopped` state
    // proves the server process is terminal (#10083); anything else keeps
    // the session's host references `live` so a collector cannot delete the
    // candidate under a possibly-still-running process.
    shutdownProvedTerminal = mayReleaseManagedCandidateReferences(
      languageClientLifecycle.snapshot.state,
    );
    syncLifecycleProjection();
  }
  // The server process bound to the managed candidate is proven terminal
  // now, so this session's exact host references can be released (#10083).
  // Crashes and unproven shutdowns never reach the release — their
  // references stay `live` and conservative for a later recovery path
  // (#11539) rather than authorizing deletion.
  const managedStorageRoot = extensionContext?.globalStorageUri?.fsPath;
  if (typeof managedStorageRoot !== 'string') {
    outputChannel.info(
      '[managed-candidate] no managed storage root available; host reference release skipped.',
    );
  } else if (shutdownProvedTerminal) {
    releaseManagedCandidateSessionReferences(managedStorageRoot, vscode.env.sessionId, (message) =>
      outputChannel.info(`[managed-candidate] ${message}`),
    );
  } else {
    outputChannel.info(
      '[managed-candidate] shutdown did not prove process termination; host references retained.',
    );
  }
  // No generation is running any more. Reinstall stops the client and then
  // restarts it, so leaving demand on `running` here would make that restart a
  // no-op and strand the user on a stopped server.
  serverDemand?.noteStopped();
  userInitiatedStopPending = false;
}
