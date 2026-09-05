import * as fs from 'fs';
import * as path from 'path';
import { randomUUID } from 'crypto';
import type { ManagedCandidateManifest, ManagedCandidateSubject } from './managedCacheProtocol';
import {
  buildManagedCandidateManifest,
  isCanonicalManagedCandidateId,
} from './managedCacheProtocol';
import type {
  ManagedCandidateCatalogEntry,
  ManagedCandidateRetentionClass,
  ManagedCurrentSelection,
  ManagedHostCandidateReference,
  ManagedRetentionInput,
} from './managedCandidateSelection';
import {
  classifyManagedCandidateRetention,
  createManagedHostReference,
  mayGarbageCollectManagedCandidate,
  publishManagedCurrentSelection,
  releaseManagedHostReference,
} from './managedCandidateSelection';

/**
 * Filesystem wiring for the managed candidate selection and GC policy landed
 * in #11780 (`managedCandidateSelection.ts`). That module is deliberately pure;
 * this one owns every disk read/write the policy needs in the live extension
 * host (#10083):
 *
 * - each published install dir carries a `candidate.json` manifest minted from
 *   the verified download digests, so a directory maps to exactly one
 *   immutable candidate identity;
 * - each commit also writes the versioned `managed_current_selection.v1`
 *   record next to the legacy `current` dir pointer, which stays in place for
 *   path resolution and rollback to older extension builds;
 * - each extension-host session persists a `managed_host_candidate_reference.v1`
 *   before its server process can spawn and releases it after the process is
 *   terminal, so a collector can prove which candidates remain in use;
 * - garbage collection consumes the landed retention policy and deletes only
 *   candidates classified `stale_unreferenced`.
 *
 * Every failure here is fail-safe: unreadable or malformed evidence blocks
 * destructive decisions instead of widening them.
 */
export const MANAGED_CANDIDATE_MANIFEST_FILE = 'candidate.json';
export const MANAGED_CURRENT_SELECTION_FILE = 'selection.json';
export const MANAGED_HOST_REFERENCE_DIR = 'host-refs';

export type RuntimeLog = (message: string) => void;

export interface ManagedCandidateCatalogEnumeration {
  /**
   * Catalog entries in caller preference order (most recently installed
   * first). Structurally invalid manifests are included on purpose: the
   * landed classifier treats any malformed entry as `partial_or_invalid` for
   * the whole namespace, which is the intended fail-safe.
   */
  entries: ManagedCandidateCatalogEntry[];
  /**
   * Canonical candidate id → owning install dirs (newest first). Only dirs
   * whose manifest carries a canonical id appear here; those are the only
   * possible deletion subjects.
   */
  candidateDirs: Map<string, string[]>;
  /** `false` when the directory listing or a manifest read failed. */
  complete: boolean;
}

export interface ManagedHostReferenceEnumeration {
  references: ManagedHostCandidateReference[];
  /**
   * `false` when any reference file was unreadable or unparseable. Absence of
   * references is only evidence when enumeration was exhaustive.
   */
  complete: boolean;
}

export interface ManagedGarbageCollectResult {
  removed: string[];
  /** Candidate ids that survived, bucketed by their landed retention class. */
  retained: Partial<Record<ManagedCandidateRetentionClass | 'uncatalogued', string[]>>;
  /** Set when destructive GC was refused before classifying anything. */
  blockedReason: string | null;
}

/**
 * Session ids become filenames under `host-refs/`, so this layer accepts only
 * ids that are safe as Windows AND POSIX filenames. It is intentionally
 * narrower than the landed policy's in-memory session grammar
 * (`managedCandidateSelection.ts` allows `:` for record identity): on NTFS a
 * `:` in a filename addresses an alternate data stream, which `readdirSync`
 * never lists — a reference could then hide outside a "complete" enumeration,
 * the unsafe direction. A session id this layer cannot persist degrades to
 * the documented unprotected-launch path, never to unsafe deletion.
 */
const HOST_SESSION_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

