/**
 * Pure coexistence detection over observed host facts (#7214).
 *
 * The detector consumes a bounded observation snapshot and returns typed,
 * deduplicated findings. It performs no I/O, reads no environment, executes
 * nothing, and never mutates state. Classification rules encode the #7214
 * ruling exactly:
 *
 * - PATH-discovered `perlcritic`/`perltidy` residue is not a conflict;
 * - `.perlcriticrc` is first-party configuration-compatibility evidence, never
 *   an external provider;
 * - ptkdb peer configuration is not a competing DAP server (peer configs are
 *   not even representable as observations);
 * - `PL*` spelling alone classifies nothing — only exact reviewed identities
 *   or exact declared debugger types count.
 */

import {
  COEXISTENCE_CLAIM_BOUNDARY,
  NATIVE_EXTENSION_ID,
  REGISTRY_REASON_CODES,
  reviewedPerlExtension,
  type CoexistenceFinding,
  type ReviewedPerlExtension,
} from './coexistenceRegistry';

/**
 * Retired first-party critic-engine aliases (#8253/#9072). A stored value in
 * this set is a migration observation, never a runtime engine selection.
 */
const RETIRED_CRITIC_ENGINE_ALIASES: readonly string[] = ['legacy', 'external', 'perlcritic'];

export interface ObservedExtension {
  /** Lowercase-normalized marketplace id. */
  readonly id: string;
  readonly isActive: boolean;
  /** Declared `contributes.debuggers[].type` values, if any. */
  readonly debuggerTypes?: readonly string[] | undefined;
}

export interface CoexistenceObservations {
  readonly selfExtensionId: string;
  /** Host inventory restricted to what the collector could establish. */
  readonly installedExtensions?: readonly ObservedExtension[] | undefined;
  /**
   * Exact extension ids the host established as Perl document-formatting
   * providers. The current VS Code API cannot enumerate runtime provider
   * registrations, so the shipped collector leaves this unset; a future host
   * feed may populate it without changing these rules.
   */
  readonly perlFormatterProviderIds?: readonly string[] | undefined;
  readonly nativeFormatOnSave?: boolean | undefined;
  readonly editorFormatOnSave?: boolean | undefined;
  readonly defaultFormatterSetting?: string | undefined;
  readonly nativeCriticEnabled?: boolean | undefined;
  readonly staleCriticEngineValue?: string | undefined;
  readonly perltidyProfileSelected?: boolean | undefined;
  readonly perltidyrcPresentInFolder?: boolean | undefined;
  /**
   * Advisory tool-name residue (e.g. from a future doctor feed). It can never
   * classify into a provider; the detector provably ignores it.
   */
  readonly pathToolResidue?: readonly string[] | undefined;
  /** `.perlcriticrc` presence. First-party compatibility evidence only. */
  readonly criticConfigResiduePresent?: boolean | undefined;
  readonly folderName?: string | undefined;
}

const NATIVE_FORMAT_OWNER = 'perl-lsp native formatter';

function finding(
  partial: Omit<CoexistenceFinding, 'claimBoundary'> & { claimBoundary?: string },
): CoexistenceFinding {
  return { claimBoundary: COEXISTENCE_CLAIM_BOUNDARY, ...partial };
}

function otherReviewedExtensions(observations: CoexistenceObservations): ReviewedPerlExtension[] {
  const self = observations.selfExtensionId.trim().toLowerCase();
  const seen = new Set<string>();
  const found: ReviewedPerlExtension[] = [];
  for (const extension of observations.installedExtensions ?? []) {
    const id = extension.id.trim().toLowerCase();
    if (id === self || seen.has(id)) {
      continue;
    }
    const identity = reviewedPerlExtension(id);
    if (identity) {
      seen.add(id);
      found.push(identity);
    }
  }
  return found;
}

