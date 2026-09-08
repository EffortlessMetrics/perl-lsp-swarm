/**
 * Advisory coexistence collection and explanation for VS Code (#7214).
 *
 * The collector turns bounded, observable host facts into
 * {@link CoexistenceObservations}; the advisory flow deduplicates findings by
 * exact conflict identity, honors per-conflict suppression, and never mutates
 * any external tool or setting. Suppressions are pruned when their condition
 * disappears so a later recurrence is reported again.
 */

import * as vscode from 'vscode';
import {
  COEXISTENCE_CLAIM_BOUNDARY,
  NATIVE_EXTENSION_ID,
  REVIEWED_PERL_EXTENSIONS,
  buildRedactedCoexistencePacket,
  coexistenceConflictKey,
  type CoexistenceFinding,
} from './coexistenceRegistry';
import {
  detectCoexistenceFindings,
  type CoexistenceObservations,
  type ObservedExtension,
} from './coexistenceDetector';

const SUPPRESSED_KEY_PREFIX = 'perl-lsp.coexistence.suppressed.';
const SHOWN_SIGNATURE_KEY = 'perl-lsp.coexistence.shownSignature';

/**
 * Every configuration input `collectCoexistenceFindings` reads. The
 * re-evaluation listener in the composition layer must watch exactly these so
 * a change to any collected input reaches the advisory (#7214 clear/restore
 * semantics) instead of leaving findings stale until restart.
 */
export const COEXISTENCE_CONFIGURATION_INPUTS: readonly string[] = [
  'perl-lsp.formatOnSave',
  'perl-lsp.critic.enabled',
  'perl-lsp.critic.engine',
  'perl-lsp.perltidyConfig',
  'editor.formatOnSave',
  'editor.defaultFormatter',
];

/**
 * Whether a configuration change should re-run the advisory: true exactly when
 * the change affects any collected input.
 */
export function coexistenceReevaluationRequested(
  affectsConfiguration: (setting: string) => boolean,
): boolean {
  return COEXISTENCE_CONFIGURATION_INPUTS.some((setting) => affectsConfiguration(setting));
}

type Inspected<T> = { globalValue?: T; workspaceValue?: T; workspaceFolderValue?: T };

function normalizeId(value: string | undefined): string {
  return (value ?? '').trim().toLowerCase();
}

function firstInspected<T>(inspected: Inspected<T> | undefined): T | undefined {
  if (!inspected) {
    return undefined;
  }
  return inspected.workspaceFolderValue ?? inspected.workspaceValue ?? inspected.globalValue;
}

function observedExtensions(selfId: string): ObservedExtension[] {
  const observed: ObservedExtension[] = [];
  for (const extension of vscode.extensions.all as ReadonlyArray<{
    id?: string;
    isActive?: boolean;
    packageJSON?: {
      contributes?: { debuggers?: Array<{ type?: string }> };
    };
  }>) {
    const id = normalizeId(extension?.id);
    if (!id || id === selfId) {
      continue;
    }
    observed.push({
      id,
      isActive: extension?.isActive === true,
      debuggerTypes: (extension?.packageJSON?.contributes?.debuggers ?? [])
        .map((debuggerEntry) => debuggerEntry?.type ?? '')
        .filter((type) => type.length > 0),
    });
  }
  return observed;
}

function folderSettingSnapshot(
  scope: vscode.ConfigurationScope | undefined,
): Pick<
  CoexistenceObservations,
  | 'nativeFormatOnSave'
  | 'editorFormatOnSave'
  | 'defaultFormatterSetting'
  | 'nativeCriticEnabled'
  | 'staleCriticEngineValue'
  | 'perltidyProfileSelected'
> {
  // The explicit Perl language scope is required so `[perl]` language
  // overrides are visible (same convention as getPerlCriticConfiguration).
  const perlLspConfig = vscode.workspace.getConfiguration('perl-lsp', scope);
  const editorConfig = vscode.workspace.getConfiguration('editor', scope);
  const perlEditorConfig = vscode.workspace.getConfiguration('[perl]', scope);

  const languageDefaultFormatter =
    perlEditorConfig.get<string>('editor.defaultFormatter') ?? undefined;
  const plainDefaultFormatter =
    firstInspected<string>(editorConfig.inspect<string>('defaultFormatter')) ?? undefined;

  return {
    nativeFormatOnSave: perlLspConfig.get<boolean>('formatOnSave', false),
    editorFormatOnSave: editorConfig.get<boolean>('formatOnSave', true),
    defaultFormatterSetting: languageDefaultFormatter ?? plainDefaultFormatter,
    nativeCriticEnabled: perlLspConfig.get<boolean>('critic.enabled', true),
    staleCriticEngineValue: firstInspected<string>(perlLspConfig.inspect<string>('critic.engine')),
    perltidyProfileSelected:
      (perlLspConfig.get<string>('perltidyConfig', '') ?? '').trim().length > 0,
  };
}

