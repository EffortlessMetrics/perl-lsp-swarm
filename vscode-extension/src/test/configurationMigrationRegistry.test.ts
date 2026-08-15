import {
  type ConfigurationMigrationRegistry,
  V018_CONFIGURATION_MIGRATIONS,
  findMigrationRows,
  serializeMigrationRegistry,
  validateMigrationRegistry,
} from '../configurationMigrationRegistry';

function cloneRegistry(): ConfigurationMigrationRegistry {
  return JSON.parse(
    JSON.stringify(V018_CONFIGURATION_MIGRATIONS),
  ) as ConfigurationMigrationRegistry;
}

describe('public-beta configuration migration registry', () => {
  test('ships a valid v0.17 to v0.18 registry with the generic MCP setting inert', () => {
    expect(validateMigrationRegistry(V018_CONFIGURATION_MIGRATIONS)).toEqual([]);

    expect(findMigrationRows(V018_CONFIGURATION_MIGRATIONS, 'perl-lsp.mcp.servers')).toEqual([
      expect.objectContaining({
        migration_id: 'v017_mcp_servers_removed',
        migration_disposition: 'removed_inert',
        automatic_read_compatibility: false,
        explicit_write_allowed: false,
        security_trust_class: 'process_execution',
        expiry_version_or_issue: '#7119',
        installed_proof_requirement: '#7841',
      }),
    ]);
  });

  test('serializes deterministically by migration identity', () => {
    const registry = cloneRegistry();
    registry.rows.push({
      ...registry.rows[0]!,
      migration_id: 'aaa_future_row',
      old_key: 'perl-lsp.example.old',
      old_value_shape: 'boolean',
      security_trust_class: 'ordinary',
      migration_disposition: 'deprecated_read_only',
      new_key_or_authority: 'perl-lsp.example.current',
      new_scope: 'resource',
      old_scope: 'resource',
      automatic_read_compatibility: true,
      old_plus_new_conflict_policy: 'current_wins',
      expiry_version_or_issue: '#9999',
      installed_proof_requirement: '#9998',
    });

    const first = serializeMigrationRegistry(registry);
    const second = serializeMigrationRegistry(registry);

    expect(first).toBe(second);
    expect(first.indexOf('aaa_future_row')).toBeLessThan(first.indexOf('v017_mcp_servers_removed'));
  });

  test('rejects duplicate exact historical subjects', () => {
    const registry = cloneRegistry();
    registry.rows.push({
      ...registry.rows[0]!,
      migration_id: 'duplicate_subject',
    });

    expect(validateMigrationRegistry(registry)).toContain(
      'overlapping historical migration subject: perl-lsp.mcp.servers',
    );
  });

  test('rejects read or write compatibility for removed inert settings', () => {
    const registry = cloneRegistry();
    registry.rows[0] = {
      ...registry.rows[0]!,
      automatic_read_compatibility: true,
      explicit_write_allowed: true,
    };

    expect(validateMigrationRegistry(registry)).toContain(
      'removed_inert migration cannot retain read/write compatibility: v017_mcp_servers_removed',
    );
  });

  test('rejects sensitive scope widening', () => {
    const registry = cloneRegistry();
    registry.rows[0] = {
      ...registry.rows[0]!,
      migration_disposition: 'renamed_requires_user_action',
      new_key_or_authority: 'perl-lsp.mcp.replacement',
      new_scope: 'workspace',
      automatic_read_compatibility: false,
      explicit_write_allowed: false,
      old_plus_new_conflict_policy: 'action_required',
    };

    expect(validateMigrationRegistry(registry)).toContain(
      'sensitive migration cannot widen configuration authority: v017_mcp_servers_removed',
    );
  });

  test('rejects a sensitive relocation that hides its authority behind a null scope', () => {
    // Without an explicit new_scope the widening comparison is skipped entirely, so a
    // sensitive row could name a repository-controlled authority and still validate —
    // certifying a move of execution-sensitive configuration from machine scope to
    // project authority.
    const registry = cloneRegistry();
    registry.rows[0] = {
      ...registry.rows[0]!,
      migration_disposition: 'replaced_by_server_or_project_config',
      new_key_or_authority: '.perl-lsp.toml',
      new_scope: null,
      automatic_read_compatibility: false,
      explicit_write_allowed: false,
      old_plus_new_conflict_policy: 'action_required',
    };

    expect(validateMigrationRegistry(registry)).toContain(
      'sensitive migration must declare new_scope unless authority is retired: v017_mcp_servers_removed',
    );
  });

  test('still allows a null scope when the disposition retires the authority', () => {
    const registry = cloneRegistry();
    expect(registry.rows[0]!.security_trust_class).not.toBe('ordinary');
    expect(registry.rows[0]!.new_scope).toBeNull();
    expect(registry.rows[0]!.migration_disposition).toBe('removed_inert');

    expect(validateMigrationRegistry(registry)).toEqual([]);
  });

  test('orders rows by code point rather than host collation', () => {
    // localeCompare is host-dependent: Swedish collation orders 'ä' after 'z',
    // English does not. A locale-sensitive sort would serialize the same registry
    // differently on CI and on a developer machine.
    const registry = cloneRegistry();
    const base = registry.rows[0]!;
    registry.rows = [
      { ...base, migration_id: 'zz_row' },
      { ...base, migration_id: 'ää_row' },
      { ...base, migration_id: 'aa_row' },
    ];

    const order = (
      JSON.parse(serializeMigrationRegistry(registry)) as ConfigurationMigrationRegistry
    ).rows.map((row) => row.migration_id);

    expect(order).toEqual(['aa_row', 'zz_row', 'ää_row']);
  });
});
