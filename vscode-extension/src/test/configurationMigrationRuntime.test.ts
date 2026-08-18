import {
  type ConfigurationMigrationRegistry,
  type ConfigurationMigrationRow,
  V018_CONFIGURATION_MIGRATIONS,
  validateMigrationRegistry,
} from '../configurationMigrationRegistry';
import {
  MigrationNoticeDedupe,
  interpretLegacyConfiguration,
  safeMigrationRuntimeSnapshot,
} from '../configurationMigrationRuntime';

function compatibleRegistry(): ConfigurationMigrationRegistry {
  return {
    schema_version: 'vscode_configuration_migration.v1',
    source_public_release: '0.17.0',
    target_release: '0.18.0',
    rows: [
      {
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
        expiry_version_or_issue: '#9000',
        installed_proof_requirement: '#9001',
      },
    ],
  };
}

function registryWithConflictPolicy(
  policy: ConfigurationMigrationRow['old_plus_new_conflict_policy'],
): ConfigurationMigrationRegistry {
  const base = compatibleRegistry();
  const row = base.rows[0];
  if (row === undefined) {
    throw new Error('compatibleRegistry must define one row');
  }
  return { ...base, rows: [{ ...row, old_plus_new_conflict_policy: policy }] };
}

describe('configuration migration runtime', () => {
  test('keeps removed MCP process-execution settings inert without carrying their value', () => {
    const secretLegacyValue = [
      { label: 'private', command: '/private/tool', env: { TOKEN: 'secret' } },
    ];
    const result = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'machine',
      legacy_value_present: true,
      legacy_value: secretLegacyValue,
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      migration_id: 'v017_mcp_servers_removed',
      status: 'inert',
      canonical_key_or_authority: null,
      canonical_value_present: false,
      canonical_value: null,
      reason_code: 'legacy_mcp_passthrough_removed',
      notice_required: true,
      disk_write_allowed: false,
    });
    expect(JSON.stringify(safeMigrationRuntimeSnapshot(result))).not.toContain('private/tool');
    expect(JSON.stringify(safeMigrationRuntimeSnapshot(result))).not.toContain('secret');
  });

  test('uses compatible legacy value when the current key is absent', () => {
    const result = interpretLegacyConfiguration(compatibleRegistry(), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: true,
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      status: 'compatible_legacy',
      canonical_key_or_authority: 'perl-lsp.newSetting',
      canonical_value_present: true,
      canonical_value: true,
      disk_write_allowed: false,
    });
  });

  test('current canonical key wins explicitly without mutating disk', () => {
    const result = interpretLegacyConfiguration(compatibleRegistry(), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: true,
      current_value_present: true,
      current_value: false,
    });

    expect(result).toMatchObject({
      status: 'compatible_current_wins',
      canonical_value_present: true,
      canonical_value: false,
      disk_write_allowed: false,
    });
  });

  test('a registry the validator certifies is never blamed on the user as an unknown key', () => {
    // The registry's uniqueness key spans the version window and value shape, so one
    // setting may legitimately carry a row per historical era. Such a registry is valid,
    // but this interpreter takes no version input and cannot choose between the eras.
    const multiEra: ConfigurationMigrationRegistry = (() => {
      const base = compatibleRegistry();
      const row = base.rows[0];
      if (row === undefined) {
        throw new Error('compatibleRegistry must define one row');
      }
      return {
        ...base,
        rows: [
          { ...row, migration_id: 'era_a', introduced_version: '0.15.0' },
          {
            ...row,
            migration_id: 'era_b',
            introduced_version: '0.16.0',
            old_value_shape: 'string',
          },
        ],
      };
    })();

    // Load-bearing: the two modules must not disagree about what a valid registry is.
    expect(validateMigrationRegistry(multiEra)).toEqual([]);

    const result = interpretLegacyConfiguration(multiEra, {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: true,
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      status: 'invalid',
      canonical_value_present: false,
      notice_required: true,
      // Not `legacy_key_not_registered`: the key is registered. Reporting a registry
      // defect as an unknown user setting sends the user to fix the wrong thing.
      reason_code: 'legacy_registry_ambiguous',
    });
  });

  test('an unregistered legacy key names itself as unregistered', () => {
    expect(
      interpretLegacyConfiguration(compatibleRegistry(), {
        old_key: 'perl-lsp.neverShipped',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: true,
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({ status: 'invalid', reason_code: 'legacy_key_not_registered' });
  });

  test('wrong legacy scope is invalid rather than widening authority', () => {
    const result = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'workspace',
      legacy_value_present: true,
      legacy_value: [],
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      migration_id: null,
      status: 'invalid',
      canonical_value_present: false,
      notice_required: true,
      // This is a process-executing machine-scoped setting found in repository-controlled
      // workspace configuration. The notice has to be able to say that, so the cause must
      // survive as more than a bare `invalid`.
      reason_code: 'legacy_key_scope_not_permitted',
    });
  });

  test('no legacy value means migration is not applicable', () => {
    expect(
      interpretLegacyConfiguration(compatibleRegistry(), {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: false,
        legacy_value: null,
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({
      migration_id: null,
      status: 'not_applicable',
      canonical_value_present: false,
      notice_required: false,
    });
  });

  test.each([['not_applicable' as const], ['current_wins' as const]])(
    'a %s conflict policy never lets the legacy value silently outrank the current key',
    (policy) => {
      const result = interpretLegacyConfiguration(registryWithConflictPolicy(policy), {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: true,
        current_value: 'current',
      });

      expect(result).toMatchObject({
        status: 'compatible_current_wins',
        canonical_value_present: true,
        canonical_value: 'current',
        disk_write_allowed: false,
      });
    },
  );

  test('an explicit legacy_only policy is the only way the legacy value wins', () => {
    const result = interpretLegacyConfiguration(registryWithConflictPolicy('legacy_only'), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: true,
      current_value: 'current',
    });

    expect(result).toMatchObject({
      status: 'compatible_legacy',
      canonical_value_present: true,
      canonical_value: 'legacy',
      disk_write_allowed: false,
    });
  });

  test('an action_required conflict policy coerces neither value', () => {
    const result = interpretLegacyConfiguration(registryWithConflictPolicy('action_required'), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: true,
      current_value: 'current',
    });

    expect(result).toMatchObject({
      status: 'action_required',
      canonical_value_present: false,
      canonical_value: null,
      notice_required: true,
    });
  });

  test('an invalid legacy key can actually surface its required notice exactly once', () => {
    const runtime = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'workspace',
      legacy_value_present: true,
      legacy_value: [],
      current_value_present: false,
      current_value: null,
    });
    const dedupe = new MigrationNoticeDedupe();

    expect(runtime).toMatchObject({ migration_id: null, notice_required: true });
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(false);
  });

  test('distinct invalid legacy keys do not suppress each others notices', () => {
    const dedupe = new MigrationNoticeDedupe();
    const invalidFor = (oldKey: string) =>
      interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
        old_key: oldKey,
        source_scope: 'workspace',
        legacy_value_present: true,
        legacy_value: [],
        current_value_present: false,
        current_value: null,
      });

    expect(dedupe.shouldShow(invalidFor('perl-lsp.unknownAlpha'), 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(invalidFor('perl-lsp.unknownBeta'), 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(invalidFor('perl-lsp.unknownAlpha'), 'generation-1')).toBe(false);
  });

  test('the safe snapshot carries the legacy key but never the legacy value', () => {
    const runtime = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'machine',
      legacy_value_present: true,
      legacy_value: [{ label: 'x', command: '/opt/secret-tool', env: { TOKEN: 'hunter2' } }],
      current_value_present: false,
      current_value: null,
    });
    const snapshot = safeMigrationRuntimeSnapshot(runtime);

    expect(snapshot.legacy_key).toBe('perl-lsp.mcp.servers');
    expect(JSON.stringify(snapshot)).not.toContain('secret-tool');
    expect(JSON.stringify(snapshot)).not.toContain('hunter2');
  });

  test('the safe snapshot redacts a compatible legacy value, not only inert ones', () => {
    // A compatible result DOES carry the legacy value at runtime; a snapshot
    // implementation that leaks canonical_value only fails here, never on the
    // inert fixtures above.
    const secret = 'hunter2-compatible-legacy-secret';
    const runtime = interpretLegacyConfiguration(compatibleRegistry(), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: secret,
      current_value_present: false,
      current_value: null,
    });

    expect(runtime.status).toBe('compatible_legacy');
    expect(runtime.canonical_value).toBe(secret);
    const snapshot = safeMigrationRuntimeSnapshot(runtime);
    expect('canonical_value' in snapshot).toBe(false);
    expect(JSON.stringify(snapshot)).not.toContain(secret);
  });

  test.each([
    {
      name: 'renamed_compatible without automatic read compatibility',
      disposition: 'renamed_compatible',
      automaticReadCompatibility: false,
      expectedStatus: 'action_required',
    },
    {
      name: 'renamed_requires_user_action',
      disposition: 'renamed_requires_user_action',
      automaticReadCompatibility: true,
      expectedStatus: 'action_required',
    },
    {
      name: 'replaced_by_standard_vscode_setting',
      disposition: 'replaced_by_standard_vscode_setting',
      automaticReadCompatibility: true,
      expectedStatus: 'action_required',
    },
    {
      name: 'replaced_by_server_or_project_config',
      disposition: 'replaced_by_server_or_project_config',
      automaticReadCompatibility: true,
      expectedStatus: 'action_required',
    },
    {
      name: 'unsupported_legacy_value',
      disposition: 'unsupported_legacy_value',
      automaticReadCompatibility: true,
      expectedStatus: 'invalid',
    },
  ] as const)(
    'disposition never silently adopts the legacy value: $name',
    ({ disposition, automaticReadCompatibility, expectedStatus }) => {
      const base = compatibleRegistry();
      const row = base.rows[0];
      if (row === undefined) {
        throw new Error('compatibleRegistry must define one row');
      }
      const registry: ConfigurationMigrationRegistry = {
        ...base,
        rows: [
          {
            ...row,
            migration_disposition: disposition,
            automatic_read_compatibility: automaticReadCompatibility,
          },
        ],
      };

      const result = interpretLegacyConfiguration(registry, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: true,
        current_value_present: false,
        current_value: null,
      });

      // A wrong implementation that returns compatible_legacy with the legacy
      // value fails every row of this table.
      expect(result).toMatchObject({
        status: expectedStatus,
        canonical_value_present: false,
        canonical_value: null,
        notice_required: true,
        disk_write_allowed: false,
      });
    },
  );

  test('deduplicates migration notices per migration and configuration generation', () => {
    const runtime = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'machine',
      legacy_value_present: true,
      legacy_value: [],
      current_value_present: false,
      current_value: null,
    });
    const dedupe = new MigrationNoticeDedupe();

    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(false);
    expect(dedupe.shouldShow(runtime, 'generation-2')).toBe(true);
    dedupe.clear();
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(true);
  });
});
