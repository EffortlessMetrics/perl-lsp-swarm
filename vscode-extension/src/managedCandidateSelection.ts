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
  | 'stale_unreferenced';

export interface ManagedCandidateCatalogEntry {
  manifest: ManagedCandidateManifest;
  immutable: true;
}

export interface ManagedHostSelectionInput {
  current: ManagedCurrentSelection;
  candidates: ManagedCandidateCatalogEntry[];
  compatible_candidate_ids: string[];
  running_candidate_id: string | null;
}

function validateGeneration(generation: number): void {
  if (!Number.isInteger(generation) || generation < 1) {
    throw new Error('selection generation must be a positive integer');
  }
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

  const selectionGeneration = (prior?.selection_generation ?? 0) + 1;
  return {
    schema_version: 'managed_current_selection.v1',
    selection_generation: selectionGeneration,
    candidate_id: manifest.candidate_id,
  };
}

export function validateManagedCurrentSelection(
  selection: ManagedCurrentSelection,
  candidates: ManagedCandidateCatalogEntry[],
): string[] {
  const errors: string[] = [];
  try {
    validateGeneration(selection.selection_generation);
  } catch (error: unknown) {
    errors.push(error instanceof Error ? error.message : String(error));
  }

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

export function resolveManagedCandidateForHost(input: ManagedHostSelectionInput): string | null {
  const candidateIds = new Set(input.candidates.map((entry) => entry.manifest.candidate_id));
  const compatible = new Set(input.compatible_candidate_ids);

  // A running host remains bound to the exact candidate it already launched.
  // Moving the shared default never hot-swaps its process identity.
  if (
    input.running_candidate_id !== null &&
    candidateIds.has(input.running_candidate_id) &&
    compatible.has(input.running_candidate_id)
  ) {
    return input.running_candidate_id;
  }

  if (candidateIds.has(input.current.candidate_id) && compatible.has(input.current.candidate_id)) {
    return input.current.candidate_id;
  }

  // Side-by-side compatibility is host-local selection only. Do not mutate the
  // global current/default record merely because an older client needs another
  // retained candidate.
  for (const entry of input.candidates) {
    if (compatible.has(entry.manifest.candidate_id)) {
      return entry.manifest.candidate_id;
    }
  }

  return null;
}

export function classifyManagedCandidateRetention(
  candidateId: string,
  current: ManagedCurrentSelection,
  hostReferences: ManagedHostCandidateReference[],
  compatibleRetainedIds: ReadonlySet<string>,
): ManagedCandidateRetentionClass {
  if (candidateId === current.candidate_id) {
    return 'current_default';
  }

  const references = hostReferences.filter((reference) => reference.candidate_id === candidateId);
  if (references.some((reference) => reference.state === 'live')) {
    return 'live_referenced';
  }
  if (references.some((reference) => reference.state === 'unknown')) {
    return 'unknown_reference';
  }
  if (compatibleRetainedIds.has(candidateId)) {
    return 'compatible_retained';
  }
  return 'stale_unreferenced';
}

export function mayGarbageCollectManagedCandidate(
  candidateId: string,
  current: ManagedCurrentSelection,
  hostReferences: ManagedHostCandidateReference[],
  compatibleRetainedIds: ReadonlySet<string>,
): boolean {
  return (
    classifyManagedCandidateRetention(
      candidateId,
      current,
      hostReferences,
      compatibleRetainedIds,
    ) === 'stale_unreferenced'
  );
}

export function candidateBytesMayChangeAfterPublication(
  entry: ManagedCandidateCatalogEntry,
): false {
  // The type intentionally requires immutable=true. This function exists as a
  // load-bearing review/test seam: published candidate bytes are never updated
  // in place; a new artifact creates a new candidate identity instead.
  void entry;
  return false;
}
