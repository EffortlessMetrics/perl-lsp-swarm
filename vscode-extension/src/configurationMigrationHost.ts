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
  describeLegacyMigrationOccurrence,
  legacyMigrationState,
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
 * Notices dedupe on `(migration identity, configuration generation)`, and the generation
 * advances only when the redacted state actually changes. So an unrelated configuration
 * change — or a second read at activation — repeats nothing, while a profile that gains
 * or loses a legacy setting is reported again.
 */
export class LegacyMigrationSurface {
  private readonly dedupe = new MigrationNoticeDedupe();
  private generation = 0;
  private lastStateFingerprint: string | null = null;
  private state: LegacyMigrationState;

  public constructor(
    private readonly notices: MigrationNoticeSink,
    private readonly extensionVersion: string,
    private readonly registry: ConfigurationMigrationRegistry = V018_CONFIGURATION_MIGRATIONS,
  ) {
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
    if (fingerprint !== this.lastStateFingerprint) {
      this.lastStateFingerprint = fingerprint;
      this.generation += 1;
    }

    this.reportUnwiredCanonicalValues(occurrences);
    for (const occurrence of occurrences) {
      if (this.dedupe.shouldShow(occurrence.runtime, `${this.generation}`)) {
        this.notices.warn(
          `[configuration-migration] ${describeLegacyMigrationOccurrence(occurrence)}`,
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
