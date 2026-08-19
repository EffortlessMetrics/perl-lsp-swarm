import type { ManagedCandidateManifest } from './managedCacheProtocol';
import { validateManagedCandidateManifest } from './managedCacheProtocol';

export interface ManagedCurrentSelection {
  schema_version: 'managed_current_selection.v1';
  selection_generation: number;
  candidate_id: string;
}

export type ManagedHostReferenceState = 'live' | 'unknown' | 'released';

export interface ManagedHostCandidateReference {
  schema_version: 'managed_host_candidate_reference.v1';
  session_id: string;
  candidate_id: string;
  state: ManagedHostReferenceState;
}

export type ManagedCandidateRetentionClass =
  | 'current_default'
  | 'live_referenced'
  | 'unknown_reference'
  | 'compatible_retained'
  | 'unknown_not_safe_to_delete'
  | 'partial_or_invalid'
  | 'stale_unreferenced';

/**
 * A published candidate directory is immutable by contract: a later artifact
 * produces a different candidate subject, and therefore a different
 * `candidate_id`, rather than replacing bytes under an existing identity.
 * `immutable` is a literal `true` so a record deserialized from disk that
 * claims otherwise is rejected by {@link validateManagedCurrentSelection}.
 */
export interface ManagedCandidateCatalogEntry {
  manifest: ManagedCandidateManifest;
  immutable: true;
}

export interface ManagedHostSelectionInput {
  current: ManagedCurrentSelection;
  /**
   * Immutable candidate catalog in the caller's compatibility preference
   * order. Ranking compatible candidates against a client version is owned by
   * the compatibility policy (#4838 / #6854), not by this module; when the
   * current selection is unusable this module takes the caller's first
   * compatible entry rather than inventing a version ordering.
   */
  candidates: ManagedCandidateCatalogEntry[];
  compatible_candidate_ids: string[];
  running_candidate_id: string | null;
}

/**
 * Why a host ended up on a candidate. `restart_required` and
 * `no_compatible_candidate` are the action-required outcomes: a bare candidate
 * id cannot tell a running host that its launched candidate is gone, so
 * callers must not silently rebind a live process to a replacement.
 */
export type ManagedHostSelectionOutcome =
  | { kind: 'bound_running'; candidate_id: string }
  | { kind: 'selected_current'; candidate_id: string }
  | { kind: 'selected_compatible'; candidate_id: string }
  | { kind: 'restart_required'; candidate_id: string }
  | { kind: 'no_compatible_candidate' };

export interface ManagedRetentionInput {
  /**
   * `null` means the current-selection record could not be read or parsed. GC
   * must then refuse every candidate: it cannot prove any given candidate is
   * not the current default.
   */
  current: ManagedCurrentSelection | null;
  catalog: ManagedCandidateCatalogEntry[];
  host_references: ManagedHostCandidateReference[];
  /**
   * `false` when host-reference enumeration was not proven exhaustive (I/O
   * error, unparseable reference record, partial directory listing). A short
   * reference list must never be read as "nothing references this candidate".
   */
  host_references_complete: boolean;
  compatible_retained_ids: ReadonlySet<string>;
}

function validateGeneration(generation: number): string[] {
  if (!Number.isInteger(generation) || generation < 1) {
    return ['selection generation must be a positive integer'];
  }
  return [];
}

function validateSessionId(sessionId: string): void {
  if (!/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(sessionId)) {
    throw new Error('host session id must be bounded and path-independent');
  }
}

export function publishManagedCurrentSelection(
  manifest: ManagedCandidateManifest,
  prior: ManagedCurrentSelection | null,
): ManagedCurrentSelection {
  const manifestErrors = validateManagedCandidateManifest(manifest);
  if (manifestErrors.length > 0) {
    throw new Error(`cannot publish invalid managed candidate: ${manifestErrors.join('; ')}`);
  }

  if (prior !== null) {
    const priorErrors = validateGeneration(prior.selection_generation);
    if (priorErrors.length > 0) {
      throw new Error(`cannot publish over an invalid prior selection: ${priorErrors.join('; ')}`);
    }
  }

  return {
    schema_version: 'managed_current_selection.v1',
    selection_generation: (prior?.selection_generation ?? 0) + 1,
    candidate_id: manifest.candidate_id,
  };
}

