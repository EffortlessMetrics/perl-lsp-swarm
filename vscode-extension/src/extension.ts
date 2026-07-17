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
import { registerCriticCommandGroup } from './criticCommandGroup';
import { registerTestCommandGroup } from './testCommandGroup';
import { registerOnboardingCommandGroup } from './onboardingCommandGroup';
import { registerNavigationCommandGroup } from './navigationCommandGroup';
import {
  organizeImportsCommand,
  showStatusMenuCommand,
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
let streamingController: StreamingCompletionController | undefined;
let languageClientLifecycle:
  | ExtensionLanguageClientLifecycle<LanguageClient, StateChangeEvent>
  | undefined;
const languageClientStartupMetrics = new LanguageClientStartupMetrics();
const featureActivationMetrics = new FeatureActivationMetrics();

export function getLanguageClientStartupMetrics(): LanguageClientStartupMetricsSnapshot {
  return languageClientStartupMetrics.snapshot();
}

export function getFeatureActivationMetrics(): FeatureActivationMetricsSnapshot {
  return featureActivationMetrics.snapshot();
}

export function markLanguageClientStartupMilestone(
  milestone: LanguageClientStartupMilestone,
): void {
  languageClientStartupMetrics.markMilestone(milestone);
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

export async function syncPerlCriticConfiguration(
  activeClient: Pick<LanguageClient, 'sendNotification'> | undefined = client,
  documentUri?: vscode.Uri,
): Promise<void> {
  await syncPerlCriticConfigurationFromConfig(activeClient, documentUri);
}

export async function runPerlCriticOnActiveFile(
  activeClient: Pick<LanguageClient, 'sendRequest' | 'sendNotification'> | undefined = client,
): Promise<void> {
  const channel = outputChannel ?? vscode.window.createOutputChannel('Perl Language Server');
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

  channel.appendLine(
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
      { label: '1', description: 'Very permissive' },
      { label: '2', description: 'Permissive' },
      { label: '3', description: 'Balanced default' },
      { label: '4', description: 'Strict' },
      { label: '5', description: 'Very strict' },
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
  outputChannel = vscode.window.createOutputChannel('Perl Language Server', { log: true });
  const mcpDisposable = featureActivationMetrics.measure('mcp', true, () =>
    registerMcpSupport(outputChannel),
  );
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusBarItem.command = 'perl-lsp.showStatusMenu';
  statusBarItem.show();
  healthWidget = new HealthWidget(statusBarItem);
  healthWidget.onStateChange(ClientState.Starting);
  languageClientLifecycle = createLanguageClientLifecycle(context);
  syncLifecycleProjection();
  context.subscriptions.push(statusBarItem);

  // Register server-facing commands through an explicit dependency context.
  // Lifecycle transitions remain owned by the authoritative composition.
  const serverCommandDisposables = registerServerCommandGroup({
    outputChannel,
    currentServerPath: () => currentServerPath,
    reinstallServerBinary: () => reinstallServerBinary(context),
    restartServer: () => restartServer(context),
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
          outputChannel.appendLine(`Trace level changed to: ${newTrace}`);
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
        }
      },
      onRestartRequired: () => promptForClientRefresh(context),
      onError: (error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        outputChannel.appendLine(`[configuration] change handling failed: ${message}`);
      },
    }),
  );

  const fileCreationWatcher = vscode.workspace.onDidCreateFiles(async (event) => {
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
    outputChannel.appendLine('[extension-test] Skipping automatic server startup.');
    languageClientStartupMetrics.markMilestone('activate_returned');
    return {
      getLanguageClientStartupMetrics,
      getFeatureActivationMetrics,
      markLanguageClientStartupMilestone,
      stop: deactivate,
    };
  }

  startLanguageClientAfterActivation(context, whatsNewManager);
  languageClientStartupMetrics.markMilestone('activate_returned');
  return {
    getLanguageClientStartupMetrics,
    getFeatureActivationMetrics,
    markLanguageClientStartupMilestone,
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
    outputChannel.appendLine(`[startup] Background startup failed: ${msg}`);
    healthWidget?.onStateChange(ClientState.Stopped);
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
  const updateDownloader = new BinaryDownloader(context, outputChannel);
  updateDownloader.checkForUpdateSilent().catch((err: unknown) => {
    const msg = err instanceof Error ? err.message : String(err);
    outputChannel.appendLine(`[update-check] Error: ${msg}`);
  });

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
      outputChannel.appendLine(`[onboarding] Error showing welcome: ${msg}`);
    });
    // Mark the version seen on first install so the next activation
    // (after an update) triggers the What's New panel instead of welcome.
    whatsNewManager.markVersionSeen().catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      outputChannel.appendLine(`[whats-new] Error marking version seen: ${msg}`);
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
        outputChannel.appendLine(`[whats-new] Error showing What's New: ${msg}`);
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
  channel: vscode.OutputChannel,
): string | null {
  if (!userPath || pathExists) {
    return null;
  }
  channel.appendLine(
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
    outputChannel.appendLine(`Using user-configured Perl LSP binary: ${userPath}`);
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
          outputChannel.appendLine(`Found Perl LSP binary in PATH: ${fullPath}`);
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

      outputChannel.appendLine(`Using bundled Perl LSP binary: ${bundledPath}`);
      if (platform !== 'win32') {
        try {
          fs.chmodSync(bundledPath, 0o755);
        } catch (chmodError: unknown) {
          const msg = chmodError instanceof Error ? chmodError.message : String(chmodError);
          outputChannel.appendLine(
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

  if (autoDownload) {
    outputChannel.appendLine('Perl LSP binary not found, attempting to download...');
    const downloader = new BinaryDownloader(context, outputChannel);
    const downloadedPath = await downloader.ensureBinary();

    if (downloadedPath) {
      outputChannel.appendLine(`Downloaded Perl LSP binary to: ${downloadedPath}`);
      return { path: downloadedPath, source: 'downloaded' };
    }
  } else {
    outputChannel.appendLine('Perl LSP binary not found and auto-download is disabled');
  }

  outputChannel.appendLine('Failed to obtain a Perl LSP binary');
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
      await finalizeStartedLanguageClient(context, startedClient);
    },
    onFailed: () => {
      languageClientStartupMetrics.finishServerStart('error');
      languageClientStartupMetrics.finishInitialize('error');
    },
    onStateChange: (snapshot) => {
      languageClientStartupMetrics.setLifecycleState(snapshot.state);
      syncLifecycleProjection();
      healthWidget?.onStateChange(clientStateForLifecycle(snapshot.state));
    },
    onClientStateChange: (_activeClient, event) => {
      if (event.newState === LanguageClientState.Starting) {
        languageClientStartupMetrics.markMilestone('process_started');
        languageClientStartupMetrics.finishServerStart('ok');
      }
      handleClientStateChange(event);
    },
    onCallbackError: (error, phase) => {
      const message = error instanceof Error ? error.message : String(error);
      outputChannel.appendLine(`[lifecycle] ${phase} callback failed: ${message}`);
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
): Promise<void> {
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
    outputChannel.appendLine(`[ai-completion] Error suggesting AI completion: ${msg}`);
  });

  await refreshTestAdapter(context);
  refreshStreamingController(startedClient);
  try {
    await syncLanguageClientConfiguration(startedClient);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.appendLine(`[configuration] initial synchronization failed: ${message}`);
  }
  lastStartupDiagnosis = undefined;
  outputChannel.appendLine('Perl Language Server started successfully');
}

async function initializeLanguageClient(context: vscode.ExtensionContext): Promise<boolean> {
  healthWidget?.onStateChange(ClientState.Starting);

  const lifecycle = languageClientLifecycle;
  if (!lifecycle) {
    outputChannel.appendLine('[startup] Language client lifecycle was not composed');
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
      outputChannel.appendLine('[startup] Lifecycle started without a server path');
      return false;
    }

    return true;
  } catch (startError: unknown) {
    const msg = startError instanceof Error ? startError.message : String(startError);
    outputChannel.appendLine(`[startup] Language client failed to start: ${msg}`);

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
      await vscode.commands.executeCommand('perl-lsp.runHealthCheck', lifecycle.serverPath);
    } else if (choice === 'Reinstall') {
      await reinstallServerBinary(context);
    } else if (choice === 'Check serverPath Setting') {
      void vscode.commands.executeCommand('workbench.action.openSettings', 'perl-lsp.serverPath');
    }
    return false;
  }
}

