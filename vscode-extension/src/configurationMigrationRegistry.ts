export type MigrationDisposition =
  | 'unchanged'
  | 'renamed_compatible'
  | 'renamed_requires_user_action'
  | 'deprecated_read_only'
  | 'removed_inert'
  | 'replaced_by_standard_vscode_setting'
  | 'replaced_by_server_or_project_config'
  | 'unsupported_legacy_value';

export type MigrationScope =
  | 'user'
  | 'workspace'
  | 'workspace-folder'
  | 'resource'
  | 'machine'
  | 'machine-overridable';

export type MigrationSecurityClass = 'ordinary' | 'machine_sensitive' | 'process_execution';

export interface ConfigurationMigrationRow {
  migration_id: string;
  old_key: string;
  old_value_shape: string;
  introduced_version: string;
  last_supported_version: string;
  new_key_or_authority: string | null;
  old_scope: MigrationScope;
  new_scope: MigrationScope | null;
  migration_disposition: MigrationDisposition;
  automatic_read_compatibility: boolean;
  explicit_write_allowed: boolean;
  old_plus_new_conflict_policy:
    | 'current_wins'
    | 'legacy_only'
    | 'action_required'
    | 'not_applicable';
  security_trust_class: MigrationSecurityClass;
  warning_reason_code: string;
  expiry_version_or_issue: string;
  installed_proof_requirement: string;
}

export interface ConfigurationMigrationRegistry {
  schema_version: 'vscode_configuration_migration.v1';
  target_release: string;
  source_public_release: string;
  rows: ConfigurationMigrationRow[];
}

/// Dispositions under which a setting keeps no configuration authority at all,
/// and a null `new_scope` is therefore the truthful value rather than an omission.
const AUTHORITY_RETIRING_DISPOSITIONS: ReadonlySet<MigrationDisposition> = new Set([
  'removed_inert',
  'unsupported_legacy_value',
]);

const SCOPE_RANK: Record<MigrationScope, number> = {
  resource: 0,
  'workspace-folder': 1,
  workspace: 2,
  user: 3,
  'machine-overridable': 4,
  machine: 5,
};

export const V018_CONFIGURATION_MIGRATIONS: ConfigurationMigrationRegistry = {
  schema_version: 'vscode_configuration_migration.v1',
  target_release: '0.18.0',
  source_public_release: '0.17.0',
  rows: [
    {
      migration_id: 'v017_mcp_servers_removed',
      old_key: 'perl-lsp.mcp.servers',
      old_value_shape: 'array<object{label,command,args?,cwd?,env?,version?,enabled?}>',
      introduced_version: '0.17.0',
      last_supported_version: '0.17.x',
      new_key_or_authority: null,
      old_scope: 'machine',
      new_scope: null,
      migration_disposition: 'removed_inert',
      automatic_read_compatibility: false,
      explicit_write_allowed: false,
      old_plus_new_conflict_policy: 'not_applicable',
      security_trust_class: 'process_execution',
      warning_reason_code: 'legacy_mcp_passthrough_removed',
      expiry_version_or_issue: '#7119',
      installed_proof_requirement: '#7841',
    },
  ],
};

export function normalizedMigrationRegistry(
  registry: ConfigurationMigrationRegistry,
): ConfigurationMigrationRegistry {
  return {
    ...registry,
    // Code-point ordering, not localeCompare: host collation differs between
    // environments (Swedish orders 'ä' after 'z', English does not), so a
    // locale-sensitive sort would serialize the same registry into different row
    // orders on CI and on a developer machine.
    rows: [...registry.rows].sort((left, right) =>
      left.migration_id < right.migration_id ? -1 : left.migration_id > right.migration_id ? 1 : 0,
    ),
  };
}

export function serializeMigrationRegistry(registry: ConfigurationMigrationRegistry): string {
  return `${JSON.stringify(normalizedMigrationRegistry(registry), null, 2)}\n`;
}

export function findMigrationRows(
  registry: ConfigurationMigrationRegistry,
  oldKey: string,
): ConfigurationMigrationRow[] {
  return registry.rows.filter((row) => row.old_key === oldKey);
}

export function validateMigrationRegistry(registry: ConfigurationMigrationRegistry): string[] {
  const errors: string[] = [];
  const migrationIds = new Set<string>();
  const exactHistoricalSubjects = new Set<string>();

  for (const row of registry.rows) {
    if (migrationIds.has(row.migration_id)) {
      errors.push(`duplicate migration_id: ${row.migration_id}`);
    }
    migrationIds.add(row.migration_id);

    const exactSubject = `${row.old_key}\u0000${row.introduced_version}\u0000${row.last_supported_version}\u0000${row.old_value_shape}`;
    if (exactHistoricalSubjects.has(exactSubject)) {
      errors.push(`overlapping historical migration subject: ${row.old_key}`);
    }
    exactHistoricalSubjects.add(exactSubject);

    if (row.migration_disposition === 'removed_inert') {
      if (row.new_key_or_authority !== null || row.new_scope !== null) {
        errors.push(`removed_inert migration must not name a new authority: ${row.migration_id}`);
      }
      if (row.automatic_read_compatibility || row.explicit_write_allowed) {
        errors.push(
          `removed_inert migration cannot retain read/write compatibility: ${row.migration_id}`,
        );
      }
      if (row.old_plus_new_conflict_policy !== 'not_applicable') {
        errors.push(
          `removed_inert migration must use not_applicable conflict policy: ${row.migration_id}`,
        );
      }
    }

    if (row.security_trust_class !== 'ordinary') {
      if (row.new_scope === null) {
        // A null new_scope skips the scope-widening comparison entirely. That is
        // only honest when the setting genuinely retains no authority. Otherwise a
        // sensitive row could name a repository-controlled authority via
        // `replaced_by_server_or_project_config`, leave new_scope null, and be
        // certified valid while moving execution-sensitive configuration from
        // machine scope to project authority.
        if (!AUTHORITY_RETIRING_DISPOSITIONS.has(row.migration_disposition)) {
          errors.push(
            `sensitive migration must declare new_scope unless authority is retired: ${row.migration_id}`,
          );
        }
      } else if (SCOPE_RANK[row.new_scope] < SCOPE_RANK[row.old_scope]) {
        errors.push(
          `sensitive migration cannot widen configuration authority: ${row.migration_id}`,
        );
      }
    }

    if (row.warning_reason_code.length === 0) {
      errors.push(`migration must define warning_reason_code: ${row.migration_id}`);
    }
    if (row.expiry_version_or_issue.length === 0) {
      errors.push(`migration must define expiry owner: ${row.migration_id}`);
    }
    if (row.installed_proof_requirement.length === 0) {
      errors.push(`migration must define installed proof requirement: ${row.migration_id}`);
    }
  }

  return errors;
}
