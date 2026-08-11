import * as vscode from 'vscode';

type NavigationOutputChannel = Pick<vscode.OutputChannel, 'show'>;

export interface NavigationCommandDependencies {
  readonly currentServerPath: () => string | null;
  readonly outputChannel: NavigationOutputChannel;
  readonly serverNotRunningMessage: () => string;
  readonly getServerVersion: (serverPath: string) => Promise<string>;
}

export type WorkspaceStatusMode = 'starting' | 'indexing' | 'running' | 'stopped';
export type WorkspaceStatusReadiness = 'building' | 'ready' | 'ready_limited' | 'legacy';

export interface WorkspaceStatusSnapshot {
  readonly mode: WorkspaceStatusMode;
  readonly version?: string;
  readonly fileCount?: number;
  readonly errorCount?: number;
  readonly lifecycle?: string;
  readonly lifecycleDetail?: string;
  readonly readinessState?: WorkspaceStatusReadiness;
  readonly readinessReason?: string;
  readonly activeDocumentReady?: boolean;
  readonly nextAction?: string;
}

/** Invoke VS Code's organize-imports command. */
export async function organizeImportsCommand(): Promise<void> {
  await vscode.commands.executeCommand('editor.action.organizeImports');
}

/** Display the active server version or an actionable recovery prompt. */
export async function showVersionCommand(
  dependencies: NavigationCommandDependencies,
): Promise<void> {
  const serverPath = dependencies.currentServerPath();
  if (!serverPath) {
    const selection = await vscode.window.showErrorMessage(
      dependencies.serverNotRunningMessage(),
      'Restart Server',
      'Show Output',
      'Run Health Check',
    );
    if (selection === 'Restart Server') {
      void vscode.commands.executeCommand('perl-lsp.restart');
    } else if (selection === 'Show Output') {
      dependencies.outputChannel.show();
    } else if (selection === 'Run Health Check') {
      void vscode.commands.executeCommand('perl-lsp.runHealthCheck');
    }
    return;
  }

  try {
    const version = await dependencies.getServerVersion(serverPath);
    const selection = await vscode.window.showInformationMessage(
      `Perl LSP Version: ${version}`,
      'Copy',
    );
    if (selection === 'Copy') {
      void vscode.env.clipboard.writeText(version);
    }
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    const selection = await vscode.window.showErrorMessage(
      `Could not get Perl LSP version: ${message}. The server binary may be missing or corrupt — try reinstalling.`,
      'Reinstall',
    );
    if (selection === 'Reinstall') {
      void vscode.commands.executeCommand('perl-lsp.reinstall');
    }
  }
}

export async function showWorkspaceStatusCommand(dependencies: {
  readonly getWorkspaceStatus: () => WorkspaceStatusSnapshot;
}): Promise<void> {
  const status = dependencies.getWorkspaceStatus();
  const modeLabel = {
    starting: 'starting',
    indexing: 'indexing',
    running: 'running',
    stopped: 'stopped',
  }[status.mode];
  const lines = [`Perl LSP workspace status`, `Server: ${modeLabel}`];
  const hasLiveServer = status.mode === 'running' || status.mode === 'indexing';
  if (hasLiveServer && status.version) {
    lines.push(`Version: ${status.version}`);
  }
  if (status.fileCount !== undefined) {
    lines.push(`Workspace files: ${status.fileCount}`);
  }
  if (status.errorCount !== undefined) {
    lines.push(`Diagnostics: ${status.errorCount} error${status.errorCount === 1 ? '' : 's'}`);
  }
  if (status.lifecycle) {
    lines.push(`Lifecycle: ${status.lifecycle}`);
  }
  if (status.lifecycleDetail) {
    lines.push(`Detail: ${status.lifecycleDetail}`);
  }
  if (status.readinessState === 'legacy') {
    lines.push('Workspace index: legacy server (enhanced readiness unavailable)');
  } else if (status.readinessState) {
    lines.push(`Workspace index: ${status.readinessState}`);
  } else if (hasLiveServer) {
    lines.push('Workspace index: legacy server (enhanced readiness unavailable)');
  }
  if (status.activeDocumentReady !== undefined) {
    lines.push(`Active document: ${status.activeDocumentReady ? 'ready' : 'not ready'}`);
  }
  if (status.readinessReason) {
    lines.push(`Coverage: ${status.readinessReason}`);
  }
  if (status.nextAction) {
    lines.push(`Next: ${status.nextAction}`);
  }

  const actions =
    status.mode === 'stopped'
      ? (['Restart Server', 'Run Health Check', 'Show Output', 'Open Actions'] as const)
      : (['Run Health Check', 'Show Output', 'Open Actions'] as const);
  const selection =
    status.mode === 'stopped'
      ? await vscode.window.showWarningMessage(lines.join('\n'), ...actions)
      : await vscode.window.showInformationMessage(lines.join('\n'), ...actions);

  if (selection === 'Restart Server') {
    void vscode.commands.executeCommand('perl-lsp.restart');
  } else if (selection === 'Run Health Check') {
    void vscode.commands.executeCommand('perl-lsp.runHealthCheck');
  } else if (selection === 'Show Output') {
    void vscode.commands.executeCommand('perl-lsp.showOutput');
  } else if (selection === 'Open Actions') {
    void vscode.commands.executeCommand('perl-lsp.showStatusMenu');
  }
}

