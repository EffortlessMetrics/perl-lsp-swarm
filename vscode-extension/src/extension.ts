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
import { runLanguageServerHealthCheck } from './languageServerHealth';
import { OnboardingManager } from './onboarding';
import {
  openDemoProjectCommand,
  suggestAiCompletionIfSupported,
  suggestDiscoveredIncludePaths,
  validateIncludePaths,
  warnAboutPerlExtensionConflicts,
} from './extensionWorkspaceGuidance';
export {
  openDemoProjectCommand,
  suggestAiCompletionIfSupported,
  suggestDiscoveredIncludePaths,
  validateIncludePaths,
  warnAboutPerlExtensionConflicts,
} from './extensionWorkspaceGuidance';
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
  organizeImportsCommand,
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
import type { LifecycleState } from './languageClientLifecycle';
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
let languageClientLifecycle:
  | ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent>
  | undefined;

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
let latestLanguageClientGeneration = 0;
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
 * Mid-session crash recovery state (#4625).
 *
 * `autoRestartAttempts` counts consecutive crash-triggered auto-restart
 * attempts. It is reset to 0 once the server has been stably `Running` for
 * at least `STABLE_RUN_GRACE_MS` (so a transient crash does not permanently
 * exhaust the retry budget, but a tight crash→restart→crash loop is capped).
 *
 * `stableRunningSince` records the timestamp at which the server last reached
 * `Running`; consulted at crash time to decide whether the prior run was long
 * enough to count as a new episode.
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
let autoRestartAttempts = 0;
let watchdogTimer: NodeJS.Timeout | undefined;
let stableRunningSince: number | undefined;
let userInitiatedStopPending = false;

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
  autoRestartAttempts = 0;
  stableRunningSince = undefined;
  userInitiatedStopPending = false;
}

/**
 * Test helper — read the current auto-restart attempt counter.
 * @internal
 */
export function _autoRestartAttemptsForTest(): number {
  return autoRestartAttempts;
}

/**
 * Test helper — mark the server as having been stably running since the given
 * timestamp (defaults to long enough ago to satisfy the stable-run grace).
 * @internal
 */
export function _markStableRunningForTest(since?: number): void {
  stableRunningSince = since ?? Date.now() - (STABLE_RUN_GRACE_MS + 1_000);
}

/**
 * Test helper — inject the extension context required by the crash handler.
 * @internal
 */
export function _setExtensionContextForTest(context: vscode.ExtensionContext): void {
  extensionContext = context;
}

/**
 * Test helper — set the user-initiated-stop sentinel.
 * @internal
 */
