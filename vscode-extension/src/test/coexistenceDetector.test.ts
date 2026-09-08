import {
  COEXISTENCE_CLAIM_BOUNDARY,
  buildRedactedCoexistencePacket,
  coexistenceConflictKey,
} from '../coexistenceRegistry';
import {
  detectCoexistenceFindings,
  type CoexistenceObservations,
  type ObservedExtension,
} from '../coexistenceDetector';

const SELF = 'effortlessmetrics.perl-lsp-rs';

function extension(id: string, extra: Partial<ObservedExtension> = {}): ObservedExtension {
  return { id, isActive: true, ...extra };
}

function base(overrides: Partial<CoexistenceObservations> = {}): CoexistenceObservations {
  return { selfExtensionId: SELF, ...overrides };
}

function classes(findings: ReturnType<typeof detectCoexistenceFindings>): string[] {
  return findings.map((finding) => finding.conflictClass);
}

describe('typed coexistence detection (#7214)', () => {
  test('native extension only yields no findings', () => {
    expect(detectCoexistenceFindings(base({ installedExtensions: [extension(SELF)] }))).toEqual([]);
  });

  test('known alternate Perl LSP identities are detected by exact id', () => {
    const findings = detectCoexistenceFindings(
      base({
        installedExtensions: [
          extension('bscan.perlnavigator'),
          extension('richterger.perl'),
          extension('fractalboy.pls'),
        ],
      }),
    );
    // Three reviewed language-server identities, plus one combined
    // diagnostic-provider finding for the two third-party providers.
    expect(classes(findings)).toEqual([
      'multiple_language_servers',
      'multiple_language_servers',
      'multiple_language_servers',
      'multiple_diagnostic_providers',
    ]);
    const pls = findings.find((finding) => finding.otherOwner === 'PLS');
    expect(pls?.registryReasonCode).toBe('runtime_enablement_forbidden');
    expect(pls?.requiresReload).toBe(true);
  });

  test('unreviewed extensions and PL* spellings alone classify nothing', () => {
    const findings = detectCoexistenceFindings(
      base({
        installedExtensions: [
          extension('vendor.perl-tools-plus'),
          extension('other.pls-wrapper'),
          extension('acme.perlcriticizer'),
        ],
      }),
    );
    expect(findings).toEqual([]);
  });

  test('two third-party diagnostic providers are reported as one subject-bound class', () => {
    const findings = detectCoexistenceFindings(
      base({
        installedExtensions: [extension('bscan.perlnavigator'), extension('richterger.perl')],
      }),
    );
    const provider = findings.find(
      (finding) => finding.conflictClass === 'multiple_diagnostic_providers',
    );
    expect(provider?.subject).toBe('bscan.perlnavigator+richterger.perl');
    expect(provider?.scopeKind).toBe('user');
  });

  test('both format-on-save owners are detected with the canonical owner name', () => {
    const findings = detectCoexistenceFindings(
      base({
        nativeFormatOnSave: true,
        editorFormatOnSave: true,
        defaultFormatterSetting: 'bscan.perlnavigator',
      }),
    );
    expect(classes(findings)).toEqual(['multiple_format_on_save_owners']);
    expect(findings[0]?.otherOwner).toBe('Perl Navigator');
  });

  test('an explicit default that cannot own Perl formatting classifies nothing', () => {
    const saveOwners = (observations: CoexistenceObservations) =>
      detectCoexistenceFindings(observations).filter(
        (finding) => finding.conflictClass === 'multiple_format_on_save_owners',
      );
    // A common non-Perl default and a stale marketplace id both lack any
    // evidence of Perl formatting ownership, so neither is a conflict owner
    // (the installed navigator still yields its own inventory-derived class).
    for (const override of ['esbenp.prettier-vscode', 'vendor.retired-formatter']) {
      expect(
        saveOwners(
          base({
            nativeFormatOnSave: true,
            editorFormatOnSave: true,
            installedExtensions: [extension('bscan.perlnavigator')],
            defaultFormatterSetting: override,
          }),
        ),
      ).toEqual([]);
    }
    // The host provider feed is accepted as evidence even without a
    // reviewed registry row.
    const feedObserved = saveOwners(
      base({
        nativeFormatOnSave: true,
        editorFormatOnSave: true,
        defaultFormatterSetting: 'Vendor.Perl-Formatter',
        perlFormatterProviderIds: ['vendor.perl-formatter'],
      }),
    );
    expect(feedObserved).toHaveLength(1);
    expect(feedObserved[0]?.otherOwner).toBe('Vendor.Perl-Formatter');
  });

  test('a sole other formatter owns save when no default is set', () => {
    const findings = detectCoexistenceFindings(
      base({
        nativeFormatOnSave: true,
        editorFormatOnSave: true,
        installedExtensions: [extension('bscan.perlnavigator')],
      }),
    );
    // Navigator is also a reviewed language server, so both classes apply.
    expect(classes(findings)).toContain('multiple_format_on_save_owners');
    const saveFinding = findings.find(
      (finding) => finding.conflictClass === 'multiple_format_on_save_owners',
    );
    expect(saveFinding?.otherOwner).toBe('Perl Navigator');
  });

  test('ambiguous formatter fleets surface as unknown_possible_overlap', () => {
    const findings = detectCoexistenceFindings(
      base({
        nativeFormatOnSave: true,
        editorFormatOnSave: true,
        perlFormatterProviderIds: ['vendor.formatter-two', 'vendor.formatter-three'],
      }),
    );
    expect(classes(findings)).toEqual(['unknown_possible_overlap']);
  });

  test('the formatter-provider feed excludes the native identity before counting', () => {
    expect(
      detectCoexistenceFindings(
        base({
          nativeFormatOnSave: true,
          editorFormatOnSave: true,
          perlFormatterProviderIds: ['EffortlessMetrics.perl-lsp-rs'],
        }),
      ),
    ).toEqual([]);
  });

  test('format-on-save ownership requires both save channels to be on', () => {
    for (const nativeFormatOnSave of [false, undefined]) {
      expect(
        detectCoexistenceFindings(
          base({ nativeFormatOnSave, editorFormatOnSave: true, defaultFormatterSetting: 'x.y' }),
        ),
      ).toEqual([]);
    }
    expect(
      detectCoexistenceFindings(base({ nativeFormatOnSave: true, editorFormatOnSave: false })),
    ).toEqual([]);
  });

  test('native critic plus a separate known critic diagnostic provider is a conflict', () => {
    const findings = detectCoexistenceFindings(
      base({
        nativeCriticEnabled: true,
        installedExtensions: [extension('bscan.perlnavigator')],
      }),
    );
    const critic = findings.find(
      (finding) => finding.conflictClass === 'native_critic_and_other_diagnostic_provider',
    );
    expect(critic?.subject).toBe('bscan.perlnavigator');
    expect(critic?.evidenceSource).toContain('perl-lsp.critic.enabled');
  });

  test('the same provider without the native critic is not a critic conflict', () => {
    const findings = detectCoexistenceFindings(
      base({
        nativeCriticEnabled: false,
        installedExtensions: [extension('bscan.perlnavigator')],
      }),
    );
    expect(
      findings.filter(
        (finding) => finding.conflictClass === 'native_critic_and_other_diagnostic_provider',
      ),
    ).toEqual([]);
  });

  test.each(['legacy', 'external', 'perlcritic'])(
    'retired first-party engine value %s routes to migration guidance',
    (value) => {
      const findings = detectCoexistenceFindings(base({ staleCriticEngineValue: value }));
      expect(classes(findings)).toEqual(['legacy_first_party_critic_setting_active']);
      expect(findings[0]?.remediationChoices).toContain('open_stale_setting_migration');
      expect(findings[0]?.registryReasonCode).toBe('runtime_enablement_forbidden');
      // The alias cannot construct runtime state (#8253): no reload required.
      expect(findings[0]?.requiresReload).toBe(false);
    },
  );

  test.each(['native', '', '  ', 'engine-of-the-future'])(
    'non-retired engine values produce nothing (%s)',
    (value) => {
      expect(detectCoexistenceFindings(base({ staleCriticEngineValue: value }))).toEqual([]);
    },
  );

  test('only perlcritic present on PATH is never a conflict', () => {
    expect(
      detectCoexistenceFindings(
        base({
          pathToolResidue: ['perlcritic'],
          criticConfigResiduePresent: false,
          nativeCriticEnabled: true,
          installedExtensions: [extension(SELF)],
        }),
      ),
    ).toEqual([]);
  });

  test('.perlcriticrc with no external tool is never an external provider', () => {
    expect(
      detectCoexistenceFindings(
        base({
          criticConfigResiduePresent: true,
          pathToolResidue: [],
          nativeCriticEnabled: true,
        }),
      ),
    ).toEqual([]);
  });

  test('perlcritic PATH residue plus .perlcriticrc still produces zero findings', () => {
    expect(
      detectCoexistenceFindings(
        base({
          pathToolResidue: ['perlcritic', 'perltidy'],
          criticConfigResiduePresent: true,
          nativeCriticEnabled: true,
        }),
      ),
    ).toEqual([]);
  });

  test('an unselected .perltidyrc candidate is folder-scoped and advisory', () => {
    const findings = detectCoexistenceFindings(
      base({ perltidyrcPresentInFolder: true, folderName: 'root-a' }),
    );
    expect(classes(findings)).toEqual(['external_tool_candidate_not_selected']);
    expect(findings[0]?.scopeKind).toBe('workspace-folder');
    expect(findings[0]?.folderName).toBe('root-a');
    expect(findings[0]?.registryReasonCode).toBe('explicit_adapter_only');
    expect(findings[0]?.risk).toContain('No behavior conflict');
  });

  test('a selected perltidy profile is not reported as unselected', () => {
    expect(
      detectCoexistenceFindings(
        base({ perltidyrcPresentInFolder: true, perltidyProfileSelected: true }),
      ),
    ).toEqual([]);
  });

  test('another debugger contribution with exact type "perl" is a conflict', () => {
    const findings = detectCoexistenceFindings(
      base({ installedExtensions: [extension('richterger.perl', { debuggerTypes: ['perl'] })] }),
    );
    expect(classes(findings)).toEqual(
      expect.arrayContaining(['multiple_perl_debugger_contributions']),
    );
    const debuggerFinding = findings.find(
      (finding) => finding.conflictClass === 'multiple_perl_debugger_contributions',
    );
    expect(debuggerFinding?.nativeOwner).toBe('perl-dap (native)');
  });

  test('ptkdb peer configuration and near-miss debugger types are not competing servers', () => {
    expect(
      detectCoexistenceFindings(
        base({
          installedExtensions: [
            extension('devel.ptkdb-peer', { debuggerTypes: ['perl-peer', 'perl-dap-peer'] }),
            extension('vendor.ptkdb'),
          ],
        }),
      ),
    ).toEqual([]);
  });

  test('findings bind to their exact root in multi-root workspaces', () => {
    const rootA = detectCoexistenceFindings(
      base({ perltidyrcPresentInFolder: true, folderName: 'root-a' }),
    );
    const rootB = detectCoexistenceFindings(
      base({ perltidyrcPresentInFolder: true, folderName: 'root-b' }),
    );
    expect(rootA).toHaveLength(1);
    expect(rootB).toHaveLength(1);
    const findingA = rootA[0];
    const findingB = rootB[0];
    if (!findingA || !findingB) {
      throw new Error('expected one finding per root');
    }
    const keyA = coexistenceConflictKey(findingA);
    const keyB = coexistenceConflictKey(findingB);
    expect(keyA).toContain('root-a');
    expect(keyB).toContain('root-b');
    // A conflict observed on root A is never the subject reported for root B.
    expect(keyA).not.toBe(keyB);
  });

  test('suppression identity binds class, scope, folder, and subject exactly', () => {
    const findings = detectCoexistenceFindings(
      base({
        perltidyrcPresentInFolder: true,
        folderName: 'root-a',
        staleCriticEngineValue: 'legacy',
      }),
    );
    const keys = findings.map(coexistenceConflictKey);
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys.some((key) => key.includes('root-a'))).toBe(true);
    expect(keys.every((key) => !key.includes('root-b'))).toBe(true);
  });

  test('settings-derived findings keep the scope they were observed in', () => {
    const folderObservations = base({
      installedExtensions: [extension('bscan.perlnavigator')],
      nativeCriticEnabled: true,
      nativeFormatOnSave: true,
      editorFormatOnSave: true,
      staleCriticEngineValue: 'legacy',
      folderName: 'root-a',
    });
    const scopedClasses = new Set([
      'native_critic_and_other_diagnostic_provider',
      'multiple_format_on_save_owners',
      'legacy_first_party_critic_setting_active',
    ]);
    for (const finding of detectCoexistenceFindings(folderObservations)) {
      if (!scopedClasses.has(finding.conflictClass)) {
        continue;
      }
      expect(finding.scopeKind).toBe('workspace-folder');
      expect(finding.folderName).toBe('root-a');
    }
    // The same settings without a folder stay host-wide.
    for (const finding of detectCoexistenceFindings({
      ...folderObservations,
      folderName: undefined,
    })) {
      if (!scopedClasses.has(finding.conflictClass)) {
        continue;
      }
      expect(finding.scopeKind).toBe('user');
      expect(finding.folderName).toBeUndefined();
    }
  });

  test('every finding names its evidence source and carries the claim boundary', () => {
    const findings = detectCoexistenceFindings(
      base({
        installedExtensions: [extension('bscan.perlnavigator')],
        nativeCriticEnabled: true,
        nativeFormatOnSave: true,
        editorFormatOnSave: true,
        staleCriticEngineValue: 'external',
        perltidyrcPresentInFolder: true,
        folderName: 'root-a',
      }),
    );
    expect(classes(findings).sort()).toEqual([
      'external_tool_candidate_not_selected',
      'legacy_first_party_critic_setting_active',
      'multiple_format_on_save_owners',
      'multiple_language_servers',
      'native_critic_and_other_diagnostic_provider',
    ]);
    for (const finding of findings) {
      expect(finding.evidenceSource.length).toBeGreaterThan(0);
      expect(finding.claimBoundary).toBe(COEXISTENCE_CLAIM_BOUNDARY);
      expect(finding.remediationChoices.length).toBeGreaterThan(0);
    }
  });

  test('remediation stays advisory: no mutation and no external critic engine offer', () => {
    const findings = detectCoexistenceFindings(
      base({
        installedExtensions: [extension('bscan.perlnavigator'), extension('richterger.perl')],
        nativeCriticEnabled: true,
        nativeFormatOnSave: true,
        editorFormatOnSave: true,
        staleCriticEngineValue: 'legacy',
        perltidyrcPresentInFolder: true,
        folderName: 'root-a',
      }),
    );
    const allowed = new Set([
      'keep_native_provider',
      'open_conflicting_extension',
      'open_stale_setting_migration',
      'show_provider_tool_status',
      'show_critic_compatibility_status',
      'show_perltidy_compatibility_status',
      'disable_warning_for_exact_conflict',
      'copy_redacted_support_packet',
    ]);
    for (const finding of findings) {
      for (const choice of finding.remediationChoices) {
        expect(allowed.has(choice)).toBe(true);
      }
    }
    const serialized = JSON.stringify(findings);
    expect(serialized.toLowerCase()).not.toContain('select external perl::critic');
    expect(serialized).not.toContain('Configure External Perl::Critic');
    // PLS is a retired oracle, never a supported resolution.
    expect(serialized).not.toContain('install PLS');
  });

  test('detection is deterministic across repeated evaluation', () => {
    const observations = base({
      installedExtensions: [extension('bscan.perlnavigator')],
      nativeCriticEnabled: true,
    });
    expect(detectCoexistenceFindings(observations)).toEqual(
      detectCoexistenceFindings(observations),
    );
  });
});

describe('redacted support packet (#7214)', () => {
  test('carries conflict identity without paths, folders, or inventory', () => {
    const findings = detectCoexistenceFindings(
      base({
        installedExtensions: [extension('bscan.perlnavigator')],
        nativeCriticEnabled: true,
        perltidyrcPresentInFolder: true,
        folderName: '/home/dev/private-project',
      }),
    );
    const packet = buildRedactedCoexistencePacket(findings);
    expect(packet).toContain('"schema_version": "perl_lsp_coexistence_packet.v1"');
    expect(packet).toContain('Perl Navigator');
    expect(packet).not.toContain('/home/dev/private-project');
    expect(packet).not.toContain('folderName');
    expect(packet).not.toContain('effortlessmetrics.perl-lsp-rs');
  });
});
