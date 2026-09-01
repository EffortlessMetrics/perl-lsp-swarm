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

export function sanitizeDiagnosticField(value: string | undefined, fallback: string): string {
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

/**
 * Render the human packet projection, or `null` when the packet cannot be built.
 *
 * The raw failure is deliberately dropped rather than surfaced: packet-validation
 * messages name the offending field contents, which is exactly the class of data the
 * packet exists to keep out of a public report.
 *
 * The catch is also deliberately silent rather than logging an error class. This
 * extension emits no `console` output from production code — there are zero
 * `console.*` calls outside tests and `.oxlintrc.json` sets `no-console`, against a
 * 0/0 warning budget, so a `console.error` here fails `npm run lint` outright. The
 * sanctioned diagnostic surface is an `OutputChannel` (see `diagnosticCommands.ts`),
 * which this command does not take; wiring one is tracked separately rather than
 * widened into this claim.
 */
function renderSupportPacketSafely(
  dependencies: SupportCommandDependencies,
  serverVersion: string,
) {
  try {
    return formatSupportPacketHuman(
      buildBasicSupportPacket({
        serverVersion,
        extensionVersion: dependencies.extensionVersion,
        editorVersion: dependencies.editorVersion,
        platform: dependencies.platform,
        arch: dependencies.arch,
        editorName: dependencies.editorName,
      }),
    );
  } catch {
    return null;
  }
}

async function openIssueForm(): Promise<void> {
  await vscode.env.openExternal(vscode.Uri.parse(PUBLIC_BUG_REPORT_URL));
}

/**
 * Show the packet in a native, inspectable editor document before the user shares it.
 *
 * Opening an untitled document can fail (host teardown, editor limits), and letting
 * that reject would dead-end the command the same way an unguarded packet render did.
 * Report it bounded and keep the issue form reachable instead.
 */
async function showSupportPacket(humanPacket: string): Promise<void> {
  try {
    const document = await vscode.workspace.openTextDocument({
      content: humanPacket,
      language: 'plaintext',
    });
    await vscode.window.showTextDocument(document, { preview: true });
  } catch {
    const recovery = await vscode.window.showWarningMessage(
      'Could not open the support packet in an editor tab. You can still open the issue form and describe the problem.',
      'Open Issue Form',
    );
    if (recovery === 'Open Issue Form') {
      await openIssueForm();
    }
  }
}

async function copySupportPacket(humanPacket: string): Promise<void> {
  try {
    await vscode.env.clipboard.writeText(humanPacket);
  } catch {
    const recovery = await vscode.window.showWarningMessage(
      'Could not write the support packet to the clipboard. Show the packet to copy it manually, or open the issue form and describe the problem.',
      'Show Support Packet',
      'Open Issue Form',
    );
    if (recovery === 'Show Support Packet') {
      await showSupportPacket(humanPacket);
    } else if (recovery === 'Open Issue Form') {
      await openIssueForm();
    }
    return;
  }
  vscode.window.showInformationMessage(
    'Support packet copied. Review it, then choose Open Issue Form to paste it into the report.',
  );
}

/**
 * Collect bounded support context and offer inspect, copy, and issue-form actions.
 *
 * Each action is independent: copying never opens the browser and showing never
 * copies, so no support data leaves the machine without an explicit user choice.
 */
export async function reportIssueCommand(dependencies: SupportCommandDependencies): Promise<void> {
  const serverVersion = await getServerVersionSafely(dependencies);
  const humanPacket = renderSupportPacketSafely(dependencies, serverVersion);

  if (humanPacket === null) {
    const fallback = await vscode.window.showWarningMessage(
      'The support packet could not be generated. You can still open the issue form and describe the problem manually.',
      'Open Issue Form',
    );
    if (fallback === 'Open Issue Form') {
      await openIssueForm();
    }
    return;
  }

  const selection = await vscode.window.showInformationMessage(
    'Report a perl-lsp issue. Review the support packet before you share it.',
    'Show Support Packet',
    'Copy Support Packet',
    'Open Issue Form',
  );

  if (selection === 'Show Support Packet') {
    await showSupportPacket(humanPacket);
  } else if (selection === 'Copy Support Packet') {
    await copySupportPacket(humanPacket);
  } else if (selection === 'Open Issue Form') {
    await openIssueForm();
  }
}
