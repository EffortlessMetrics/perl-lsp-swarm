import * as fs from 'fs';
import * as path from 'path';
import { createHash, randomUUID } from 'crypto';
import type {
  ManagedMutationAttempt,
  ManagedMutationLease,
  ManagedMutationOperation,
} from './managedCacheProtocol';
import {
  buildManagedMutationLease,
  classifyManagedLeaseRecovery,
  mayReleaseManagedLease,
  type ManagedAbandonmentEvidence,
  type ManagedOwnerLiveness,
} from './managedCacheProtocol';

export const MANAGED_MUTATION_LOCK_DIR = 'managed_mutation_lock';
export const MANAGED_MUTATION_LEASE_FILE = 'lease.json';

const DEFAULT_LOCK_TIMEOUT_MS = 120_000;
const DEFAULT_POLL_INTERVAL_MS = 100;

/** User-facing error when another extension host holds the managed cache lock. */
export class ManagedCacheMutationBusyError extends Error {
  readonly code = 'managed_install_busy';

  constructor(message = 'Managed binary install is already in progress in another window.') {
    super(message);
    this.name = 'ManagedCacheMutationBusyError';
  }
}

export class ManagedCacheMutationCancelledError extends Error {
  readonly code = 'managed_install_cancelled';

  constructor(message = 'Managed binary install was cancelled.') {
    super(message);
    this.name = 'ManagedCacheMutationCancelledError';
  }
}

export interface ManagedCacheLockHandle {
  readonly leaseToken: string;
  release(): void;
}

