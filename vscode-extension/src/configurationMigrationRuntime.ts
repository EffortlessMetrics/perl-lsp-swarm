import type {
  ConfigurationMigrationRegistry,
  ConfigurationMigrationRow,
  CompatibilityWindow,
  MigrationScope,
} from './configurationMigrationRegistry';
import {
  findMigrationRows,
  isValidCompatibilityWindow,
  parseMigrationVersion,
  validateMigrationRegistry,
} from './configurationMigrationRegistry';
import { compareStrictSemver } from './strictSemver';

export type MigrationRuntimeStatus =
  | 'not_applicable'
  | 'compatible_legacy'
  | 'compatible_current_wins'
  | 'action_required'
  | 'inert'
  | 'expired'
  | 'invalid';

export interface MigrationRuntimeInput {
  old_key: string;
  source_scope: MigrationScope;
  legacy_value_present: boolean;
  legacy_value: unknown;
  current_value_present: boolean;
  current_value: unknown;
  /** Exact running extension version; missing input fails closed for expiry-bearing rows. */
  extension_version?: string;
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
  compatibility_window: CompatibilityWindow;
  post_expiry_disposition: 'action_required' | 'invalid' | 'inert' | null;
}

export interface SafeMigrationRuntimeSnapshot {
  migration_id: string | null;
  legacy_key: string;
  status: MigrationRuntimeStatus;
  source_scope: MigrationScope;
  canonical_key_or_authority: string | null;
  reason_code: string | null;
  notice_required: boolean;
  post_expiry_disposition: 'action_required' | 'invalid' | 'inert' | null;
}

const MISSING_VALUE = Symbol('configuration-migration-missing');

/**
 * Why a legacy key could not be interpreted. All three are `invalid`, but they are not
 * the same event and must not reach the user as the same notice: only the first is
 * caused by the user's own configuration.
 */
export const INVALID_REASON_CODES = {
  /** The key appears nowhere in the registry — a typo, or a setting we never shipped. */
  unregistered: 'legacy_key_not_registered',
  /**
   * The key is registered, but not at the scope it was found in. Security-relevant: this
   * is how a `machine`-scoped, process-executing setting looks when it turns up in
   * repository-controlled workspace configuration.
   */
  scope_not_permitted: 'legacy_key_scope_not_permitted',
  /**
   * Several registry rows claim this key at this scope, so no single interpretation
   * exists. This is a defect in the registry, not in the user's settings.
   */
  ambiguous: 'legacy_registry_ambiguous',
  /** The registry row contains an unknown or malformed compatibility window. */
  registry_invalid: 'legacy_registry_invalid',
  /** The running extension version is absent or malformed, so expiry cannot be established. */
  extension_version_invalid: 'migration_extension_version_invalid',
} as const;

function result(
  input: MigrationRuntimeInput,
  row: ConfigurationMigrationRow | null,
  status: MigrationRuntimeStatus,
  canonicalValue: unknown = MISSING_VALUE,
  noticeRequired = false,
  reasonCode: string | null = row?.warning_reason_code ?? null,
): MigrationRuntimeResult {
  const compatibilityWindow =
    row && isValidCompatibilityWindow(row.compatibility_window)
      ? row.compatibility_window
      : { kind: 'no_expiry' as const };
  return {
    migration_id: row?.migration_id ?? null,
    legacy_key: input.old_key,
    status,
    source_scope: input.source_scope,
    canonical_key_or_authority: row?.new_key_or_authority ?? null,
    canonical_value_present: canonicalValue !== MISSING_VALUE,
    canonical_value: canonicalValue === MISSING_VALUE ? null : canonicalValue,
    reason_code: reasonCode,
    notice_required: noticeRequired,
    disk_write_allowed: row?.explicit_write_allowed ?? false,
    compatibility_window: compatibilityWindow,
    post_expiry_disposition:
      compatibilityWindow.kind === 'no_expiry' ? null : compatibilityWindow.post_expiry_disposition,
  };
}

function isExpired(row: ConfigurationMigrationRow, extensionVersion: string): boolean {
  const current = parseMigrationVersion(extensionVersion);
  const threshold =
    row.compatibility_window.kind === 'no_expiry'
      ? null
      : parseMigrationVersion(row.compatibility_window.version);
  if (!current || !threshold) return false;
  const comparison = compareStrictSemver(current, threshold);
  return row.compatibility_window.kind === 'through_extension_version'
    ? comparison > 0
    : comparison >= 0;
}

type RowSelection =
  | { kind: 'selected'; row: ConfigurationMigrationRow }
  | { kind: keyof typeof INVALID_REASON_CODES };

/**
 * The registry deliberately allows several rows per `old_key` — its uniqueness key spans
 * the version window and value shape, so one setting can carry a row per historical era.
 * This interpreter has no historical-era input and therefore cannot choose between eras, so an
 * ambiguous match is reported as such rather than silently resolved to whichever row
 * happens to sort first.
 */
function selectMigrationRow(
  registry: ConfigurationMigrationRegistry,
  input: MigrationRuntimeInput,
): RowSelection {
  const keyRows = findMigrationRows(registry, input.old_key);
  if (keyRows.length === 0) {
    return { kind: 'unregistered' };
  }

  const scopedRows = keyRows.filter((row) => row.old_scope === input.source_scope);
  const row = scopedRows[0];
  if (row === undefined) {
    return { kind: 'scope_not_permitted' };
  }
  if (scopedRows.length > 1) {
    return { kind: 'ambiguous' };
  }
  return { kind: 'selected', row };
}

export function interpretLegacyConfiguration(
  registry: ConfigurationMigrationRegistry,
  input: MigrationRuntimeInput,
): MigrationRuntimeResult {
  if (!input.legacy_value_present) {
    return result(input, null, 'not_applicable');
  }

  if (validateMigrationRegistry(registry).length > 0) {
    return result(
      input,
      null,
      'invalid',
      MISSING_VALUE,
      true,
      INVALID_REASON_CODES.registry_invalid,
    );
  }

  const selection = selectMigrationRow(registry, input);
  if (selection.kind !== 'selected') {
    return result(
      input,
      null,
      'invalid',
      MISSING_VALUE,
      true,
      INVALID_REASON_CODES[selection.kind],
    );
  }

  const row = selection.row;
  if (!isValidCompatibilityWindow(row.compatibility_window)) {
    return result(
      input,
      row,
      'invalid',
      MISSING_VALUE,
      true,
      INVALID_REASON_CODES.registry_invalid,
    );
  }
  if (
    row.compatibility_window.kind !== 'no_expiry' &&
    parseMigrationVersion(input.extension_version) === null
  ) {
    return result(
      input,
      row,
      'invalid',
      MISSING_VALUE,
      true,
      INVALID_REASON_CODES.extension_version_invalid,
    );
  }
  if (
    row.compatibility_window.kind !== 'no_expiry' &&
    isExpired(row, input.extension_version ?? '')
  ) {
    // Expiry revokes only legacy-derived authority. A value already present at
    // the current key remains canonical, so downstream consumers never have to
    // choose between reporting the expired legacy row and preserving user data.
    const currentValue =
      input.current_value_present &&
      row.migration_disposition !== 'removed_inert' &&
      row.migration_disposition !== 'unsupported_legacy_value'
        ? input.current_value
        : MISSING_VALUE;
    return result(input, row, 'expired', currentValue, true);
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
    post_expiry_disposition: runtime.post_expiry_disposition,
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