function languageServerFindings(others: readonly ReviewedPerlExtension[]): CoexistenceFinding[] {
  return others
    .filter((identity) => identity.domains.includes('language_server'))
    .map((identity) => {
      // richterger.perl and fractalboy.pls are clients of the retired
      // Perl::LanguageServer runtime; their rows carry the mirrored #7209
      // doctor reason code (conformance oracle only, never a runtime backend).
      return finding({
        conflictClass: 'multiple_language_servers',
        scopeKind: 'user',
        subject: identity.extensionId,
        nativeOwner: 'perl-lsp (native)',
        otherOwner: identity.canonicalName,
        evidenceSource: `VS Code extension inventory: exact reviewed identity ${identity.extensionId}`,
        symptom:
          'Two language servers may publish overlapping diagnostics, navigation, and completion for Perl.',
        risk: 'Duplicated or contradictory results; unclear ownership of actions and quick fixes.',
        remediationChoices: [
          'keep_native_provider',
          'open_conflicting_extension',
          'show_provider_tool_status',
          'disable_warning_for_exact_conflict',
        ],
        requiresReload: true,
        ...(identity.registryReasonCode === undefined
          ? {}
          : { registryReasonCode: identity.registryReasonCode }),
      });
    });
}

function diagnosticProviderFindings(
  others: readonly ReviewedPerlExtension[],
): CoexistenceFinding[] {
  const providers = others.filter((identity) => identity.domains.includes('diagnostics'));
  if (providers.length < 2) {
    return [];
  }
  const subject = providers
    .map((identity) => identity.extensionId)
    .sort()
    .join('+');
  return [
    finding({
      conflictClass: 'multiple_diagnostic_providers',
      scopeKind: 'user',
      subject,
      nativeOwner: 'perl-lsp (native)',
      otherOwner: providers
        .map((identity) => identity.canonicalName)
        .sort()
        .join(' + '),
      evidenceSource:
        'VS Code extension inventory: multiple reviewed diagnostic-capable identities',
      symptom: 'More than one third-party provider may publish diagnostics for the same Perl file.',
      risk: 'Interleaved squiggles and duplicated findings that are hard to attribute.',
      remediationChoices: [
        'keep_native_provider',
        'open_conflicting_extension',
        'show_provider_tool_status',
        'disable_warning_for_exact_conflict',
      ],
      requiresReload: true,
    }),
  ];
}

function observedScope(observations: CoexistenceObservations): {
  scopeKind: CoexistenceFinding['scopeKind'];
  folderName?: string | undefined;
} {
  return observations.folderName === undefined
    ? { scopeKind: 'user' }
    : { scopeKind: 'workspace-folder', folderName: observations.folderName };
}

function criticFindings(
  others: readonly ReviewedPerlExtension[],
  observations: CoexistenceObservations,
): CoexistenceFinding[] {
  if (!observations.nativeCriticEnabled) {
    return [];
  }
  return others
    .filter((identity) => identity.domains.includes('critic_diagnostics'))
    .map((identity) =>
      finding({
        conflictClass: 'native_critic_and_other_diagnostic_provider',
        ...observedScope(observations),
        subject: identity.extensionId,
        nativeOwner: 'perl-lsp native Perl::Critic analysis',
        otherOwner: identity.canonicalName,
        evidenceSource: `settings inspection (perl-lsp.critic.enabled) + reviewed identity ${identity.extensionId}`,
        symptom:
          'The native critic and another provider may both publish Perl::Critic-like diagnostics.',
        risk: 'Duplicated logical findings with different severities and rule spellings.',
        remediationChoices: [
          'keep_native_provider',
          'show_critic_compatibility_status',
          'open_conflicting_extension',
          'disable_warning_for_exact_conflict',
        ],
        requiresReload: false,
      }),
    );
}

