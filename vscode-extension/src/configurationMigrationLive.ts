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
 *
 * Two limits are worth stating rather than implying.
 *
 * It authorizes by declared scope, not by `security_trust_class`. Today no interpretation
 * can produce a canonical value at all — every row is `removed_inert`, and
 * `unwiredCanonicalValues` reports any that ever does — so nothing repository-controlled
 * can become authority regardless. A registry row that pairs `process_execution` with a
 * repo-settable scope would need that stronger rule before an effective-configuration
 * consumer (#6736) lands; #14968 owns it.
 *
 * And `user` is a *target*, not a provenance: VS Code merges machine/remote settings into
 * the same `globalValue`, and a dev container's `customizations.vscode.settings` is
 * written there. `inspect()` exposes no way to separate the two, so an occurrence at the
 * `user` target is not proof that a human typed it.
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
   * Identity for the owning workspace folder, within one published state.
   *
   * Callers must pass an opaque identity — never a filesystem path — because this value
   * reaches the published migration state, which is redacted by contract.
   *
   * It distinguishes folders inside a single read; it is not stable across changes to the
   * folder set. The host supplies the folder's position, so removing an earlier folder
   * renumbers the rest. A consumer correlating entries across refreshes must re-read the
   * whole state rather than track a folder by this value.
   */
  readonly folderId: string;
  readonly value: unknown;
}

/**
 * Where one legacy key was found. An absent property means "not set at that target".
 *
 * `undefined` is not representable as a stored value here — the host adapter tests
 * `!== undefined` against `inspect()`, and VS Code removes a key rather than storing
 * `undefined` for it. A stored `null` is a value like any other and is carried through.
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

/**
 * The most entries a published state will carry.
 *
 * The occurrence count is `rows x (user + workspace + folders)`, and the folder term is
 * whatever the user has open — nothing in the registry bounds it. Retained state that a
 * profile can grow without limit is not bounded state, so the published array is capped
 * and the remainder is counted rather than hidden.
 *
 * The cap governs only what is *published*. Notices are emitted from the full occurrence
 * list, so no refusal goes unannounced because of it — which matters: refusals are how a
 * `process_execution` setting in repository-controlled configuration is surfaced.
 *
 * 64 is far above any real multi-root workspace that also carries a removed setting in
 * that many folders, so ordinary profiles never truncate.
 */
export const MAX_PUBLISHED_ENTRIES = 64;

/** Bounded, redacted migration state for the whole profile. */
export interface LegacyMigrationState {
  readonly registrySchemaVersion: string;
  readonly registryTargetRelease: string;
  readonly extensionVersion: string;
  readonly entries: readonly LegacyMigrationStateEntry[];
  /** Occurrences past {@link MAX_PUBLISHED_ENTRIES}; every one was still announced. */
  readonly omittedEntryCount: number;
}

interface SiteOccurrence {
  readonly target: ConfigurationTarget;
  readonly folderId: string | null;
  readonly value: unknown;
}

/**
 * Flatten the sites carrying one key into one occurrence per target.
 *
 * Order is fixed (`user`, `workspace`, then folders in the caller's order) so a profile
 * always produces the same state for the same configuration — the published state is
 * fingerprinted to decide whether a notice repeats, and an unstable order would advance
 * that fingerprint on every read.
 */
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
 * reading, and this module refuses to pick: it again reports the target's own scope. The
 * result is order-independent, which is the property that matters — it never resolves to
 * whichever row happens to sort first.
 *
 * It is not, however, uniformly conservative, and the two outcomes differ sharply:
 *
 * - none of the authorizing scopes is the target's own → no row matches →
 *   `scope_not_permitted`. That refusal is honest about ambiguity but reads as the
 *   repository-planted-value signal, which is not what happened.
 * - one of them *is* the target's own → that row applies, and any other authorizing era
 *   for the same key is shadowed. A stricter era (`removed_inert`, say) can therefore be
 *   passed over at the one target where the key can legally sit.
 *
 * Neither is reachable from the shipped registry, which has a single row. #14968 owns
 * giving multi-era keys a real reading before one exists.
 */
