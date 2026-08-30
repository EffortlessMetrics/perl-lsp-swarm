import { parseStrictSemver, type ParsedSemver } from './strictSemver';

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

function isMigrationDisposition(value: unknown): value is MigrationDisposition {
  switch (value) {
    case 'unchanged':
    case 'renamed_compatible':
    case 'renamed_requires_user_action':
    case 'deprecated_read_only':
    case 'removed_inert':
    case 'replaced_by_standard_vscode_setting':
    case 'replaced_by_server_or_project_config':
    case 'unsupported_legacy_value':
      return true;
    default:
      return false;
  }
}

function isMigrationScope(value: unknown): value is MigrationScope {
  switch (value) {
    case 'user':
    case 'workspace':
    case 'workspace-folder':
    case 'resource':
    case 'machine':
    case 'machine-overridable':
      return true;
    default:
      return false;
  }
}

function isConflictPolicy(
  value: unknown,
): value is ConfigurationMigrationRow['old_plus_new_conflict_policy'] {
  switch (value) {
    case 'current_wins':
    case 'legacy_only':
    case 'action_required':
    case 'not_applicable':
      return true;
    default:
      return false;
  }
}

function isMigrationSecurityClass(value: unknown): value is MigrationSecurityClass {
  switch (value) {
    case 'ordinary':
    case 'machine_sensitive':
    case 'process_execution':
      return true;
    default:
      return false;
  }
}

export type CompatibilityWindow =
  | { kind: 'no_expiry' }
  | {
      kind: 'through_extension_version';
      version: string;
      post_expiry_disposition: 'action_required' | 'invalid' | 'inert';
    }
  | {
      kind: 'removed_in_extension_version';
      version: string;
      post_expiry_disposition: 'action_required' | 'invalid' | 'inert';
    };

export type MigrationVersion = ParsedSemver;

/** Parse the exact SemVer subset accepted by both registry validation and runtime expiry. */
export function parseMigrationVersion(value: unknown): MigrationVersion | null {
  return parseStrictSemver(value);
}

/** Keep JSON-loaded or future registry variants from becoming runtime policy by accident. */
export function isValidCompatibilityWindow(value: unknown): value is CompatibilityWindow {
  if (typeof value !== 'object' || value === null || !('kind' in value)) return false;
  const window = value as { kind?: unknown; version?: unknown; post_expiry_disposition?: unknown };
  if (window.kind === 'no_expiry') return true;
  return (
    (window.kind === 'through_extension_version' ||
      window.kind === 'removed_in_extension_version') &&
    parseMigrationVersion(window.version) !== null &&
    (window.post_expiry_disposition === 'action_required' ||
      window.post_expiry_disposition === 'invalid' ||
      window.post_expiry_disposition === 'inert')
  );
}

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
  compatibility_window: CompatibilityWindow;
  expiry_owner_issue: number | null;
  installed_proof_requirement: string;
}

export interface ConfigurationMigrationRegistry {
  schema_version: 'vscode_configuration_migration.v2';
  target_release: string;
  source_public_release: string;
  rows: ConfigurationMigrationRow[];
}

/** Reject missing or future registry envelopes before their contents become policy. */
export function isSupportedMigrationRegistry(
  value: unknown,
): value is ConfigurationMigrationRegistry {
  if (typeof value !== 'object' || value === null) return false;
  const registry = value as {
    schema_version?: unknown;
    target_release?: unknown;
    source_public_release?: unknown;
    rows?: unknown;
  };
  return (
    registry.schema_version === 'vscode_configuration_migration.v2' &&
    parseMigrationVersion(registry.target_release) !== null &&
    parseMigrationVersion(registry.source_public_release) !== null &&
    Array.isArray(registry.rows)
  );
}

