import * as vscode from 'vscode';

import {
  LegacyMigrationSurface,
  refreshLegacyMigrationOnConfigurationChange,
  registerLegacyMigrationFolderWatcher,
} from '../configurationMigrationHost';
import type { ConfigurationMigrationRegistry } from '../configurationMigrationRegistry';
import { V018_CONFIGURATION_MIGRATIONS } from '../configurationMigrationRegistry';

jest.mock('vscode');

const MCP_KEY = 'perl-lsp.mcp.servers';
const STALE_MCP_VALUE = [
  { label: 'local', command: '/opt/secret-tools/agent', env: { T: 's3cr3t' } },
];

type Inspection = {
  globalValue?: unknown;
  workspaceValue?: unknown;
  workspaceFolderValue?: unknown;
};

const workspaceMock = vscode.workspace as unknown as {
  getConfiguration: jest.Mock;
  workspaceFolders: Array<{ uri: { fsPath: string } }> | undefined;
  onDidChangeWorkspaceFolders: jest.Mock;
};

/** Every `update` the code under test could have called, across all scopes. */
const updateCalls: unknown[][] = [];

/**
 * Stand in for the host's layered configuration: `rootInspection` answers the unscoped
 * `getConfiguration()`, and `folderInspections[i]` answers the folder-scoped read.
 */
function stubConfiguration(rootInspection: Inspection, folderInspections: Inspection[] = []): void {
  updateCalls.length = 0;
  workspaceMock.workspaceFolders =
    folderInspections.length > 0
      ? folderInspections.map((_, index) => ({ uri: { fsPath: `/w/${index}` } }))
      : undefined;

  workspaceMock.getConfiguration.mockImplementation(
    (_section?: string, scope?: { fsPath: string }) => {
      const folderIndex = scope
        ? Number.parseInt(scope.fsPath.slice(scope.fsPath.lastIndexOf('/') + 1), 10)
        : null;
      const inspection =
        folderIndex === null ? rootInspection : (folderInspections[folderIndex] ?? {});
      return {
        get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
        has: jest.fn(() => false),
        inspect: jest.fn((key: string) => (key === MCP_KEY ? inspection : undefined)),
        update: jest.fn((...args: unknown[]) => {
          updateCalls.push(args);
        }),
      };
    },
  );
}

/** A surface whose notice sink records lines, so notices can be counted and inspected. */
function surface(registry?: ConfigurationMigrationRegistry): {
  surface: LegacyMigrationSurface;
  warnings: string[];
  errors: string[];
} {
  const warnings: string[] = [];
  const errors: string[] = [];
  return {
    surface: new LegacyMigrationSurface(
      {
        warn: (message) => warnings.push(message),
        error: (message) => errors.push(message),
      },
      '0.18.0',
      registry ?? V018_CONFIGURATION_MIGRATIONS,
    ),
    warnings,
    errors,
  };
}

/**
 * A registry whose single row reads a legacy value straight through as canonical.
 *
 * No shipped row does this, so the unwired-canonical seam has no other way to be
 * exercised against the host.
 */
function compatibleRegistry(): ConfigurationMigrationRegistry {
  return {
    schema_version: 'vscode_configuration_migration.v2',
    source_public_release: '0.17.0',
    target_release: '0.18.0',
    rows: [
      {
        migration_id: 'legacy_rename',
        old_key: MCP_KEY,
        old_value_shape: 'array',
        introduced_version: '0.17.0',
        last_supported_version: '0.17.x',
        new_key_or_authority: 'perl-lsp.newSetting',
        old_scope: 'user',
        new_scope: 'user',
        migration_disposition: 'renamed_compatible',
        automatic_read_compatibility: true,
        explicit_write_allowed: false,
        old_plus_new_conflict_policy: 'current_wins',
        security_trust_class: 'ordinary',
        warning_reason_code: 'legacy_setting_renamed',
        compatibility_window: { kind: 'no_expiry' },
        expiry_owner_issue: null,
        installed_proof_requirement: '#9001',
      },
    ],
  };
}

