import * as vscode from 'vscode';
import {
  type SupportAtom,
  type SupportBinaryIdentity,
  type SupportDigest,
  type SupportPacketV1,
  formatSupportPacketHuman,
  supportAtom,
  supportKnown,
  supportState,
} from './supportPacket';

export const PRODUCT_NAME = 'perl-lsp';
export const SERVER_EXECUTABLE = 'perllsp';
export const EXTENSION_ID = 'EffortlessMetrics.perl-lsp-rs';

const MAX_DIAGNOSTIC_FIELD_LENGTH = 200;
const PUBLIC_BUG_REPORT_URL =
  'https://github.com/EffortlessMetrics/perl-lsp/issues/new?template=bug_report.yml';

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

function safeSupportAtom(value: string | undefined, fallback: string): SupportAtom {
  const sanitized = sanitizeDiagnosticField(value, fallback);
  try {
    return supportAtom(sanitized);
  } catch {
    return supportAtom(fallback);
  }
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

function basicPerllspIdentity(serverVersion: string): SupportBinaryIdentity {
  const observed = sanitizeDiagnosticField(serverVersion, 'unavailable');
  if (observed === 'unavailable') {
    return {
      state: 'known_absent',
      role: 'unknown',
      version: supportState<SupportAtom>('known_absent'),
      target: supportState<SupportAtom>('not_proven'),
      digest: supportState<SupportDigest>('not_proven'),
      compatibility: 'missing',
    };
  }

  if (observed.startsWith(`${SERVER_EXECUTABLE} `)) {
    const version = safeSupportAtom(observed.slice(SERVER_EXECUTABLE.length + 1), 'unknown');
    return {
      state: 'known',
      role: 'unknown',
      version: supportKnown(version),
      target: supportState<SupportAtom>('not_proven'),
      digest: supportState<SupportDigest>('not_proven'),
      compatibility: 'unknown',
    };
  }

  return {
    state: 'known',
    role: 'ambient',
    version: supportState<SupportAtom>('unknown'),
    target: supportState<SupportAtom>('not_proven'),
    digest: supportState<SupportDigest>('not_proven'),
    compatibility: 'action_required',
  };
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

export function buildBasicSupportPacket(params: {
  serverVersion: string;
  extensionVersion: string;
  editorVersion: string;
  platform: string;
  arch: string;
  editorName?: string | undefined;
}): SupportPacketV1 {
  return {
    schema_version: 'perl_lsp_support_packet.v1',
    product: {
      name: supportAtom(PRODUCT_NAME),
      version: supportState<SupportAtom>('not_proven'),
      track: supportKnown(supportAtom('public-beta')),
    },
    extension: {
      id: supportAtom(EXTENSION_ID),
      version: supportKnown(safeSupportAtom(params.extensionVersion, 'unknown')),
      artifact_digest: supportState<SupportDigest>('not_proven'),
    },
    host: {
      editor: safeSupportAtom(params.editorName ?? 'VS Code', 'VS Code'),
      editor_version: supportKnown(safeSupportAtom(params.editorVersion, 'unknown')),
      platform: safeSupportAtom(params.platform, 'unknown'),
      architecture: safeSupportAtom(params.arch, 'unknown'),
      extension_host: 'unknown',
      workspace_mode: 'unknown',
      trust: 'unknown',
    },
    perllsp: basicPerllspIdentity(params.serverVersion),
    perl_dap: {
      state: 'not_proven',
      role: 'unknown',
      version: supportState<SupportAtom>('not_proven'),
      target: supportState<SupportAtom>('not_proven'),
      digest: supportState<SupportDigest>('not_proven'),
      compatibility: 'not_proven',
    },
    lifecycle: {
      generation: supportState<SupportAtom>('not_proven'),
      readiness: supportAtom('unknown'),
      startup_disposition: supportAtom('unknown'),
      crash_disposition: supportAtom('unknown'),
      activation_disposition: supportAtom('unknown'),
      managed_install_disposition: supportAtom('unknown'),
    },
    protocol: {
      families: [],
    },
    configuration: {
      user_present: supportState<boolean>('not_proven'),
      workspace_present: supportState<boolean>('not_proven'),
      folder_present: supportState<boolean>('not_proven'),
      project_config_present: supportState<boolean>('not_proven'),
      formatter_mode: supportAtom('unknown'),
      critic_mode: supportAtom('unknown'),
      migration: {
        registry: supportState<SupportAtom>('not_proven'),
        encountered: [],
        status: 'unknown',
      },
    },
    failure: {
      startup_reason: supportAtom('unknown'),
      network_reason: supportAtom('unknown'),
      provider_state: supportAtom('unknown'),
      cache_state: supportAtom('unknown'),
    },
  };
}

async function getServerVersionSafely(dependencies: SupportCommandDependencies): Promise<string> {
  try {
    return await dependencies.getServerVersion();
  } catch {
    return 'unavailable';
  }
}

/** Collect bounded support context and open the repository's issue form. */
export async function reportIssueCommand(dependencies: SupportCommandDependencies): Promise<void> {
  const serverVersion = await getServerVersionSafely(dependencies);
  const supportPacket = buildBasicSupportPacket({
    serverVersion,
    extensionVersion: dependencies.extensionVersion,
    editorVersion: dependencies.editorVersion,
    platform: dependencies.platform,
    arch: dependencies.arch,
    editorName: dependencies.editorName,
  });
  const humanPacket = formatSupportPacketHuman(supportPacket);

  const selection = await vscode.window.showInformationMessage(
    'Open a GitHub issue to report a bug or request a feature.',
    'Copy Support Packet',
    'Open Issue Form',
  );

  if (selection === 'Copy Support Packet') {
    try {
      await vscode.env.clipboard.writeText(humanPacket);
      vscode.window.showInformationMessage(
        'Support packet copied. Review it, then paste it into the issue form.',
      );
    } catch {
      // Clipboard unavailable — continue to open browser anyway.
    }
  }

  if (selection === 'Copy Support Packet' || selection === 'Open Issue Form') {
    await vscode.env.openExternal(vscode.Uri.parse(PUBLIC_BUG_REPORT_URL));
  }
}