export function validateManagedCurrentSelection(
  selection: ManagedCurrentSelection,
  candidates: ManagedCandidateCatalogEntry[],
): string[] {
  const errors: string[] = [...validateGeneration(selection.selection_generation)];

  const selected = candidates.find(
    (entry) => entry.manifest.candidate_id === selection.candidate_id,
  );
  if (!selected) {
    errors.push('current selection references an unknown candidate manifest');
    return errors;
  }
  if (!selected.immutable) {
    errors.push('current selection candidate must be immutable');
  }
  errors.push(...validateManagedCandidateManifest(selected.manifest));
  return errors;
}

export function createManagedHostReference(
  sessionId: string,
  candidateId: string,
): ManagedHostCandidateReference {
  validateSessionId(sessionId);
  if (!candidateId.startsWith('candidate-')) {
    throw new Error('host reference must name a canonical managed candidate');
  }
  return {
    schema_version: 'managed_host_candidate_reference.v1',
    session_id: sessionId,
    candidate_id: candidateId,
    state: 'live',
  };
}

export function releaseManagedHostReference(
  reference: ManagedHostCandidateReference,
): ManagedHostCandidateReference {
  return {
    ...reference,
    state: 'released',
  };
}

export function resolveManagedCandidateForHost(
  input: ManagedHostSelectionInput,
): ManagedHostSelectionOutcome {
  const candidateIds = new Set(input.candidates.map((entry) => entry.manifest.candidate_id));
  const compatible = new Set(input.compatible_candidate_ids);
  const usable = (candidateId: string): boolean =>
    candidateIds.has(candidateId) && compatible.has(candidateId);

  // A running host remains bound to the exact candidate it already launched.
  // Moving the shared default never hot-swaps its process identity.
  if (input.running_candidate_id !== null && usable(input.running_candidate_id)) {
    return { kind: 'bound_running', candidate_id: input.running_candidate_id };
  }

  // Side-by-side compatibility is host-local selection only. Do not mutate the
  // global current/default record merely because an older client needs another
  // retained candidate.
  const replacement = usable(input.current.candidate_id)
    ? input.current.candidate_id
    : (input.candidates.find((entry) => compatible.has(entry.manifest.candidate_id))?.manifest
        .candidate_id ?? null);

  if (replacement === null) {
    return { kind: 'no_compatible_candidate' };
  }

  // The host launched a candidate that is no longer in the catalog or no
  // longer compatible. Report the replacement as action-required rather than
  // returning it as if the live process were already running it.
  if (input.running_candidate_id !== null) {
    return { kind: 'restart_required', candidate_id: replacement };
  }

  return replacement === input.current.candidate_id
    ? { kind: 'selected_current', candidate_id: replacement }
    : { kind: 'selected_compatible', candidate_id: replacement };
}

export function classifyManagedCandidateRetention(
  candidateId: string,
  input: ManagedRetentionInput,
): ManagedCandidateRetentionClass {
  // An unreadable current-selection record cannot prove this candidate is not
  // the current default, so nothing is collectible.
  if (input.current === null) {
    return 'unknown_not_safe_to_delete';
  }
  if (candidateId === input.current.candidate_id) {
    return 'current_default';
  }

  const references = input.host_references.filter(
    (reference) => reference.candidate_id === candidateId,
  );
  if (references.some((reference) => reference.state === 'live')) {
    return 'live_referenced';
  }
  // Any state that is not a proven release protects the candidate. A parseable
  // reference record carrying an unrecognized state (for example written by a
  // newer extension version in a mixed-VSIX install) is unknown evidence, not
  // proof that nothing references the candidate.
  if (references.some((reference) => reference.state !== 'released')) {
    return 'unknown_reference';
  }
  // Absence of a reference is only evidence when enumeration was exhaustive.
  if (!input.host_references_complete) {
    return 'unknown_not_safe_to_delete';
  }
  if (input.compatible_retained_ids.has(candidateId)) {
    return 'compatible_retained';
  }

  const entry = input.catalog.find((known) => known.manifest.candidate_id === candidateId);
  if (!entry || validateManagedCandidateManifest(entry.manifest).length > 0) {
    // Half-published or invalid bytes are not proven-stale product state.
    // Leave them for an explicit repair path rather than deleting blind.
    return 'partial_or_invalid';
  }

  return 'stale_unreferenced';
}

export function mayGarbageCollectManagedCandidate(
  candidateId: string,
  input: ManagedRetentionInput,
): boolean {
  return classifyManagedCandidateRetention(candidateId, input) === 'stale_unreferenced';
}
