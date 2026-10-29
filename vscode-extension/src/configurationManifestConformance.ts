import {
  migrationEraCoversVersion,
  parseMigrationEra,
  parseMigrationVersion,
  validateMigrationRegistry,
  type ConfigurationMigrationRegistry,
} from './configurationMigrationRegistry';

export type SettingDefinition = Record<string, unknown>;
export type SettingsInventory = Record<string, SettingDefinition>;

/** An exact, unresolved review boundary, never a claim of runtime compatibility. */
export interface ManifestMigrationExclusion {
  key: string;
  before: SettingDefinition;
  after: SettingDefinition | null;
  issue: number;
  reason: string;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Compare all definition fields, including descriptions and nested schema constraints. */
function canonicalJson(value: unknown): string {
  function ordered(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(ordered);
    if (!isObject(value)) return value;
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, ordered(value[key])]),
    );
  }
  return JSON.stringify(ordered(value));
}

/** VS Code's manifest default is window; runtime MigrationScope stays unchanged. */
function scopeOf(definition: SettingDefinition): unknown {
  return definition.scope ?? 'window';
}

/** Fail on ambiguous contributions instead of silently overwriting a duplicate key. */
export function configurationInventory(manifest: unknown): SettingsInventory {
  if (!isObject(manifest) || !isObject(manifest.contributes)) {
    throw new Error('manifest must declare contributes.configuration');
  }
  const configuration = manifest.contributes.configuration;
  const groups = Array.isArray(configuration) ? configuration : [configuration];
  const inventory: SettingsInventory = Object.create(null) as SettingsInventory;
  for (const group of groups) {
    if (!isObject(group) || !isObject(group.properties)) {
      throw new Error('configuration group must declare properties');
    }
    for (const [key, definition] of Object.entries(group.properties)) {
      if (Object.hasOwn(inventory, key)) throw new Error(`duplicate contributed setting: ${key}`);
      if (!isObject(definition)) throw new Error(`invalid contributed setting: ${key}`);
      inventory[key] = definition;
    }
  }
  return inventory;
}

/**
 * Build/test guard for an authentic released manifest and an exact source candidate.
 * This does not run migrations or certify installed UX, trust, or external authorities.
 */
