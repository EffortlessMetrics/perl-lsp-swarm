import {
  type ManagedCandidateSubject,
  type ManagedMutationAttempt,
  buildManagedCandidateManifest,
  buildManagedMutationLease,
  classifyManagedLeaseRecovery,
  managedAttemptDirectoryName,
  managedCandidateId,
  mayReleaseManagedLease,
  validateManagedCandidateManifest,
  validateManagedMutationAttempt,
} from '../managedCacheProtocol';

const DIGEST_A = 'a'.repeat(64);
const DIGEST_B = 'b'.repeat(64);
const DIGEST_C = 'c'.repeat(64);

function subject(): ManagedCandidateSubject {
  return {
    release: '0.18.0',
    version: '0.18.0',
    target: 'x86_64-unknown-linux-gnu',
    topology_digest: DIGEST_A,
    perllsp_digest: DIGEST_B,
    perl_dap_digest: DIGEST_C,
  };
}

function attempt(): ManagedMutationAttempt {
  return {
    schema_version: 'managed_mutation_attempt.v1',
    attempt_id: 'install-018-linux-x64',
    operation: 'install',
    candidate_id: managedCandidateId(subject()),
    owner_nonce: 'owner-7f2a',
    owner_process_hint: 4242,
  };
}

describe('managed cache protocol', () => {
  test('derives deterministic candidate identity from the complete pair subject', () => {
    const first = managedCandidateId(subject());
    const second = managedCandidateId(subject());
    expect(first).toBe(second);
    expect(first).toMatch(/^candidate-[0-9a-f]{64}$/);

    const changed = subject();
    changed.perl_dap_digest = 'd'.repeat(64);
    expect(managedCandidateId(changed)).not.toBe(first);
  });

  test('builds a complete verified server and DAP candidate manifest', () => {
    const manifest = buildManagedCandidateManifest(subject());
    expect(validateManagedCandidateManifest(manifest)).toEqual([]);
    expect(manifest.subject.topology_digest).toBe(`sha256:${DIGEST_A}`);
    expect(manifest.subject.perllsp_digest).toBe(`sha256:${DIGEST_B}`);
    expect(manifest.subject.perl_dap_digest).toBe(`sha256:${DIGEST_C}`);
    expect(manifest.verification.perl_dap).toBe('verified');
  });

  test('rejects candidate identity drift and DAP pair inconsistency', () => {
    const manifest = buildManagedCandidateManifest(subject());
    manifest.candidate_id = `candidate-${'0'.repeat(64)}`;
    manifest.verification.perl_dap = 'not_present';

    expect(validateManagedCandidateManifest(manifest)).toEqual(
      expect.arrayContaining([
        'candidate_id does not match the canonical subject',
        'perl-dap verification must agree with candidate pair membership',
      ]),
    );
  });

  test('requires path-safe attempt and owner identities', () => {
    const value = attempt();
    value.attempt_id = '../steal-other-attempt';
    value.owner_nonce = 'owner/other';

    expect(validateManagedMutationAttempt(value)).toEqual(
      expect.arrayContaining([
        'attempt_id must be a bounded path-safe identity',
        'owner_nonce must be a bounded path-safe identity',
      ]),
    );
  });

  test('releases only with the exact lease token rather than PID or age', () => {
    const lease = buildManagedMutationLease(attempt(), 'lease-3f3d');
    expect(mayReleaseManagedLease(lease, 'lease-3f3d')).toBe(true);
    expect(mayReleaseManagedLease(lease, 'lease-other')).toBe(false);
  });

  test('does not reclaim an unknown or live owner merely because state looks old', () => {
    expect(
      classifyManagedLeaseRecovery({
        owner_liveness: 'alive',
        exact_attempt_token_matches: true,
        candidate_manifest_published: false,
      }),
    ).toBe('busy_current_writer');
    expect(
      classifyManagedLeaseRecovery({
        owner_liveness: 'unknown',
        exact_attempt_token_matches: true,
        candidate_manifest_published: false,
      }),
    ).toBe('busy_current_writer');
  });

  test('requires exact attempt identity before abandoned-owner recovery', () => {
    expect(
      classifyManagedLeaseRecovery({
        owner_liveness: 'definitely_gone',
        exact_attempt_token_matches: false,
        candidate_manifest_published: false,
      }),
    ).toBe('invalid_lease_state');
    expect(
      classifyManagedLeaseRecovery({
        owner_liveness: 'definitely_gone',
        exact_attempt_token_matches: true,
        candidate_manifest_published: true,
      }),
    ).toBe('stale_or_abandoned_needs_recovery');
  });

  test('derives attempt-private directory names without paths', () => {
    expect(managedAttemptDirectoryName(attempt())).toBe('attempt-install-018-linux-x64-owner-7f2a');
  });
});
