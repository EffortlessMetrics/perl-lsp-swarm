// Generated projection of the perl-lsp/loadedModuleReload v1 custom DAP family
// contract. Do not hand-edit.
// `crates/perl-dap/src/reload_family.rs` and its seam tests verify every
// literal, enum value, and marker in this file against the frozen #10097
// contract, the canonical registry entry, the wire schema, and the JSON
// vectors under .spec/10138-loaded-module-reload-family/fixtures/.

export const LOADED_MODULE_RELOAD_FAMILY = 'perl-lsp/loadedModuleReload' as const;
export const LOADED_MODULE_RELOAD_REQUEST = 'perl-lsp/loadedModuleReload' as const;
export const LOADED_MODULE_RELOAD_FAMILY_VERSION = 1 as const;

/**
 * Known terminal outcome kinds, projected verbatim from the frozen contract.
 * The bounded unknown representation keeps an unrecognized mandatory
 * variant representable so the client can fail closed instead of crashing
 * or guessing.
 */
export type KnownReloadOutcomeKind =
  | 'reloaded'
  | 'refused'
  | 'failed_before_mutation'
  | 'indeterminate_possibly_applied';
export type ReloadOutcomeKind = KnownReloadOutcomeKind | (string & {});

export type KnownReloadTransactionPhase =
  | 'admission'
  | 'preflight'
  | 'prepare'
  | 'runtime_mutation_begins'
  | 'runtime_acknowledgement_read_back'
  | 'commit_generation'
  | 'post_reload_reconciliation'
  | 'terminal_projection';
export type ReloadTransactionPhase = KnownReloadTransactionPhase | (string & {});

export type KnownReloadRefusalDisposition =
  | 'not_loaded'
  | 'source_not_exact_or_stale'
  | 'dirty_or_unsaved_source'
  | 'active_frame_in_target'
  | 'main_program_not_module'
  | 'xs_or_native_module'
  | 'source_filter_or_compile_hook_boundary'
  | 'generated_or_eval_source'
  | 'ambiguous_runtime_mapping'
  | 'outside_launch_authority'
  | 'unsupported_runtime'
  | 'not_stopped_or_not_command_ready';
export type ReloadRefusalDisposition = KnownReloadRefusalDisposition | (string & {});

export type KnownReloadFailureCause =
  | 'prepare_failed'
  | 'cancelled_before_mutation_began'
  | 'timeout_after_mutation_began'
  | 'transport_loss_after_mutation_began'
  | 'ambiguous_acknowledgement'
  | 'read_back_inconclusive';
export type ReloadFailureCause = KnownReloadFailureCause | (string & {});

export type KnownReloadRejectionCode =
  | 'family_not_negotiated'
  | 'family_name_mismatch'
  | 'family_version_unsupported'
  | 'session_stale'
  | 'operation_stale'
  | 'operation_id_invalid'
  | 'unknown_field_rejected'
  | 'unknown_variant_rejected'
  | 'raw_client_input_refused'
  | 'subject_identity_insufficient'
  | 'payload_too_large'
  | 'identity_too_large'
  | 'detail_too_large'
  | 'deadline_out_of_range'
  | 'family_not_backed_for_session'
  | 'malformed_request';
export type ReloadRejectionCode = KnownReloadRejectionCode | (string & {});

/** Registry-recorded bounds, mirrored from the Rust family module. */
export const RELOAD_FAMILY_BOUNDS = {
  maxRequestBytes: 8192,
  maxIdentityChars: 256,
  maxDigestChars: 128,
  maxReasons: 16,
  maxReasonChars: 96,
  maxDetailChars: 256,
  maxRetainedOperations: 64,
  minDeadlineMs: 100,
  maxDeadlineMs: 60000,
} as const;

export const REASONS_TRUNCATED_MARKER = 'reasons_truncated' as const;
export const DETAIL_REDACTED_MARKER = 'detail_redacted' as const;

/**
 * The typed, adapter-issued opaque subject. This is the only admissible
 * request payload shape: raw paths, debugger commands, and Perl
 * expressions are refused server-side and are not even representable here.
 */
export interface LoadedModuleReloadSubject {
  moduleIdentity: string;
  savedSourceDigest: string;
  logicalSourceUri: string;
  observationGeneration: number;
}

