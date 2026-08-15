import type {
  ConfigurationMigrationRegistry,
  ConfigurationMigrationRow,
  MigrationScope,
} from './configurationMigrationRegistry';
import { findMigrationRows } from './configurationMigrationRegistry';

export type MigrationRuntimeStatus =
  | 'not_applicable'
  | 'compatible_legacy'
  | 'compatible_current_wins'
  | 'action_required'
  | 'inert'
  | 'invalid';

export interface MigrationRuntimeInput {
  old_key: string;
  source_scope: MigrationScope;
  legacy_value_present: boolean;
  legacy_value: unknown;
  current_value_present: boolean;
  current_value: unknown;
}

export interface MigrationRuntimeResult {
  migration_id: string | null;
  /** The legacy configuration key this result interprets. Always a setting name, never a value. */
  legacy_key: string;
  status: MigrationRuntimeStatus;
  source_scope: MigrationScope;
  canonical_key_or_authority: string | null;
  canonical_value_present: boolean;
  canonical_value: unknown;
  reason_code: string | null;
  notice_required: boolean;
  disk_write_allowed: boolean;
}

export interface SafeMigrationRuntimeSnapshot {
  migration_id: string | null;
  legacy_key: string;
  status: MigrationRuntimeStatus;
  source_scope: MigrationScope;
  canonical_key_or_authority: string | null;
  reason_code: string | null;
  notice_required: boolean;
}

const MISSING_VALUE = Symbol('configuration-migration-missing');

function result(
  input: MigrationRuntimeInput,
  row: ConfigurationMigrationRow | null,
  status: MigrationRuntimeStatus,
  canonicalValue: unknown = MISSING_VALUE,
  noticeRequired = false,
): MigrationRuntimeResult {
  return {
    migration_id: row?.migration_id ?? null,
    legacy_key: input.old_key,
    status,
    source_scope: input.source_scope,
    canonical_key_or_authority: row?.new_key_or_authority ?? null,
    canonical_value_present: canonicalValue !== MISSING_VALUE,
    canonical_value: canonicalValue === MISSING_VALUE ? null : canonicalValue,
    reason_code: row?.warning_reason_code ?? null,
    notice_required: noticeRequired,
    disk_write_allowed: row?.explicit_write_allowed ?? false,
  };
}

function selectMigrationRow(
  registry: ConfigurationMigrationRegistry,
  input: MigrationRuntimeInput,
): ConfigurationMigrationRow | null {
  const rows = findMigrationRows(registry, input.old_key).filter(
    (row) => row.old_scope === input.source_scope,
  );
  if (rows.length !== 1) {
    return null;
  }
  return rows[0] ?? null;
}

export function interpretLegacyConfiguration(
  registry: ConfigurationMigrationRegistry,
  input: MigrationRuntimeInput,
): MigrationRuntimeResult {
  if (!input.legacy_value_present) {
    return result(input, null, 'not_applicable');
  }

  const row = selectMigrationRow(registry, input);
  if (!row) {
    return result(input, null, 'invalid', MISSING_VALUE, true);
  }

  switch (row.migration_disposition) {
    case 'unchanged':
    case 'renamed_compatible':
    case 'deprecated_read_only': {
      if (input.current_value_present) {
        // Only an explicit `legacy_only` policy lets a historical value outrank a value
        // the user set under the current key. Every other policy — including
        // `not_applicable`, which states no conflict rule at all — resolves toward the
        // current key, because silently preferring the legacy value is the unsafe
        // direction and is invisible to the user.
        if (row.old_plus_new_conflict_policy === 'action_required') {
          return result(input, row, 'action_required', MISSING_VALUE, true);
        }
        if (row.old_plus_new_conflict_policy !== 'legacy_only') {
          return result(input, row, 'compatible_current_wins', input.current_value);
        }
      }
      if (!row.automatic_read_compatibility) {
        return result(input, row, 'action_required', MISSING_VALUE, true);
      }
      return result(input, row, 'compatible_legacy', input.legacy_value, true);
    }

    case 'removed_inert':
      return result(input, row, 'inert', MISSING_VALUE, true);

    case 'renamed_requires_user_action':
    case 'replaced_by_standard_vscode_setting':
    case 'replaced_by_server_or_project_config':
      return result(input, row, 'action_required', MISSING_VALUE, true);

    case 'unsupported_legacy_value':
      return result(input, row, 'invalid', MISSING_VALUE, true);
  }
}

export function safeMigrationRuntimeSnapshot(
  runtime: MigrationRuntimeResult,
): SafeMigrationRuntimeSnapshot {
  return {
    migration_id: runtime.migration_id,
    legacy_key: runtime.legacy_key,
    status: runtime.status,
    source_scope: runtime.source_scope,
    canonical_key_or_authority: runtime.canonical_key_or_authority,
    reason_code: runtime.reason_code,
    notice_required: runtime.notice_required,
  };
}

export class MigrationNoticeDedupe {
  private readonly shown = new Set<string>();

  public shouldShow(runtime: MigrationRuntimeResult, configurationGeneration: string): boolean {
    if (!runtime.notice_required) {
      return false;
    }
    // Unregistered or wrong-scope legacy keys still require a notice but carry no
    // migration identity, so they dedupe on the legacy key instead. Keying only on
    // `migration_id` made every `invalid` notice permanently unshowable.
    const subject = runtime.migration_id ?? `legacy_key:${runtime.legacy_key}`;
    const identity = `${subject}\u0000${configurationGeneration}`;
    if (this.shown.has(identity)) {
      return false;
    }
    this.shown.add(identity);
    return true;
  }

  public clear(): void {
    this.shown.clear();
  }
}
