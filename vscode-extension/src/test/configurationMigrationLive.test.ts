import {
  type ConfigurationMigrationRegistry,
  type ConfigurationMigrationRow,
  V018_CONFIGURATION_MIGRATIONS,
} from '../configurationMigrationRegistry';
import {
  type ConfigurationTarget,
  type LegacyConfigurationSites,
  type LegacyMigrationOccurrence,
  authorizedTargetsForScope,
  describeLegacyMigrationOccurrence,
  legacyMigrationState,
  readLegacyConfiguration,
  unwiredCanonicalValues,
} from '../configurationMigrationLive';

const MCP_KEY = 'perl-lsp.mcp.servers';
const EXTENSION_VERSION = '0.18.0';

/**
 * A stored value shaped like the historical setting: a list of commands with an
 * environment. Nothing this specific may reach the published state.
 */
const STALE_MCP_VALUE = [
  { label: 'local', command: '/opt/secret-tools/run-agent', env: { TOKEN: 'hunter2' } },
];

function sitesFor(sites: LegacyConfigurationSites): (key: string) => LegacyConfigurationSites {
  return (key) => (key === MCP_KEY ? sites : {});
}

function read(sites: LegacyConfigurationSites): readonly LegacyMigrationOccurrence[] {
  return readLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, EXTENSION_VERSION, sitesFor(sites));
}

function onlyOccurrence(sites: LegacyConfigurationSites): LegacyMigrationOccurrence {
  const occurrences = read(sites);
  expect(occurrences).toHaveLength(1);
  const occurrence = occurrences[0];
  if (occurrence === undefined) {
    throw new Error('expected exactly one occurrence');
  }
  return occurrence;
}

function registryWithRow(overrides: Partial<ConfigurationMigrationRow>): {
  registry: ConfigurationMigrationRegistry;
  key: string;
} {
  const row: ConfigurationMigrationRow = {
    migration_id: 'legacy_rename',
    old_key: 'perl-lsp.oldSetting',
    old_value_shape: 'boolean',
    introduced_version: '0.17.0',
    last_supported_version: '0.17.x',
    new_key_or_authority: 'perl-lsp.newSetting',
    old_scope: 'resource',
    new_scope: 'resource',
    migration_disposition: 'renamed_compatible',
    automatic_read_compatibility: true,
    explicit_write_allowed: false,
    old_plus_new_conflict_policy: 'current_wins',
    security_trust_class: 'ordinary',
    warning_reason_code: 'legacy_setting_renamed',
    compatibility_window: { kind: 'no_expiry' },
    expiry_owner_issue: 9000,
    installed_proof_requirement: '#9001',
    ...overrides,
  };
  return {
    registry: {
      schema_version: 'vscode_configuration_migration.v2',
      source_public_release: '0.17.0',
      target_release: '0.18.0',
      rows: [row],
    },
    key: row.old_key,
  };
}