export function _setUserInitiatedStopPendingForTest(value: boolean): void {
  userInitiatedStopPending = value;
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
  languageClientStartupMetrics.markMilestone('activate_entered');
  featureActivationMetrics.beginActivation();
  // Set activation context so commands are available even without a Perl file open (#UX4.4)
  vscode.commands.executeCommand('setContext', 'perl-lsp.activated', true);
  // Cache the context so the mid-session crash handler (#4625) can drive an
  // auto-restart without a parameter of its own.
  extensionContext = context;
  // LogOutputChannel is required by vscode-languageclient for outputChannel and
  // traceOutputChannel. Messages are routed through level-aware methods
  // (debug/info/warn/error) so the VS Code Output panel level filter works.
  outputChannel = vscode.window.createOutputChannel('Perl Language Server', { log: true });
  const mcpDisposable = featureActivationMetrics.measure('mcp', true, () =>
    registerMcpSupport(outputChannel),
  );
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusBarItem.command = 'perl-lsp.showWorkspaceStatus';
  statusBarItem.accessibilityInformation = {
    label: 'Perl Language Server',
    role: 'button',
  };
  statusBarItem.show();
  healthWidget = new HealthWidget(statusBarItem);
  healthWidget.onStateChange(ClientState.Starting);
  // Wire the file/error-count setters to client-side telemetry (#4620).
  // Without this, the running-state status bar never shows the
  // `perl-lsp v<x>: <N> files | <M> errors` indicator the widget promises.
  healthWidgetDataSource = HealthWidgetDataSource.fromDeps(healthWidget, {
    languages: vscode.languages,
    workspace: vscode.workspace,
  });
  healthWidgetDataSource.start();
  context.subscriptions.push(healthWidgetDataSource);
  languageClientLifecycle = createLanguageClientLifecycle(context);
  syncLifecycleProjection();
  context.subscriptions.push(statusBarItem);

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

      try {
        // Coalesce with activation's in-flight startup so a first-run health
        // check never observes the transient null projection while the
        // managed binary is being resolved.
        await lifecycle.start();
      } catch (error: unknown) {
        const message = error instanceof Error ? error.message : String(error);
        outputChannel.warn(`[health-check] Server startup did not complete: ${message}`);
      }
      syncLifecycleProjection();
      return lifecycle.serverPath;
    },
    reinstallServerBinary: () => reinstallServerBinary(context),
    restartServer: () => restartServer(context),
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

  const openDemoProject = async () => {
    await openDemoProjectCommand(context);
  };

  const criticCommandDisposables = registerCriticCommandGroup({
    runPerlCriticOnActiveFile: () => runPerlCriticOnActiveFile(),
    setPerlCriticSeverity: () => setPerlCriticSeverity(),
  });

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

  const navigationCommandDisposables = registerNavigationCommandGroup({
    openDemoProject,
    organizeImports: organizeImportsCommand,
    showVersion: () =>
      showVersionCommand({
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
      }),
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

  const refactoringCommandDisposables = registerRefactoringCommandGroup({
    extractVariable: () =>
      extractVariableCommand({ activeClient: client, serverNotRunningMessage }),
    extractMethod: () => extractMethodCommand({ activeClient: client, serverNotRunningMessage }),
    showRefactoringOptions: showRefactoringOptionsCommand,
  });

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

  const formatOnSaveDisposable = vscode.workspace.onWillSaveTextDocument((event) => {
    if (!shouldFormatOnSave(event.document)) {
      return;
    }

    event.waitUntil(formatDocumentOnSave(event.document));
  });

  const configurationWatcher = featureActivationMetrics.measure('configuration', true, () =>
    registerWorkspaceConfigurationEvents({
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

  const fileCreationWatcher = vscode.workspace.onDidCreateFiles(async (event) => {
    try {
      const config = vscode.workspace.getConfiguration('perl-lsp');
      if (!config.get<boolean>('autoPopulateNewFiles', true)) {
        return;
      }

      for (const uri of event.files) {
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
    } catch (e) {
      outputChannel.error('File creation handler error', e);
    }
  });

  const arrowCompletionWatcher = vscode.workspace.onDidChangeTextDocument((event) => {
    maybeNudgeArrowCompletion(event);
  });

  const providerDisposables = featureActivationMetrics.measure('providers', true, () => [
    ...registerDocumentFeatureGroup({
      extensionContext: context,
      registerGherkinProviders,
      registerGherkinStepDefinitionSupport,
      registerPodPreview,
    }),
  ]);

  context.subscriptions.push(
    ...serverCommandDisposables,
    ...criticCommandDisposables,
    ...testCommandDisposables,
    ...navigationCommandDisposables,
    ...documentCommandDisposables,
    ...diagnosticCommandDisposables,
    ...onboardingCommandDisposables,
    ...refactoringCommandDisposables,
    ...supportCommandDisposables,
    formatOnSaveDisposable,
    configurationWatcher,
    fileCreationWatcher,
    arrowCompletionWatcher,
    ...(mcpDisposable ? [mcpDisposable] : []),
    ...providerDisposables,
  );
  languageClientStartupMetrics.markMilestone('commands_registered');

  // Initialize debug adapter
  featureActivationMetrics.measure('debugger', true, () => activateDebugger(context));

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
      markLanguageClientStartupMilestone,
      waitForActiveDocumentReady,
      stop: deactivate,
    };
  }

  // Workspace Trust gate: do not download binaries or spawn the language
  // server in an untrusted workspace. Defer startup until trust is granted.
  if (!vscode.workspace.isTrusted) {
    outputChannel.info(
      '[startup] Workspace is not trusted — deferring language server startup until trust is granted.',
    );
    // Deferred startup is a configuration decision, not a slow start. Without
    // this the widget stays on 'starting' indefinitely, which is
    // indistinguishable from a server that hung — the exact conflation the
    // #5900 experience contract forbids.
    healthWidget?.setWorkspaceLifecycleState('configuration_action_required', {
      detail: 'Perl language features are paused because this workspace is not trusted.',
      action: 'Trust this workspace to start the Perl language server.',
      reasonCode: 'workspace_untrusted',
    });
    const trustDisposable = vscode.workspace.onDidGrantWorkspaceTrust(() => {
      outputChannel.info('[startup] Workspace trust granted — starting language server.');
      // Hand the widget back to the ordinary startup lifecycle so the
      // action-required state cannot outlive the condition that caused it.
      healthWidget?.setWorkspaceLifecycleState('starting');
      startLanguageClientAfterActivation(context, whatsNewManager);
    });
    context.subscriptions.push(trustDisposable);
    languageClientStartupMetrics.markMilestone('activate_returned');
    return {
      getLanguageClientStartupMetrics,
      getFeatureActivationMetrics,
      getActiveDocumentReadiness,
      markLanguageClientStartupMilestone,
      waitForActiveDocumentReady,
      stop: deactivate,
    };
  }

  startLanguageClientAfterActivation(context, whatsNewManager);
  languageClientStartupMetrics.markMilestone('activate_returned');
  return {
    getLanguageClientStartupMetrics,
    getFeatureActivationMetrics,
    getActiveDocumentReadiness,
    markLanguageClientStartupMilestone,
    waitForActiveDocumentReady,
    stop: deactivate,
  };
}

export async function deactivate() {
  try {
    await disposeLanguageClient();
  } finally {
    languageClientStartupMetrics.markMilestone('shutdown');
  }
}

function startLanguageClientAfterActivation(
  context: vscode.ExtensionContext,
  whatsNewManager: WhatsNewManager,
): void {
  finishStartupAfterActivation(context, whatsNewManager).catch((err: unknown) => {
    const msg = err instanceof Error ? err.message : String(err);
    outputChannel.error(`[startup] Background startup failed: ${msg}`);
    healthWidget?.setWorkspaceLifecycleState('failed', {
      detail: `Perl Language Server failed to start: ${msg}`,
      action: 'Run the Health Check or fix the server configuration.',
      reasonCode: 'startup_failure',
    });
  });
}

async function finishStartupAfterActivation(
  context: vscode.ExtensionContext,
  whatsNewManager: WhatsNewManager,
): Promise<void> {
  const initialized = await initializeLanguageClient(context);
  if (initialized) {
    languageClientStartupMetrics.markMilestone('workspace_ready');
  }
  await validateIncludePaths(context);
  await suggestDiscoveredIncludePaths(context);
  await warnAboutPerlExtensionConflicts(context);

  // Background update check — fire-and-forget after startup completes.
  // Runs at most once per updateCheckInterval hours; no-ops when serverPath
  // is user-managed, channel='tag', or updateCheckInterval=0.
  // Skipped in untrusted workspaces as a defense-in-depth measure.
  if (vscode.workspace.isTrusted) {
    const updateDownloader = new BinaryDownloader(context, outputChannel);
    updateDownloader.checkForUpdateSilent().catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      outputChannel.error(`[update-check] Error: ${msg}`);
    });
  } else {
    outputChannel.info('[update-check] Skipped background update check in untrusted workspace.');
  }

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
        );
        return resolution.path;
      } catch (error: unknown) {
        languageClientStartupMetrics.finishBinaryResolution('error');
        throw error;
      }
    },
    createClient: (serverPath) => {
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
      await finalizeStartedLanguageClient(context, startedClient, latestLanguageClientGeneration);
    },
    onFailed: () => {
      languageClientStartupMetrics.finishServerStart('error');
      languageClientStartupMetrics.finishInitialize('error');
    },
    onStateChange: (snapshot) => {
      languageClientStartupMetrics.setLifecycleState(snapshot.state);
      syncLifecycleProjection();
      healthWidget?.onStateChange(clientStateForLifecycle(snapshot.state));
      // Only `resolving` needs an explicit projection: onStateChange maps it to
      // generic Starting, and every other lifecycle state is already owned by
      // onStateChange (including active indexing tokens and client_stopped detail).
      if (snapshot.state === 'resolving') {
        healthWidget?.setWorkspaceLifecycleState(projectWorkspaceLifecycle(snapshot.state));
      }
    },
    onClientStateChange: (_activeClient, event) => {
      if (event.newState === LanguageClientState.Starting) {
        languageClientStartupMetrics.markMilestone('process_started');
        languageClientStartupMetrics.finishServerStart('ok');
      }
      if (event.newState === LanguageClientState.Running) {
        // Record when the server last reached Running so the crash handler
        // (#4625) can decide whether the prior run was stable long enough to
        // reset the auto-restart attempt counter.
        stableRunningSince = Date.now();
      }
      handleClientStateChange(event);
    },
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
  // A LanguageClient restart does not replay didOpen for documents that VS Code
  // kept open while the previous client was stopped. Rehydrate those documents
  // before providers issue requests against the new server.
  if (generation > 1) {
    await synchronizeOpenPerlDocuments(startedClient);
  }

  // This hook is part of the lifecycle controller so initial startup and
  // restart/reinstall generations rebuild the same client integrations.
  const serverVersion = startedClient.initializeResult?.serverInfo?.version;
  if (serverVersion) {
    healthWidget?.setVersion(serverVersion);
  }

  // Offer AI inline completion once if the server advertises support (#1634).
  // Fire-and-forget; failures must not block lifecycle finalization.
  suggestAiCompletionIfSupported(context, startedClient).catch((err: unknown) => {
    const msg = err instanceof Error ? err.message : String(err);
    outputChannel.error(`[ai-completion] Error suggesting AI completion: ${msg}`);
  });

  await refreshTestAdapter(context);
  refreshStreamingController(startedClient);
  try {
    await syncLanguageClientConfiguration(startedClient);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.error(`[configuration] initial synchronization failed: ${message}`);
  }
  lastStartupDiagnosis = undefined;
  outputChannel.info('Perl Language Server started successfully');
}

