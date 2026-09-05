import {
  type ConfigurationMigrationRegistry,
  type ConfigurationMigrationRow,
  V018_CONFIGURATION_MIGRATIONS,
  validateMigrationRegistry,
} from '../configurationMigrationRegistry';
import {
  MigrationNoticeDedupe,
  interpretLegacyConfiguration,
  safeMigrationRuntimeSnapshot,
} from '../configurationMigrationRuntime';

function compatibleRegistry(): ConfigurationMigrationRegistry {
  return {
    schema_version: 'vscode_configuration_migration.v2',
    source_public_release: '0.17.0',
    target_release: '0.18.0',
    rows: [
      {
        migration_id: 'legacy_rename',
        old_key: 'perl-lsp.oldSetting',
        old_value_shape: 'boolean',
        introduced_version: '0.17.0',
        last_supported_version: '0.17.x',
        new_key_or_authority: 'perl-lsp.newSetting',
        old_scope: 'resource',
        new_scope: 'resource',
        migration_disposition: 'renamed_compatible',
        automatic_read_compatibility: true,
        explicit_write_allowed: false,
        old_plus_new_conflict_policy: 'current_wins',
        security_trust_class: 'ordinary',
        warning_reason_code: 'legacy_setting_renamed',
        compatibility_window: { kind: 'no_expiry' },
        expiry_owner_issue: 9000,
        installed_proof_requirement: '#9001',
      },
    ],
  };
}

function registryWithConflictPolicy(
  policy: ConfigurationMigrationRow['old_plus_new_conflict_policy'],
): ConfigurationMigrationRegistry {
  const base = compatibleRegistry();
  const row = base.rows[0];
  if (row === undefined) {
    throw new Error('compatibleRegistry must define one row');
  }
  return { ...base, rows: [{ ...row, old_plus_new_conflict_policy: policy }] };
}

function expiringRegistry(
  window: ConfigurationMigrationRow['compatibility_window'],
): ConfigurationMigrationRegistry {
  const base = compatibleRegistry();
  const row = base.rows[0]!;
  return {
    ...base,
    rows: [{ ...row, compatibility_window: window, expiry_owner_issue: 7838 }],
  };
}

