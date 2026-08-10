import * as vscode from 'vscode';

export interface SupportCommandDependencies {
  readonly getServerVersion: () => Promise<string>;
  readonly extensionVersion: string;
  readonly editorVersion: string;
  readonly platform: string;
  readonly arch: string;
  readonly editorName?: string | undefined;
}

export function formatIssueDiagnosticInfo(params: {
  serverVersion: string;
  extensionVersion: string;
  editorVersion: string;
  platform: string;
  arch: string;
  editorName?: string | undefined;
}): string {
  const editorName = (params.editorName ?? 'VS Code').trim() || 'VS Code';
  return [
    `perl-lsp server: ${params.serverVersion}`,
    `Extension: ${params.extensionVersion}`,
    `${editorName}: ${params.editorVersion}`,
    `Platform: ${params.platform}/${params.arch}`,
  ].join('\n');
}

/** Collect diagnostic context and open the repository's issue form. */
export async function reportIssueCommand(dependencies: SupportCommandDependencies): Promise<void> {
  const diagnosticInfo = formatIssueDiagnosticInfo({
    serverVersion: await dependencies.getServerVersion(),
    extensionVersion: dependencies.extensionVersion,
    editorVersion: dependencies.editorVersion,
    platform: dependencies.platform,
    arch: dependencies.arch,
    editorName: dependencies.editorName,
  });

  const selection = await vscode.window.showInformationMessage(
    'Open a GitHub issue to report a bug or request a feature.',
    'Copy Diagnostic Info',
    'Open Issue Form',
  );

  if (selection === 'Copy Diagnostic Info') {
    try {
      await vscode.env.clipboard.writeText(diagnosticInfo);
      vscode.window.showInformationMessage('Diagnostic info copied. Paste it into the issue form.');
    } catch {
      // Clipboard unavailable — continue to open browser anyway.
    }
  }

  if (selection === 'Copy Diagnostic Info' || selection === 'Open Issue Form') {
    await vscode.env.openExternal(
      vscode.Uri.parse(
        'https://github.com/EffortlessMetrics/perl-lsp/issues/new?template=bug_report.yml',
      ),
    );
  }
}
