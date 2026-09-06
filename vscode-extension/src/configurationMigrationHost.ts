/**
 * Host adapter for the live legacy-setting reader (#14966, under #7838).
 *
 * `configurationMigrationLive.ts` owns the target/scope authorization rule and knows
 * nothing about VS Code. This module owns the two things that need the extension host:
 * turning `WorkspaceConfiguration.inspect()` into the reader's site model, and emitting
 * bounded, deduplicated notices onto the extension's output channel.
 *
 * It reads configuration and never writes it. Migration compatibility must not rewrite a
 * user's `settings.json`, so no `update()` call belongs anywhere in this file.
 */

import * as vscode from 'vscode';

import type { ConfigurationMigrationRegistry } from './configurationMigrationRegistry';
import { V018_CONFIGURATION_MIGRATIONS } from './configurationMigrationRegistry';
import { MigrationNoticeDedupe } from './configurationMigrationRuntime';
import type {
  LegacyConfigurationSites,
  LegacyFolderValue,
  LegacyMigrationOccurrence,
  LegacyMigrationState,
} from './configurationMigrationLive';
import {
  describeLegacyMigrationEntry,
  legacyMigrationState,
  legacyMigrationStateEntry,
  readLegacyConfiguration,
  unwiredCanonicalValues,
} from './configurationMigrationLive';

/** Where a notice line is written. Narrowed to what this module may do to a channel. */
export interface MigrationNoticeSink {
  warn(message: string): void;
  error(message: string): void;
}

/**
 * Opaque per-session identity for a workspace folder.
 *
 * Deliberately the folder's index rather than its name or path: this identity reaches
 * the published migration state, which must carry no filesystem path.
 */
function folderIdentity(index: number): string {
  return `folder:${index}`;
}

/**
 * The three ordinary configuration targets, and deliberately not the language-override
 * variants (`globalLanguageValue` and friends).
 *
 * VS Code only honours a language override for a setting whose declared scope is
 * language-overridable, and no registry row is: the one current row is `machine`, which
 * cannot be overridden per language at all. Reading those fields would therefore report
 * a value the host itself ignores, and the registry's scope vocabulary has no per-language
 * target to judge it under. A future language-overridable row needs that vocabulary first.
 */
function inspectLegacyKey(key: string): LegacyConfigurationSites {
  const rootInspection = vscode.workspace.getConfiguration().inspect(key);
  const workspaceFolders: LegacyFolderValue[] = [];

  (vscode.workspace.workspaceFolders ?? []).forEach((folder, index) => {
    const scoped = vscode.workspace.getConfiguration(undefined, folder.uri).inspect(key);
    if (scoped?.workspaceFolderValue !== undefined) {
      workspaceFolders.push({
        folderId: folderIdentity(index),
        value: scoped.workspaceFolderValue,
      });
    }
  });

  return {
    ...(rootInspection?.globalValue !== undefined
      ? { user: { value: rootInspection.globalValue } }
      : {}),
    ...(rootInspection?.workspaceValue !== undefined
      ? { workspace: { value: rootInspection.workspaceValue } }
      : {}),
    ...(workspaceFolders.length > 0 ? { workspaceFolders } : {}),
  };
}

/**
 * The extension's live migration surface: current state plus the notices already shown.
 *
 * The published redacted state *is* the configuration generation: a refresh producing an
 * identical state says nothing, so an unrelated configuration change — or a second read at
 * activation — repeats nothing. When the state does change, what was already said is
 * forgotten, so a profile that gains or loses a legacy setting is reported again. Within
 * one state, notices dedupe on `(migration identity, site)`.
 */
export class LegacyMigrationSurface {
  private readonly dedupe = new MigrationNoticeDedupe();
  private lastStateFingerprint: string | null = null;
  private state: LegacyMigrationState;

