/**
 * Typed coexistence model for competing Perl tooling providers (#7214).
 *
 * This module is the single vocabulary for advisory coexistence findings on the
 * VS Code surface. It composes the reviewed external-tool role registry (#7209)
 * and its doctor projection (#7212) rather than re-encoding tool policy:
 *
 * - conflict classes use exactly the stable identities ruled in #7214;
 * - `registryReasonCode` values mirror `external_tool_doctor.rs` so doctor and
 *   editor share one set of status/reason codes;
 * - detection inputs are bounded, observed host facts. PATH presence of
 *   `perlcritic`/`perltidy` and a first-party `.perlcriticrc` are residue
 *   evidence that can never classify into a provider (#7214 ruling);
 * - remediation choices are strictly advisory: nothing here disables,
 *   configures, removes, or impersonates another product.
 */

export type CoexistenceConflictClass =
  | 'multiple_language_servers'
  | 'multiple_diagnostic_providers'
  | 'multiple_format_on_save_owners'
  | 'native_critic_and_other_diagnostic_provider'
  | 'legacy_first_party_critic_setting_active'
  | 'multiple_perl_debugger_contributions'
  | 'external_tool_candidate_not_selected'
  | 'unknown_possible_overlap';

export type CoexistenceDomain =
  | 'language_server'
  | 'diagnostics'
  | 'critic_diagnostics'
  | 'formatting'
  | 'debugger';

/** Bounded remediation menu; every entry is informational or navigational. */
export type CoexistenceRemediationChoice =
  | 'keep_native_provider'
  | 'open_conflicting_extension'
  | 'open_stale_setting_migration'
  | 'show_provider_tool_status'
  | 'show_critic_compatibility_status'
  | 'show_perltidy_compatibility_status'
  | 'disable_warning_for_exact_conflict'
  | 'copy_redacted_support_packet';

/**
 * Status/reason-code vocabulary mirrored from
 * `crates/perl-lsp-rs-core/src/external_tool_doctor.rs` (#7209/#7212). Editor
 * findings cite these codes instead of inventing editor-local ones.
 */
export const REGISTRY_REASON_CODES = {
  runtimeEnablementForbidden: 'runtime_enablement_forbidden',
  explicitAdapterOnly: 'explicit_adapter_only',
  explicitOptionalPeer: 'explicit_optional_peer',
  unclassified: 'unclassified',
} as const;

/**
 * Reviewed VS Code identities whose presence is high-confidence evidence.
 *
 * Matching is exact extension-id equality against this table; there is no
 * name/description scanner (#7214 negative control).
 */
export interface ReviewedPerlExtension {
  readonly extensionId: string;
  readonly canonicalName: string;
  readonly domains: readonly CoexistenceDomain[];
  /**
   * Mirrored #7209/#7212 doctor reason code when this client maps to a
   * registry row; absent when no reviewed row owns the identity.
   */
  readonly registryReasonCode?: (typeof REGISTRY_REASON_CODES)[keyof typeof REGISTRY_REASON_CODES];
}

/** The native first-party extension identity, lowercase-normalized. */
export const NATIVE_EXTENSION_ID = 'effortlessmetrics.perl-lsp-rs';

export const REVIEWED_PERL_EXTENSIONS: readonly ReviewedPerlExtension[] = [
  {
    extensionId: 'bscan.perlnavigator',
    canonicalName: 'Perl Navigator',
    domains: ['language_server', 'diagnostics', 'critic_diagnostics', 'formatting'],
  },
  {
    // Perl::LanguageServer client (#7209 registry row: conformance oracle
    // only, never a runtime backend; doctor reason code mirrored below).
    extensionId: 'richterger.perl',
    canonicalName: 'Perl::LanguageServer',
    domains: ['language_server', 'diagnostics', 'debugger'],
    registryReasonCode: REGISTRY_REASON_CODES.runtimeEnablementForbidden,
  },
  {
    // PLS client — same retired runtime identity as richterger.perl.
    extensionId: 'fractalboy.pls',
    canonicalName: 'PLS',
    domains: ['language_server', 'diagnostics'],
    registryReasonCode: REGISTRY_REASON_CODES.runtimeEnablementForbidden,
  },
];

const REVIEWED_BY_ID = new Map(
  REVIEWED_PERL_EXTENSIONS.map((entry) => [entry.extensionId.trim().toLowerCase(), entry]),
);

/** Exact reviewed-identity lookup; no substring or fuzzy matching. */
export function reviewedPerlExtension(extensionId: string): ReviewedPerlExtension | undefined {
  return REVIEWED_BY_ID.get(extensionId.trim().toLowerCase());
}

/** Uniform claim boundary carried by every finding and report. */
export const COEXISTENCE_CLAIM_BOUNDARY =
  'Advisory only: perl-lsp never disables, configures, removes, terminates, or impersonates another tool.';

export interface CoexistenceFinding {
  readonly conflictClass: CoexistenceConflictClass;
  /** `user` scope findings are host-wide; folder scope names its exact root. */
  readonly scopeKind: 'user' | 'workspace-folder';
  readonly folderName?: string | undefined;
  /** Exact conflict subject; suppression binds to this identity. */
  readonly subject: string;
  readonly nativeOwner: string;
  readonly otherOwner?: string | undefined;
  /** Where the observation came from; never speculative. */
  readonly evidenceSource: string;
  readonly symptom: string;
  readonly risk: string;
  readonly remediationChoices: readonly CoexistenceRemediationChoice[];
  readonly requiresReload: boolean;
  readonly claimBoundary: string;
  /** Mirrored #7209/#7212 reason code when a registry row owns the domain. */
  readonly registryReasonCode?: string | undefined;
}

/** Exact suppression/notification identity for one finding. */
export function coexistenceConflictKey(finding: CoexistenceFinding): string {
  return [finding.conflictClass, finding.scopeKind, finding.folderName ?? '', finding.subject].join(
    '|',
  );
}

export interface RedactedCoexistencePacketEntry {
  readonly conflictClass: CoexistenceConflictClass;
  readonly scopeKind: 'user' | 'workspace-folder';
  readonly otherOwner?: string | undefined;
  readonly registryReasonCode?: string | undefined;
}

/**
 * Redacted support output (#7214): conflict classes, involved owner names, and
 * registry reason codes only. No paths, no folder names, no full extension
 * inventory ever appears.
 */
export function buildRedactedCoexistencePacket(findings: readonly CoexistenceFinding[]): string {
  const packet = {
    schema_version: 'perl_lsp_coexistence_packet.v1',
    claim_boundary: COEXISTENCE_CLAIM_BOUNDARY,
    findings: findings.map<RedactedCoexistencePacketEntry>((finding) => ({
      conflictClass: finding.conflictClass,
      scopeKind: finding.scopeKind,
      ...(finding.otherOwner === undefined ? {} : { otherOwner: finding.otherOwner }),
      ...(finding.registryReasonCode === undefined
        ? {}
        : { registryReasonCode: finding.registryReasonCode }),
    })),
  };
  return `${JSON.stringify(packet, null, 2)}\n`;
}