beforeEach(() => {
  jest.clearAllMocks();
  stubConfiguration({});
});

describe('legacy migration host adapter', () => {
  test('a clean profile publishes empty state and warns about nothing', () => {
    const { surface: reader, warnings, errors } = surface();

    expect(reader.refresh().entries).toEqual([]);
    expect(warnings).toEqual([]);
    expect(errors).toEqual([]);
  });

  test('a user-settings value is read from the unscoped inspection', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, warnings } = surface();

    const state = reader.refresh();

    expect(state.entries).toHaveLength(1);
    expect(state.entries[0]?.target).toBe('user');
    expect(state.entries[0]?.status).toBe('inert');
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain(MCP_KEY);
    expect(warnings[0]).not.toContain('s3cr3t');
  });

  test('a workspace-settings value is refused rather than treated as the removed row', () => {
    stubConfiguration({ workspaceValue: STALE_MCP_VALUE });
    const { surface: reader } = surface();

    const state = reader.refresh();

    expect(state.entries[0]?.target).toBe('workspace');
    expect(state.entries[0]?.status).toBe('invalid');
    expect(state.entries[0]?.reason_code).toBe('legacy_key_scope_not_permitted');
  });

  test('folder values are read per folder and identified without a filesystem path', () => {
    stubConfiguration({}, [
      { workspaceFolderValue: STALE_MCP_VALUE },
      {},
      { workspaceFolderValue: STALE_MCP_VALUE },
    ]);
    const { surface: reader } = surface();

    const state = reader.refresh();

    expect(state.entries.map((entry) => entry.folderId)).toEqual(['folder:0', 'folder:2']);
    expect(JSON.stringify(state)).not.toContain('/w/');
  });

  test('reading configuration never writes it', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE }, [{ workspaceFolderValue: [] }]);
    const { surface: reader } = surface();

    reader.refresh();
    reader.refresh();

    expect(updateCalls).toEqual([]);
  });

  test('an unchanged profile repeats no notice across refreshes', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, warnings } = surface();

    reader.refresh();
    reader.refresh();
    reader.refresh();

    expect(warnings).toHaveLength(1);
  });

  test('a changed profile is reported as the new state', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, warnings } = surface();
    reader.refresh();

    stubConfiguration({ workspaceValue: STALE_MCP_VALUE });
    const state = reader.refresh();

    expect(state.entries).toHaveLength(1);
    expect(state.entries[0]?.target).toBe('workspace');
    expect(warnings).toHaveLength(2);
  });

  // The test above moves the value between targets, which also changes the dedupe subject
  // — so it would pass even if nothing released prior notices. Removing and restoring the
  // same value holds subject and site fixed, leaving the state change as the only thing
  // that can release the second notice.
  test('a state change, not a differing subject, is what releases a repeat notice', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, warnings } = surface();

    reader.refresh();
    expect(warnings).toHaveLength(1);

    stubConfiguration({});
    expect(reader.refresh().entries).toEqual([]);
    expect(warnings).toHaveLength(1);

    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    reader.refresh();

    expect(warnings).toHaveLength(2);
    expect(warnings[1]).toBe(warnings[0]);
  });

  // Both copies are things the user has to remove. Keying the notice on migration
  // identity alone collapses them: every refused occurrence carries a null identity, so
  // one key's refusals all share a subject and only the first is ever announced.
  test('every refused copy of one key is announced, not just the first', () => {
    stubConfiguration({ workspaceValue: STALE_MCP_VALUE }, [
      { workspaceFolderValue: STALE_MCP_VALUE },
      { workspaceFolderValue: STALE_MCP_VALUE },
    ]);
    const { surface: reader, warnings } = surface();

    const state = reader.refresh();

    expect(state.entries).toHaveLength(3);
    expect(
      state.entries.every((entry) => entry.reason_code === 'legacy_key_scope_not_permitted'),
    ).toBe(true);
    expect(warnings).toHaveLength(3);
    expect(warnings.filter((line) => line.includes('workspace folder settings'))).toHaveLength(2);
    expect(new Set(warnings).size).toBe(3);
  });

  test('a canonical value with no wired consumer is reported as a defect', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, errors } = surface(compatibleRegistry());

    reader.refresh();

    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain('#6736/#7838');
    expect(errors[0]).not.toContain('s3cr3t');
  });

  test('an unchanged profile does not repeat the unwired-canonical defect', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, errors } = surface(compatibleRegistry());

    reader.refresh();
    reader.refresh();
    reader.refresh();

    expect(errors).toHaveLength(1);
  });

  test('snapshot returns the last published state without re-reading configuration', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader } = surface();

    expect(reader.snapshot().entries).toEqual([]);
    const published = reader.refresh();
    expect(reader.snapshot()).toBe(published);
  });

  // The state depends on the folder list, not only on configuration content. A folder
  // added mid-session brings its own settings, and VS Code announces that on its own
  // event rather than as a configuration change.
  test('a folder added mid-session is read without any configuration change', () => {
    stubConfiguration({});
    const { surface: reader, warnings } = surface();
    let listener: (() => void) | undefined;
    workspaceMock.onDidChangeWorkspaceFolders.mockImplementation((callback: () => void) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    registerLegacyMigrationFolderWatcher(reader, () => undefined);
    expect(reader.refresh().entries).toEqual([]);

    stubConfiguration({}, [{ workspaceFolderValue: STALE_MCP_VALUE }]);
    listener?.();

    expect(reader.snapshot().entries).toHaveLength(1);
    expect(reader.snapshot().entries[0]?.folderId).toBe('folder:0');
    expect(warnings).toHaveLength(1);
  });

  test('a removed folder drops out of the published state', () => {
    stubConfiguration({}, [{ workspaceFolderValue: STALE_MCP_VALUE }]);
    const { surface: reader } = surface();
    let listener: (() => void) | undefined;
    workspaceMock.onDidChangeWorkspaceFolders.mockImplementation((callback: () => void) => {
      listener = callback;
      return { dispose: jest.fn() };
    });

    registerLegacyMigrationFolderWatcher(reader, () => undefined);
    expect(reader.refresh().entries).toHaveLength(1);

    stubConfiguration({});
    listener?.();

    expect(reader.snapshot().entries).toEqual([]);
  });

  test('a folder-change read failure is reported, not thrown at the host', () => {
    stubConfiguration({});
    const { surface: reader } = surface();
    let listener: (() => void) | undefined;
    workspaceMock.onDidChangeWorkspaceFolders.mockImplementation((callback: () => void) => {
      listener = callback;
      return { dispose: jest.fn() };
    });
    const failures: unknown[] = [];

    registerLegacyMigrationFolderWatcher(reader, (error) => failures.push(error));
    workspaceMock.getConfiguration.mockImplementation(() => {
      throw new Error('host refused inspection');
    });

    expect(() => listener?.()).not.toThrow();
    expect(failures).toHaveLength(1);
  });

  test('a configuration change refreshes only when a registered legacy key changed', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, warnings } = surface();

    refreshLegacyMigrationOnConfigurationChange(
      reader,
      { affectsConfiguration: () => false },
      V018_CONFIGURATION_MIGRATIONS,
    );
    expect(warnings).toEqual([]);
    expect(reader.snapshot().entries).toEqual([]);

    refreshLegacyMigrationOnConfigurationChange(
      reader,
      { affectsConfiguration: (key) => key === MCP_KEY },
      V018_CONFIGURATION_MIGRATIONS,
    );
    expect(warnings).toHaveLength(1);
    expect(reader.snapshot().entries).toHaveLength(1);
  });
});