  /**
   * @param notices where migration lines are written — the extension's output channel in
   *   production, a collector in tests.
   * @param extensionVersion the exact running version, passed in rather than read from
   *   package state so expiry stays decidable from an explicit subject (#12886). A value
   *   the version parser rejects makes expiry-bearing rows `invalid` rather than
   *   silently unexpired.
   * @param registry defaulted to the shipped registry; overridable so the authorization
   *   and notice behavior can be proven against rows that do not exist yet.
   */
  public constructor(
    private readonly notices: MigrationNoticeSink,
    private readonly extensionVersion: string,
    private readonly registry: ConfigurationMigrationRegistry = V018_CONFIGURATION_MIGRATIONS,
  ) {
    // Published before the first read so a consumer that asks early gets an empty,
    // well-formed state rather than `undefined`.
    this.state = legacyMigrationState(this.registry, this.extensionVersion, []);
  }

  /** Redacted state for status, doctor, and support surfaces. */
  public snapshot(): LegacyMigrationState {
    return this.state;
  }

  /** Re-read configuration, publish the redacted state, and emit any new notices. */
  public refresh(): LegacyMigrationState {
    const occurrences = readLegacyConfiguration(
      this.registry,
      this.extensionVersion,
      inspectLegacyKey,
    );
    this.state = legacyMigrationState(this.registry, this.extensionVersion, occurrences);

    const fingerprint = JSON.stringify(this.state.entries);
    if (fingerprint === this.lastStateFingerprint) {
      // Nothing the user could act on changed, so nothing is said again.
      return this.state;
    }
    this.lastStateFingerprint = fingerprint;
    // Forgetting what was already said is what lets a notice reappear once the profile
    // genuinely changes, and it bounds the set to one state's occurrences rather than
    // letting it grow for the window's lifetime.
    this.dedupe.clear();

    this.reportUnwiredCanonicalValues(occurrences);
    for (const occurrence of occurrences) {
      // `MigrationNoticeDedupe` keys on the migration identity, which is null for every
      // occurrence no row matched — so all of a key's refusals would share one subject and
      // only the first would ever be announced. Two repository-controlled copies of the
      // same setting are two things the user has to remove, so the site separates them.
      const site = `${occurrence.target} ${occurrence.folderId ?? ''}`;
      if (this.dedupe.shouldShow(occurrence.runtime, site)) {
        this.notices.warn(
          `[configuration-migration] ${describeLegacyMigrationEntry(
            legacyMigrationStateEntry(occurrence),
          )}`,
        );
      }
    }
    return this.state;
  }

  /**
   * A legacy value interpreted as canonical current configuration has no consumer here:
   * publishing one is #6736/#7838 work this seam does not own. Reaching this branch means
   * a registry row changed disposition without that wiring, so it is reported as a defect
   * rather than dropped.
   *
   * Reached only when the published state changed. `MigrationNoticeDedupe` guards the
   * warnings but not this line, so an ungated call would repeat the same defect on every
   * unrelated configuration change.
   */
  private reportUnwiredCanonicalValues(occurrences: readonly LegacyMigrationOccurrence[]): void {
    for (const occurrence of unwiredCanonicalValues(occurrences)) {
      this.notices.error(
        `[configuration-migration] \`${occurrence.legacyKey}\` resolved to a canonical value, ` +
          'but no effective-configuration consumer is wired (#6736/#7838); the value was not applied.',
      );
    }
  }
}

/**
 * Re-read the migration surface when a *registered legacy key* changed.
 *
 * Driven from the workspace event owner's unclassified hook rather than a second
 * `onDidChangeConfiguration` registration: one host listener keeps activation resource
 * ownership and mid-rollback dispatch semantics unchanged.
 */
export function refreshLegacyMigrationOnConfigurationChange(
  surface: LegacyMigrationSurface,
  event: { affectsConfiguration(key: string): boolean },
  registry: ConfigurationMigrationRegistry = V018_CONFIGURATION_MIGRATIONS,
): void {
  if (registry.rows.some((row) => event.affectsConfiguration(row.old_key))) {
    surface.refresh();
  }
}
