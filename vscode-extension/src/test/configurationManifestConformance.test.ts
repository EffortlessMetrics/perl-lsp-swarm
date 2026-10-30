import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import {
  checkConfigurationManifestConformance,
  configurationInventory,
  type ManifestMigrationExclusion,
  type SettingsInventory,
} from '../configurationManifestConformance';
import {
  V018_CONFIGURATION_MIGRATIONS,
  type ConfigurationMigrationRegistry,
  type ConfigurationMigrationRow,
} from '../configurationMigrationRegistry';

const extensionRoot = path.resolve(__dirname, '../..');
const fixtureRoot = path.join(extensionRoot, 'test/fixtures/configuration-migration');
const fixtureBytes = fs.readFileSync(path.join(fixtureRoot, 'v0.17.0.package.json.fixture'));
const released = JSON.parse(fixtureBytes.toString('utf8')) as Record<string, unknown>;
const candidate = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
) as Record<string, unknown>;
const exclusions = JSON.parse(
  fs.readFileSync(
    path.join(extensionRoot, 'docs/migrations/v017-manifest-exclusions.json'),
    'utf8',
  ),
) as ManifestMigrationExclusion[];
const registry = V018_CONFIGURATION_MIGRATIONS;
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const check = (next: unknown = candidate, rows = registry, omit = exclusions): string[] =>
  checkConfigurationManifestConformance(released, next, rows, omit);

function withInventory(properties: SettingsInventory): Record<string, unknown> {
  return { ...candidate, contributes: { configuration: { properties } } };
}

function migration(overrides: Partial<ConfigurationMigrationRow>): ConfigurationMigrationRow {
  return { ...registry.rows[0]!, ...overrides };
}

function changedRow(row: ConfigurationMigrationRow): ConfigurationMigrationRegistry {
  return { ...registry, rows: [...registry.rows, row] };
}

