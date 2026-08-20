import type { ManagedCandidateManifest } from './managedCacheProtocol';
import {
  isCanonicalManagedCandidateId,
  validateManagedCandidateManifest,
} from './managedCacheProtocol';

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

function validateManagedCandidateManifestRecord(manifest: unknown): string[] {
  if (typeof manifest !== 'object' || manifest === null) {
    return ['candidate manifest must be an object'];
  }

  const record = manifest as Partial<ManagedCandidateManifest>;
  const errors: string[] = [];
  if (record.schema_version !== 'managed_candidate_manifest.v1') {
    errors.push('candidate manifest carries an unsupported schema version');
  }
  if (typeof record.candidate_id !== 'string') {
    errors.push('candidate manifest must carry a string candidate id');
  }
  const subject = record.subject;
  if (typeof subject !== 'object' || subject === null) {
    errors.push('candidate manifest subject must be an object');
  } else {
    const candidateSubject = subject as Partial<ManagedCandidateManifest['subject']>;
    if (typeof candidateSubject.release !== 'string') {
      errors.push('candidate manifest release must be a string');
    }
    if (typeof candidateSubject.version !== 'string') {
      errors.push('candidate manifest version must be a string');
    }
    if (typeof candidateSubject.target !== 'string') {
      errors.push('candidate manifest target must be a string');
    }
    if (typeof candidateSubject.topology_digest !== 'string') {
      errors.push('candidate manifest topology digest must be a string');
    }
    if (typeof candidateSubject.perllsp_digest !== 'string') {
      errors.push('candidate manifest perllsp digest must be a string');
    }
    if (
      typeof candidateSubject.perl_dap_digest !== 'string' &&
      candidateSubject.perl_dap_digest !== null
    ) {
      errors.push('candidate manifest perl-dap digest must be a string or null');
    }
  }
  const verification = record.verification;
  if (typeof verification !== 'object' || verification === null) {
    errors.push('candidate manifest verification must be an object');
  } else {
    const candidateVerification = verification as Partial<ManagedCandidateManifest['verification']>;
    if (typeof candidateVerification.perllsp !== 'string') {
      errors.push('candidate manifest perllsp verification must be a string');
    }
    if (typeof candidateVerification.perl_dap !== 'string') {
      errors.push('candidate manifest perl-dap verification must be a string');
    }
    if (typeof candidateVerification.topology !== 'string') {
      errors.push('candidate manifest topology verification must be a string');
    }
    if (
      candidateVerification.provenance !== 'verified' &&
      candidateVerification.provenance !== 'not_proven'
    ) {
      errors.push('candidate manifest provenance verification must be verified or not_proven');
    }
  }

  if (errors.length > 0) {
    return errors;
  }
  try {
    return validateManagedCandidateManifest(record as ManagedCandidateManifest);
  } catch {
    return ['candidate manifest is not structurally valid'];
  }
}

function validateManagedCandidateCatalogEntry(entry: unknown): string[] {
  if (typeof entry !== 'object' || entry === null) {
    return ['candidate catalog entry must be an object'];
  }
  const record = entry as Partial<ManagedCandidateCatalogEntry>;
  const errors = record.immutable === true ? [] : ['candidate catalog entry must be immutable'];
  errors.push(...validateManagedCandidateManifestRecord(record.manifest));
  return errors;
}

/**
 * Validate the host-selection envelope before the resolver interprets any of
 * its records. The running identity is an input contract, not a resolver
 * ranking hint: malformed identities must never flow into restart/fallback
 * paths as if they were a real candidate.
 */
export function validateManagedHostSelectionInput(input: unknown): string[] {
  if (typeof input !== 'object' || input === null) {
    return ['managed host selection input must be an object'];
  }

  const record = input as Partial<ManagedHostSelectionInput>;
  const runningCandidateId = record.running_candidate_id;
  if (
    runningCandidateId !== null &&
    (typeof runningCandidateId !== 'string' || !isCanonicalManagedCandidateId(runningCandidateId))
  ) {
    return ['running candidate id must be null or a canonical managed candidate'];
  }
  return [];
}