function isMigrationRowShape(value: unknown): value is ConfigurationMigrationRow {
  if (typeof value !== 'object' || value === null) return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.migration_id === 'string' &&
    typeof row.old_key === 'string' &&
    typeof row.old_value_shape === 'string' &&
    typeof row.introduced_version === 'string' &&
    typeof row.last_supported_version === 'string' &&
    (typeof row.new_key_or_authority === 'string' || row.new_key_or_authority === null) &&
    isMigrationScope(row.old_scope) &&
    (isMigrationScope(row.new_scope) || row.new_scope === null) &&
    isMigrationDisposition(row.migration_disposition) &&
    typeof row.automatic_read_compatibility === 'boolean' &&
    typeof row.explicit_write_allowed === 'boolean' &&
    isConflictPolicy(row.old_plus_new_conflict_policy) &&
    isMigrationSecurityClass(row.security_trust_class) &&
    typeof row.warning_reason_code === 'string' &&
    'compatibility_window' in row &&
    (typeof row.expiry_owner_issue === 'number' || row.expiry_owner_issue === null) &&
    typeof row.installed_proof_requirement === 'string'
  );
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
  schema_version: 'vscode_configuration_migration.v2',
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
      compatibility_window: { kind: 'no_expiry' },
      expiry_owner_issue: 7119,
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

function canonicalizeJson(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalizeJson);
  if (typeof value !== 'object' || value === null) return value;

  const canonical: Record<string, unknown> = {};
  for (const key of Object.keys(value).sort()) {
    canonical[key] = canonicalizeJson((value as Record<string, unknown>)[key]);
  }
  return canonical;
}

export function serializeMigrationRegistry(registry: ConfigurationMigrationRegistry): string {
  return `${JSON.stringify(canonicalizeJson(normalizedMigrationRegistry(registry)), null, 2)}\n`;
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

  const candidate: unknown = registry;
  if (!isSupportedMigrationRegistry(candidate)) {
    errors.push('migration registry envelope is missing or unsupported');
    return errors;
  }

  for (const candidateRow of registry.rows as unknown[]) {
    if (!isMigrationRowShape(candidateRow)) {
      errors.push('migration row is missing required fields');
      continue;
    }
    const row = candidateRow;
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
    if (
      row.expiry_owner_issue !== null &&
      (!Number.isSafeInteger(row.expiry_owner_issue) || row.expiry_owner_issue <= 0)
    ) {
      errors.push(`migration expiry owner must be a positive issue number: ${row.migration_id}`);
    }
    const compatibilityWindow: unknown = row.compatibility_window;
    if (!isValidCompatibilityWindow(compatibilityWindow)) {
      if (
        typeof compatibilityWindow === 'object' &&
        compatibilityWindow !== null &&
        'kind' in compatibilityWindow &&
        compatibilityWindow.kind !== 'no_expiry' &&
        'version' in compatibilityWindow &&
        parseMigrationVersion(compatibilityWindow.version) === null
      ) {
        errors.push(`migration expiry version is not valid SemVer: ${row.migration_id}`);
      } else {
        errors.push(`migration compatibility window is not valid: ${row.migration_id}`);
      }
    }
    if (
      isValidCompatibilityWindow(row.compatibility_window) &&
      row.migration_disposition === 'removed_inert' &&
      row.compatibility_window.kind !== 'no_expiry' &&
      row.compatibility_window.post_expiry_disposition !== 'inert'
    ) {
      errors.push(`removed_inert migration must remain inert after expiry: ${row.migration_id}`);
    }
    if (
      isValidCompatibilityWindow(row.compatibility_window) &&
      row.migration_disposition === 'unsupported_legacy_value' &&
      row.compatibility_window.kind !== 'no_expiry' &&
      row.compatibility_window.post_expiry_disposition !== 'invalid'
    ) {
      errors.push(`unsupported migration must remain invalid after expiry: ${row.migration_id}`);
    }
    if (row.installed_proof_requirement.length === 0) {
      errors.push(`migration must define installed proof requirement: ${row.migration_id}`);
    }
  }

  return errors;
}
