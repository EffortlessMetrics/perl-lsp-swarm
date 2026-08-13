import { createHash } from 'crypto';

export type ManagedMutationOperation = 'install' | 'repair' | 'update' | 'cleanup';
export type ManagedLeaseDisposition =
  | 'acquired'
  | 'busy_current_writer'
  | 'stale_or_abandoned_needs_recovery'
  | 'invalid_lease_state'
  | 'cancelled'
  | 'timed_out'
  | 'instrument_failed';
export type ManagedOwnerLiveness = 'alive' | 'definitely_gone' | 'unknown';
export type ManagedCoreVerification = 'verified' | 'not_proven';
export type ManagedDapVerification = 'verified' | 'not_present' | 'not_proven';

export interface ManagedCandidateSubject {
  release: string;
  version: string;
  target: string;
  topology_digest: string;
  perllsp_digest: string;
  perl_dap_digest: string | null;
}

export interface ManagedCandidateManifest {
  schema_version: 'managed_candidate_manifest.v1';
  candidate_id: string;
  subject: ManagedCandidateSubject;
  verification: {
    perllsp: ManagedCoreVerification;
    perl_dap: ManagedDapVerification;
    topology: ManagedCoreVerification;
    provenance: ManagedCoreVerification;
  };
}

export interface ManagedMutationAttempt {
  schema_version: 'managed_mutation_attempt.v1';
  attempt_id: string;
  operation: ManagedMutationOperation;
  candidate_id: string;
  owner_nonce: string;
  owner_process_hint: number | null;
}

export interface ManagedMutationLease {
  schema_version: 'managed_mutation_lease.v1';
  attempt_id: string;
  candidate_id: string;
  operation: ManagedMutationOperation;
  lease_token: string;
  owner_nonce: string;
  owner_process_hint: number | null;
}

export interface ManagedAbandonmentEvidence {
  owner_liveness: ManagedOwnerLiveness;
  exact_attempt_token_matches: boolean;
  candidate_manifest_published: boolean;
}

const SHA256 = /^(?:sha256:)?[0-9a-fA-F]{64}$/;
const SAFE_ID = /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/;

function normalizeDigest(value: string): string {
  const normalized = value.toLowerCase();
  return normalized.startsWith('sha256:') ? normalized : `sha256:${normalized}`;
}

function canonicalCandidateSubject(subject: ManagedCandidateSubject): string {
  return [
    subject.release,
    subject.version,
    subject.target,
    normalizeDigest(subject.topology_digest),
    normalizeDigest(subject.perllsp_digest),
    subject.perl_dap_digest === null ? 'dap:none' : normalizeDigest(subject.perl_dap_digest),
  ].join('\n');
}

export function managedCandidateId(subject: ManagedCandidateSubject): string {
  return `candidate-${createHash('sha256').update(canonicalCandidateSubject(subject)).digest('hex')}`;
}

export function buildManagedCandidateManifest(
  subject: ManagedCandidateSubject,
  provenance: ManagedCoreVerification = 'verified',
): ManagedCandidateManifest {
  return {
    schema_version: 'managed_candidate_manifest.v1',
    candidate_id: managedCandidateId(subject),
    subject: {
      ...subject,
      topology_digest: normalizeDigest(subject.topology_digest),
      perllsp_digest: normalizeDigest(subject.perllsp_digest),
      perl_dap_digest:
        subject.perl_dap_digest === null ? null : normalizeDigest(subject.perl_dap_digest),
    },
    verification: {
      perllsp: 'verified',
      perl_dap: subject.perl_dap_digest === null ? 'not_present' : 'verified',
      topology: 'verified',
      provenance,
    },
  };
}

export function validateManagedCandidateManifest(manifest: ManagedCandidateManifest): string[] {
  const errors: string[] = [];
  const expectedCandidateId = managedCandidateId(manifest.subject);

  if (manifest.candidate_id !== expectedCandidateId) {
    errors.push('candidate_id does not match the canonical subject');
  }
  if (!SHA256.test(manifest.subject.topology_digest)) {
    errors.push('topology_digest must be sha256');
  }
  if (!SHA256.test(manifest.subject.perllsp_digest)) {
    errors.push('perllsp_digest must be sha256');
  }
  if (manifest.subject.perl_dap_digest !== null && !SHA256.test(manifest.subject.perl_dap_digest)) {
    errors.push('perl_dap_digest must be sha256 when present');
  }
  if (
    (manifest.subject.perl_dap_digest === null && manifest.verification.perl_dap !== 'not_present') ||
    (manifest.subject.perl_dap_digest !== null && manifest.verification.perl_dap !== 'verified')
  ) {
    errors.push('perl-dap verification must agree with candidate pair membership');
  }
  if (manifest.verification.perllsp !== 'verified' || manifest.verification.topology !== 'verified') {
    errors.push('candidate manifest cannot represent a partially verified core subject');
  }

  return errors;
}

export function validateManagedMutationAttempt(attempt: ManagedMutationAttempt): string[] {
  const errors: string[] = [];
  if (!SAFE_ID.test(attempt.attempt_id)) {
    errors.push('attempt_id must be a bounded path-safe identity');
  }
  if (!SAFE_ID.test(attempt.owner_nonce)) {
    errors.push('owner_nonce must be a bounded path-safe identity');
  }
  if (!attempt.candidate_id.startsWith('candidate-')) {
    errors.push('candidate_id must use canonical candidate identity');
  }
  if (
    attempt.owner_process_hint !== null &&
    (!Number.isInteger(attempt.owner_process_hint) || attempt.owner_process_hint <= 0)
  ) {
    errors.push('owner_process_hint must be a positive integer when present');
  }
  return errors;
}

export function buildManagedMutationLease(
  attempt: ManagedMutationAttempt,
  leaseToken: string,
): ManagedMutationLease {
  const errors = validateManagedMutationAttempt(attempt);
  if (errors.length > 0) {
    throw new Error(`invalid managed mutation attempt: ${errors.join('; ')}`);
  }
  if (!SAFE_ID.test(leaseToken)) {
    throw new Error('lease token must be a bounded path-safe identity');
  }
  return {
    schema_version: 'managed_mutation_lease.v1',
    attempt_id: attempt.attempt_id,
    candidate_id: attempt.candidate_id,
    operation: attempt.operation,
    lease_token: leaseToken,
    owner_nonce: attempt.owner_nonce,
    owner_process_hint: attempt.owner_process_hint,
  };
}

export function mayReleaseManagedLease(lease: ManagedMutationLease, leaseToken: string): boolean {
  return lease.lease_token === leaseToken;
}

export function classifyManagedLeaseRecovery(
  evidence: ManagedAbandonmentEvidence,
): ManagedLeaseDisposition {
  if (!evidence.exact_attempt_token_matches) {
    return 'invalid_lease_state';
  }
  if (evidence.owner_liveness === 'alive') {
    return 'busy_current_writer';
  }
  if (evidence.owner_liveness === 'unknown') {
    return 'busy_current_writer';
  }
  return 'stale_or_abandoned_needs_recovery';
}

export function managedAttemptDirectoryName(attempt: ManagedMutationAttempt): string {
  const errors = validateManagedMutationAttempt(attempt);
  if (errors.length > 0) {
    throw new Error(`invalid managed mutation attempt: ${errors.join('; ')}`);
  }
  return `attempt-${attempt.attempt_id}-${attempt.owner_nonce}`;
}
