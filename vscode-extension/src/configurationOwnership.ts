import * as vscode from 'vscode';

/**
 * Checked ownership table for every contributed `perl-lsp.*` setting (#14447).
 *
 * The VS Code manifest declares a *syntactic* scope (`machine`, `window`,
 * `resource`). That scope alone does not say who consumes the value, how it
 * reaches the server, or whether the extension can actually preserve the
 * declared scope end to end. This table records those facts explicitly so a new
 * contributed setting cannot be added without stating them, and so the known
 * places where the manifest scope is *not* honourable are visible as data
 * rather than as folklore.
 *
 * This module is descriptive. It does not read or write configuration values;
 * `configurationPull.ts` owns the folder-scoped transport and
 * `languageClientConfiguration.ts` owns payload construction.
 */

/** Scope as declared in `package.json` `contributes.configuration`. */
export type ManifestScope = 'machine' | 'window' | 'resource';

/**
 * Scope the value can actually carry at runtime.
 *
 * - `machine`   — one value per installation; never folder-specific.
 * - `client-session` — one value per `LanguageClient`/server process. A single
 *   client advertises one initialize capability surface, so these cannot vary
 *   per folder even when the manifest says `resource`.
 * - `workspace-folder` — genuinely folder-owned; different folders may hold
 *   different effective values simultaneously.
 */
export type SemanticScope = 'machine' | 'client-session' | 'workspace-folder';

/** Which side consumes the value. */
export type RuntimeOwner = 'extension' | 'server' | 'both';

/** How the value reaches its consumer. */
export type Transport =
  /** Read once when constructing the client; changing it needs a restart. */
  | 'initialize'
  /** Answered per folder through `workspace/configuration` (server pull). */
  | 'workspace/configuration'
  /** Pushed to the server as a session-global snapshot. */
  | 'didChangeConfiguration'
  /** Never leaves the extension host. */
  | 'local-only';

/**
 * A manifest scope the extension and server cannot honour end to end.
 *
 * Recorded rather than silently corrected: changing a published setting's scope
 * is a user-visible breaking change and is owned by its own claim.
 */
export type ScopeDefect = {
  /** Why the declared manifest scope cannot be preserved. */
  readonly reason: string;
  /** Issue that owns the corrective scope transition. */
  readonly owner: string;
};

export type SettingOwnership = {
  readonly key: string;
  readonly manifestScope: ManifestScope;
  readonly semanticScope: SemanticScope;
  readonly owner: RuntimeOwner;
  readonly transport: Transport;
  readonly scopeDefect?: ScopeDefect;
};

const STATIC_CAPABILITY_DEFECT: ScopeDefect = {
  reason:
    'Declared `resource` but selects the static initialize capability surface. One ' +
    'LanguageClient advertises one capability set, so the value cannot differ per folder.',
  owner: '#14447',
};

/**
 * One row per contributed `perl-lsp.*` setting.
 *
 * `configurationOwnership.test.ts` proves this table and
 * `package.json` describe exactly the same set of keys with the same manifest
 * scopes, so a contributed setting cannot appear without an ownership row.
 */
export const SETTING_OWNERSHIP: readonly SettingOwnership[] = [
  {
    key: 'perl-lsp.aiCompletion.enabled',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'both',
    transport: 'didChangeConfiguration',
  },
  {
    key: 'perl-lsp.aiCompletion.streaming.enabled',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'both',
    transport: 'didChangeConfiguration',
  },
  {
    key: 'perl-lsp.autoDownload',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.autoPopulateNewFiles',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.autoUpdate',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.channel',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.critic.enabled',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.critic.engine',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.critic.exclude',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.critic.include',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.critic.profile',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.critic.severity',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.disabledFeatures',
    manifestScope: 'window',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'initialize',
  },
  {
    key: 'perl-lsp.downloadBaseUrl',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.enableFormatting',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'initialize',
    scopeDefect: STATIC_CAPABILITY_DEFECT,
  },
  {
    key: 'perl-lsp.enableSemanticTokens',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'initialize',
    scopeDefect: STATIC_CAPABILITY_DEFECT,
  },
  {
    key: 'perl-lsp.enableTestIntegration',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'extension',
    transport: 'local-only',
    scopeDefect: {
      reason:
        'Declared `resource` but gates construction of the single Test Explorer ' +
        'controller, which is one per extension host rather than one per folder.',
      owner: '#14447',
    },
  },
  {
    key: 'perl-lsp.externalIncludePaths',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.featureProfile',
    manifestScope: 'window',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'initialize',
  },
  {
    key: 'perl-lsp.formatOnSave',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.includePaths',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.linuxLibc',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.mcp.servers',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.perlcritic.enabled',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.perlcritic.profile',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.perlcritic.severity',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.perlcritic.theme',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'workspace/configuration',
  },
  {
    key: 'perl-lsp.perltidyConfig',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'initialize',
  },
  {
    key: 'perl-lsp.serverPath',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.testAdapterMaxOutputBytes',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.testAdapterTerminationGraceMs',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.testAdapterTimeoutMs',
    manifestScope: 'resource',
    semanticScope: 'workspace-folder',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.trace.server',
    manifestScope: 'window',
    semanticScope: 'client-session',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.updateCheckInterval',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
  {
    key: 'perl-lsp.versionTag',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
  },
];

const OWNERSHIP_BY_KEY = new Map(SETTING_OWNERSHIP.map((row) => [row.key, row]));

/** Ownership row for a fully qualified `perl-lsp.*` key, when one exists. */
export function settingOwnership(key: string): SettingOwnership | undefined {
  return OWNERSHIP_BY_KEY.get(key);
}

/**
 * Keys the server learns through its per-folder `workspace/configuration` pull.
 *
 * Returned unqualified (without the `perl-lsp.` prefix) because that is how
 * they are read from a scoped `WorkspaceConfiguration`.
 */
export function folderScopedServerSettingKeys(): string[] {
  return SETTING_OWNERSHIP.filter((row) => row.transport === 'workspace/configuration').map((row) =>
    row.key.slice('perl-lsp.'.length),
  );
}

/**
 * Resolve the write target for a setting whose value is owned by one folder.
 *
 * A folder-local action must write `WorkspaceFolder` so the value stays bound
 * to the folder the user acted on. Writing `Workspace` in a multi-root
 * workspace silently applies the value to every other folder as well (#14447).
 *
 * Falls back to `Workspace` only when the resource belongs to no workspace
 * folder but a workspace exists, and to `Global` when there is no workspace at
 * all (for example a lone open file).
 */
export function resolveResourceWriteTarget(
  resource: vscode.Uri | undefined,
): vscode.ConfigurationTarget {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return vscode.ConfigurationTarget.Global;
  }

  if (resource && vscode.workspace.getWorkspaceFolder(resource)) {
    return vscode.ConfigurationTarget.WorkspaceFolder;
  }

  return vscode.ConfigurationTarget.Workspace;
}