function atomicWriteFileSync(filePath: string, content: string): void {
  // A unique temporary name per write keeps concurrent extension hosts from
  // interleaving writes onto one predictable path (last rename would clobber
  // the other's partial bytes into the final record); a failed write removes
  // its own leftover so enumeration never mistakes it for evidence.
  const tmpPath = `${filePath}.${process.pid}-${randomUUID()}.tmp`;
  try {
    fs.writeFileSync(tmpPath, content, { encoding: 'utf8' });
    fs.renameSync(tmpPath, filePath);
  } catch (error: unknown) {
    try {
      fs.rmSync(tmpPath, { force: true });
    } catch {
      /* the failed write's leftover, if any, is skipped by enumeration */
    }
    throw error;
  }
}

type JsonRead = { value: unknown } | { error: 'absent' | 'read' };

function readJsonFileSync(filePath: string): JsonRead {
  let raw: string;
  try {
    raw = fs.readFileSync(filePath, 'utf8');
  } catch (error: unknown) {
    const code = (error as NodeJS.ErrnoException | null)?.code;
    if (code === 'ENOENT') {
      return { error: 'absent' };
    }
    return { error: 'read' };
  }
  try {
    return { value: JSON.parse(raw) as unknown };
  } catch {
    return { error: 'read' };
  }
}

function isSessionIdString(sessionId: unknown): sessionId is string {
  return typeof sessionId === 'string' && HOST_SESSION_ID.test(sessionId);
}

function hostReferencePath(baseDir: string, sessionId: string): string {
  return path.join(baseDir, MANAGED_HOST_REFERENCE_DIR, `${sessionId}.json`);
}

/** Reads the namespace's policy-governed current-selection record, or `null`. */
export function readManagedCurrentSelection(baseDir: string): ManagedCurrentSelection | null {
  const read = readJsonFileSync(path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE));
  if (!('value' in read)) {
    return null;
  }
  const record = read.value as Partial<ManagedCurrentSelection>;
  if (
    typeof record !== 'object' ||
    record === null ||
    record.schema_version !== 'managed_current_selection.v1'
  ) {
    return null;
  }
  return record as ManagedCurrentSelection;
}

/**
 * Mints and persists the candidate manifest for a freshly populated install
 * dir. Returns the manifest, or `null` when it could not be written (the
 * install itself still succeeds; the dir then simply never becomes a deletion
 * subject, which is the fail-safe direction).
 */