describe('configuration migration runtime', () => {
  test.each([
    ['0.18.0-rc.1', 'compatible_legacy'],
    ['0.18.0', 'compatible_legacy'],
    ['0.18.0+build.7', 'compatible_legacy'],
    ['0.18.1', 'expired'],
  ] as const)('applies a versioned expiry threshold at %s', (extensionVersion, status) => {
    const result = interpretLegacyConfiguration(
      expiringRegistry({
        kind: 'through_extension_version',
        version: '0.18.0',
        post_expiry_disposition: 'action_required',
      }),
      {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
        extension_version: extensionVersion,
      },
    );

    expect(result.status).toBe(status);
    if (status === 'expired') {
      expect(result.canonical_value_present).toBe(false);
      expect(result.post_expiry_disposition).toBe('action_required');
    }
  });

  test.each([
    ['0.18.0-rc.2', 'compatible_legacy'],
    ['0.18.0-rc.10', 'compatible_legacy'],
    ['0.18.0-rc.11', 'expired'],
  ] as const)('orders numeric prerelease identifiers at %s', (extensionVersion, status) => {
    const result = interpretLegacyConfiguration(
      expiringRegistry({
        kind: 'through_extension_version',
        version: '0.18.0-rc.10',
        post_expiry_disposition: 'action_required',
      }),
      {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
        extension_version: extensionVersion,
      },
    );

    expect(result.status).toBe(status);
  });

  test('removed-in windows expire at the named version while through windows include it', () => {
    const input = {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource' as const,
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: false,
      current_value: null,
      extension_version: '0.18.0',
    };

    const through = interpretLegacyConfiguration(
      expiringRegistry({
        kind: 'through_extension_version',
        version: '0.18.0',
        post_expiry_disposition: 'action_required',
      }),
      input,
    );
    const removed = interpretLegacyConfiguration(
      expiringRegistry({
        kind: 'removed_in_extension_version',
        version: '0.18.0',
        post_expiry_disposition: 'action_required',
      }),
      input,
    );

    expect(through.status).toBe('compatible_legacy');
    expect(removed.status).toBe('expired');
  });

  test('build metadata is accepted and ignored for version precedence', () => {
    const result = interpretLegacyConfiguration(
      expiringRegistry({
        kind: 'removed_in_extension_version',
        version: '0.18.0+policy.3',
        post_expiry_disposition: 'action_required',
      }),
      {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
        extension_version: '0.17.0+local.1',
      },
    );

    expect(result.status).toBe('compatible_legacy');
  });

  test('current configuration remains authoritative after compatibility expiry', () => {
    const result = interpretLegacyConfiguration(
      expiringRegistry({
        kind: 'through_extension_version',
        version: '0.18.0',
        post_expiry_disposition: 'action_required',
      }),
      {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: true,
        current_value: 'current',
        extension_version: '0.18.1',
      },
    );

    expect(result).toMatchObject({
      status: 'expired',
      canonical_key_or_authority: 'perl-lsp.newSetting',
      canonical_value_present: true,
      canonical_value: 'current',
    });
  });

  test('expiry preserves a current value even when canonical authority metadata is absent', () => {
    const registry = expiringRegistry({
      kind: 'through_extension_version',
      version: '0.18.0',
      post_expiry_disposition: 'action_required',
    });
    registry.rows = [{ ...registry.rows[0]!, new_key_or_authority: null }];
    expect(validateMigrationRegistry(registry)).toEqual([]);

    const result = interpretLegacyConfiguration(registry, {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: true,
      current_value: 'current',
      extension_version: '0.18.1',
    });

    expect(result).toMatchObject({
      status: 'expired',
      canonical_value_present: true,
      canonical_value: 'current',
    });
  });

  test.each([
    ['migration_disposition', 'future_disposition'],
    ['old_scope', 'future_scope'],
    ['new_scope', 'future_scope'],
    ['old_plus_new_conflict_policy', 'future_policy'],
    ['security_trust_class', 'future_security_class'],
  ] as const)('unknown registry enum %s fails closed', (field, unknownValue) => {
    const registry = compatibleRegistry();
    registry.rows = [{ ...registry.rows[0]!, [field]: unknownValue } as never];

    const result = interpretLegacyConfiguration(registry, {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: false,
      current_value: null,
      extension_version: '0.18.0',
    });

    expect(result).toMatchObject({
      status: 'invalid',
      reason_code: 'legacy_registry_invalid',
      canonical_value_present: false,
    });
  });

  test('malformed expiry thresholds invalidate the registry', () => {
    const registry = expiringRegistry({
      kind: 'through_extension_version',
      version: 'not-a-version',
      post_expiry_disposition: 'action_required',
    });
    expect(validateMigrationRegistry(registry)).toContain(
      'migration expiry version is not valid SemVer: legacy_rename',
    );
    expect(
      interpretLegacyConfiguration(registry, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
        extension_version: '0.19.0',
      }),
    ).toMatchObject({
      status: 'invalid',
      reason_code: 'legacy_registry_invalid',
      canonical_value_present: false,
    });
  });

  test.each([undefined, 'not-a-version'])(
    'unknown extension version %s fails closed without asserting expiry',
    (extensionVersion) => {
      const invalid = interpretLegacyConfiguration(
        expiringRegistry({
          kind: 'through_extension_version',
          version: '0.18.0',
          post_expiry_disposition: 'action_required',
        }),
        {
          old_key: 'perl-lsp.oldSetting',
          source_scope: 'resource',
          legacy_value_present: true,
          legacy_value: 'legacy',
          current_value_present: false,
          current_value: null,
          ...(extensionVersion === undefined ? {} : { extension_version: extensionVersion }),
        },
      );
      expect(invalid).toMatchObject({
        status: 'invalid',
        reason_code: 'migration_extension_version_invalid',
        canonical_value_present: false,
        post_expiry_disposition: 'action_required',
      });
    },
  );

  test('missing extension version remains compatible for a row with no expiry', () => {
    expect(
      interpretLegacyConfiguration(compatibleRegistry(), {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({ status: 'compatible_legacy', canonical_value: 'legacy' });
  });

  test('unknown compatibility variants fail closed without exposing their payload', () => {
    const registry = expiringRegistry({
      kind: 'through_extension_version',
      version: '0.18.0',
      post_expiry_disposition: 'action_required',
    });
    registry.rows[0] = {
      ...registry.rows[0]!,
      compatibility_window: {
        kind: 'future_policy',
        version: '0.18.0',
        post_expiry_disposition: 'action_required',
      } as never,
    };

    const result = interpretLegacyConfiguration(registry, {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'secret legacy value',
      current_value_present: false,
      current_value: null,
      extension_version: '0.19.0',
    });

    expect(result).toMatchObject({
      status: 'invalid',
      reason_code: 'legacy_registry_invalid',
      canonical_value_present: false,
    });
    expect(JSON.stringify(safeMigrationRuntimeSnapshot(result))).not.toContain('secret legacy');
  });

  test('malformed rows fail closed before selection', () => {
    const registry = compatibleRegistry();
    registry.rows = [null as never];

    expect(
      interpretLegacyConfiguration(registry, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'secret legacy value',
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({
      status: 'invalid',
      reason_code: 'legacy_registry_invalid',
      canonical_value_present: false,
    });
  });

  test('missing or future registry envelopes fail closed', () => {
    const futureRegistry = {
      ...compatibleRegistry(),
      schema_version: 'vscode_configuration_migration.v3',
    } as never;

    expect(
      interpretLegacyConfiguration(futureRegistry, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: false,
        legacy_value: null,
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({ status: 'not_applicable' });

    expect(
      interpretLegacyConfiguration(futureRegistry, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({
      status: 'invalid',
      reason_code: 'legacy_registry_invalid',
      canonical_value_present: false,
    });
  });

  test('expiry ownership and network availability cannot affect runtime expiry', () => {
    const first = expiringRegistry({
      kind: 'through_extension_version',
      version: '0.18.0',
      post_expiry_disposition: 'action_required',
    });
    const second = {
      ...first,
      rows: [{ ...first.rows[0]!, expiry_owner_issue: 999999 }],
    };
    const unowned = {
      ...first,
      rows: [{ ...first.rows[0]!, expiry_owner_issue: null }],
    };

    for (const extensionVersion of ['0.17.0', '0.18.1']) {
      const input = {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource' as const,
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: false,
        current_value: null,
        extension_version: extensionVersion,
      };
      const status = interpretLegacyConfiguration(first, input).status;
      expect(interpretLegacyConfiguration(second, input).status).toBe(status);
      expect(interpretLegacyConfiguration(unowned, input).status).toBe(status);
    }
  });

  test('removed-inert expiry remains inert and unsupported expiry remains invalid', () => {
    const base = compatibleRegistry().rows[0]!;
    const inert = expiringRegistry({
      kind: 'removed_in_extension_version',
      version: '0.18.0',
      post_expiry_disposition: 'inert',
    });
    inert.rows = [
      {
        ...base,
        migration_disposition: 'removed_inert',
        automatic_read_compatibility: false,
        explicit_write_allowed: false,
        new_key_or_authority: null,
        new_scope: null,
        old_plus_new_conflict_policy: 'not_applicable',
        compatibility_window: inert.rows[0]!.compatibility_window,
      },
    ];
    expect(
      interpretLegacyConfiguration(inert, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'secret',
        current_value_present: true,
        current_value: 'must-not-become-authoritative',
        extension_version: '0.18.0',
      }),
    ).toMatchObject({
      status: 'expired',
      post_expiry_disposition: 'inert',
      canonical_value_present: false,
      canonical_value: null,
    });

    const unsupported: ConfigurationMigrationRegistry = {
      ...inert,
      rows: [
        {
          ...inert.rows[0]!,
          migration_disposition: 'unsupported_legacy_value',
          compatibility_window: {
            kind: 'removed_in_extension_version',
            version: '0.18.0',
            post_expiry_disposition: 'invalid',
          },
        },
      ],
    };
    expect(
      interpretLegacyConfiguration(unsupported, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: true,
        current_value: 'must-not-become-authoritative',
        extension_version: '0.18.0',
      }),
    ).toMatchObject({
      status: 'expired',
      post_expiry_disposition: 'invalid',
      canonical_value_present: false,
      canonical_value: null,
    });
  });

  test('keeps removed MCP process-execution settings inert without carrying their value', () => {
    const secretLegacyValue = [
      { label: 'private', command: '/private/tool', env: { TOKEN: 'secret' } },
    ];
    const result = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'machine',
      legacy_value_present: true,
      legacy_value: secretLegacyValue,
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      migration_id: 'v017_mcp_servers_removed',
      status: 'inert',
      canonical_key_or_authority: null,
      canonical_value_present: false,
      canonical_value: null,
      reason_code: 'legacy_mcp_passthrough_removed',
      notice_required: true,
      disk_write_allowed: false,
    });
    expect(JSON.stringify(safeMigrationRuntimeSnapshot(result))).not.toContain('private/tool');
    expect(JSON.stringify(safeMigrationRuntimeSnapshot(result))).not.toContain('secret');
  });

  test('uses compatible legacy value when the current key is absent', () => {
    const result = interpretLegacyConfiguration(compatibleRegistry(), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: true,
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      status: 'compatible_legacy',
      canonical_key_or_authority: 'perl-lsp.newSetting',
      canonical_value_present: true,
      canonical_value: true,
      disk_write_allowed: false,
    });
  });

  test('current canonical key wins explicitly without mutating disk', () => {
    const result = interpretLegacyConfiguration(compatibleRegistry(), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: true,
      current_value_present: true,
      current_value: false,
    });

    expect(result).toMatchObject({
      status: 'compatible_current_wins',
      canonical_value_present: true,
      canonical_value: false,
      disk_write_allowed: false,
    });
  });

  test('a registry the validator certifies is never blamed on the user as an unknown key', () => {
    // The registry's uniqueness key spans the version window and value shape, so one
    // setting may legitimately carry a row per historical era. Such a registry is valid,
    // but this interpreter takes no version input and cannot choose between the eras.
    const multiEra: ConfigurationMigrationRegistry = (() => {
      const base = compatibleRegistry();
      const row = base.rows[0];
      if (row === undefined) {
        throw new Error('compatibleRegistry must define one row');
      }
      return {
        ...base,
        rows: [
          { ...row, migration_id: 'era_a', introduced_version: '0.15.0' },
          {
            ...row,
            migration_id: 'era_b',
            introduced_version: '0.16.0',
            old_value_shape: 'string',
          },
        ],
      };
    })();

    // Load-bearing: the two modules must not disagree about what a valid registry is.
    expect(validateMigrationRegistry(multiEra)).toEqual([]);

    const result = interpretLegacyConfiguration(multiEra, {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: true,
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      status: 'invalid',
      canonical_value_present: false,
      notice_required: true,
      // Not `legacy_key_not_registered`: the key is registered. Reporting a registry
      // defect as an unknown user setting sends the user to fix the wrong thing.
      reason_code: 'legacy_registry_ambiguous',
    });
  });

  test('an unregistered legacy key names itself as unregistered', () => {
    expect(
      interpretLegacyConfiguration(compatibleRegistry(), {
        old_key: 'perl-lsp.neverShipped',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: true,
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({ status: 'invalid', reason_code: 'legacy_key_not_registered' });
  });

  test('wrong legacy scope is invalid rather than widening authority', () => {
    const result = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'workspace',
      legacy_value_present: true,
      legacy_value: [],
      current_value_present: false,
      current_value: null,
    });

    expect(result).toMatchObject({
      migration_id: null,
      status: 'invalid',
      canonical_value_present: false,
      notice_required: true,
      // This is a process-executing machine-scoped setting found in repository-controlled
      // workspace configuration. The notice has to be able to say that, so the cause must
      // survive as more than a bare `invalid`.
      reason_code: 'legacy_key_scope_not_permitted',
    });
  });

  test('no legacy value means migration is not applicable', () => {
    expect(
      interpretLegacyConfiguration(compatibleRegistry(), {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: false,
        legacy_value: null,
        current_value_present: false,
        current_value: null,
      }),
    ).toMatchObject({
      migration_id: null,
      status: 'not_applicable',
      canonical_value_present: false,
      notice_required: false,
    });
  });

  test.each([['not_applicable' as const], ['current_wins' as const]])(
    'a %s conflict policy never lets the legacy value silently outrank the current key',
    (policy) => {
      const result = interpretLegacyConfiguration(registryWithConflictPolicy(policy), {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: 'legacy',
        current_value_present: true,
        current_value: 'current',
      });

      expect(result).toMatchObject({
        status: 'compatible_current_wins',
        canonical_value_present: true,
        canonical_value: 'current',
        disk_write_allowed: false,
      });
    },
  );

  test('an explicit legacy_only policy is the only way the legacy value wins', () => {
    const result = interpretLegacyConfiguration(registryWithConflictPolicy('legacy_only'), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: true,
      current_value: 'current',
    });

    expect(result).toMatchObject({
      status: 'compatible_legacy',
      canonical_value_present: true,
      canonical_value: 'legacy',
      disk_write_allowed: false,
    });
  });

  test('an action_required conflict policy coerces neither value', () => {
    const result = interpretLegacyConfiguration(registryWithConflictPolicy('action_required'), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: 'legacy',
      current_value_present: true,
      current_value: 'current',
    });

    expect(result).toMatchObject({
      status: 'action_required',
      canonical_value_present: false,
      canonical_value: null,
      notice_required: true,
    });
  });

  test('an invalid legacy key can actually surface its required notice exactly once', () => {
    const runtime = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'workspace',
      legacy_value_present: true,
      legacy_value: [],
      current_value_present: false,
      current_value: null,
    });
    const dedupe = new MigrationNoticeDedupe();

    expect(runtime).toMatchObject({ migration_id: null, notice_required: true });
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(false);
  });

  test('distinct invalid legacy keys do not suppress each others notices', () => {
    const dedupe = new MigrationNoticeDedupe();
    const invalidFor = (oldKey: string) =>
      interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
        old_key: oldKey,
        source_scope: 'workspace',
        legacy_value_present: true,
        legacy_value: [],
        current_value_present: false,
        current_value: null,
      });

    expect(dedupe.shouldShow(invalidFor('perl-lsp.unknownAlpha'), 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(invalidFor('perl-lsp.unknownBeta'), 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(invalidFor('perl-lsp.unknownAlpha'), 'generation-1')).toBe(false);
  });

  test('the safe snapshot carries the legacy key but never the legacy value', () => {
    const runtime = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'machine',
      legacy_value_present: true,
      legacy_value: [{ label: 'x', command: '/opt/secret-tool', env: { TOKEN: 'hunter2' } }],
      current_value_present: false,
      current_value: null,
    });
    const snapshot = safeMigrationRuntimeSnapshot(runtime);

    expect(snapshot.legacy_key).toBe('perl-lsp.mcp.servers');
    expect(JSON.stringify(snapshot)).not.toContain('secret-tool');
    expect(JSON.stringify(snapshot)).not.toContain('hunter2');
  });

  test('the safe snapshot redacts a compatible legacy value, not only inert ones', () => {
    // A compatible result DOES carry the legacy value at runtime; a snapshot
    // implementation that leaks canonical_value only fails here, never on the
    // inert fixtures above.
    const secret = 'hunter2-compatible-legacy-secret';
    const runtime = interpretLegacyConfiguration(compatibleRegistry(), {
      old_key: 'perl-lsp.oldSetting',
      source_scope: 'resource',
      legacy_value_present: true,
      legacy_value: secret,
      current_value_present: false,
      current_value: null,
    });

    expect(runtime.status).toBe('compatible_legacy');
    expect(runtime.canonical_value).toBe(secret);
    const snapshot = safeMigrationRuntimeSnapshot(runtime);
    expect('canonical_value' in snapshot).toBe(false);
    expect(JSON.stringify(snapshot)).not.toContain(secret);
  });

  test.each([
    {
      name: 'renamed_compatible without automatic read compatibility',
      disposition: 'renamed_compatible',
      automaticReadCompatibility: false,
      expectedStatus: 'action_required',
    },
    {
      name: 'renamed_requires_user_action',
      disposition: 'renamed_requires_user_action',
      automaticReadCompatibility: true,
      expectedStatus: 'action_required',
    },
    {
      name: 'replaced_by_standard_vscode_setting',
      disposition: 'replaced_by_standard_vscode_setting',
      automaticReadCompatibility: true,
      expectedStatus: 'action_required',
    },
    {
      name: 'replaced_by_server_or_project_config',
      disposition: 'replaced_by_server_or_project_config',
      automaticReadCompatibility: true,
      expectedStatus: 'action_required',
    },
    {
      name: 'unsupported_legacy_value',
      disposition: 'unsupported_legacy_value',
      automaticReadCompatibility: true,
      expectedStatus: 'invalid',
    },
  ] as const)(
    'disposition never silently adopts the legacy value: $name',
    ({ disposition, automaticReadCompatibility, expectedStatus }) => {
      const base = compatibleRegistry();
      const row = base.rows[0];
      if (row === undefined) {
        throw new Error('compatibleRegistry must define one row');
      }
      const registry: ConfigurationMigrationRegistry = {
        ...base,
        rows: [
          {
            ...row,
            migration_disposition: disposition,
            automatic_read_compatibility: automaticReadCompatibility,
          },
        ],
      };

      const result = interpretLegacyConfiguration(registry, {
        old_key: 'perl-lsp.oldSetting',
        source_scope: 'resource',
        legacy_value_present: true,
        legacy_value: true,
        current_value_present: false,
        current_value: null,
      });

      // A wrong implementation that returns compatible_legacy with the legacy
      // value fails every row of this table.
      expect(result).toMatchObject({
        status: expectedStatus,
        canonical_value_present: false,
        canonical_value: null,
        notice_required: true,
        disk_write_allowed: false,
      });
    },
  );

  test('deduplicates migration notices per migration and configuration generation', () => {
    const runtime = interpretLegacyConfiguration(V018_CONFIGURATION_MIGRATIONS, {
      old_key: 'perl-lsp.mcp.servers',
      source_scope: 'machine',
      legacy_value_present: true,
      legacy_value: [],
      current_value_present: false,
      current_value: null,
    });
    const dedupe = new MigrationNoticeDedupe();

    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(true);
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(false);
    expect(dedupe.shouldShow(runtime, 'generation-2')).toBe(true);
    dedupe.clear();
    expect(dedupe.shouldShow(runtime, 'generation-1')).toBe(true);
  });
});