function resolveSaveConflictOtherOwner(
  observations: CoexistenceObservations,
): { owner: string } | { ambiguous: true } | undefined {
  const override = observations.defaultFormatterSetting?.trim();
  if (override && override.toLowerCase() !== NATIVE_EXTENSION_ID) {
    // An explicit default counts as an observed Perl-formatting owner only
    // with evidence that the selected extension actually owns Perl
    // formatting: a reviewed registry row with the formatting domain, or the
    // host-established provider feed. Anything else (a stale id or a
    // non-Perl default such as a web formatter) classifies nothing.
    const identity = reviewedPerlExtension(override);
    const ownsPerlFormatting =
      (identity !== undefined && identity.domains.includes('formatting')) ||
      (observations.perlFormatterProviderIds ?? []).some(
        (id) => id.trim().toLowerCase() === override.toLowerCase(),
      );
    if (!ownsPerlFormatting) {
      return undefined;
    }
    return { owner: identity?.canonicalName ?? override };
  }
  const formatterIds =
    observations.perlFormatterProviderIds ??
    otherReviewedExtensions(observations)
      .filter((identity) => identity.domains.includes('formatting'))
      .map((identity) => identity.extensionId);
  const self = observations.selfExtensionId.trim().toLowerCase();
  const others = formatterIds.filter((id) => id.trim().toLowerCase() !== self);
  const [sole] = others;
  if (others.length === 1 && sole !== undefined) {
    return { owner: reviewedPerlExtension(sole)?.canonicalName ?? sole };
  }
  if (others.length >= 2) {
    return { ambiguous: true };
  }
  return undefined;
}

function formatOnSaveFindings(observations: CoexistenceObservations): CoexistenceFinding[] {
  if (!observations.nativeFormatOnSave || !observations.editorFormatOnSave) {
    return [];
  }
  const resolved = resolveSaveConflictOtherOwner(observations);
  if (!resolved) {
    return [];
  }
  if ('ambiguous' in resolved) {
    return [
      finding({
        conflictClass: 'unknown_possible_overlap',
        ...observedScope(observations),
        subject: 'perl format-on-save formatter selection',
        nativeOwner: NATIVE_FORMAT_OWNER,
        evidenceSource:
          'settings inspection (editor.formatOnSave) + multiple reviewed formatter identities without an explicit editor.defaultFormatter',
        symptom:
          'Multiple formatters can format Perl on save; VS Code will ask which one to use each time.',
        risk: 'Non-deterministic save formatting across sessions.',
        remediationChoices: ['show_provider_tool_status', 'disable_warning_for_exact_conflict'],
        requiresReload: false,
      }),
    ];
  }
  return [
    finding({
      conflictClass: 'multiple_format_on_save_owners',
      ...observedScope(observations),
      subject: 'perl format-on-save ownership',
      nativeOwner: NATIVE_FORMAT_OWNER,
      otherOwner: resolved.owner,
      evidenceSource:
        'settings inspection (perl-lsp.formatOnSave, editor.formatOnSave, editor.defaultFormatter)',
      symptom:
        'Native save formatting routes through the VS Code formatter resolution, so another formatter may run on every save alongside or instead of the native one.',
      risk: 'Double formatting or unexpected style churn on save.',
      remediationChoices: [
        'keep_native_provider',
        'open_conflicting_extension',
        'show_provider_tool_status',
        'disable_warning_for_exact_conflict',
      ],
      requiresReload: false,
    }),
  ];
}

function legacyCriticFindings(observations: CoexistenceObservations): CoexistenceFinding[] {
  const value = observations.staleCriticEngineValue?.trim().toLowerCase();
  if (!value || !RETIRED_CRITIC_ENGINE_ALIASES.includes(value)) {
    return [];
  }
  return [
    finding({
      conflictClass: 'legacy_first_party_critic_setting_active',
      ...observedScope(observations),
      subject: 'perl-lsp.critic.engine',
      nativeOwner: 'perl-lsp native critic (only accepted engine family)',
      otherOwner: `retired first-party setting value "${value}"`,
      evidenceSource: 'settings inspection of perl-lsp.critic.engine (#9072 migration identities)',
      symptom:
        'A retired first-party external/legacy critic-engine selection is still present in settings.',
      risk: 'None at runtime: the deprecated alias cannot construct a runtime engine state (#8253). The stale key keeps migration guidance active and misleads readers.',
      remediationChoices: [
        'open_stale_setting_migration',
        'show_critic_compatibility_status',
        'disable_warning_for_exact_conflict',
      ],
      requiresReload: false,
      registryReasonCode: REGISTRY_REASON_CODES.runtimeEnablementForbidden,
    }),
  ];
}