export function writeInstalledManagedCandidateManifest(
  installDir: string,
  subject: ManagedCandidateSubject,
  log: RuntimeLog,
): ManagedCandidateManifest | null {
  let manifest: ManagedCandidateManifest;
  try {
    manifest = buildManagedCandidateManifest(subject);
  } catch (error: unknown) {
    log(
      `could not mint managed candidate manifest: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
  try {
    atomicWriteFileSync(
      path.join(installDir, MANAGED_CANDIDATE_MANIFEST_FILE),
      `${JSON.stringify(manifest, null, 2)}\n`,
    );
  } catch (error: unknown) {
    log(
      `could not record managed candidate manifest: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
  return manifest;
}

/** Reads one install dir's candidate manifest, or `null` when absent/malformed. */
export function readInstalledManagedCandidateManifest(
  installDir: string,
): ManagedCandidateManifest | null {
  const read = readJsonFileSync(path.join(installDir, MANAGED_CANDIDATE_MANIFEST_FILE));
  if (!('value' in read)) {
    return null;
  }
  const record = read.value as Partial<ManagedCandidateManifest>;
  if (
    typeof record !== 'object' ||
    record === null ||
    record.schema_version !== 'managed_candidate_manifest.v1' ||
    typeof record.candidate_id !== 'string'
  ) {
    return null;
  }
  return record as ManagedCandidateManifest;
}

type PriorSelectionRead =
  | { kind: 'absent' }
  | { kind: 'unreadable' }
  | { kind: 'present'; selection: ManagedCurrentSelection };

/**
 * Distinguishes "no selection record exists" from "evidence exists but this
 * version cannot interpret it". Only the first may be treated as a fresh
 * namespace: overwriting unreadable bytes would silently reset the
 * generation counter and destroy the record.
 */
function readPriorManagedCurrentSelection(baseDir: string): PriorSelectionRead {
  const read = readJsonFileSync(path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE));
  if (!('value' in read)) {
    return read.error === 'absent' ? { kind: 'absent' } : { kind: 'unreadable' };
  }
  const record = read.value as Partial<ManagedCurrentSelection>;
  if (
    typeof record !== 'object' ||
    record === null ||
    record.schema_version !== 'managed_current_selection.v1'
  ) {
    return { kind: 'unreadable' };
  }
  return { kind: 'present', selection: record as ManagedCurrentSelection };
}

/**
 * Commits the policy-governed selection record for a freshly installed
 * candidate: reads the prior record, mints the next generation through the
 * landed `publishManagedCurrentSelection`, and writes it atomically. Returns
 * the new record, or `null` when the commit was refused — an invalid prior
 * generation is never overwritten with a fabricated one, and an existing
 * record this version cannot read is left untouched rather than replaced by
 * a generation-1 reset.
 */
export function commitManagedCandidateSelection(
  baseDir: string,
  manifest: ManagedCandidateManifest,
  log: RuntimeLog,
): ManagedCurrentSelection | null {
  const priorRead = readPriorManagedCurrentSelection(baseDir);
  if (priorRead.kind === 'unreadable') {
    log(
      'refusing managed selection commit: an existing selection record exists but cannot be interpreted',
    );
    return null;
  }
  const prior = priorRead.kind === 'absent' ? null : priorRead.selection;
  let selection: ManagedCurrentSelection;
  try {
    selection = publishManagedCurrentSelection(manifest, prior);
  } catch (error: unknown) {
    log(
      `could not publish managed current selection: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
  try {
    atomicWriteFileSync(
      path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE),
      `${JSON.stringify(selection, null, 2)}\n`,
    );
  } catch (error: unknown) {
    log(
      `could not record managed current selection: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
  return selection;
}

/**
 * Persists this session's `live` host reference for an exact candidate, before
 * the server process spawns. Overwriting the session's own earlier reference
 * is the reselection transition: the caller has already stopped the process
 * bound to the previous candidate (the release path runs at client teardown).
 */
export function acquireSessionManagedHostReference(
  baseDir: string,
  sessionId: string,
  candidateId: string,
  log: RuntimeLog,
): ManagedHostCandidateReference | null {
  if (!isSessionIdString(sessionId)) {
    log('cannot persist managed host reference without a valid session id');
    return null;
  }
  let reference: ManagedHostCandidateReference;
  try {
    reference = createManagedHostReference(sessionId, candidateId);
  } catch (error: unknown) {
    log(
      `could not create managed host reference: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
  try {
    fs.mkdirSync(path.join(baseDir, MANAGED_HOST_REFERENCE_DIR), { recursive: true });
    atomicWriteFileSync(
      hostReferencePath(baseDir, sessionId),
      `${JSON.stringify(reference, null, 2)}\n`,
    );
  } catch (error: unknown) {
    log(
      `could not persist managed host reference: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
  return reference;
}

/**
 * Reads this session's own reference in one namespace, or `null` when absent
 * or malformed. A malformed own-session reference yields no running binding,
 * which is safe at the pre-launch resolution seam where it is consumed.
 */
export function readSessionManagedHostReference(
  baseDir: string,
  sessionId: string,
): ManagedHostCandidateReference | null {
  if (!isSessionIdString(sessionId)) {
    return null;
  }
  const read = readJsonFileSync(hostReferencePath(baseDir, sessionId));
  if (!('value' in read)) {
    return null;
  }
  const record = read.value as Partial<ManagedHostCandidateReference>;
  if (
    typeof record !== 'object' ||
    record === null ||
    record.schema_version !== 'managed_host_candidate_reference.v1' ||
    typeof record.candidate_id !== 'string' ||
    typeof record.state !== 'string'
  ) {
    return null;
  }
  return record as ManagedHostCandidateReference;
}

/**
 * Enumerates every host reference file in one namespace. Any unreadable or
 * unparseable file marks the enumeration incomplete: absence is not evidence.
 * Structurally parseable records are passed through unvalidated — the landed
 * classifier itself rejects malformed records fail-safe.
 */
export function enumerateManagedHostReferences(baseDir: string): ManagedHostReferenceEnumeration {
  const referencesDir = path.join(baseDir, MANAGED_HOST_REFERENCE_DIR);
  let files: fs.Dirent[];
  try {
    files = fs.readdirSync(referencesDir, { withFileTypes: true });
  } catch (error: unknown) {
    const code = (error as NodeJS.ErrnoException | null)?.code;
    // A namespace that never wrote references has exhaustively none.
    if (code === 'ENOENT') {
      return { references: [], complete: true };
    }
    return { references: [], complete: false };
  }
  const references: ManagedHostCandidateReference[] = [];
  for (const file of files) {
    if (!file.isFile()) {
      continue;
    }
    // In-flight or leftover atomic-write temporaries (`.tmp` suffix) are
    // partial writes by construction, never published reference records;
    // the atomic rename publishes only complete `.json` files. Skipping
    // them keeps one crashed write from blocking the namespace's GC forever.
    if (!file.name.endsWith('.json')) {
      continue;
    }
    const read = readJsonFileSync(path.join(referencesDir, file.name));
    if (!('value' in read) || typeof read.value !== 'object' || read.value === null) {
      return { references: [], complete: false };
    }
    references.push(read.value as ManagedHostCandidateReference);
  }
  return { references, complete: true };
}

/**
 * Decides whether a language-client shutdown proved process termination and
 * may therefore release this session's host references (#10083). The
 * lifecycle's `stop()` resolves — it does not reject — after transitioning
 * to `failed` when stop/dispose times out, so "stop() returned" alone is
 * not termination evidence. Only the clean `stopped` state proves the
 * bound process is terminal; `failed` and every transient state retain the
 * `live` reference conservatively (crash recovery remains #11539).
 */
export function mayReleaseManagedCandidateReferences(shutdownState: string): boolean {
  return shutdownState === 'stopped';
}

/**
 * Marks this session's reference `released` in every compatibility namespace
 * under one storage root. Runs at language-client teardown after shutdown
 * was proven terminal (see {@link mayReleaseManagedCandidateReferences}).
 * Only the exact session-owned file is touched; other sessions' references
 * and unrecognizable states are left alone for conservative recovery
 * (#11539).
 */
export function releaseManagedCandidateSessionReferences(
  storageRoot: string,
  sessionId: string,
  log: RuntimeLog,
): void {
  if (!isSessionIdString(sessionId)) {
    return;
  }
  const managedRoot = path.join(storageRoot, 'managed');
  let namespaces: fs.Dirent[];
  try {
    namespaces = fs.readdirSync(managedRoot, { withFileTypes: true });
  } catch {
    return;
  }
  for (const namespace of namespaces) {
    if (!namespace.isDirectory()) {
      continue;
    }
    const referencePath = hostReferencePath(path.join(managedRoot, namespace.name), sessionId);
    const read = readJsonFileSync(referencePath);
    if (!('value' in read)) {
      continue;
    }
    const record = read.value as Partial<ManagedHostCandidateReference>;
    if (
      typeof record !== 'object' ||
      record === null ||
      record.schema_version !== 'managed_host_candidate_reference.v1'
    ) {
      log(`leaving unrecognizable managed host reference untouched: ${namespace.name}`);
      continue;
    }
    try {
      const released = releaseManagedHostReference(record as ManagedHostCandidateReference);
      atomicWriteFileSync(referencePath, `${JSON.stringify(released, null, 2)}\n`);
    } catch (error: unknown) {
      log(
        `could not release managed host reference: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
}

/**
 * Binds a launch to its candidate: derives the install namespace from the
 * resolved server path and persists the session's `live` reference before the
 * process spawns. A no-op for user-managed or pre-policy install dirs (they
 * carry no manifest and are never deletion subjects).
 */
export function acquireLaunchManagedCandidateReference(
  serverPath: string,
  sessionId: string,
  log: RuntimeLog,
): string | null {
  const installDir = path.dirname(serverPath);
  const manifest = readInstalledManagedCandidateManifest(installDir);
  if (manifest === null) {
    return null;
  }
  const baseDir = path.dirname(installDir);
  const reference = acquireSessionManagedHostReference(
    baseDir,
    sessionId,
    manifest.candidate_id,
    log,
  );
  return reference === null ? null : reference.candidate_id;
}

/**
 * Enumerates the namespace's install dirs and their manifests. A manifest
 * that exists but is unreadable or unparseable marks the enumeration
 * incomplete (GC must then refuse); an absent manifest is expected for
 * pre-policy and legacy dirs, which are simply not deletion subjects.
 */
export function enumerateManagedCandidateCatalog(
  baseDir: string,
): ManagedCandidateCatalogEnumeration {
  const candidateDirs = new Map<string, string[]>();
  let dirs: { dir: string; mtime: number }[];
  try {
    dirs = fs
      .readdirSync(baseDir, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => {
        const dir = path.join(baseDir, entry.name);
        let mtime = 0;
        try {
          mtime = fs.statSync(dir).mtimeMs;
        } catch {
          mtime = 0;
        }
        return { dir, mtime };
      })
      .sort((a, b) => b.mtime - a.mtime);
  } catch {
    return { entries: [], candidateDirs, complete: false };
  }

  const entries: ManagedCandidateCatalogEntry[] = [];
  for (const { dir } of dirs) {
    const manifestRead = readJsonFileSync(path.join(dir, MANAGED_CANDIDATE_MANIFEST_FILE));
    if (!('value' in manifestRead)) {
      if (manifestRead.error === 'read') {
        return { entries: [], candidateDirs, complete: false };
      }
      // Absent manifest: pre-policy/legacy dir. Not a catalog entry and not a
      // deletion subject; it does not poison enumeration of the rest.
      continue;
    }
    const record = manifestRead.value as Partial<ManagedCandidateManifest>;
    // Include even structurally invalid manifests: the landed classifier
    // turns any malformed catalog entry into a namespace-wide deletion block,
    // which is exactly the fail-safe this wiring must not bypass.
    entries.push({ manifest: record as ManagedCandidateManifest, immutable: true });
    if (
      typeof record === 'object' &&
      record !== null &&
      typeof record.candidate_id === 'string' &&
      isCanonicalManagedCandidateId(record.candidate_id)
    ) {
      const owned = candidateDirs.get(record.candidate_id) ?? [];
      owned.push(dir);
      candidateDirs.set(record.candidate_id, owned);
    }
  }
  return { entries, candidateDirs, complete: true };
}

/**
 * Garbage-collects one compatibility namespace through the landed retention
 * policy. Deletes only install dirs whose candidate classifies
 * `stale_unreferenced`; every other class, every manifest-less dir, and every
 * file at the namespace root is preserved. The immediately previous committed
 * generation is supplied to the policy's caller-retention input
 * (`compatible_retained_ids`) as the previous-known-good fallback the runtime
 * has always kept — recency selects what to protect here, never what to
 * delete. Deletion failures are logged and never propagate.
 */
export function collectStaleManagedCandidates(
  baseDir: string,
  log: RuntimeLog,
): ManagedGarbageCollectResult {
  const result: ManagedGarbageCollectResult = { removed: [], retained: {}, blockedReason: null };
  const current = readManagedCurrentSelection(baseDir);
  if (current === null) {
    result.blockedReason = 'current selection record is absent or unreadable';
    return result;
  }
  const catalog = enumerateManagedCandidateCatalog(baseDir);
  if (!catalog.complete) {
    result.blockedReason = 'candidate catalog enumeration was incomplete';
    return result;
  }
  const hostReferences = enumerateManagedHostReferences(baseDir);
  if (!hostReferences.complete) {
    result.blockedReason = 'host reference enumeration was incomplete';
    return result;
  }

  // Previous-known-good: the most recently installed catalog candidate that
  // is not the current selection. Catalog enumeration is newest-first.
  const previousKnownGoodIds = new Set<string>();
  for (const candidateId of catalog.candidateDirs.keys()) {
    if (candidateId !== current.candidate_id) {
      previousKnownGoodIds.add(candidateId);
      break;
    }
  }

  const input: ManagedRetentionInput = {
    current,
    catalog: catalog.entries,
    host_references: hostReferences.references,
    host_references_complete: hostReferences.complete,
    compatible_retained_ids: previousKnownGoodIds,
  };

  for (const [candidateId, dirs] of catalog.candidateDirs) {
    const retentionClass = classifyManagedCandidateRetention(candidateId, input);
    if (!mayGarbageCollectManagedCandidate(candidateId, input)) {
      const retained = result.retained[retentionClass] ?? [];
      retained.push(candidateId);
      result.retained[retentionClass] = retained;
      continue;
    }
    for (const dir of dirs) {
      try {
        fs.rmSync(dir, { recursive: true, force: true });
        result.removed.push(dir);
        log(`Removed stale managed candidate: ${path.basename(dir)}`);
      } catch (error: unknown) {
        log(
          `Could not remove stale managed candidate ${path.basename(dir)}: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
  }
  return result;
}
