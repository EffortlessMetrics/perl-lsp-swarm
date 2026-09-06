/**
 * Live compatibility reader for registered legacy settings (#14966, under #7838).
 *
 * `configurationMigrationRegistry.ts` owns *what* each historical setting means and
 * `configurationMigrationRuntime.ts` owns *how* a disposition, conflict rule, or expiry
 * threshold resolves. Both were proven but unreachable: nothing in the shipped extension
 * called them, so a user still carrying a removed setting saw no state at all.
 *
 * This module owns exactly one judgment those two cannot make: **which VS Code
 * configuration target is an authorized home for a registry row's declared scope**. The
 * host reports where a value physically sits (`inspect()`); the registry declares where
 * the value was ever allowed to have authority. Reconciling those two is the whole job.
 *
 * It is deliberately free of `vscode` imports so the authorization rule can be falsified
 * without an extension host; `configurationMigrationHost.ts` owns the host adapter.
 */

import type {
  ConfigurationMigrationRegistry,
  MigrationScope,
} from './configurationMigrationRegistry';
import { findMigrationRows } from './configurationMigrationRegistry';
import type {
  MigrationRuntimeResult,
  SafeMigrationRuntimeSnapshot,
} from './configurationMigrationRuntime';
import {
  interpretLegacyConfiguration,
  safeMigrationRuntimeSnapshot,
} from './configurationMigrationRuntime';

/**
 * Where a value physically sits in the user's configuration, as VS Code reports it.
 *
 * This is a *location*, not an authority: `inspect()` happily reports a `machine`-scoped
 * key that someone wrote into a repository's `.vscode/settings.json`, which is precisely
 * the case the authorization table below exists to refuse.
 */
export type ConfigurationTarget = 'user' | 'workspace' | 'workspace-folder';

/**
 * Targets that may legitimately carry a value for each registry scope.
 *
 * `machine` and `user` live in user settings only — that is what makes them
 * installation-owned rather than repository-owned. `machine-overridable` and `resource`
 * are genuinely settable at any target. `workspace` and `workspace-folder` name their
 * own target.
 *
 * This table is the security-load-bearing part of the module: `perl-lsp.mcp.servers` is
 * `machine` + `process_execution`, so a copy planted in repository-controlled workspace
 * configuration must not resolve to its registry row.
 */
const AUTHORIZED_TARGETS: Readonly<Record<MigrationScope, readonly ConfigurationTarget[]>> = {
  machine: ['user'],
  user: ['user'],
  'machine-overridable': ['user', 'workspace', 'workspace-folder'],
  resource: ['user', 'workspace', 'workspace-folder'],
  workspace: ['workspace'],
  'workspace-folder': ['workspace-folder'],
};

/** The scope a target speaks for when no registry row authorizes that target. */
const TARGET_OWN_SCOPE: Readonly<Record<ConfigurationTarget, MigrationScope>> = {
  user: 'user',
  workspace: 'workspace',
  'workspace-folder': 'workspace-folder',
};

/** Targets at which a value declared under `scope` may legitimately be found. */
export function authorizedTargetsForScope(scope: MigrationScope): readonly ConfigurationTarget[] {
  return AUTHORIZED_TARGETS[scope];
}

/** One folder-scoped occurrence of a legacy value. */
export interface LegacyFolderValue {
  /**
   * Stable identity for the owning workspace folder.
   *
   * Callers must pass an opaque identity — never a filesystem path — because this value
   * reaches the published migration state, which is redacted by contract.
   */
  readonly folderId: string;
  readonly value: unknown;
}

/**
 * Where one legacy key was found. An absent property means "not set at that target",
 * which is not the same as "set to undefined"; the host adapter distinguishes them.
 */
export interface LegacyConfigurationSites {
  readonly user?: { readonly value: unknown };
  readonly workspace?: { readonly value: unknown };
  readonly workspaceFolders?: readonly LegacyFolderValue[];
}

/** Resolves the sites carrying one fully qualified legacy key. */
export type LegacyConfigurationSiteReader = (key: string) => LegacyConfigurationSites;

/** One interpreted occurrence: a legacy key at one target, with the runtime verdict. */
export interface LegacyMigrationOccurrence {
  readonly legacyKey: string;
  readonly target: ConfigurationTarget;
  /** Owning folder for `workspace-folder` occurrences; `null` at every other target. */
  readonly folderId: string | null;
  readonly runtime: MigrationRuntimeResult;
}

/** Redacted projection of one occurrence, safe for status, doctor, and support surfaces. */
export interface LegacyMigrationStateEntry extends SafeMigrationRuntimeSnapshot {
  readonly target: ConfigurationTarget;
  readonly folderId: string | null;
}

/** Bounded, redacted migration state for the whole profile. */
export interface LegacyMigrationState {
  readonly registrySchemaVersion: string;
  readonly registryTargetRelease: string;
  readonly extensionVersion: string;
  readonly entries: readonly LegacyMigrationStateEntry[];
}

interface SiteOccurrence {
  readonly target: ConfigurationTarget;
  readonly folderId: string | null;
  readonly value: unknown;
}

function siteOccurrences(sites: LegacyConfigurationSites): readonly SiteOccurrence[] {
  const occurrences: SiteOccurrence[] = [];
  if (sites.user !== undefined) {
    occurrences.push({ target: 'user', folderId: null, value: sites.user.value });
  }
  if (sites.workspace !== undefined) {
    occurrences.push({ target: 'workspace', folderId: null, value: sites.workspace.value });
  }
  for (const folder of sites.workspaceFolders ?? []) {
    occurrences.push({
      target: 'workspace-folder',
      folderId: folder.folderId,
      value: folder.value,
    });
  }
  return occurrences;
}

