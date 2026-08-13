import {
  type ConfigurationMigrationRegistry,
  V018_CONFIGURATION_MIGRATIONS,
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
