import type {
  BinaryCompatibilityReason,
  BinaryCompatibilityState,
  BinaryIdentityPacketV1,
  BinaryIdentityResponseV1,
} from '../binaryIdentityProtocol.generated';
import {
  BinaryIdentityNoticeTracker,
  projectBinaryIdentityStatus,
} from '../binaryIdentityStatus';

function packet(role: 'server' | 'dap' = 'server'): BinaryIdentityPacketV1 {
  const executable = role === 'server' ? 'perllsp' : 'perl-dap';
  return {
    schema_version: 'perl_lsp.binary_identity.v1',
    product: {
      name: 'perl-lsp',
      public_repository: 'EffortlessMetrics/perl-lsp',
      development_repository: 'EffortlessMetrics/perl-lsp-swarm',
    },
    binary: {
      executable,
      cargo_package: executable,
      role,
      version: '0.18.0',
    },
    build: {
      source_revision: 'abc123',
      target: 'x86_64-unknown-linux-gnu',
      identity_state: 'exact',
    },
    artifact: {
      role: 'managed',
      candidate_identity: 'rc1',
    },
    compatibility: {
      expected_product_identity_version: 1,
      dap_posture: 'preview',
    },
  };
}

function response(
  compatibility: BinaryCompatibilityState,
  reasons: BinaryCompatibilityReason[],
): BinaryIdentityResponseV1 {
  return {
    feature_version: 1,
    server: packet('server'),
    dap: packet('dap'),
    expected_extension: {
      id: 'EffortlessMetrics.perl-lsp-rs',
      version: '0.18.0',
      candidate_identity: 'rc1',
      target: 'x86_64-unknown-linux-gnu',
    },
    server_instance_id: 'server-1',
    environment_snapshot_id: 'env-1',
    compatibility,
    reasons,
    redacted: true,
  };
}

describe('binary identity status', () => {
  test('exact identity is quiet and ready', () => {
    const presentation = projectBinaryIdentityStatus(
      response('exact_match', ['exact_identity_match']),
      'managed',
    );
    expect(presentation.state).toBe('ready_exact');
    expect(presentation.action).toBe('none');
    expect(presentation.quiet).toBe(true);
  });

  test('partial identity stays usable without claiming exact parity', () => {
    const presentation = projectBinaryIdentityStatus(
      response('compatible_partial', ['source_revision_not_proven']),
      'user_supplied',
    );
    expect(presentation.state).toBe('ready_partial');
    expect(presentation.quiet).toBe(true);
    expect(presentation.detail).toContain('not fully proven');
  });

  test('managed mismatch routes to governed repair', () => {
    const presentation = projectBinaryIdentityStatus(
      response('mismatch', ['candidate_mismatch']),
      'managed',
    );
    expect(presentation.state).toBe('update_or_repair_required');
    expect(presentation.action).toBe('repair_managed_pair');
    expect(presentation.quiet).toBe(false);
  });

  test('user-supplied mismatch is never replaced automatically', () => {
    const presentation = projectBinaryIdentityStatus(
      response('mismatch', ['target_mismatch']),
      'user_supplied',
    );
    expect(presentation.state).toBe('configured_binary_incompatible');
    expect(presentation.action).toBe('inspect_configured_binary');
  });

  test('stale status refreshes rather than escalating to reinstall', () => {
    const presentation = projectBinaryIdentityStatus(
      response('stale', ['server_instance_stale']),
      'managed',
    );
    expect(presentation.state).toBe('not_proven');
    expect(presentation.action).toBe('refresh_identity');
    expect(presentation.quiet).toBe(true);
  });

  test('support packet is redacted and contains no install path', () => {
    const presentation = projectBinaryIdentityStatus(
      response('mismatch', ['version_mismatch']),
      'managed',
    );
    expect(presentation.supportPacket).toContain('"redacted": true');
    expect(presentation.supportPacket).not.toContain('/Users/');
    expect(presentation.supportPacket).not.toContain('C:\\Users\\');
  });

  test('repeated identical mismatch notifies once and recovery resets', () => {
    const tracker = new BinaryIdentityNoticeTracker();
    const mismatch = projectBinaryIdentityStatus(
      response('mismatch', ['version_mismatch']),
      'managed',
    );
    expect(tracker.shouldNotify(mismatch)).toBe(true);
    expect(tracker.shouldNotify(mismatch)).toBe(false);

    const ready = projectBinaryIdentityStatus(
      response('exact_match', ['exact_identity_match']),
      'managed',
    );
    expect(tracker.shouldNotify(ready)).toBe(false);
    expect(tracker.shouldNotify(mismatch)).toBe(true);
  });
});