async function synchronizeOpenPerlDocuments(client: LanguageClient): Promise<void> {
  for (const document of vscode.workspace.textDocuments) {
    if (
      document.languageId !== 'perl' ||
      (document.uri.scheme !== 'file' && document.uri.scheme !== 'untitled')
    ) {
      continue;
    }

    await client.sendNotification('textDocument/didOpen', {
      textDocument: {
        uri: document.uri.toString(),
        languageId: document.languageId,
        version: document.version,
        text: document.getText(),
      },
    });
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

function createLanguageClient(serverPath: string): LanguageClient {
  const generation = activeDocumentReadiness.beginGeneration();
  latestLanguageClientGeneration = generation;
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
      provideCompletionItem: async (document, position, context, token, next) => {
        try {
          const result = await next(document, position, context, token);
          recordLspProviderOutcome('Completion', document, result);
          return result;
        } catch (error: unknown) {
          return handleLspProviderError('Completion', error);
        }
      },
      provideDefinition: async (document, position, token, next) => {
        try {
          const result = await next(document, position, token);
          recordLspProviderOutcome('Definition', document, result);
          return result;
        } catch (error: unknown) {
          return handleLspProviderError('Definition', error);
        }
      },
      provideHover: async (document, position, token, next) => {
        try {
          const result = await next(document, position, token);
          recordLspProviderOutcome('Hover', document, result);
          return result;
        } catch (error: unknown) {
          return handleLspProviderError('Hover', error);
        }
      },
      provideReferences: async (document, position, options, token, next) => {
        try {
          const result = await next(document, position, options, token);
          recordLspProviderOutcome('References', document, result);
          return result;
        } catch (error: unknown) {
          return handleLspProviderError('References', error);
        }
      },
      provideDocumentSymbols: async (document, token, next) => {
        try {
          const result = await next(document, token);
          recordLspProviderOutcome('Symbols', document, result);
          return result;
        } catch (error: unknown) {
          return handleLspProviderError('Symbols', error);
        }
      },
      provideRenameEdits: async (document, position, newName, token, next) => {
        try {
          const result = await next(document, position, newName, token);
          recordLspProviderOutcome('Rename', document, result, 'safe_refusal');
          return result;
        } catch (error: unknown) {
          return handleLspProviderError('Rename', error);
        }
      },
      provideCodeLenses: async (document, token, next) => {
        const lenses = await next(document, token);
        return lenses?.map(rewriteTestLensCommand);
      },
      resolveCodeLens: async (codeLens, token, next) => {
        const resolved = await next(codeLens, token);
        return rewriteTestLensCommand(resolved ?? codeLens);
      },
      provideDocumentFormattingEdits: async (document, options, token, next) => {
        try {
          const edits = await next(document, options, token);
          const presentation = presentFormattingProviderOutcome(edits?.length ?? 0);
          healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
          return edits;
        } catch (err: unknown) {
          const code =
            err && typeof err === 'object' && 'code' in err
              ? (err as { code: unknown }).code
              : undefined;
          // Do not notify for request cancellations (code -32800)
          if (code !== -32800) {
            const msg = err instanceof Error ? err.message : String(err);
            handleFormattingError(msg, outputChannel);
            const presentation = presentFormattingProviderError(msg);
            healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
          }
          return null;
        }
      },
      provideDocumentRangeFormattingEdits: async (document, range, options, token, next) => {
        try {
          const edits = await next(document, range, options, token);
          const presentation = presentFormattingProviderOutcome(edits?.length ?? 0, true);
          healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
          return edits;
        } catch (err: unknown) {
          const code =
            err && typeof err === 'object' && 'code' in err
              ? (err as { code: unknown }).code
              : undefined;
          if (code !== -32800) {
            const msg = err instanceof Error ? err.message : String(err);
            handleFormattingError(msg, outputChannel);
            const presentation = presentFormattingProviderError(msg, true);
            healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
          }
          return null;
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
  const presentation = presentLspProviderOutcome(
    label,
    result,
    activeDocumentReadiness.isReady(document.uri.toString()),
    emptyOutcome,
  );
  healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
}

function handleLspProviderError(label: string, error: unknown): null {
  if (isRequestCancellation(error)) {
    return null;
  }
  const message = error instanceof Error ? error.message : String(error);
  const presentation = presentLspProviderError(label, message);
  healthWidget?.setProviderOutcome(presentation.providerOutcome, presentation);
  outputChannel?.warn(`[provider] ${label} failed: ${message}`);
  return null;
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

async function restartServer(_context: vscode.ExtensionContext) {
  const lifecycle = languageClientLifecycle;
  if (!lifecycle || (!client && !currentServerPath && !lifecycle.hasPendingServerPathOverride)) {
    vscode.window.showWarningMessage('Perl Language Server is not initialized yet.');
    return;
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
      return;
    }
    languageClientStartupMetrics.markMilestone('restart');
    syncLifecycleProjection();
    vscode.window
      .showInformationMessage('Perl Language Server restarted', 'Show Output')
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
      });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.error(`Failed to restart perl-lsp: ${message}`);
    vscode.window
      .showErrorMessage(`Failed to restart Perl Language Server: ${message}`, 'Show Output')
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
      });
  } finally {
    userInitiatedStopPending = false;
  }
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

  // The server crashed mid-session. Capture a generic diagnosis so
  // serverNotRunningMessage() returns an actionable hint instead of the stale
  // "not running" fallback. We can't run probeStartupFailure here (async), so
  // we set a generic "server stopped" diagnosis; the user can run Health Check
  // for details.
  lastStartupDiagnosis = {
    kind: StartupErrorKind.Unknown,
    hint: 'The Perl Language Server stopped unexpectedly. Check the Output panel for details.',
    remediation:
      'Try restarting the server (Command Palette: "Perl: Restart Server") or run the Health Check.',
  };

  // ClientState.Stopped is ambiguous and is rendered neutrally by the widget.
  // This path has established the stronger meaning: an unexpected mid-session
  // crash with an actionable diagnosis.
  healthWidget?.setWorkspaceLifecycleState('failed', {
    detail: 'The Perl Language Server stopped unexpectedly.',
    action: 'Restart the server or run the Health Check.',
    reasonCode: 'unexpected_server_stop',
  });

  outputChannel?.info('[lifecycle] Perl Language Server stopped unexpectedly (mid-session crash).');

  // If the prior run was stable long enough, treat this crash as a new
  // episode and reset the auto-restart budget so transient crashes don't
  // permanently exhaust it.
  if (stableRunningSince !== undefined && Date.now() - stableRunningSince >= STABLE_RUN_GRACE_MS) {
    autoRestartAttempts = 0;
  }
  stableRunningSince = undefined;

  void handleUnexpectedServerStop();
}