/**
 * Collect observations once per workspace folder plus one host-wide pass.
 * Every pass carries the full inventory so settings-derived classes combine
 * scoped settings with installed extensions; the host pass evaluates without
 * a resource scope, each folder pass its own `{ uri, languageId: 'perl' }`
 * snapshot and file evidence. Findings keep the scope they were observed in
 * and are deduplicated by exact conflict identity.
 */
export async function collectCoexistenceFindings(
  context: vscode.ExtensionContext,
): Promise<CoexistenceFinding[]> {
  const selfId =
    normalizeId(
      context.extension?.id ??
        `${context.extension?.packageJSON?.publisher ?? ''}.${context.extension?.packageJSON?.name ?? ''}`,
    ) || NATIVE_EXTENSION_ID;
  const inventory = observedExtensions(selfId);

  const userScopeObservations: CoexistenceObservations = {
    selfExtensionId: selfId,
    installedExtensions: inventory,
    ...folderSettingSnapshot({ languageId: 'perl' }),
  };
  const findings = detectCoexistenceFindings(userScopeObservations);

  const folders = vscode.workspace.workspaceFolders ?? [];
  for (const [index, folder] of folders.entries()) {
    let perltidyrcPresent = false;
    try {
      const matches = await vscode.workspace.findFiles(
        new vscode.RelativePattern(folder, '.perltidyrc'),
        null,
        1,
      );
      perltidyrcPresent = matches.length > 0;
    } catch {
      perltidyrcPresent = false;
    }
    findings.push(
      ...detectCoexistenceFindings({
        selfExtensionId: selfId,
        installedExtensions: inventory,
        folderName: folder.name || `folder-${index + 1}`,
        perltidyrcPresentInFolder: perltidyrcPresent,
        ...folderSettingSnapshot({ uri: folder.uri, languageId: 'perl' }),
      }),
    );
  }

  const byKey = new Map(findings.map((finding) => [coexistenceConflictKey(finding), finding]));
  return [...byKey.values()];
}

function readSuppressedKeys(context: vscode.ExtensionContext): Set<string> {
  const suppressed = new Set<string>();
  for (const key of context.globalState.keys()) {
    if (key.startsWith(SUPPRESSED_KEY_PREFIX) && context.globalState.get(key) === true) {
      suppressed.add(key.slice(SUPPRESSED_KEY_PREFIX.length));
    }
  }
  return suppressed;
}

/** Drop suppressions whose exact conflict no longer exists; restore on return. */
async function pruneStaleSuppressions(
  context: vscode.ExtensionContext,
  currentKeys: ReadonlySet<string>,
): Promise<void> {
  for (const key of readSuppressedKeys(context)) {
    if (!currentKeys.has(key)) {
      await context.globalState.update(`${SUPPRESSED_KEY_PREFIX}${key}`, undefined);
    }
  }
}

function findingSummary(finding: CoexistenceFinding): string {
  const where =
    finding.scopeKind === 'workspace-folder' && finding.folderName
      ? ` in folder "${finding.folderName}"`
      : '';
  const who = finding.otherOwner ? ` — ${finding.otherOwner}` : '';
  return `${finding.conflictClass}${where}${who}`;
}

/**
 * Human-readable explanation of every current finding. Each block names the
 * authoritative observation source, owners, risk, bounded remediation choices,
 * reload requirement, and the claim boundary. With no findings it states the
 * honest detection coverage instead of inventing a clean bill of health.
 */
