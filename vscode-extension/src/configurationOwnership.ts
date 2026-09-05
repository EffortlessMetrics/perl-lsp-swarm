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
 * Something about a row the extension and server cannot honour end to end —
 * a declared scope that cannot be preserved, or a transport that reaches the
 * server but has no effect there.
 *
 * Recorded rather than silently corrected: changing a published setting's scope
 * or wiring a new server payload is its own claim.
 */
export type OwnershipDefect = {
  /** Why the recorded row is not honoured end to end today. */
  readonly reason: string;
  /** Issue that owns the corrective change. */
  readonly owner: string;
};

export type SettingOwnership = {
  readonly key: string;
  readonly manifestScope: ManifestScope;
  readonly semanticScope: SemanticScope;
  readonly owner: RuntimeOwner;
  readonly transport: Transport;
  readonly defect?: OwnershipDefect;
};

/**
 * Critic settings are declared `resource`, but the server keeps exactly one
 * accepted Critic state per session (#8253).
 *
 * `workspace/configuration` results are applied by
 * `WorkspaceConfig::update_from_value_with_context`, which reads only the
 * `workspace` key and has no Critic field; Critic is parsed exclusively by
 * `ServerConfig::update_from_value`, reachable from the
 * `workspace/didChangeConfiguration` push and the lifecycle paths. So a
 * per-folder Critic value cannot take effect today no matter how it is
 * transported.
 */
const CRITIC_SESSION_STATE_DEFECT: OwnershipDefect = {
  reason:
    'Declared `resource`, but the server holds one session-global Critic state ' +
    '(#8253). Folder-scoped `workspace/configuration` results are parsed by ' +
    'WorkspaceConfig, which has no Critic field, so only the session-wide ' +
    'didChangeConfiguration push can take effect.',
  owner: '#14447',
};

const STATIC_CAPABILITY_DEFECT: OwnershipDefect = {
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
    semanticScope: 'client-session',
    owner: 'extension',
    transport: 'local-only',
    defect: {
      reason:
        'Declared `resource`, but the file-creation listener reads ' +
        "getConfiguration('perl-lsp') once without a created-file URI before " +
        'iterating the event, so a folder override is never selected.',
      owner: '#14447',
    },
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
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
  },
  {
    key: 'perl-lsp.critic.engine',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'didChangeConfiguration',
  },
  {
    key: 'perl-lsp.critic.exclude',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
  },
  {
    key: 'perl-lsp.critic.include',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
  },
  {
    key: 'perl-lsp.critic.profile',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
  },
  {
    key: 'perl-lsp.critic.severity',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
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
    defect: STATIC_CAPABILITY_DEFECT,
  },
  {
    key: 'perl-lsp.enableSemanticTokens',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'initialize',
    defect: STATIC_CAPABILITY_DEFECT,
  },
  {
    key: 'perl-lsp.enableTestIntegration',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'extension',
    transport: 'local-only',
    defect: {
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
    defect: {
      reason:
        'Travels the pull, but `apply_workspace_configuration_results` classifies ' +
        'both the unscoped and the folder-scoped result item as ' +
        'ExternalIncludePathAuthority::Untrusted, so every non-empty external root ' +
        'is rejected with a warning (#4998). Only a server-owned trusted operator ' +
        'adapter may admit these; that adapter is #10817.',
      owner: '#10817',
    },
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
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
  },
  {
    key: 'perl-lsp.perlcritic.profile',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'didChangeConfiguration',
  },
  {
    key: 'perl-lsp.perlcritic.severity',
    manifestScope: 'resource',
    semanticScope: 'client-session',
    owner: 'server',
    transport: 'didChangeConfiguration',
    defect: CRITIC_SESSION_STATE_DEFECT,
  },
  {
    key: 'perl-lsp.perlcritic.theme',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'server',
    transport: 'didChangeConfiguration',
  },
  {
    key: 'perl-lsp.perltidyConfig',
    manifestScope: 'machine',
    semanticScope: 'machine',
    owner: 'extension',
    transport: 'local-only',
    defect: {
      reason:
        'Names a server-side formatter config but never reaches the server: ' +
        'initializationOptions carries only disabledFeatures, and neither ' +
        'configuration transport includes this key. Today it only feeds local ' +
        'coexistence advisories and formatter error text.',
      owner: '#14447',
    },
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