function debuggerFindings(observations: CoexistenceObservations): CoexistenceFinding[] {
  const self = observations.selfExtensionId.trim().toLowerCase();
  const findings: CoexistenceFinding[] = [];
  const seen = new Set<string>();
  for (const extension of observations.installedExtensions ?? []) {
    const id = extension.id.trim().toLowerCase();
    if (id === self || seen.has(id)) {
      continue;
    }
    if (!(extension.debuggerTypes ?? []).some((type) => type === 'perl')) {
      continue;
    }
    seen.add(id);
    findings.push(
      finding({
        conflictClass: 'multiple_perl_debugger_contributions',
        scopeKind: 'user',
        subject: id,
        nativeOwner: 'perl-dap (native)',
        otherOwner: reviewedPerlExtension(id)?.canonicalName ?? id,
        evidenceSource: `declared contributes.debuggers type "perl" on ${id}`,
        symptom: 'Another extension also registers launch configurations for debug type "perl".',
        risk: 'F5 may start the competing adapter instead of the native perl-dap session.',
        remediationChoices: [
          'keep_native_provider',
          'open_conflicting_extension',
          'show_provider_tool_status',
          'disable_warning_for_exact_conflict',
        ],
        requiresReload: true,
      }),
    );
  }
  return findings;
}

function perltidyCandidateFindings(observations: CoexistenceObservations): CoexistenceFinding[] {
  if (!observations.perltidyrcPresentInFolder || observations.perltidyProfileSelected) {
    return [];
  }
  return [
    finding({
      conflictClass: 'external_tool_candidate_not_selected',
      scopeKind: 'workspace-folder',
      folderName: observations.folderName,
      subject: '.perltidyrc',
      nativeOwner: `${NATIVE_FORMAT_OWNER} (default; no compatibility profile selected)`,
      otherOwner: 'Perl::Tidy compatibility candidate (.perltidyrc)',
      evidenceSource: 'workspace file scan found .perltidyrc; perl-lsp.perltidyConfig is unset',
      symptom:
        'A .perltidyrc compatibility candidate exists in this folder while native formatting owns save.',
      risk: 'No behavior conflict. Native formatting stays authoritative until Perl::Tidy is explicitly selected as compatibility mode (#7209).',
      remediationChoices: [
        'keep_native_provider',
        'show_perltidy_compatibility_status',
        'disable_warning_for_exact_conflict',
      ],
      requiresReload: false,
      registryReasonCode: REGISTRY_REASON_CODES.explicitAdapterOnly,
    }),
  ];
}

/** Detect deduplicated advisory coexistence findings for one observation set. */
export function detectCoexistenceFindings(
  observations: CoexistenceObservations,
): CoexistenceFinding[] {
  const others = otherReviewedExtensions(observations);
  const candidates: CoexistenceFinding[] = [
    ...languageServerFindings(others),
    ...diagnosticProviderFindings(others),
    ...criticFindings(others, observations),
    ...formatOnSaveFindings(observations),
    ...legacyCriticFindings(observations),
    ...debuggerFindings(observations),
    ...perltidyCandidateFindings(observations),
  ];

  const byKey = new Map<string, CoexistenceFinding>();
  for (const item of candidates) {
    const key = [item.conflictClass, item.scopeKind, item.folderName ?? '', item.subject].join('|');
    if (!byKey.has(key)) {
      byKey.set(key, item);
    }
  }
  return [...byKey.values()];
}