export function renderCoexistenceStatusReport(findings: readonly CoexistenceFinding[]): string {
  const lines: string[] = [
    '# Perl LSP coexistence status',
    '',
    `Claim boundary: ${COEXISTENCE_CLAIM_BOUNDARY}`,
    '',
  ];

  if (findings.length === 0) {
    lines.push(
      'No conflicts detected among the facts this product can observe.',
      '',
      'Detection coverage (bounded by design):',
      `- reviewed extension identities: ${REVIEWED_PERL_EXTENSIONS.map((identity) => identity.extensionId).join(', ')}`,
      '- settings: perl-lsp.formatOnSave, editor.formatOnSave, editor.defaultFormatter ([perl] override), perl-lsp.critic.enabled, perl-lsp.critic.engine, perl-lsp.perltidyConfig',
      '- declared debugger contributions with type "perl"',
      '- workspace .perltidyrc candidate files',
      '',
      "PATH presence of perlcritic/perltidy and a project's .perlcriticrc are not providers and are never reported as conflicts.",
      '',
    );
    return lines.join('\n');
  }

  for (const finding of findings) {
    lines.push(
      `## ${findingSummary(finding)}`,
      '',
      `- Conflict class: \`${finding.conflictClass}\``,
      `- Scope: ${finding.scopeKind}${finding.folderName ? ` (${finding.folderName})` : ''}`,
      `- Native owner: ${finding.nativeOwner}`,
      `- Other observed owner: ${finding.otherOwner ?? 'not identified'}`,
      `- Evidence source: ${finding.evidenceSource}`,
      ...(finding.registryReasonCode
        ? [`- Registry reason code (#7209/#7212): ${finding.registryReasonCode}`]
        : []),
      `- Symptom: ${finding.symptom}`,
      `- Risk: ${finding.risk}`,
      `- Remediation choices: ${finding.remediationChoices.join(', ')}`,
      `- Reload required to take effect: ${finding.requiresReload ? 'yes' : 'no'}`,
      `- Claim boundary: ${finding.claimBoundary}`,
      '',
    );
  }
  return lines.join('\n');
}

async function openStatusReport(report: string): Promise<void> {
  const document = await vscode.workspace.openTextDocument({
    content: report,
    language: 'markdown',
  });
  await vscode.window.showTextDocument(document, { preview: true });
}

/** Show the full coexistence explanation for the current host state. */
export async function showCoexistenceStatusCommand(
  context: vscode.ExtensionContext,
): Promise<void> {
  const findings = await collectCoexistenceFindings(context);
  await openStatusReport(renderCoexistenceStatusReport(findings));
}

/**
 * Run the advisory startup flow: collect, detect, prune stale suppressions,
 * and notify at most once per distinct finding set. Returns the findings that
 * were current after evaluation (exposed for tests and the status command).
 */
export async function runCoexistenceAdvisory(
  context: vscode.ExtensionContext,
): Promise<CoexistenceFinding[]> {
  const findings = await collectCoexistenceFindings(context);
  const keysByKey = new Map(findings.map((finding) => [coexistenceConflictKey(finding), finding]));
  await pruneStaleSuppressions(context, new Set(keysByKey.keys()));

  const suppressed = readSuppressedKeys(context);
  const active = findings.filter((finding) => !suppressed.has(coexistenceConflictKey(finding)));
  const signature = active.map(coexistenceConflictKey).sort().join('\n');

  if (signature !== (context.globalState.get<string>(SHOWN_SIGNATURE_KEY) ?? '')) {
    const [primary] = active;
    if (primary) {
      const message = `Perl LSP coexistence: ${active.length} potential tooling overlap${active.length === 1 ? '' : 's'} detected. First: ${findingSummary(primary)}. perl-lsp made no changes to any other tool.`;
      const choice = await vscode.window.showWarningMessage(
        message,
        'Show coexistence status',
        'Disable for this exact conflict',
        'Copy redacted support packet',
      );
      if (choice === 'Show coexistence status') {
        await openStatusReport(renderCoexistenceStatusReport(active));
      } else if (choice === 'Disable for this exact conflict') {
        await context.globalState.update(
          `${SUPPRESSED_KEY_PREFIX}${coexistenceConflictKey(primary)}`,
          true,
        );
      } else if (choice === 'Copy redacted support packet') {
        await vscode.env.clipboard.writeText(buildRedactedCoexistencePacket(active));
      }
    }
    await context.globalState.update(SHOWN_SIGNATURE_KEY, signature);
  }

  return findings;
}
