import * as fs from 'fs';
import * as path from 'path';
import { SETTING_OWNERSHIP, settingOwnership } from '../configurationOwnership';

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
        row.manifestScope === 'resource' && row.semanticScope !== 'workspace-folder' && !row.defect,
    ).map((row) => row.key);

    expect(undocumented).toEqual([]);

    for (const row of SETTING_OWNERSHIP) {
      if (row.defect) {
        expect(row.defect.reason.length).toBeGreaterThan(0);
        expect(row.defect.owner).toMatch(/^#\d+$/);
      }
    }
  });

  test('the rows that are not honoured end to end are exactly the recorded set', () => {
    // Pinning the set keeps a newly introduced unhonoured row from joining the
    // existing ones silently. Correcting these is a user-visible breaking
    // change or a new server payload, each owned by its own claim.
    const defective = SETTING_OWNERSHIP.filter((row) => row.defect).map((row) => row.key);

    expect(defective).toEqual([
      'perl-lsp.critic.enabled',
      'perl-lsp.critic.exclude',
      'perl-lsp.critic.include',
      'perl-lsp.critic.profile',
      'perl-lsp.critic.severity',
      'perl-lsp.enableFormatting',
      'perl-lsp.enableSemanticTokens',
      'perl-lsp.enableTestIntegration',
      'perl-lsp.externalIncludePaths',
      'perl-lsp.perlcritic.enabled',
      'perl-lsp.perlcritic.severity',
      'perl-lsp.perltidyConfig',
    ]);
  });

  test('autoPopulateNewFiles is recorded folder-owned with no outstanding defect (#14547)', () => {
    // The row moved off the defect list because `populateCreatedFiles` resolves
    // the gate per created URI. Recording it back as `client-session`, or
    // re-adding a defect, would mean the runtime read was hoisted again.
    expect(settingOwnership('perl-lsp.autoPopulateNewFiles')).toEqual({
      key: 'perl-lsp.autoPopulateNewFiles',
      manifestScope: 'resource',
      semanticScope: 'workspace-folder',
      owner: 'extension',
      transport: 'local-only',
    });
  });

  test('a row claiming a server transport names a server consumer or a defect', () => {
    // Guards the class of error these rows were corrected for: a transport
    // recorded because it sounds right, rather than because the server end
    // actually consumes the value.
    const unproven = SETTING_OWNERSHIP.filter(
      (row) =>
        (row.transport === 'workspace/configuration' || row.transport === 'initialize') &&
        row.owner === 'extension' &&
        !row.defect,
    ).map((row) => row.key);

    expect(unproven).toEqual([]);
  });

  test('only settings the server reads per folder claim the pull transport', () => {
    // `WorkspaceConfig::update_from_value_with_context` — the function that
    // applies a `workspace/configuration` result item — reads exactly one key,
    // `workspace`, and has no Critic field. Claiming any other setting travels
    // that transport would assert folder ownership the server cannot honour.
    const pulled = SETTING_OWNERSHIP.filter(
      (row) => row.transport === 'workspace/configuration',
    ).map((row) => row.key);

    expect(pulled).toEqual(['perl-lsp.externalIncludePaths', 'perl-lsp.includePaths']);
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