function validateSessionId(sessionId: string): void {
  if (!/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(sessionId)) {
    throw new Error('host session id must be bounded and path-independent');
  }
}

function validateManagedHostReference(reference: unknown): string[] {
  if (typeof reference !== 'object' || reference === null) {
    return ['host reference must be an object'];
  }

  const candidate = reference as Partial<ManagedHostCandidateReference>;
  const errors: string[] = [];
  if (candidate.schema_version !== 'managed_host_candidate_reference.v1') {
    errors.push('host reference carries an unsupported schema version');
  }
  if (typeof candidate.session_id !== 'string') {
    errors.push('host reference session id must be a string');
  } else {
    try {
      validateSessionId(candidate.session_id);
    } catch (error) {
      errors.push(error instanceof Error ? error.message : 'host session id is invalid');
    }
  }
  if (
    typeof candidate.candidate_id !== 'string' ||
    !isCanonicalManagedCandidateId(candidate.candidate_id)
  ) {
    errors.push('host reference must name a canonical managed candidate');
  }
  return errors;
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
  const errors: string[] = [];
  if (typeof selection !== 'object' || selection === null) {
    return ['current selection must be an object'];
  }
  const record = selection as Partial<ManagedCurrentSelection>;
  // A record deserialized from disk may claim a schema this version cannot
  // interpret; reject it rather than reading known fields off unknown bytes.
  if (record.schema_version !== 'managed_current_selection.v1') {
    errors.push('current selection carries an unsupported schema version');
  }
  if (typeof record.selection_generation !== 'number') {
    errors.push('selection generation must be a positive integer');
  } else {
    errors.push(...validateGeneration(record.selection_generation));
  }
  if (
    typeof record.candidate_id !== 'string' ||
    !isCanonicalManagedCandidateId(record.candidate_id)
  ) {
    errors.push('current selection must name a canonical managed candidate');
  }

  if (typeof record.candidate_id !== 'string') {
    return errors;
  }

  const catalog = Array.isArray(candidates) ? candidates : [];
  const selected = catalog.find(
    (entry) =>
      typeof entry === 'object' &&
      entry !== null &&
      typeof entry.manifest === 'object' &&
      entry.manifest !== null &&
      entry.manifest.candidate_id === record.candidate_id,
  );
  if (!selected) {
    errors.push('current selection references an unknown candidate manifest');
    return errors;
  }
  errors.push(
    ...validateManagedCandidateCatalogEntry(selected).map((error) =>
      error === 'candidate catalog entry must be immutable'
        ? 'current selection candidate must be immutable'
        : error,
    ),
  );
  return errors;
}

