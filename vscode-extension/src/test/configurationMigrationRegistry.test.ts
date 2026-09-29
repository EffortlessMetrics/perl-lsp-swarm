import {
  type ConfigurationMigrationRegistry,
  type ConfigurationMigrationRow,
  type MigrationEra,
  type MigrationVersion,
  V018_CONFIGURATION_MIGRATIONS,
  findMigrationRows,
  migrationEraCoversVersion,
  migrationErasOverlap,
  parseMigrationEra,
  parseMigrationEraBound,
  parseMigrationVersion,
  serializeMigrationRegistry,
  validateMigrationRegistry,
} from '../configurationMigrationRegistry';

function cloneRegistry(): ConfigurationMigrationRegistry {
  return JSON.parse(
    JSON.stringify(V018_CONFIGURATION_MIGRATIONS),
  ) as ConfigurationMigrationRegistry;
}

describe('public-beta configuration migration registry', () => {
  test('ships a valid v0.17 to v0.18 registry with the generic MCP setting inert', () => {
    expect(validateMigrationRegistry(V018_CONFIGURATION_MIGRATIONS)).toEqual([]);

    expect(findMigrationRows(V018_CONFIGURATION_MIGRATIONS, 'perl-lsp.mcp.servers')).toEqual([
      expect.objectContaining({
        migration_id: 'v017_mcp_servers_removed',
        migration_disposition: 'removed_inert',
        automatic_read_compatibility: false,
        explicit_write_allowed: false,
        security_trust_class: 'process_execution',
        compatibility_window: { kind: 'no_expiry' },
        expiry_owner_issue: 7119,
        installed_proof_requirement: '#7841',
      }),
    ]);
  });

  test('serializes deterministically by migration identity', () => {
    const registry = cloneRegistry();
    registry.rows.push({
      ...registry.rows[0]!,
      migration_id: 'aaa_future_row',
      old_key: 'perl-lsp.example.old',
      old_value_shape: 'boolean',
      security_trust_class: 'ordinary',
      migration_disposition: 'deprecated_read_only',
      new_key_or_authority: 'perl-lsp.example.current',
      new_scope: 'resource',
      old_scope: 'resource',
      automatic_read_compatibility: true,
      old_plus_new_conflict_policy: 'current_wins',
      compatibility_window: { kind: 'no_expiry' },
      expiry_owner_issue: 9999,
      installed_proof_requirement: '#9998',
    });

    const first = serializeMigrationRegistry(registry);
    const second = serializeMigrationRegistry(registry);

    expect(first).toBe(second);
    expect(first.indexOf('aaa_future_row')).toBeLessThan(first.indexOf('v017_mcp_servers_removed'));
  });

  test('serializes equivalent registries independently of object insertion order', () => {
    const registry = cloneRegistry();
    const row = registry.rows[0]!;
    const reorderedRow = Object.fromEntries(Object.entries(row).reverse());
    const reorderedRegistry = Object.fromEntries(
      Object.entries({ ...registry, rows: [reorderedRow] }).reverse(),
    ) as unknown as ConfigurationMigrationRegistry;

    expect(serializeMigrationRegistry(reorderedRegistry)).toBe(
      serializeMigrationRegistry(registry),
    );
  });

  test('rejects duplicate exact historical subjects', () => {
    const registry = cloneRegistry();
    registry.rows.push({
      ...registry.rows[0]!,
      migration_id: 'duplicate_subject',
    });

    expect(validateMigrationRegistry(registry)).toContain(
      'overlapping historical migration subject: perl-lsp.mcp.servers',
    );
  });

  /** Push a second row for the shipped key, varying only the fields a test names. */
  const withSecondEra = (
    overrides: Partial<ConfigurationMigrationRow>,
  ): ConfigurationMigrationRegistry => {
    const registry = cloneRegistry();
    const first = registry.rows[0];
    if (first === undefined) {
      throw new Error('the shipped registry must define one row');
    }
    registry.rows.push({ ...first, migration_id: 'second_era', ...overrides });
    return registry;
  };

  const OVERLAP_ERROR = 'overlapping historical migration subject: perl-lsp.mcp.servers';

  test('admits disjoint historical eras for one key at one scope', () => {
    // The whole point of a versioned registry: one setting may carry a row per era. The
    // shipped row covers 0.17.0-0.17.x, so 0.15.0-0.16.x sits entirely below it.
    expect(
      validateMigrationRegistry(
        withSecondEra({ introduced_version: '0.15.0', last_supported_version: '0.16.x' }),
      ),
    ).toEqual([]);
  });

  test('rejects eras that overlap without sharing an exact window', () => {
    // 0.15.0-0.17.x against the shipped 0.17.0-0.17.x: different text, same releases
    // claimed twice. Exact-tuple comparison certified this pair as valid.
    expect(
      validateMigrationRegistry(
        withSecondEra({ introduced_version: '0.15.0', last_supported_version: '0.17.x' }),
      ),
    ).toContain(OVERLAP_ERROR);
  });

  test('rejects an era overlapping the shipped one by a single release', () => {
    // Minimal overlap: 0.17.0 is the only release both eras claim. A comparison that
    // tested only whether one window started inside the other would let this through.
    expect(
      validateMigrationRegistry(
        withSecondEra({ introduced_version: '0.14.0', last_supported_version: '0.17.0' }),
      ),
    ).toContain(OVERLAP_ERROR);
  });

  test('a minor-series upper bound admits every patch it contains', () => {
    // 0.17.9 must count as inside the shipped 0.17.x era, so an era opening there
    // overlaps. Treating `0.17.x` as the literal string, or as 0.17.0, would not.
    expect(
      validateMigrationRegistry(
        withSecondEra({ introduced_version: '0.17.9', last_supported_version: '0.18.0' }),
      ),
    ).toContain(OVERLAP_ERROR);
  });

  test('does not treat rows at different scopes as competing eras', () => {
    // Two scopes are two subjects; the reader picks between them by scope, not by era,
    // so identical windows there are not a registry defect.
    expect(
      validateMigrationRegistry(
        withSecondEra({ old_scope: 'user', warning_reason_code: 'legacy_other_scope' }),
      ),
    ).toEqual([]);
  });

  test.each([
    ['0.17', 'a two-part version is not a bound'],
    ['0.x.0', 'only the patch position may be a wildcard'],
    ['x', 'a bare wildcard names no series'],
    ['0.17.*', 'the wildcard spelling is `x`'],
    ['0.17.0-rc.1', 'a release era is not a prerelease'],
    ['v0.17.0', 'one bound must have one spelling'],
    ['0.17.0+build.1', 'build metadata is discarded, so it would be a second spelling'],
    ['0.17.x+build.1', 'a minor series takes no build metadata either'],
    ['latest', 'a moving target is not a historical bound'],
    ['', 'an empty bound admits nothing'],
  ])('rejects %s as an era bound (%s)', (bound) => {
    expect(validateMigrationRegistry(withSecondEra({ introduced_version: bound }))).toContain(
      'migration historical era is not a valid window: second_era',
    );
  });

  test('accepts both spellings the grammar does admit', () => {
    expect(parseMigrationEraBound('0.17.0')).toMatchObject({ kind: 'exact' });
    expect(parseMigrationEraBound('0.17.x')).toMatchObject({
      kind: 'minor_series',
      major: '0',
      minor: '17',
    });
  });

  test('rejects an inverted era rather than silently admitting no release', () => {
    expect(
      validateMigrationRegistry(
        withSecondEra({ introduced_version: '0.16.0', last_supported_version: '0.15.0' }),
      ),
    ).toContain('migration historical era is not a valid window: second_era');
  });

  test('era comparison is numeric, not lexicographic, across a digit boundary', () => {
    // '10' sorts before '9' as text. Every other fixture here uses two-digit minors, where
    // text and numeric order coincide — so a lexicographic compare would pass them all while
    // placing 0.10.0 inside an era bounded above by 0.9.x.
    const eraOf = (introduced: string, lastSupported: string): MigrationEra => {
      const base = V018_CONFIGURATION_MIGRATIONS.rows[0];
      if (base === undefined) {
        throw new Error('the shipped registry must define one row');
      }
      const parsed = parseMigrationEra({
        ...base,
        introduced_version: introduced,
        last_supported_version: lastSupported,
      });
      if (parsed === null) {
        throw new Error(`era ${introduced}..${lastSupported} must parse`);
      }
      return parsed;
    };
    const versionOf = (value: string): MigrationVersion => {
      const parsed = parseMigrationVersion(value);
      if (parsed === null) {
        throw new Error(`${value} must parse`);
      }
      return parsed;
    };

    // Coverage: 0.10 is above the 0.9 series, not inside it.
    expect(migrationEraCoversVersion(eraOf('0.0.0', '0.9.x'), versionOf('0.10.0'))).toBe(false);
    // ...while a two-digit patch inside that series still is.
    expect(migrationEraCoversVersion(eraOf('0.0.0', '0.9.x'), versionOf('0.9.10'))).toBe(true);
    // Overlap: these share the whole 0.10 series, so admitting them would be the bug.
    expect(migrationErasOverlap(eraOf('0.9.0', '0.10.x'), eraOf('0.10.0', '0.10.x'))).toBe(true);
    // ...and these genuinely do not touch.
    expect(migrationErasOverlap(eraOf('0.9.0', '0.9.x'), eraOf('0.10.0', '0.10.x'))).toBe(false);
  });

  test('the shipped registry declares a valid era covering its own source release', () => {
    // Guards the seeded row against the new grammar: if 0.17.0-0.17.x stopped parsing, or
    // stopped covering source_public_release, every live interpretation would change.
    expect(validateMigrationRegistry(V018_CONFIGURATION_MIGRATIONS)).toEqual([]);

    const row = V018_CONFIGURATION_MIGRATIONS.rows[0];
    const sourceRelease = parseMigrationVersion(
      V018_CONFIGURATION_MIGRATIONS.source_public_release,
    );
    if (row === undefined || sourceRelease === null) {
      throw new Error('the shipped registry must define one row and a parsable source release');
    }
    const era = parseMigrationEra(row);
    if (era === null) {
      throw new Error('the shipped row must declare a valid era');
    }
    expect(migrationEraCoversVersion(era, sourceRelease)).toBe(true);
  });

  test.each(['target_release', 'source_public_release'] as const)(
    'rejects an envelope missing required release identity %s',
    (field) => {
      const registry = cloneRegistry() as unknown as Record<string, unknown>;
      delete registry[field];

      expect(
        validateMigrationRegistry(registry as unknown as ConfigurationMigrationRegistry),
      ).toEqual(['migration registry envelope is missing or unsupported']);
    },
  );

  test.each(['target_release', 'source_public_release'] as const)(
    'rejects an envelope with malformed release identity %s',
    (field) => {
      const registry = cloneRegistry() as unknown as Record<string, unknown>;
      registry[field] = 'not-a-version';

      expect(
        validateMigrationRegistry(registry as unknown as ConfigurationMigrationRegistry),
      ).toEqual(['migration registry envelope is missing or unsupported']);
    },
  );

  test('rejects read or write compatibility for removed inert settings', () => {
    const registry = cloneRegistry();
    registry.rows[0] = {
      ...registry.rows[0]!,
      automatic_read_compatibility: true,
      explicit_write_allowed: true,
    };

    expect(validateMigrationRegistry(registry)).toContain(
      'removed_inert migration cannot retain read/write compatibility: v017_mcp_servers_removed',
    );
  });

  test('rejects sensitive scope widening', () => {
    const registry = cloneRegistry();
    registry.rows[0] = {
      ...registry.rows[0]!,
      migration_disposition: 'renamed_requires_user_action',
      new_key_or_authority: 'perl-lsp.mcp.replacement',
      new_scope: 'workspace',
      automatic_read_compatibility: false,
      explicit_write_allowed: false,
      old_plus_new_conflict_policy: 'action_required',
    };

    expect(validateMigrationRegistry(registry)).toContain(
      'sensitive migration cannot widen configuration authority: v017_mcp_servers_removed',
    );
  });

  test('rejects a sensitive relocation that hides its authority behind a null scope', () => {
    // Without an explicit new_scope the widening comparison is skipped entirely, so a
    // sensitive row could name a repository-controlled authority and still validate —
    // certifying a move of execution-sensitive configuration from machine scope to
    // project authority.
    const registry = cloneRegistry();
    registry.rows[0] = {
      ...registry.rows[0]!,
      migration_disposition: 'replaced_by_server_or_project_config',
      new_key_or_authority: '.perl-lsp.toml',
      new_scope: null,
      automatic_read_compatibility: false,
      explicit_write_allowed: false,
      old_plus_new_conflict_policy: 'action_required',
    };

    expect(validateMigrationRegistry(registry)).toContain(
      'sensitive migration must declare new_scope unless authority is retired: v017_mcp_servers_removed',
    );
  });

  test('still allows a null scope when the disposition retires the authority', () => {
    const registry = cloneRegistry();
    expect(registry.rows[0]!.security_trust_class).not.toBe('ordinary');
    expect(registry.rows[0]!.new_scope).toBeNull();
    expect(registry.rows[0]!.migration_disposition).toBe('removed_inert');

    expect(validateMigrationRegistry(registry)).toEqual([]);
  });

  test.each(['01.18.0', '0.18.0-01'])(
    'rejects SemVer forms runtime cannot parse: %s',
    (version) => {
      const registry = cloneRegistry();
      registry.rows[0] = {
        ...registry.rows[0]!,
        compatibility_window: {
          kind: 'removed_in_extension_version',
          version,
          post_expiry_disposition: 'inert',
        },
      };

      expect(validateMigrationRegistry(registry)).toContain(
        'migration expiry version is not valid SemVer: v017_mcp_servers_removed',
      );
    },
  );

  test('rejects malformed rows without dereferencing their fields', () => {
    const registry = cloneRegistry();
    registry.rows = [null as never];

    expect(validateMigrationRegistry(registry)).toEqual([
      'migration row is missing required fields',
    ]);
  });

  test('orders rows by code point rather than host collation', () => {
    // localeCompare is host-dependent: Swedish collation orders 'ä' after 'z',
    // English does not. A locale-sensitive sort would serialize the same registry
    // differently on CI and on a developer machine.
    const registry = cloneRegistry();
    const base = registry.rows[0]!;
    registry.rows = [
      { ...base, migration_id: 'zz_row' },
      { ...base, migration_id: 'ää_row' },
      { ...base, migration_id: 'aa_row' },
    ];

    const order = (
      JSON.parse(serializeMigrationRegistry(registry)) as ConfigurationMigrationRegistry
    ).rows.map((row) => row.migration_id);

    expect(order).toEqual(['aa_row', 'zz_row', 'ää_row']);
  });
});