export function checkConfigurationManifestConformance(
  releasedManifest: unknown,
  candidateManifest: unknown,
  registry: ConfigurationMigrationRegistry,
  exclusions: readonly ManifestMigrationExclusion[],
): string[] {
  const errors = validateMigrationRegistry(registry);
  if (errors.length > 0) return errors.sort();
  if (!isObject(releasedManifest) || !isObject(candidateManifest)) {
    return ['released and candidate manifests must be objects'];
  }
  if (
    releasedManifest.name !== candidateManifest.name ||
    releasedManifest.publisher !== candidateManifest.publisher
  )
    errors.push('released and candidate extension identities differ');
  const sourceVersion = parseMigrationVersion(releasedManifest.version);
  if (sourceVersion === null || releasedManifest.version !== registry.source_public_release) {
    return [
      ...errors,
      'released manifest version must match registry source_public_release',
    ].sort();
  }
  let before: SettingsInventory;
  let after: SettingsInventory;
  try {
    before = configurationInventory(releasedManifest);
    after = configurationInventory(candidateManifest);
  } catch (error) {
    return [...errors, error instanceof Error ? error.message : String(error)].sort();
  }
  const changed = new Set(
    Object.keys(before).filter((key) => canonicalJson(before[key]) !== canonicalJson(after[key])),
  );
  const covered = new Map<string, number>();
  const cover = (key: string): void => {
    covered.set(key, (covered.get(key) ?? 0) + 1);
  };

  for (const row of registry.rows) {
    const era = parseMigrationEra(row);
    // Other valid historical eras are not subjects of this release comparison.
    if (!era || !migrationEraCoversVersion(era, sourceVersion)) continue;
    const prior = before[row.old_key];
    const current = after[row.old_key];
    if (!prior) {
      errors.push(`stale migration row for published inventory: ${row.migration_id}`);
      continue;
    }
    cover(row.old_key);
    if (row.old_scope !== scopeOf(prior)) {
      errors.push(`published scope mismatch: ${row.migration_id} (${String(scopeOf(prior))})`);
    }
    switch (row.migration_disposition) {
      case 'removed_inert':
        // Authority may be retired while its contributed deprecated schema remains.
        // Canonical registry validation checks no authority/read/write is retained.
        break;
      case 'unchanged':
        if (!current || canonicalJson(prior) !== canonicalJson(current)) {
          errors.push(`unchanged disposition cannot cover manifest drift: ${row.old_key}`);
        }
        if (row.new_key_or_authority !== row.old_key || row.new_scope !== scopeOf(prior)) {
          errors.push(`unchanged disposition must retain key and scope: ${row.migration_id}`);
        }
        break;
      case 'renamed_compatible':
      case 'renamed_requires_user_action': {
        if (
          row.migration_disposition === 'renamed_requires_user_action' &&
          row.automatic_read_compatibility
        ) {
          errors.push(
            `action-required rename cannot declare automatic read compatibility: ${row.migration_id}`,
          );
        }
        const target =
          row.new_key_or_authority === null ? undefined : after[row.new_key_or_authority];
        if (!target || row.new_key_or_authority === row.old_key) {
          errors.push(`rename target must be a different contributed setting: ${row.migration_id}`);
          break;
        }
        if (row.new_scope !== scopeOf(target)) {
          errors.push(`candidate target scope mismatch: ${row.migration_id}`);
        }
        if (row.migration_disposition === 'renamed_compatible') {
          // Presentation can change on rename; every value constraint/default and
          // scope must remain equal. Unknown future schema fields remain constraints.
          const valueSchema = (definition: SettingDefinition): SettingDefinition =>
            Object.fromEntries(
              Object.entries(definition).filter(
                ([key]) =>
                  ![
                    'description',
                    'markdownDescription',
                    'order',
                    'deprecationMessage',
                    'markdownDeprecationMessage',
                    'enumDescriptions',
                    'markdownEnumDescriptions',
                  ].includes(key),
              ),
            );
          if (canonicalJson(valueSchema(prior)) !== canonicalJson(valueSchema(target))) {
            errors.push(`compatible rename changes value schema or scope: ${row.migration_id}`);
          }
          if (
            current &&
            canonicalJson(valueSchema(prior)) !== canonicalJson(valueSchema(current))
          ) {
            errors.push(
              `compatible retained alias changes value schema or scope: ${row.migration_id}`,
            );
          }
        }
        break;
      }
      default:
        errors.push(
          `disposition needs reviewed runtime/external authority evidence; use an exact issue-linked exclusion until supported: ${row.migration_id}`,
        );
    }
  }

  const excluded = new Set<string>();
  for (const candidate of exclusions as readonly unknown[]) {
    if (
      !isObject(candidate) ||
      typeof candidate.key !== 'string' ||
      !isObject(candidate.before) ||
      !(candidate.after === null || isObject(candidate.after)) ||
      !Number.isSafeInteger(candidate.issue) ||
      Number(candidate.issue) <= 0 ||
      typeof candidate.reason !== 'string' ||
      candidate.reason.trim().length === 0
    ) {
      errors.push(
        'invalid manifest exclusion: exact definitions, positive issue and reason required',
      );
      continue;
    }
    const key = candidate.key;
    if (excluded.has(key)) errors.push(`duplicate manifest exclusion: ${key}`);
    excluded.add(key);
    if (!changed.has(key)) errors.push(`stale manifest exclusion: ${key}`);
    if (
      canonicalJson(candidate.before) !== canonicalJson(before[key]) ||
      canonicalJson(candidate.after) !== canonicalJson(after[key] ?? null)
    ) {
      errors.push(`manifest exclusion definition drift: ${key} (#${Number(candidate.issue)})`);
    }
    cover(key);
  }
  for (const key of changed) {
    const count = covered.get(key) ?? 0;
    if (count !== 1) {
      errors.push(
        `${key}: changed/removed published definition needs exactly one applicable disposition or exact issue-linked exclusion (found ${count})`,
      );
    }
  }
  return errors.sort();
}