describe('authentic public settings migration conformance (#7837)', () => {
  test('pins exact extracted bytes and official release identity, without network', () => {
    const provenance = JSON.parse(
      fs.readFileSync(path.join(fixtureRoot, 'provenance.json'), 'utf8'),
    ) as Record<string, unknown>;
    expect(crypto.createHash('sha256').update(fixtureBytes).digest('hex')).toBe(
      '3186e23ef455e4111df92711b9b926cfbe68a949db9bc84f536ea8b2908c0e6a',
    );
    expect(provenance.sha256).toBe(
      'b67d84a149efd99ae26310742bc3773c6a4644258391796489b7a9476d2e47d4',
    );
    expect(provenance.manifestSha256).toBe(
      crypto.createHash('sha256').update(fixtureBytes).digest('hex'),
    );
    expect(provenance.officialDownload).toBe(
      'https://github.com/EffortlessMetrics/perl-lsp/releases/download/v0.17.0/perl-lsp-rs-0.17.0.vsix',
    );
    expect(released).toMatchObject({
      name: 'perl-lsp-rs',
      publisher: 'EffortlessMetrics',
      version: '0.17.0',
    });
    expect(Object.keys(configurationInventory(released))).toHaveLength(25);
  });

  test('first falsifier: current MCP-only registry misses actual published setting removal', () => {
    const errors = check(candidate, registry, []);
    expect(errors).toContain(
      'perl-lsp.autoPopulateNewFiles: changed/removed published definition needs exactly one applicable disposition or exact issue-linked exclusion (found 0)',
    );
    expect(errors.filter((error) => error.includes('found 0'))).toHaveLength(13);
    expect(errors.some((error) => error.includes('perl-lsp.mcp.servers'))).toBe(false);
  });

  test('current candidate passes only with exact unresolved exclusions and canonical MCP row', () => {
    expect(check()).toEqual([]);
    expect(exclusions).toHaveLength(13);
    expect(exclusions.filter((entry) => entry.issue === 14968)).toHaveLength(5);
  });

  test('unreviewed disappearance and default drift cannot hide among retained keys', () => {
    const properties = clone(configurationInventory(candidate));
    const unchanged = Object.keys(configurationInventory(released)).find(
      (key) => !exclusions.some((entry) => entry.key === key) && key !== 'perl-lsp.mcp.servers',
    )!;
    delete properties[unchanged];
    expect(check(withInventory(properties))).toContain(
      `${unchanged}: changed/removed published definition needs exactly one applicable disposition or exact issue-linked exclusion (found 0)`,
    );
    properties[unchanged] = {
      ...configurationInventory(released)[unchanged]!,
      default: 'unreviewed-default',
    };
    expect(check(withInventory(properties))).toContain(
      `${unchanged}: changed/removed published definition needs exactly one applicable disposition or exact issue-linked exclusion (found 0)`,
    );
  });

  test('scope, nested enum and text changes stale exact exclusions', () => {
    for (const [key, modify] of [
      [
        'perl-lsp.aiCompletion.enabled',
        (value: Record<string, unknown>) => ({ ...value, scope: 'resource' }),
      ],
      [
        'perl-lsp.disabledFeatures',
        (value: Record<string, unknown>) => ({
          ...value,
          items: { type: 'string', enum: ['new-unreviewed-id'] },
        }),
      ],
      [
        'perl-lsp.includePaths',
        (value: Record<string, unknown>) => ({ ...value, markdownDescription: 'new guidance' }),
      ],
    ] as const) {
      const properties = clone(configurationInventory(candidate));
      properties[key] = modify(properties[key]!);
      expect(check(withInventory(properties))).toContain(
        `manifest exclusion definition drift: ${key} (#${key.includes('aiCompletion') ? 14968 : 7837})`,
      );
    }
  });

  test('new settings require no fabricated historical row; unrelated new schema remains free', () => {
    const properties = clone(configurationInventory(candidate));
    properties['perl-lsp.futureSetting'] = { type: 'boolean', default: false, scope: 'resource' };
    expect(check(withInventory(properties))).toEqual([]);
  });

  test('duplicate contributions are rejected instead of overwritten', () => {
    const properties = configurationInventory(candidate);
    const manifest = {
      ...candidate,
      contributes: { configuration: [{ properties }, { properties }] },
    };
    expect(check(manifest)).toEqual([expect.stringContaining('duplicate contributed setting:')]);
  });

  test('invalid, duplicate, stale and overlapping exclusion/row coverage fails', () => {
    expect(check(candidate, registry, [...exclusions, exclusions[0]!])).toEqual(
      expect.arrayContaining([
        `duplicate manifest exclusion: ${exclusions[0]!.key}`,
        expect.stringContaining('(found 2)'),
      ]),
    );
    const bad = clone(exclusions);
    bad[0]!.issue = 0;
    expect(check(candidate, registry, bad)).toContain(
      'invalid manifest exclusion: exact definitions, positive issue and reason required',
    );
    const properties = clone(configurationInventory(candidate));
    properties[exclusions[0]!.key] = exclusions[0]!.before;
    expect(check(withInventory(properties))).toContain(
      `stale manifest exclusion: ${exclusions[0]!.key}`,
    );
    expect(
      check(candidate, { ...registry, rows: [...registry.rows, registry.rows[0]!] }),
    ).toContain('duplicate migration_id: v017_mcp_servers_removed');
    expect(
      check(
        candidate,
        changedRow(migration({ migration_id: 'absent', old_key: 'perl-lsp.neverPublished' })),
      ),
    ).toContain('stale migration row for published inventory: absent');
    expect(
      check(candidate, {
        ...registry,
        rows: [migration({ introduced_version: '0.18.0', last_supported_version: '0.18.x' })],
      }),
    ).toContain(
      'perl-lsp.mcp.servers: changed/removed published definition needs exactly one applicable disposition or exact issue-linked exclusion (found 0)',
    );
    const mcp = configurationInventory(released)['perl-lsp.mcp.servers']!;
    expect(
      check(candidate, registry, [
        ...exclusions,
        {
          key: 'perl-lsp.mcp.servers',
          before: mcp,
          after: configurationInventory(candidate)['perl-lsp.mcp.servers']!,
          issue: 7837,
          reason: 'duplicate coverage',
        },
      ]),
    ).toContain(
      'perl-lsp.mcp.servers: changed/removed published definition needs exactly one applicable disposition or exact issue-linked exclusion (found 2)',
    );
  });

  test('disjoint historical eras remain valid and do not cover this source release', () => {
    const historic = migration({
      migration_id: 'v016_mcp',
      introduced_version: '0.16.0',
      last_supported_version: '0.16.x',
    });
    expect(check(candidate, changedRow(historic))).toEqual([]);
    const malformed = migration({
      migration_id: 'bad-era',
      introduced_version: '0.18.x',
      last_supported_version: '0.16.x',
    });
    expect(check(candidate, changedRow(malformed))).toContain(
      'migration historical era is not a valid window: bad-era',
    );
    expect(check(candidate, changedRow(migration({ migration_id: 'overlap' })))).toContain(
      'overlapping historical migration subject: perl-lsp.mcp.servers',
    );
  });

  test('historical scope and inert compatibility must agree with canonical registry', () => {
    expect(
      check(candidate, { ...registry, rows: [migration({ old_scope: 'resource' })] }),
    ).toContain('published scope mismatch: v017_mcp_servers_removed (machine)');
    expect(
      check(candidate, { ...registry, rows: [migration({ automatic_read_compatibility: true })] }),
    ).toContain(
      'removed_inert migration cannot retain read/write compatibility: v017_mcp_servers_removed',
    );
    expect(
      check(candidate, {
        ...registry,
        rows: [
          migration({
            migration_disposition: 'unchanged',
            new_key_or_authority: 'perl-lsp.mcp.servers',
            new_scope: 'machine',
          }),
        ],
      }),
    ).toContain('unchanged disposition cannot cover manifest drift: perl-lsp.mcp.servers');
  });

  test('manifest-native omitted scope equals window, not a guessed runtime scope', () => {
    const before = {
      ...released,
      contributes: { configuration: { properties: { old: { type: 'boolean' } } } },
    };
    const after = { ...candidate, contributes: { configuration: { properties: {} } } };
    const rows = { ...registry, rows: [migration({ old_key: 'old', old_scope: 'resource' })] };
    expect(checkConfigurationManifestConformance(before, after, rows, [])).toContain(
      'published scope mismatch: v017_mcp_servers_removed (window)',
    );
    expect(
      checkConfigurationManifestConformance(
        before,
        after,
        { ...registry, rows: [migration({ old_key: 'old', old_scope: 'workspace' })] },
        [],
      ),
    ).toContain('published scope mismatch: v017_mcp_servers_removed (window)');
  });

  test('compatible rename must have actual target, matching scope, defaults and nested enum', () => {
    const before = {
      ...released,
      contributes: {
        configuration: {
          properties: {
            old: { type: 'array', scope: 'resource', default: [], items: { enum: ['a', 'b'] } },
          },
        },
      },
    };
    const row = migration({
      old_key: 'old',
      old_scope: 'resource',
      new_key_or_authority: 'new',
      new_scope: 'resource',
      migration_disposition: 'renamed_compatible',
      security_trust_class: 'ordinary',
    });
    const rows = { ...registry, rows: [row] };
    const run = (target: Record<string, unknown> | null): string[] =>
      checkConfigurationManifestConformance(
        before,
        {
          ...candidate,
          contributes: { configuration: { properties: target ? { new: target } : {} } },
        },
        rows,
        [],
      );
    const value = { type: 'array', scope: 'resource', default: [], items: { enum: ['a', 'b'] } };
    expect(run(value)).toEqual([]);
    expect(run(null)).toContain(
      'rename target must be a different contributed setting: v017_mcp_servers_removed',
    );
    for (const target of [
      { ...value, scope: 'machine' },
      { ...value, default: ['a'] },
      { ...value, items: { enum: ['a'] } },
    ])
      expect(run(target)).toContain(
        'compatible rename changes value schema or scope: v017_mcp_servers_removed',
      );
    expect(run({ ...value, scope: 'machine' })).toContain(
      'candidate target scope mismatch: v017_mcp_servers_removed',
    );
  });

  describe('compatible rename with retained alias', () => {
    const value = {
      type: 'array',
      scope: 'resource',
      default: ['a'],
      items: { type: 'string', enum: ['a', 'b'] },
      description: 'Published setting',
    };
    const before = {
      ...released,
      contributes: { configuration: { properties: { old: value } } },
    };
    const rows = {
      ...registry,
      rows: [
        migration({
          old_key: 'old',
          old_scope: 'resource',
          new_key_or_authority: 'new',
          new_scope: 'resource',
          migration_disposition: 'renamed_compatible',
          security_trust_class: 'ordinary',
          automatic_read_compatibility: true,
          old_plus_new_conflict_policy: 'current_wins',
        }),
      ],
    };
    const run = (alias: Record<string, unknown>): string[] =>
      checkConfigurationManifestConformance(
        before,
        {
          ...candidate,
          contributes: { configuration: { properties: { old: alias, new: value } } },
        },
        rows,
        [],
      );

    test.each([
      ['narrowed nested enum', { ...value, items: { type: 'string', enum: ['a'] } }],
      ['changed default', { ...value, default: ['b'] }],
      ['changed scope', { ...value, scope: 'machine' }],
    ])('rejects %s while the rename target stays compatible', (_name, alias) => {
      expect(run(alias)).toContain(
        'compatible retained alias changes value schema or scope: v017_mcp_servers_removed',
      );
    });

    test('accepts unchanged retained alias and presentation-only changes', () => {
      expect(run(value)).toEqual([]);
      expect(
        run({
          ...value,
          description: 'Deprecated alias; use new',
          order: 99,
          markdownDescription: 'Use `new` instead',
          deprecationMessage: 'Use new',
        }),
      ).toEqual([]);
    });
  });

  test('external authority cannot be fabricated by a target string', () => {
    const rows = {
      ...registry,
      rows: [
        migration({
          migration_disposition: 'replaced_by_standard_vscode_setting',
          new_key_or_authority: 'made-up.vscodeSetting',
          new_scope: 'machine',
        }),
      ],
    };
    expect(check(candidate, rows)).toContain(
      'disposition needs reviewed runtime/external authority evidence; use an exact issue-linked exclusion until supported: v017_mcp_servers_removed',
    );
  });

  test('action-required rename cannot declare automatic compatibility for incompatible choices', () => {
    const before = {
      ...released,
      contributes: {
        configuration: {
          properties: { old: { type: 'boolean', scope: 'resource', default: false } },
        },
      },
    };
    const after = {
      ...candidate,
      contributes: {
        configuration: { properties: { new: { type: 'string', scope: 'resource', default: 'x' } } },
      },
    };
    const row = migration({
      old_key: 'old',
      old_scope: 'resource',
      new_key_or_authority: 'new',
      new_scope: 'resource',
      migration_disposition: 'renamed_requires_user_action',
      security_trust_class: 'ordinary',
      automatic_read_compatibility: true,
      old_plus_new_conflict_policy: 'current_wins',
    });
    expect(
      checkConfigurationManifestConformance(before, after, { ...registry, rows: [row] }, []),
    ).toContain(
      'action-required rename cannot declare automatic read compatibility: v017_mcp_servers_removed',
    );
    expect(
      checkConfigurationManifestConformance(
        before,
        after,
        { ...registry, rows: [{ ...row, automatic_read_compatibility: false }] },
        [],
      ),
    ).toEqual([]);
  });

  test('output is deterministic across object, registry and exclusion ordering', () => {
    const properties = Object.fromEntries(
      Object.entries(configurationInventory(candidate)).reverse(),
    );
    const reordered = { ...registry, rows: [...registry.rows].reverse() };
    expect(check(withInventory(properties), reordered, [...exclusions].reverse())).toEqual(check());
    expect(check(withInventory(properties), reordered, [])).toEqual(check(candidate, registry, []));
  });
});