/** Show the status-bar action menu for the current editor context. */
export async function showStatusMenuCommand(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const isPerl = editor?.document.languageId === 'perl';
  const filePath = editor?.document.uri.fsPath ?? '';
  const isTestFile = isPerl && (filePath.endsWith('.t') || filePath.endsWith('.pl'));

  const items: Array<
    vscode.QuickPickItem & {
      command?: string;
      args?: unknown[];
      disabled?: boolean;
    }
  > = [
    { label: 'Actions', kind: vscode.QuickPickItemKind.Separator },
    {
      label: '$(refresh) Restart Server',
      description: 'Shift+Alt+R',
      detail: 'Restart the language server',
      command: 'perl-lsp.restart',
    },
    {
      label: '$(organization) Organize Imports',
      description: 'Shift+Alt+O',
      detail: isPerl
        ? 'Sort and organize use statements'
        : 'Sort and organize use statements (Only available for Perl files)',
      command: 'perl-lsp.organizeImports',
      disabled: !isPerl,
    },
    {
      label: '$(beaker) Run Tests in Current File',
      description: 'Shift+Alt+T',
      detail: isTestFile
        ? 'Run tests for the active file'
        : 'Run tests for the active file (Only available for .t/.pl files)',
      command: 'perl-lsp.runTests',
      disabled: !isTestFile,
    },
    {
      label: '$(checklist) Run Critic',
      detail: isPerl
        ? 'Run Critic on the active file'
        : 'Run Critic on the active file (Only available for Perl files)',
      command: 'perl-lsp.runPerlCritic',
      disabled: !isPerl,
    },
    {
      label: '$(symbol-numeric) Set Critic Severity',
      detail: isPerl
        ? 'Choose a Critic severity level'
        : 'Choose a Critic severity level (Only available for Perl files)',
      command: 'perl-lsp.setPerlCriticSeverity',
      disabled: !isPerl,
    },
    {
      label: '$(list-flat) Format Document',
      description: 'Shift+Alt+F',
      detail: isPerl
        ? 'Format the active Perl document (native formatter)'
        : 'Format the active Perl document (Only available for Perl files)',
      command: 'editor.action.formatDocument',
      disabled: !isPerl,
    },
    { label: 'Information', kind: vscode.QuickPickItemKind.Separator },
    {
      label: '$(pulse) Show Workspace Status',
      detail: 'Explain workspace lifecycle, index readiness, and active-document state',
      command: 'perl-lsp.showWorkspaceStatus',
    },
    {
      label: '$(question) Explain Provider Result',
      detail: 'Explain an exact answer, fallback, empty result, or refusal',
      command: 'perl-lsp.explainProviderDecision',
    },
    {
      label: '$(output) Show Output',
      detail: 'Open the extension output channel',
      command: 'perl-lsp.showOutput',
    },
    {
      label: '$(info) Show Version',
      detail: 'Check installed perllsp version',
      command: 'perl-lsp.showVersion',
    },
    {
      label: '$(pulse) Run Health Check',
      detail: 'Check Perl, perltidy, and LSP binary',
      command: 'perl-lsp.runHealthCheck',
    },
    {
      label: '$(cloud-download) Reinstall Server Binary',
      detail: 'Re-download the managed perllsp binary',
      command: 'perl-lsp.reinstall',
    },
    { label: 'Configuration', kind: vscode.QuickPickItemKind.Separator },
    {
      label: '$(gear) Configure Settings',
      detail: 'Open Perl LSP settings',
      command: 'workbench.action.openSettings',
      args: ['@ext:EffortlessMetrics.perl-lsp-rs'],
    },
  ];

  const selection = await vscode.window.showQuickPick(items, {
    placeHolder: 'Perl Language Server Actions',
  });
  if (selection?.command && !selection.disabled) {
    await vscode.commands.executeCommand(selection.command, ...(selection.args ?? []));
  }
}