export interface AcquireManagedCacheLockOptions {
  timeoutMs?: number;
  pollIntervalMs?: number;
  isCancelled?: () => boolean;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function namespaceCandidateId(compatibilityKey: string): string {
  const digest = createHash('sha256').update(`managed-namespace:${compatibilityKey}`).digest('hex');
  return `candidate-${digest}`;
}

/**
 * Builds a namespace-scoped mutation attempt identity for cross-process cache
 * serialization. The candidate id labels the compatibility namespace, not a
 * finished artifact digest.
 */
export function buildManagedCacheMutationAttempt(
  compatibilityKey: string,
  operation: ManagedMutationOperation,
): ManagedMutationAttempt {
  const suffix = randomUUID().replace(/-/g, '').slice(0, 12);
  return {
    schema_version: 'managed_mutation_attempt.v1',
    attempt_id: `${operation}-${suffix}`,
    operation,
    candidate_id: namespaceCandidateId(compatibilityKey),
    owner_nonce: randomUUID().replace(/-/g, '').slice(0, 16),
    owner_process_hint: process.pid,
  };
}

function lockDirPath(baseDir: string): string {
  return path.join(baseDir, MANAGED_MUTATION_LOCK_DIR);
}

function leaseFilePath(baseDir: string): string {
  return path.join(lockDirPath(baseDir), MANAGED_MUTATION_LEASE_FILE);
}

function probeOwnerLiveness(pid: number | null): ManagedOwnerLiveness {
  if (pid === null || !Number.isInteger(pid) || pid <= 0) {
    return 'unknown';
  }
  try {
    process.kill(pid, 0);
    return 'alive';
  } catch (error: unknown) {
    const code = (error as NodeJS.ErrnoException | null)?.code;
    if (code === 'ESRCH') {
      return 'definitely_gone';
    }
    return 'unknown';
  }
}

function readExistingLease(baseDir: string): ManagedMutationLease | null {
  const filePath = leaseFilePath(baseDir);
  if (!fs.existsSync(filePath)) {
    return null;
  }
  let raw: string;
  try {
    raw = fs.readFileSync(filePath, 'utf8');
  } catch {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as Partial<ManagedMutationLease>;
    if (parsed.schema_version !== 'managed_mutation_lease.v1') {
      return null;
    }
    return parsed as ManagedMutationLease;
  } catch {
    return null;
  }
}

function atomicWriteLeaseFile(baseDir: string, lease: ManagedMutationLease): void {
  const dir = lockDirPath(baseDir);
  fs.mkdirSync(dir, { recursive: true });
  const finalPath = path.join(dir, MANAGED_MUTATION_LEASE_FILE);
  const tmpPath = path.join(
    dir,
    `${MANAGED_MUTATION_LEASE_FILE}.${process.pid}-${randomUUID()}.tmp`,
  );
  const payload = `${JSON.stringify(lease, null, 2)}\n`;
  fs.writeFileSync(tmpPath, payload, { encoding: 'utf8' });
  fs.renameSync(tmpPath, finalPath);
}

function tryCreateLockDir(baseDir: string): boolean {
  try {
    fs.mkdirSync(lockDirPath(baseDir));
    return true;
  } catch (error: unknown) {
    const code = (error as NodeJS.ErrnoException | null)?.code;
    if (code === 'EEXIST') {
      return false;
    }
    throw error;
  }
}

function lockDirIsEmpty(baseDir: string): boolean {
  try {
    return fs.readdirSync(lockDirPath(baseDir)).length === 0;
  } catch {
    return false;
  }
}

function tryReclaimLockDir(baseDir: string): boolean {
  const dir = lockDirPath(baseDir);
  const trashPath = path.join(baseDir, `${MANAGED_MUTATION_LOCK_DIR}.reclaim-${randomUUID()}`);
  try {
    fs.renameSync(dir, trashPath);
  } catch {
    return false;
  }
  try {
    fs.rmSync(trashPath, { recursive: true, force: true });
  } catch {
    /* best-effort cleanup of abandoned trash */
  }
  return tryCreateLockDir(baseDir);
}

function abandonmentEvidence(lease: ManagedMutationLease): ManagedAbandonmentEvidence {
  return {
    owner_liveness: probeOwnerLiveness(lease.owner_process_hint),
    exact_attempt_token_matches: true,
    candidate_manifest_published: false,
  };
}

function tryBreakAbandonedLease(baseDir: string, lease: ManagedMutationLease): boolean {
  const disposition = classifyManagedLeaseRecovery(abandonmentEvidence(lease));
  if (disposition !== 'stale_or_abandoned_needs_recovery') {
    return false;
  }
  return tryReclaimLockDir(baseDir);
}

function tryAcquireLeaseOnce(
  baseDir: string,
  attempt: ManagedMutationAttempt,
  leaseToken: string,
): 'acquired' | 'busy' {
  fs.mkdirSync(baseDir, { recursive: true });
  const lease = buildManagedMutationLease(attempt, leaseToken);

  if (tryCreateLockDir(baseDir)) {
    atomicWriteLeaseFile(baseDir, lease);
    return 'acquired';
  }

  const existing = readExistingLease(baseDir);
  if (existing === null) {
    if (lockDirIsEmpty(baseDir) && tryReclaimLockDir(baseDir)) {
      atomicWriteLeaseFile(baseDir, lease);
      return 'acquired';
    }
    return 'busy';
  }

  if (tryBreakAbandonedLease(baseDir, existing)) {
    if (!tryCreateLockDir(baseDir)) {
      return 'busy';
    }
    atomicWriteLeaseFile(baseDir, lease);
    return 'acquired';
  }

  return 'busy';
}

/**
 * Resolves the active managed perllsp path inside one compatibility namespace,
 * following the legacy `current` dir pointer when present.
 */
export function resolveManagedBinaryPathInNamespace(baseDir: string): string | null {
  const binaryName = process.platform === 'win32' ? 'perllsp.exe' : 'perllsp';
  const pointerPath = path.join(baseDir, 'current');
  if (!fs.existsSync(pointerPath)) {
    return null;
  }
  let content = '';
  try {
    content = fs.readFileSync(pointerPath, 'utf8').trim();
  } catch {
    return null;
  }
  if (!content || !/^[A-Za-z0-9._-]+$/.test(content) || content.includes('..')) {
    return null;
  }
  const installDir = path.join(baseDir, content);
  const binaryPath = path.join(installDir, binaryName);
  if (!fs.existsSync(binaryPath)) {
    return null;
  }
  return binaryPath;
}

export async function acquireManagedCacheMutationLock(
  baseDir: string,
  attempt: ManagedMutationAttempt,
  options: AcquireManagedCacheLockOptions = {},
): Promise<ManagedCacheLockHandle> {
  const timeoutMs = options.timeoutMs ?? DEFAULT_LOCK_TIMEOUT_MS;
  const pollIntervalMs = options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
  const isCancelled = options.isCancelled;
  const leaseToken = randomUUID().replace(/-/g, '');
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    if (isCancelled?.()) {
      throw new ManagedCacheMutationCancelledError();
    }
    const outcome = tryAcquireLeaseOnce(baseDir, attempt, leaseToken);
    if (outcome === 'acquired') {
      return {
        leaseToken,
        release: () => {
          const onDisk = readExistingLease(baseDir);
          if (onDisk !== null && mayReleaseManagedLease(onDisk, leaseToken)) {
            try {
              fs.rmSync(lockDirPath(baseDir), { recursive: true, force: true });
            } catch {
              /* best-effort */
            }
          }
        },
      };
    }
    await delay(pollIntervalMs);
  }

  throw new ManagedCacheMutationBusyError();
}