/**
 * Surface an unexpected mid-session server crash and attempt an auto-restart
 * (#4625). Shows a user-visible toast (the diagnosis hint captured above) with
 * `Restart Server` and `Show Output` actions, then retries up to
 * `MAX_AUTO_RESTART_ATTEMPTS` times. Once the budget is exhausted, the toast
 * asks the user to restart manually or run a Health Check.
 */
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
      autoRestartAttempts = 0;
      await handleUnexpectedServerStop();
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

async function handleUnexpectedServerStop(): Promise<void> {
  const context = extensionContext;
  const hint =
    lastStartupDiagnosis?.hint ??
    'The Perl Language Server stopped unexpectedly. Check the Output panel for details.';

  if (autoRestartAttempts < MAX_AUTO_RESTART_ATTEMPTS) {
    autoRestartAttempts += 1;
    const attempt = autoRestartAttempts;
    outputChannel?.info(
      `[lifecycle] Auto-restarting Perl Language Server (attempt ${attempt}/${MAX_AUTO_RESTART_ATTEMPTS})\u2026`,
    );
    const message = `Perl Language Server crashed and is restarting automatically (attempt ${attempt}/${MAX_AUTO_RESTART_ATTEMPTS}). ${hint}`;
    void vscode.window.showErrorMessage(message, 'Show Output').then((selection) => {
      if (selection === 'Show Output') {
        outputChannel?.show();
      }
    });
    if (!context) {
      outputChannel?.info('[lifecycle] Cannot auto-restart: extension context is not available.');
      return;
    }
    try {
      await restartServer(context);
    } catch (error: unknown) {
      // restartServer surfaces its own dialog/log; record the failure and
      // let the next crash (if any) consume another retry slot.
      const msg = error instanceof Error ? error.message : String(error);
      outputChannel?.error(`[lifecycle] Auto-restart attempt ${attempt} failed: ${msg}`);
    }
    return;
  }

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
    // A manual restart resets the auto-restart budget.
    autoRestartAttempts = 0;
    try {
      await restartServer(context);
    } catch (error: unknown) {
      const msg = error instanceof Error ? error.message : String(error);
      outputChannel?.error(`[lifecycle] Manual restart failed: ${msg}`);
    }
  } else if (selection === 'Run Health Check') {
    const serverPath = currentServerPath ?? undefined;
    await vscode.commands.executeCommand('perl-lsp.runHealthCheck', serverPath);
  } else if (selection === 'Show Output') {
    outputChannel?.show();
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
  // clear crash-recovery state so a re-activation starts with a fresh budget.
  userInitiatedStopPending = true;
  autoRestartAttempts = 0;
  stableRunningSince = undefined;
  disposeClientIntegrations();
  if (languageClientLifecycle) {
    await languageClientLifecycle.stop();
    syncLifecycleProjection();
  }
  userInitiatedStopPending = false;
}