export interface LoadedModuleReloadRequest {
  family: typeof LOADED_MODULE_RELOAD_FAMILY;
  familyVersion: typeof LOADED_MODULE_RELOAD_FAMILY_VERSION;
  sessionEpoch: number;
  operationId: number;
  subject: LoadedModuleReloadSubject;
  deadlineMs?: number;
}

export interface ReloadGenerationWitness {
  previous: number;
  current: number;
  advanced: boolean;
}

export interface ReloadReconciliationDispositions {
  loaded_source_refresh: 'deferred';
  inspection_invalidation: 'deferred';
  breakpoint_reconciliation: 'deferred';
}

export interface LoadedModuleReloadOutcomeBody {
  kind: ReloadOutcomeKind;
  phase: ReloadTransactionPhase;
  disposition?: ReloadRefusalDisposition;
  cause?: ReloadFailureCause;
  possiblyApplied: boolean;
  generation?: ReloadGenerationWitness;
  reconciliation: ReloadReconciliationDispositions;
  reasons?: string[];
  remediation?: string;
}

export interface LoadedModuleReloadRejectionBody {
  kind: 'request_rejected';
  code: ReloadRejectionCode;
  reasons?: string[];
}

/**
 * One family response: the DAP success flag, the correlated operation
 * identity (0 only when the request carried nothing parseable), and the
 * typed body. The operation identity travels on every request/response
 * pair.
 */
export interface LoadedModuleReloadResponse {
  success: boolean;
  operationId: number;
  body: LoadedModuleReloadResponseBody;
}

export type LoadedModuleReloadResponseBody =
  | LoadedModuleReloadOutcomeBody
  | LoadedModuleReloadRejectionBody;

/**
 * Fail-closed client-side classification of a response body. The kind and
 * the `possiblyApplied` flag must agree: a body claiming a clean refusal
 * or pre-mutation failure while asserting `possiblyApplied` is
 * contradictory and fails closed, exactly like an unknown kind. The
 * indeterminate kind stays authoritative on its own so it can never be
 * demoted to an ordinary failure by a lying field.
 */
export type ReloadTerminalClassification =
  | 'reloaded_clean'
  | 'refused_clean_failure'
  | 'failed_before_mutation_clean_failure'
  | 'possibly_applied'
  | 'unknown_fail_closed';

export function classifyReloadTerminal(body: {
  kind: string;
  possiblyApplied?: boolean;
}): ReloadTerminalClassification {
  const possiblyApplied = body.possiblyApplied === true;
  switch (body.kind) {
    case 'reloaded':
      return possiblyApplied ? 'unknown_fail_closed' : 'reloaded_clean';
    case 'refused':
      return possiblyApplied ? 'unknown_fail_closed' : 'refused_clean_failure';
    case 'failed_before_mutation':
      return possiblyApplied ? 'unknown_fail_closed' : 'failed_before_mutation_clean_failure';
    case 'indeterminate_possibly_applied':
      return 'possibly_applied';
    default:
      return 'unknown_fail_closed';
  }
}

/** A client's declared family support. */
export interface ClientFamilyDeclaration {
  family: string;
  versions: number[];
}

export type FamilyNegotiationOutcome =
  | { negotiated: true; version: number }
  | {
      negotiated: false;
      reason: 'family_absent' | 'family_name_mismatch' | 'no_overlapping_version';
    };

/**
 * Negotiate the family against this projection's known version, selecting
 * the highest mutually known version and failing closed otherwise. Mirrors
 * the adapter-side rules exactly; version guessing is never negotiation.
 */
export function negotiateLoadedModuleReloadFamily(
  declaration: ClientFamilyDeclaration | null,
): FamilyNegotiationOutcome {
  if (declaration === null) {
    return { negotiated: false, reason: 'family_absent' };
  }
  if (declaration.family !== LOADED_MODULE_RELOAD_FAMILY) {
    return { negotiated: false, reason: 'family_name_mismatch' };
  }
  const mutual = declaration.versions
    .filter((version) => version >= 1 && version <= LOADED_MODULE_RELOAD_FAMILY_VERSION)
    .reduce<number>((best, version) => Math.max(best, version), 0);
  if (mutual < 1) {
    return { negotiated: false, reason: 'no_overlapping_version' };
  }
  return { negotiated: true, version: mutual };
}
