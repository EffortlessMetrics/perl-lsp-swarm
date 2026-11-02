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

export const MANAGED_MUTATION_LEASE_FILE = 'managed_mutation_lease.json';

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

export interface ManagedCacheLockHandle {
  readonly leaseToken: string;
  release(): void;
}

export interface AcquireManagedCacheLockOptions {
  timeoutMs?: number;
  pollIntervalMs?: number;
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

function lockFilePath(baseDir: string): string {
  return path.join(baseDir, MANAGED_MUTATION_LEASE_FILE);
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
  const filePath = lockFilePath(baseDir);
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

function writeLeaseFile(baseDir: string, lease: ManagedMutationLease): void {
  const filePath = lockFilePath(baseDir);
  const payload = `${JSON.stringify(lease, null, 2)}\n`;
  const handle = fs.openSync(
    filePath,
    fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_NOFOLLOW,
    0o600,
  );
  try {
    fs.writeFileSync(handle, payload, { encoding: 'utf8' });
  } finally {
    fs.closeSync(handle);
  }
}

function removeLeaseFileIfPresent(baseDir: string): void {
  try {
    fs.rmSync(lockFilePath(baseDir), { force: true });
  } catch {
    /* best-effort */
  }
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
  removeLeaseFileIfPresent(baseDir);
  return true;
}

function tryAcquireLeaseOnce(
  baseDir: string,
  attempt: ManagedMutationAttempt,
  leaseToken: string,
): 'acquired' | 'busy' {
  fs.mkdirSync(baseDir, { recursive: true });
  const lease = buildManagedMutationLease(attempt, leaseToken);
  try {
    writeLeaseFile(baseDir, lease);
    return 'acquired';
  } catch (error: unknown) {
    const code = (error as NodeJS.ErrnoException | null)?.code;
    if (code !== 'EEXIST') {
      throw error;
    }
  }

  const existing = readExistingLease(baseDir);
  if (existing === null) {
    return 'busy';
  }
  if (tryBreakAbandonedLease(baseDir, existing)) {
    try {
      writeLeaseFile(baseDir, lease);
      return 'acquired';
    } catch (retryError: unknown) {
      const code = (retryError as NodeJS.ErrnoException | null)?.code;
      if (code === 'EEXIST') {
        return 'busy';
      }
      throw retryError;
    }
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
  const leaseToken = randomUUID().replace(/-/g, '');
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const outcome = tryAcquireLeaseOnce(baseDir, attempt, leaseToken);
    if (outcome === 'acquired') {
      return {
        leaseToken,
        release: () => {
          const onDisk = readExistingLease(baseDir);
          if (onDisk !== null && mayReleaseManagedLease(onDisk, leaseToken)) {
            removeLeaseFileIfPresent(baseDir);
          }
        },
      };
    }
    await delay(pollIntervalMs);
  }

  throw new ManagedCacheMutationBusyError();
}