export function createManagedHostReference(
  sessionId: string,
  candidateId: string,
): ManagedHostCandidateReference {
  validateSessionId(sessionId);
  if (!isCanonicalManagedCandidateId(candidateId)) {
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
  const errors = validateManagedHostReference(reference);
  const state =
    typeof reference === 'object' && reference !== null
      ? (reference as Partial<ManagedHostCandidateReference>).state
      : undefined;
  if (errors.length > 0 || (state !== 'live' && state !== 'released')) {
    throw new Error(
      `cannot release invalid managed host reference: ${[
        ...errors,
        ...(state !== 'live' && state !== 'released'
          ? ['host reference carries an unsupported state']
          : []),
      ].join('; ')}`,
    );
  }
  return {
    ...reference,
    state: 'released',
  };
}

export function resolveManagedCandidateForHost(
  input: ManagedHostSelectionInput,
): ManagedHostSelectionOutcome {
  if (validateManagedHostSelectionInput(input).length > 0) {
    return { kind: 'no_compatible_candidate' };
  }
  const candidates = Array.isArray(input.candidates) ? input.candidates : [];
  const validCandidates = candidates.filter(
    (entry) => validateManagedCandidateCatalogEntry(entry).length === 0,
  );
  const candidateIds = new Set(validCandidates.map((entry) => entry.manifest.candidate_id));
  const compatible = new Set(
    Array.isArray(input.compatible_candidate_ids) ? input.compatible_candidate_ids : [],
  );
  const currentErrors = validateManagedCurrentSelection(input.current, candidates);
  const currentCandidateId =
    typeof input.current === 'object' && input.current !== null
      ? (input.current as Partial<ManagedCurrentSelection>).candidate_id
      : undefined;
  const validCurrentId =
    currentErrors.length === 0 && typeof currentCandidateId === 'string'
      ? currentCandidateId
      : null;
  const usable = (candidateId: string): boolean =>
    candidateIds.has(candidateId) && compatible.has(candidateId);

  // A running host remains bound to the exact candidate it already launched.
  // Moving the shared default never hot-swaps its process identity.
  if (typeof input.running_candidate_id === 'string' && usable(input.running_candidate_id)) {
    return { kind: 'bound_running', candidate_id: input.running_candidate_id };
  }

  // Side-by-side compatibility is host-local selection only. Do not mutate the
  // global current/default record merely because an older client needs another
  // retained candidate.
  const replacement =
    validCurrentId !== null && usable(validCurrentId)
      ? validCurrentId
      : (validCandidates.find((entry) => compatible.has(entry.manifest.candidate_id))?.manifest
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

  return replacement === validCurrentId
    ? { kind: 'selected_current', candidate_id: replacement }
    : { kind: 'selected_compatible', candidate_id: replacement };
}

export function classifyManagedCandidateRetention(
  candidateId: string,
  input: ManagedRetentionInput,
): ManagedCandidateRetentionClass {
  // These records are normally produced by deserializers, but this boundary
  // is also called by cleanup code after partial I/O.  Normalize the
  // containers before traversing them so malformed bytes can never turn a
  // fail-closed cleanup decision into a throw.
  if (typeof input !== 'object' || input === null) {
    return 'unknown_not_safe_to_delete';
  }
  const rawInput = input as Partial<ManagedRetentionInput>;
  if (
    !Array.isArray(rawInput.catalog) ||
    !Array.isArray(rawInput.host_references) ||
    typeof rawInput.host_references_complete !== 'boolean' ||
    !(rawInput.compatible_retained_ids instanceof Set)
  ) {
    return 'unknown_not_safe_to_delete';
  }
  const catalog = rawInput.catalog;
  const hostReferences = rawInput.host_references;

  if (hostReferences.some((reference) => validateManagedHostReference(reference).length > 0)) {
    return 'unknown_not_safe_to_delete';
  }

  // An unreadable current-selection record cannot prove this candidate is not
  // the current default, so nothing is collectible.
  if (rawInput.current === null || rawInput.current === undefined) {
    return 'unknown_not_safe_to_delete';
  }
  // A non-null record is not automatically authoritative: validate its
  // schema, generation, canonical identity, and catalog membership before
  // allowing any other candidate to be classified as stale.
  if (validateManagedCurrentSelection(rawInput.current, catalog).length > 0) {
    return 'unknown_not_safe_to_delete';
  }
  if (candidateId === rawInput.current.candidate_id) {
    return 'current_default';
  }

  // Every catalog entry participates in the evidence used to decide that a
  // candidate is stale.  A malformed entry anywhere means the enumeration is
  // not trustworthy, even when the malformed record names another candidate.
  // Validate the full container before the stale path; a known-invalid
  // catalog is partial product state, not deletion authority.
  if (catalog.some((entry) => validateManagedCandidateCatalogEntry(entry).length > 0)) {
    return 'partial_or_invalid';
  }

  // A structurally invalid record means enumeration is not trustworthy. Do
  // not let a forged `released` state turn an unvalidated reference into GC
  // authority, even if its candidate id happens to match this candidate.
  const references = hostReferences.filter((reference) => reference.candidate_id === candidateId);
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
  if (!rawInput.host_references_complete) {
    return 'unknown_not_safe_to_delete';
  }
  if (rawInput.compatible_retained_ids.has(candidateId)) {
    return 'compatible_retained';
  }

  const entry = catalog.find((known) => known.manifest.candidate_id === candidateId);
  if (!entry) {
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