function scopeForOccurrence(
  registry: ConfigurationMigrationRegistry,
  legacyKey: string,
  target: ConfigurationTarget,
): MigrationScope {
  const authorizedScopes = new Set<MigrationScope>();
  for (const row of findMigrationRows(registry, legacyKey)) {
    // A JSON-loaded registry can carry a scope this table does not know. Treating it as
    // authorizing nothing sends the occurrence down the target's own scope, where the
    // registry's own validation reports it — rather than throwing on the lookup, which
    // happens before `interpretLegacyConfiguration` gets to validate at all.
    if ((AUTHORIZED_TARGETS[row.old_scope] ?? []).includes(target)) {
      authorizedScopes.add(row.old_scope);
    }
  }
  // `noUncheckedIndexedAccess` widens the destructured element to
  // `MigrationScope | undefined` no matter what `size` says, so the second guard is
  // load-bearing for the compiler rather than redundant with the first.
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
        // (#6736), not here. Hard-coding absence is exact for every shipped row, all of
        // which are `removed_inert` and name no replacement key.
        //
        // For a row that does name one it would be a wrong verdict, not just a narrower
        // one: a user who already migrated would be reported `compatible_legacy` — "the
        // old value is in effect" — where the interpreter would say
        // `compatible_current_wins`. Such a row cannot ship before #6736 gives this seam
        // a current value to pass; #14968 tracks that.
        current_value_present: false,
        current_value: undefined,
        extension_version: extensionVersion,
      });
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

/**
 * Redact one occurrence.
 *
 * The single place the raw interpretation is narrowed to publishable fields. Every
 * outward surface — published state and notice text alike — goes through here, so
 * redaction is a property of the type each surface receives rather than of the care
 * taken inside it.
 */
export function legacyMigrationStateEntry(
  occurrence: LegacyMigrationOccurrence,
): LegacyMigrationStateEntry {
  return {
    ...safeMigrationRuntimeSnapshot(occurrence.runtime),
    target: occurrence.target,
    folderId: occurrence.folderId,
  };
}

/**
 * Redact interpreted occurrences into the state published to support surfaces.
 *
 * Truncation keeps the natural order — per key, the user target, then workspace, then
 * folders as the host listed them — so the entries that survive are the least
 * folder-dependent ones, and the same profile always yields the same published state.
 */
export function legacyMigrationState(
  registry: ConfigurationMigrationRegistry,
  extensionVersion: string,
  occurrences: readonly LegacyMigrationOccurrence[],
): LegacyMigrationState {
  const published = occurrences.slice(0, MAX_PUBLISHED_ENTRIES);
  return {
    registrySchemaVersion: registry.schema_version,
    registryTargetRelease: registry.target_release,
    extensionVersion,
    entries: published.map(legacyMigrationStateEntry),
    omittedEntryCount: occurrences.length - published.length,
  };
}

/**
 * A compact identity for the whole occurrence set.
 *
 * Deliberately derived from every occurrence rather than from the published entries: the
 * published array is capped, so using it to detect change would make two profiles that
 * differ only past the cap look identical and silence the notices for the second one.
 * Carries exactly the fields a notice is built from.
 */
export function migrationStateFingerprint(
  occurrences: readonly LegacyMigrationOccurrence[],
): string {
  return occurrences
    .map((occurrence) => {
      const { runtime } = occurrence;
      return [
        occurrence.legacyKey,
        occurrence.target,
        occurrence.folderId ?? '',
        runtime.status,
        runtime.migration_id ?? '',
        runtime.reason_code ?? '',
        runtime.canonical_key_or_authority ?? '',
        String(runtime.notice_required),
      ].join('\u0001');
    })
    .join('\u0002');
}

const TARGET_DESCRIPTION: Readonly<Record<ConfigurationTarget, string>> = {
  user: 'user settings',
  workspace: 'workspace settings',
  'workspace-folder': 'workspace folder settings',
};

/**
 * One actionable line for the output channel.
 *
 * Takes the redacted entry, not the occurrence: the stored value — for
 * `perl-lsp.mcp.servers`, a list of commands and environment — is then not in scope to
 * be interpolated by accident, rather than merely left out by discipline.
 */
export function describeLegacyMigrationEntry(entry: LegacyMigrationStateEntry): string {
  const where =
    entry.folderId === null
      ? TARGET_DESCRIPTION[entry.target]
      : `${TARGET_DESCRIPTION[entry.target]} (${entry.folderId})`;
  const reason = entry.reason_code === null ? '' : ` [${entry.reason_code}]`;
  // No migration id means no row was selected — the key is unregistered, or it sits at a
  // target its scope does not authorize. Naming a replacement there would assert a
  // migration the registry never made.
  if (entry.migration_id === null) {
    return `\`${entry.legacy_key}\` in ${where} is ${entry.status}.${reason}`;
  }
  const replacement =
    entry.canonical_key_or_authority === null
      ? 'it has no replacement setting'
      : `use \`${entry.canonical_key_or_authority}\` instead`;
  return `\`${entry.legacy_key}\` in ${where} is ${entry.status}: ${replacement}.${reason}`;
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
