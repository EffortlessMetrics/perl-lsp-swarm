import * as vscode from 'vscode';

import {
  LegacyMigrationSurface,
  refreshLegacyMigrationOnConfigurationChange,
} from '../configurationMigrationHost';
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

function surface(): { surface: LegacyMigrationSurface; warnings: string[]; errors: string[] } {
  const warnings: string[] = [];
  const errors: string[] = [];
  return {
    surface: new LegacyMigrationSurface(
      {
        warn: (message) => warnings.push(message),
        error: (message) => errors.push(message),
      },
      '0.18.0',
    ),
    warnings,
    errors,
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

  test('a changed profile advances the generation so the new state is reported', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader, warnings } = surface();
    reader.refresh();

    stubConfiguration({ workspaceValue: STALE_MCP_VALUE });
    const state = reader.refresh();

    expect(state.entries).toHaveLength(1);
    expect(state.entries[0]?.target).toBe('workspace');
    expect(warnings).toHaveLength(2);
  });

  test('snapshot returns the last published state without re-reading configuration', () => {
    stubConfiguration({ globalValue: STALE_MCP_VALUE });
    const { surface: reader } = surface();

    expect(reader.snapshot().entries).toEqual([]);
    const published = reader.refresh();
    expect(reader.snapshot()).toBe(published);
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
