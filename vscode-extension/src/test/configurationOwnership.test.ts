import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  folderScopedServerSettingKeys,
  resolveResourceWriteTarget,
  SETTING_OWNERSHIP,
  settingOwnership,
} from '../configurationOwnership';

type ManifestProperty = { scope?: string };

function contributedSettings(): Map<string, ManifestProperty> {
  const manifestPath = path.resolve(__dirname, '../../package.json');
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as {
    contributes: {
      configuration:
        | { properties: Record<string, ManifestProperty> }
        | Array<{ properties: Record<string, ManifestProperty> }>;
    };
  };

  const groups = Array.isArray(manifest.contributes.configuration)
    ? manifest.contributes.configuration
    : [manifest.contributes.configuration];

  const settings = new Map<string, ManifestProperty>();
  for (const group of groups) {
    for (const [key, property] of Object.entries(group.properties ?? {})) {
      settings.set(key, property);
    }
  }
  return settings;
}

describe('contributed setting ownership table (#14447)', () => {
  test('every contributed setting has exactly one ownership row', () => {
    const contributed = [...contributedSettings().keys()].sort();
    const owned = SETTING_OWNERSHIP.map((row) => row.key).sort();

    expect(owned).toEqual(contributed);
  });

  test('no ownership row describes a setting that is not contributed', () => {
    const contributed = contributedSettings();
    const orphans = SETTING_OWNERSHIP.filter((row) => !contributed.has(row.key)).map(
      (row) => row.key,
    );

    expect(orphans).toEqual([]);
  });

  test('recorded manifest scope matches the manifest', () => {
    const contributed = contributedSettings();
    const drifted = SETTING_OWNERSHIP.filter((row) => {
      const declared = contributed.get(row.key)?.scope ?? 'window';
      return declared !== row.manifestScope;
    }).map((row) => row.key);

    expect(drifted).toEqual([]);
  });

  test('ownership rows are unique and stably ordered by key', () => {
    const keys = SETTING_OWNERSHIP.map((row) => row.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys).toEqual([...keys].sort());
  });

  test('a machine-scoped setting is never claimed to be folder-owned', () => {
    const inconsistent = SETTING_OWNERSHIP.filter(
      (row) => row.manifestScope === 'machine' && row.semanticScope === 'workspace-folder',
    ).map((row) => row.key);

    expect(inconsistent).toEqual([]);
  });

  test('a resource-scoped setting that cannot be folder-owned records a defect and owner', () => {
    const undocumented = SETTING_OWNERSHIP.filter(
      (row) =>
        row.manifestScope === 'resource' &&
        row.semanticScope !== 'workspace-folder' &&
        !row.scopeDefect,
    ).map((row) => row.key);

    expect(undocumented).toEqual([]);

    for (const row of SETTING_OWNERSHIP) {
      if (row.scopeDefect) {
        expect(row.scopeDefect.reason.length).toBeGreaterThan(0);
        expect(row.scopeDefect.owner).toMatch(/^#\d+$/);
      }
    }
  });

  test('the known impossible resource scopes are exactly the recorded set', () => {
    // Pinning the set keeps a newly introduced impossible scope from joining
    // the existing ones silently. Correcting these published scopes is a
    // user-visible breaking change owned by its own claim.
    const defective = SETTING_OWNERSHIP.filter((row) => row.scopeDefect).map((row) => row.key);

    expect(defective).toEqual([
      'perl-lsp.enableFormatting',
      'perl-lsp.enableSemanticTokens',
      'perl-lsp.enableTestIntegration',
    ]);
  });

  test('server-pulled keys are exposed unqualified for scoped reads', () => {
    const keys = folderScopedServerSettingKeys();

    expect(keys).toContain('includePaths');
    expect(keys).toContain('critic.severity');
    expect(keys.every((key) => !key.startsWith('perl-lsp.'))).toBe(true);
  });

  test('settingOwnership resolves a known key and refuses an unknown one', () => {
    expect(settingOwnership('perl-lsp.includePaths')).toMatchObject({
      manifestScope: 'resource',
      semanticScope: 'workspace-folder',
      transport: 'workspace/configuration',
    });
    expect(settingOwnership('perl-lsp.notAThing')).toBeUndefined();
  });
});

describe('resource write target (#14447)', () => {
  const workspaceMock = vscode.workspace as unknown as {
    workspaceFolders: unknown;
    getWorkspaceFolder: jest.Mock;
  };

  afterEach(() => {
    workspaceMock.workspaceFolders = undefined;
    workspaceMock.getWorkspaceFolder.mockReset();
    workspaceMock.getWorkspaceFolder.mockReturnValue(undefined);
  });

  test('a resource inside a workspace folder writes that folder', () => {
    workspaceMock.workspaceFolders = [{ uri: { fsPath: '/workspace/a' } }];
    workspaceMock.getWorkspaceFolder.mockReturnValue({ uri: { fsPath: '/workspace/a' } });

    expect(resolveResourceWriteTarget(vscode.Uri.parse('file:///workspace/a/lib/M.pm'))).toBe(
      vscode.ConfigurationTarget.WorkspaceFolder,
    );
  });

  test('a resource owned by no folder falls back to the workspace', () => {
    workspaceMock.workspaceFolders = [{ uri: { fsPath: '/workspace/a' } }];
    workspaceMock.getWorkspaceFolder.mockReturnValue(undefined);

    expect(resolveResourceWriteTarget(vscode.Uri.parse('file:///elsewhere/M.pm'))).toBe(
      vscode.ConfigurationTarget.Workspace,
    );
  });

  test('no resource at all falls back to the workspace', () => {
    workspaceMock.workspaceFolders = [{ uri: { fsPath: '/workspace/a' } }];

    expect(resolveResourceWriteTarget(undefined)).toBe(vscode.ConfigurationTarget.Workspace);
  });

  test('with no workspace open the value is written globally', () => {
    workspaceMock.workspaceFolders = undefined;

    expect(resolveResourceWriteTarget(vscode.Uri.parse('file:///tmp/M.pm'))).toBe(
      vscode.ConfigurationTarget.Global,
    );
  });

  test('a multi-root resource never resolves to the workspace-wide target', () => {
    // The reported defect: a folder-local action wrote ConfigurationTarget
    // .Workspace, publishing one folder's value to every other folder.
    workspaceMock.workspaceFolders = [
      { uri: { fsPath: '/workspace/a' } },
      { uri: { fsPath: '/workspace/b' } },
    ];
    workspaceMock.getWorkspaceFolder.mockReturnValue({ uri: { fsPath: '/workspace/a' } });

    expect(resolveResourceWriteTarget(vscode.Uri.parse('file:///workspace/a/lib/M.pm'))).not.toBe(
      vscode.ConfigurationTarget.Workspace,
    );
  });
});