function createLanguageClient(serverPath: string): LanguageClient {
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
          return await next(document, options, token);
        } catch (err: unknown) {
          const code =
            err && typeof err === 'object' && 'code' in err
              ? (err as { code: unknown }).code
              : undefined;
          // Do not notify for request cancellations (code -32800)
          if (code !== -32800) {
            const msg = err instanceof Error ? err.message : String(err);
            handleFormattingError(msg, outputChannel);
          }
          return null;
        }
      },
      provideDocumentRangeFormattingEdits: async (document, range, options, token, next) => {
        try {
          return await next(document, range, options, token);
        } catch (err: unknown) {
          const code =
            err && typeof err === 'object' && 'code' in err
              ? (err as { code: unknown }).code
              : undefined;
          if (code !== -32800) {
            const msg = err instanceof Error ? err.message : String(err);
            handleFormattingError(msg, outputChannel);
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
  void lc.setTrace(getTraceLevel());
  return lc;
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
          outputChannel.appendLine(`[startup-probe] Binary probe failed: ${err.message}`);
          if (combined) {
            outputChannel.appendLine(`[startup-probe] stderr: ${combined}`);
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
    outputChannel.appendLine(`Unsupported featureProfile '${rawProfile}'. Falling back to 'auto'.`);
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
    outputChannel.appendLine(`Failed to restart perl-lsp: ${message}`);
    vscode.window
      .showErrorMessage(`Failed to restart Perl Language Server: ${message}`, 'Show Output')
      .then((selection) => {
        if (selection === 'Show Output') {
          outputChannel.show();
        }
      });
  }
}

function shouldFormatOnSave(document: vscode.TextDocument): boolean {
  if (document.languageId !== 'perl') {
    return false;
  }

  const config = vscode.workspace.getConfiguration('perl-lsp', document.uri);
  return config.get<boolean>('formatOnSave', false);
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
    outputChannel.appendLine('Perl test integration disabled.');
    return;
  }

  testAdapter = new PerlTestAdapter();
  context.subscriptions.push(testAdapter);
  outputChannel.appendLine('Perl test integration enabled.');
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
    outputChannel.appendLine('Streaming inline completion controller enabled.');
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
  outputChannel.show(true);
  outputChannel.appendLine('Reinstalling perllsp binary...');

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
    outputChannel.appendLine('[reinstall] stopping language client to release the running binary');
    try {
      await disposeLanguageClient();
    } catch (stopErr: unknown) {
      const msg = stopErr instanceof Error ? stopErr.message : String(stopErr);
      outputChannel.appendLine(`[reinstall] language client stop reported: ${msg}`);
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
      outputChannel.appendLine('[reinstall] restoring previous binary after failed download');
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
      outputChannel.appendLine('[reinstall] restoring previous binary after failed health check');
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
    outputChannel.appendLine(
      '[reinstall] restarting language client with the freshly installed binary',
    );
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

function handleClientStateChange(event: StateChangeEvent): void {
  // Preserve the status-bar signal for an unexpected language-client crash;
  // the controller state changes only on explicit lifecycle operations.
  healthWidget?.onStateChange(event.newState as unknown as ClientState);

  // When the server stops unexpectedly after a successful start (mid-session
  // crash), capture a generic diagnosis so serverNotRunningMessage() returns
  // an actionable hint instead of the stale "not running" fallback.
  // We can't run probeStartupFailure here (async) so we set a generic
  // "server stopped" diagnosis; the user can run Health Check for details.
  //
  // Note: event.newState / oldState are vscode-languageclient's `State` enum
  // which shares numeric values with ClientState (Stopped=1, Running=2) but
  // is a distinct nominal type, so we cast to number for comparison.
  const newStateNum = event.newState as unknown as number;
  const oldStateNum = event.oldState as unknown as number;
  if (newStateNum === ClientState.Stopped && oldStateNum === ClientState.Running) {
    lastStartupDiagnosis = {
      kind: StartupErrorKind.Unknown,
      hint: 'The Perl Language Server stopped unexpectedly. Check the Output panel for details.',
      remediation:
        'Try restarting the server (Command Palette: "Perl: Restart Server") or run the Health Check.',
    };
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
  disposeClientIntegrations();
  if (languageClientLifecycle) {
    await languageClientLifecycle.stop();
    syncLifecycleProjection();
  }
}
