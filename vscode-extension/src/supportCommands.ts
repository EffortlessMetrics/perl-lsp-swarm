import * as vscode from 'vscode';

export const PRODUCT_NAME = 'perl-lsp';
export const SERVER_EXECUTABLE = 'perllsp';
export const EXTENSION_ID = 'EffortlessMetrics.perl-lsp-rs';

export interface SupportCommandDependencies {
  readonly getServerVersion: () => Promise<string>;
  readonly extensionVersion: string;
  readonly editorVersion: string;
  readonly platform: string;
  readonly arch: string;
  readonly editorName?: string | undefined;
}

function normalizeServerVersion(serverVersion: string): string {
  const trimmed = serverVersion.trim();
  const match = /^(?:perllsp|perl-lsp)\s+(.+)$/.exec(trimmed);
  return match?.[1]?.trim() || trimmed || 'unavailable';
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
  const serverVersion = normalizeServerVersion(params.serverVersion);
  return [
    `Product: ${PRODUCT_NAME}`,
    `Server: ${SERVER_EXECUTABLE} ${serverVersion}`,
    `Extension: ${EXTENSION_ID} ${params.extensionVersion}`,
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