/**
 * The scope to interpret an occurrence under.
 *
 * When exactly one registry scope for this key authorizes this target, that scope is the
 * declared authority and the interpreter applies the row's ordinary disposition. When
 * none does, the occurrence is reported under the target's own scope so the interpreter
 * reaches `legacy_key_scope_not_permitted` from the registry rather than from a verdict
 * invented here.
 *
 * When *several* scopes authorize the same target the registry offers more than one
 * reading, and this module refuses to pick: it again reports the target's own scope, so
 * the outcome is either the row that names that exact scope or `scope_not_permitted`.
 * That is deliberately conservative — a registry whose authorizing scopes are all
 * indirect fails closed rather than resolving to whichever row happens to sort first.
 */
function scopeForOccurrence(
  registry: ConfigurationMigrationRegistry,
  legacyKey: string,
  target: ConfigurationTarget,
): MigrationScope {
  const authorizedScopes = new Set<MigrationScope>();
  for (const row of findMigrationRows(registry, legacyKey)) {
    if (AUTHORIZED_TARGETS[row.old_scope].includes(target)) {
      authorizedScopes.add(row.old_scope);
    }
  }
  const [onlyScope] = [...authorizedScopes];
  if (authorizedScopes.size === 1 && onlyScope !== undefined) {
    return onlyScope;
  }
  return TARGET_OWN_SCOPE[target];
}

/**
 * Interpret every registered legacy key against the profile's actual configuration.
 *
 * One result per occurrence: a key present in both user and workspace settings is two
 * verdicts, and a folder-scoped value stays bound to the folder that owns it.
 */
export function readLegacyConfiguration(
  registry: ConfigurationMigrationRegistry,
  extensionVersion: string,
  readSites: LegacyConfigurationSiteReader,
): readonly LegacyMigrationOccurrence[] {
  const occurrences: LegacyMigrationOccurrence[] = [];
  const seenKeys = new Set<string>();

  for (const row of registry.rows) {
    if (seenKeys.has(row.old_key)) {
      continue;
    }
    seenKeys.add(row.old_key);

    for (const site of siteOccurrences(readSites(row.old_key))) {
      const runtime = interpretLegacyConfiguration(registry, {
        old_key: row.old_key,
        source_scope: scopeForOccurrence(registry, row.old_key, site.target),
        legacy_value_present: true,
        legacy_value: site.value,
        // A legacy key's *current* replacement is read by the authority that owns it
        // (#6736), not here. This reader publishes no canonical value, so it cannot
        // resolve an old-versus-new conflict and does not claim to.
        current_value_present: false,
        current_value: undefined,
        extension_version: extensionVersion,
      });
      if (runtime.status === 'not_applicable') {
        continue;
      }
      occurrences.push({
        legacyKey: row.old_key,
        target: site.target,
        folderId: site.folderId,
        runtime,
      });
    }
  }

  return occurrences;
}

/** Redact interpreted occurrences into the state published to support surfaces. */
export function legacyMigrationState(
  registry: ConfigurationMigrationRegistry,
  extensionVersion: string,
  occurrences: readonly LegacyMigrationOccurrence[],
): LegacyMigrationState {
  return {
    registrySchemaVersion: registry.schema_version,
    registryTargetRelease: registry.target_release,
    extensionVersion,
    entries: occurrences.map((occurrence) => ({
      ...safeMigrationRuntimeSnapshot(occurrence.runtime),
      target: occurrence.target,
      folderId: occurrence.folderId,
    })),
  };
}

const TARGET_DESCRIPTION: Readonly<Record<ConfigurationTarget, string>> = {
  user: 'user settings',
  workspace: 'workspace settings',
  'workspace-folder': 'workspace folder settings',
};

/**
 * One actionable line for the output channel.
 *
 * Names the setting, where it was found, and what replaces it — never the stored value,
 * which for `perl-lsp.mcp.servers` is a list of commands and environment.
 */
export function describeLegacyMigrationOccurrence(occurrence: LegacyMigrationOccurrence): string {
  const { runtime } = occurrence;
  const where =
    occurrence.folderId === null
      ? TARGET_DESCRIPTION[occurrence.target]
      : `${TARGET_DESCRIPTION[occurrence.target]} (${occurrence.folderId})`;
  const reason = runtime.reason_code === null ? '' : ` [${runtime.reason_code}]`;
  // No migration id means no row was selected — the key is unregistered, or it sits at a
  // target its scope does not authorize. Naming a replacement there would assert a
  // migration the registry never made.
  if (runtime.migration_id === null) {
    return `\`${occurrence.legacyKey}\` in ${where} is ${runtime.status}.${reason}`;
  }
  const replacement =
    runtime.canonical_key_or_authority === null
      ? 'it has no replacement setting'
      : `use \`${runtime.canonical_key_or_authority}\` instead`;
  return `\`${occurrence.legacyKey}\` in ${where} is ${runtime.status}: ${replacement}.${reason}`;
}

/**
 * Legacy-derived values this reader interpreted as canonical current configuration.
 *
 * Publishing one is #6736/#7838 work that this seam does not own, so the set must stay
 * empty. It is returned rather than dropped so a registry row that starts producing a
 * canonical value fails loudly here instead of being silently ignored.
 */
export function unwiredCanonicalValues(
  occurrences: readonly LegacyMigrationOccurrence[],
): readonly LegacyMigrationOccurrence[] {
  return occurrences.filter((occurrence) => occurrence.runtime.canonical_value_present);
}
