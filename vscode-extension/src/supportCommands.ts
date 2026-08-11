import * as vscode from 'vscode';

export const PRODUCT_NAME = 'perl-lsp';
export const SERVER_EXECUTABLE = 'perllsp';
export const EXTENSION_ID = 'EffortlessMetrics.perl-lsp-rs';

const MAX_DIAGNOSTIC_FIELD_LENGTH = 200;

export interface SupportCommandDependencies {
  readonly getServerVersion: () => Promise<string>;
  readonly extensionVersion: string;
  readonly editorVersion: string;
  readonly platform: string;
  readonly arch: string;
  readonly editorName?: string | undefined;
}

function sanitizeDiagnosticField(value: string | undefined, fallback: string): string {
  const firstLine = (value ?? '').split(/\r\n|[\n\r\u2028\u2029]/, 1)[0] ?? '';
  const printable = firstLine.replace(/[\u0000-\u001f\u007f-\u009f]/g, (character) => {
    const codePoint = character.charCodeAt(0).toString(16).padStart(4, '0');
    return `\\u${codePoint}`;
  });
  const normalized = printable.trim() || fallback;
  return normalized.length <= MAX_DIAGNOSTIC_FIELD_LENGTH
    ? normalized
    : `${normalized.slice(0, MAX_DIAGNOSTIC_FIELD_LENGTH - 3)}...`;
}

function formatServerIdentity(serverVersion: string): string {
  const observed = sanitizeDiagnosticField(serverVersion, 'unavailable');
  if (observed === 'unavailable') {
    return `${SERVER_EXECUTABLE} unavailable`;
  }
  if (observed === SERVER_EXECUTABLE || observed.startsWith(`${SERVER_EXECUTABLE} `)) {
    return observed;
  }
  return `${observed} (expected ${SERVER_EXECUTABLE})`;
}

export function formatIssueDiagnosticInfo(params: {
  serverVersion: string;
  extensionVersion: string;
  editorVersion: string;
  platform: string;
  arch: string;
  editorName?: string | undefined;
}): string {
  const serverIdentity = formatServerIdentity(params.serverVersion);
  const extensionVersion = sanitizeDiagnosticField(params.extensionVersion, 'unknown');
  const editorName = sanitizeDiagnosticField(params.editorName ?? 'VS Code', 'VS Code');
  const editorVersion = sanitizeDiagnosticField(params.editorVersion, 'unknown');
  const platform = sanitizeDiagnosticField(params.platform, 'unknown');
  const arch = sanitizeDiagnosticField(params.arch, 'unknown');
  return [
    `Product: ${PRODUCT_NAME}`,
    `Server: ${serverIdentity}`,
    `Extension: ${EXTENSION_ID} ${extensionVersion}`,
    `${editorName}: ${editorVersion}`,
    `Platform: ${platform}/${arch}`,
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