describe('live legacy configuration reader', () => {
  test('a clean profile produces no occurrence and no state', () => {
    const occurrences = read({});

    expect(occurrences).toEqual([]);
    expect(
      legacyMigrationState(V018_CONFIGURATION_MIGRATIONS, EXTENSION_VERSION, occurrences).entries,
    ).toEqual([]);
  });

  test('the removed MCP setting in user settings is inert, with no canonical value', () => {
    const occurrence = onlyOccurrence({ user: { value: STALE_MCP_VALUE } });

    expect(occurrence.target).toBe('user');
    expect(occurrence.folderId).toBeNull();
    expect(occurrence.runtime.status).toBe('inert');
    expect(occurrence.runtime.migration_id).toBe('v017_mcp_servers_removed');
    expect(occurrence.runtime.reason_code).toBe('legacy_mcp_passthrough_removed');
    expect(occurrence.runtime.canonical_value_present).toBe(false);
    expect(occurrence.runtime.disk_write_allowed).toBe(false);
  });

  // The security control. `perl-lsp.mcp.servers` is `machine` + `process_execution`, so a
  // copy in repository-controlled configuration must not reach its registry row. A reader
  // that treated every target as authorized would report `inert` here instead.
  test.each([
    ['workspace', { workspace: { value: STALE_MCP_VALUE } }],
    ['workspace-folder', { workspaceFolders: [{ folderId: 'folder:0', value: STALE_MCP_VALUE }] }],
  ] as const)('a machine-scoped legacy key found in %s settings is refused', (target, sites) => {
    const occurrence = onlyOccurrence(sites);

    expect(occurrence.target).toBe(target);
    expect(occurrence.runtime.status).toBe('invalid');
    expect(occurrence.runtime.status).not.toBe('inert');
    expect(occurrence.runtime.reason_code).toBe('legacy_key_scope_not_permitted');
    expect(occurrence.runtime.migration_id).toBeNull();

    // No row was selected, so the notice must not assert a replacement the registry
    // never named for this occurrence.
    const message = describeLegacyMigrationOccurrence(occurrence);
    expect(message).toContain('legacy_key_scope_not_permitted');
    expect(message).not.toContain('replacement');
    expect(message).not.toContain('instead');
  });

  test('the authorization table keeps installation-owned scopes out of repository control', () => {
    expect(authorizedTargetsForScope('machine')).toEqual(['user']);
    expect(authorizedTargetsForScope('user')).toEqual(['user']);
    expect(authorizedTargetsForScope('workspace')).toEqual(['workspace']);
    expect(authorizedTargetsForScope('workspace-folder')).toEqual(['workspace-folder']);
    expect(authorizedTargetsForScope('resource')).toEqual([
      'user',
      'workspace',
      'workspace-folder',
    ]);
    expect(authorizedTargetsForScope('machine-overridable')).toEqual([
      'user',
      'workspace',
      'workspace-folder',
    ]);
  });

  test('one key present at two targets is two independently judged occurrences', () => {
    const occurrences = read({
      user: { value: STALE_MCP_VALUE },
      workspace: { value: STALE_MCP_VALUE },
    });

    expect(occurrences.map((occurrence) => [occurrence.target, occurrence.runtime.status])).toEqual(
      [
        ['user', 'inert'],
        ['workspace', 'invalid'],
      ],
    );
  });

  test('folder-scoped occurrences stay bound to the folder that owns them', () => {
    const { registry, key } = registryWithRow({ old_scope: 'workspace-folder' });

    const occurrences = readLegacyConfiguration(registry, EXTENSION_VERSION, (probed) =>
      probed === key
        ? {
            workspaceFolders: [
              { folderId: 'folder:0', value: true },
              { folderId: 'folder:1', value: false },
            ],
          }
        : {},
    );

    expect(occurrences.map((occurrence) => occurrence.folderId)).toEqual(['folder:0', 'folder:1']);
    expect(new Set(occurrences.map((occurrence) => occurrence.runtime.canonical_value))).toEqual(
      new Set([true, false]),
    );
  });

  // When more than one registry scope authorizes the same target, the reader makes no
  // choice: it reports the target's own scope. The outcome is therefore the row naming
  // that exact scope, or a refusal — never whichever row happens to sort first.
  test.each([
    ['workspace', 'compatible_legacy', null],
    ['resource', 'invalid', 'legacy_key_scope_not_permitted'],
  ] as const)(
    'two authorizing scopes including %s resolve without depending on row order',
    (secondScope, status, reasonCode) => {
      const { registry, key } = registryWithRow({ old_scope: 'machine-overridable' });
      const firstRow = registry.rows[0];
      if (firstRow === undefined) {
        throw new Error('registryWithRow must define one row');
      }
      const twoScopes: ConfigurationMigrationRegistry = {
        ...registry,
        rows: [
          firstRow,
          {
            ...firstRow,
            migration_id: 'legacy_rename_other_era',
            introduced_version: '0.16.0',
            last_supported_version: '0.16.x',
            old_scope: secondScope,
          },
        ],
      };
      const reversed: ConfigurationMigrationRegistry = {
        ...twoScopes,
        rows: [...twoScopes.rows].reverse(),
      };
      const sites = (probed: string): LegacyConfigurationSites =>
        probed === key ? { workspace: { value: true } } : {};

      for (const candidate of [twoScopes, reversed]) {
        const occurrences = readLegacyConfiguration(candidate, EXTENSION_VERSION, sites);
        expect(occurrences).toHaveLength(1);
        expect(occurrences[0]?.runtime.status).toBe(status);
        expect(occurrences[0]?.runtime.reason_code).toBe(
          reasonCode ?? firstRow.warning_reason_code,
        );
      }
    },
  );

  test('published state carries no raw value, path, or secret', () => {
    const state = legacyMigrationState(
      V018_CONFIGURATION_MIGRATIONS,
      EXTENSION_VERSION,
      read({ user: { value: STALE_MCP_VALUE } }),
    );
    const serialized = JSON.stringify(state);

    expect(serialized).not.toContain('secret-tools');
    expect(serialized).not.toContain('hunter2');
    expect(serialized).not.toContain('TOKEN');
    expect(state.entries).toEqual([
      {
        migration_id: 'v017_mcp_servers_removed',
        legacy_key: MCP_KEY,
        status: 'inert',
        source_scope: 'machine',
        canonical_key_or_authority: null,
        reason_code: 'legacy_mcp_passthrough_removed',
        notice_required: true,
        post_expiry_disposition: null,
        target: 'user',
        folderId: null,
      },
    ]);
    expect(state.registrySchemaVersion).toBe('vscode_configuration_migration.v2');
    expect(state.registryTargetRelease).toBe('0.18.0');
    expect(state.extensionVersion).toBe(EXTENSION_VERSION);
  });

  test('the notice names the setting and its disposition but not its value', () => {
    const message = describeLegacyMigrationOccurrence(
      onlyOccurrence({ user: { value: STALE_MCP_VALUE } }),
    );

    expect(message).toContain(MCP_KEY);
    expect(message).toContain('user settings');
    expect(message).toContain('inert');
    expect(message).toContain('legacy_mcp_passthrough_removed');
    expect(message).not.toContain('hunter2');
    expect(message).not.toContain('secret-tools');
  });

  test('a folder-scoped notice names the opaque folder identity, never a path', () => {
    const { registry, key } = registryWithRow({ old_scope: 'workspace-folder' });
    const occurrences = readLegacyConfiguration(registry, EXTENSION_VERSION, (probed) =>
      probed === key ? { workspaceFolders: [{ folderId: 'folder:3', value: true }] } : {},
    );

    const message = describeLegacyMigrationOccurrence(occurrences[0]!);
    expect(message).toContain('workspace folder settings (folder:3)');
    expect(message).toContain('perl-lsp.newSetting');
  });

  test('every current registry row is free of unwired canonical values', () => {
    // The published seam ends at the redacted state: publishing a legacy-derived value as
    // canonical current configuration is #6736/#7838 work that is not wired here. This
    // fails the moment a row starts producing one, so it cannot be silently dropped.
    const sites: LegacyConfigurationSites = { user: { value: STALE_MCP_VALUE } };
    const occurrences = readLegacyConfiguration(
      V018_CONFIGURATION_MIGRATIONS,
      EXTENSION_VERSION,
      () => sites,
    );

    expect(occurrences.length).toBeGreaterThan(0);
    expect(unwiredCanonicalValues(occurrences)).toEqual([]);
  });

  test('a compatible row is reported as an unwired canonical value rather than dropped', () => {
    const { registry, key } = registryWithRow({});
    const occurrences = readLegacyConfiguration(registry, EXTENSION_VERSION, (probed) =>
      probed === key ? { user: { value: true } } : {},
    );

    expect(occurrences[0]?.runtime.status).toBe('compatible_legacy');
    expect(unwiredCanonicalValues(occurrences)).toHaveLength(1);
  });

  test('an expired row is reported as expired against the running extension version', () => {
    const { registry, key } = registryWithRow({
      compatibility_window: {
        kind: 'removed_in_extension_version',
        version: '0.18.0',
        post_expiry_disposition: 'action_required',
      },
    });
    const sites = (probed: string): LegacyConfigurationSites =>
      probed === key ? { user: { value: true } } : {};

    expect(readLegacyConfiguration(registry, '0.17.9', sites)[0]?.runtime.status).toBe(
      'compatible_legacy',
    );
    expect(readLegacyConfiguration(registry, '0.18.0', sites)[0]?.runtime.status).toBe('expired');
  });

  test('every reported target is one of the declared configuration targets', () => {
    const targets: readonly ConfigurationTarget[] = read({
      user: { value: STALE_MCP_VALUE },
      workspace: { value: STALE_MCP_VALUE },
      workspaceFolders: [{ folderId: 'folder:0', value: STALE_MCP_VALUE }],
    }).map((occurrence) => occurrence.target);

    expect(targets).toEqual(['user', 'workspace', 'workspace-folder']);
  });
});
