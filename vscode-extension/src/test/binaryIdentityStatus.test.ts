import type {
  BinaryCompatibilityReason,
  BinaryCompatibilityState,
  BinaryIdentityPacketV1,
  BinaryIdentityResponseV1,
} from '../binaryIdentityProtocol.generated';
import {
  BinaryIdentityNoticeTracker,
  CLIENT_REASON_COMPATIBILITY_UNKNOWN,
  CLIENT_REASON_MANAGED_BINARY_ABSENT,
  CLIENT_REASON_RESPONSE_MALFORMED,
  CLIENT_REASON_SERVER_NOT_RUNNING,
  projectBinaryIdentityStatus,
  projectUnavailableBinaryIdentity,
  type BinaryIdentityPresentation,
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
  serverInstanceId = 'server-1',
): BinaryIdentityResponseV1 {
  return {
    feature_version: 1,
    server: packet('server'),
    dap: packet('dap'),
    expected_extension: {
      publisher: 'EffortlessMetrics',
      package_name: 'perl-lsp-rs',
      id: 'EffortlessMetrics.perl-lsp-rs',
      version: '0.18.0',
      candidate_identity: 'rc1',
      target: 'x86_64-unknown-linux-gnu',
      binary_artifact_role: 'managed',
      authority_identity: 'vsix:EffortlessMetrics.perl-lsp-rs',
    },
    server_instance_id: serverInstanceId,
    environment_snapshot_id: 'env-1',
    compatibility,
    reasons,
    redacted: true,
  };
}

const READY_STATES = new Set(['ready_exact', 'ready_partial']);

function expectNotReady(presentation: BinaryIdentityPresentation): void {
  expect(READY_STATES.has(presentation.state)).toBe(false);
}

describe('binary identity status', () => {
  test('exact managed server/DAP/VSIX candidate is quiet and ready', () => {
    const presentation = projectBinaryIdentityStatus(
      response('exact_match', ['exact_identity_match']),
      'managed',
    );
    expect(presentation.state).toBe('ready_exact');
    expect(presentation.action).toBe('none');
    expect(presentation.quiet).toBe(true);
    expect(presentation.noticeKey).toBeUndefined();
  });

  test('user-supplied wrapper with exact identity is ready, not rejected by its role', () => {
    const presentation = projectBinaryIdentityStatus(
      response('exact_match', ['exact_identity_match']),
      'user_supplied',
    );
    expect(presentation.state).toBe('ready_exact');
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
    expect(presentation.detail).toContain('source_revision_not_proven');
    expect(presentation.label).not.toMatch(/verified/i);
  });

  test.each<[string, BinaryCompatibilityReason[]]>([
    ['same version, different source revision', ['source_revision_mismatch']],
    [
      'stale v0.17 managed binary after a v0.18 extension update',
      ['version_mismatch', 'candidate_mismatch'],
    ],
    ['wrong target with an otherwise matching version', ['target_mismatch']],
    ['mixed server and DAP candidates', ['dap_role_mismatch', 'candidate_mismatch']],
    ['stale managed cache bytes', ['artifact_digest_mismatch']],
  ])('managed mismatch (%s) requires governed repair', (_case, reasons) => {
    const presentation = projectBinaryIdentityStatus(response('mismatch', reasons), 'managed');
    expect(presentation.state).toBe('update_or_repair_required');
    expect(presentation.action).toBe('repair_managed_pair');
    expect(presentation.quiet).toBe(false);
    expect(presentation.reasons).toEqual([...reasons].sort());
    for (const reason of reasons) {
      expect(presentation.detail).toContain(reason);
    }
    expectNotReady(presentation);
  });

  test('a mismatch whose reasons are all newer than this extension is still not ready', () => {
    const presentation = projectBinaryIdentityStatus(
      response('mismatch', ['future_identity_family_mismatch']),
      'managed',
    );
    expect(presentation.state).toBe('update_or_repair_required');
    expect(presentation.reasons).toEqual(['future_identity_family_mismatch']);
  });

  test('user-supplied mismatch is never replaced automatically', () => {
    const presentation = projectBinaryIdentityStatus(
      response('mismatch', ['target_mismatch']),
      'user_supplied',
    );
    expect(presentation.state).toBe('configured_binary_incompatible');
    expect(presentation.action).toBe('inspect_configured_binary');
    expect(presentation.detail).toContain('will not be replaced automatically');
  });

  test('an unattributed binary (PATH/bundled) mismatch never offers managed repair', () => {
    const presentation = projectBinaryIdentityStatus(
      response('mismatch', ['version_mismatch']),
      'unknown',
    );
    expect(presentation.state).toBe('configured_binary_incompatible');
    expect(presentation.action).toBe('inspect_configured_binary');
    expect(presentation.label).toContain('Resolved');
  });

  test('a filename that looks right cannot override the server mismatch verdict', () => {
    // The packet names the canonical executable and matching version, the
    // things a filename/path heuristic would check, yet the server says the
    // source differs: the server verdict controls the result.
    const mismatch = response('mismatch', ['source_revision_mismatch']);
    expect(mismatch.server.binary.executable).toBe('perllsp');
    expect(mismatch.server.binary.version).toBe(mismatch.expected_extension.version);
    expectNotReady(projectBinaryIdentityStatus(mismatch, 'managed'));
  });

  test('legacy user-supplied binary with unsupported identity keeps LSP quietly', () => {
    const presentation = projectBinaryIdentityStatus(
      response('unsupported', ['feature_version_unsupported']),
      'user_supplied',
    );
    expect(presentation.state).toBe('unsupported');
    expect(presentation.quiet).toBe(true);
    expect(presentation.detail).toContain('Ordinary LSP remains available');
  });

  test('server not_proven is distinct from failure and from healthy', () => {
    const presentation = projectBinaryIdentityStatus(
      response('not_proven', ['build_identity_not_proven']),
      'managed',
    );
    expect(presentation.state).toBe('not_proven');
    expect(presentation.quiet).toBe(true);
    expect(presentation.action).toBe('copy_support_packet');
    expect(presentation.detail).toContain('not a product-failure verdict');
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

  test.each<[string, unknown]>([
    ['null', null],
    ['an empty object', {}],
    ['a wrong feature version', { ...response('exact_match', []), feature_version: 2 }],
    ['a missing server packet', { ...response('exact_match', []), server: undefined }],
    [
      'a non-string server version',
      {
        ...response('exact_match', []),
        server: { ...packet(), binary: { ...packet().binary, version: 18 } },
      },
    ],
    [
      'a missing reasons array',
      { ...response('exact_match', []), reasons: 'exact_identity_match' },
    ],
  ])('malformed response (%s) is not_proven, never a fabricated diagnosis', (_case, malformed) => {
    const presentation = projectBinaryIdentityStatus(
      malformed as BinaryIdentityResponseV1,
      'managed',
    );
    expect(presentation.state).toBe('not_proven');
    expect(presentation.reasons).toEqual([CLIENT_REASON_RESPONSE_MALFORMED]);
    expect(presentation.quiet).toBe(true);
    expect(presentation.noticeKey).toBeUndefined();
    expect(() => JSON.parse(presentation.supportPacket) as unknown).not.toThrow();
  });

  test('an unrecognized compatibility state is not_proven, never healthy', () => {
    const future = {
      ...response('exact_match', ['exact_identity_match']),
      compatibility: 'exact_match_v2',
    };
    const presentation = projectBinaryIdentityStatus(
      future as unknown as BinaryIdentityResponseV1,
      'managed',
    );
    expect(presentation.state).toBe('not_proven');
    expect(presentation.reasons).toContain(CLIENT_REASON_COMPATIBILITY_UNKNOWN);
  });

  test('oversized and non-token reason codes are bounded', () => {
    const reasons = [
      ...Array.from(
        { length: 40 },
        (_, index) => `future_reason_${String(index).padStart(2, '0')}`,
      ),
      'x'.repeat(5000),
      'Not A Code /etc/passwd',
    ];
    const presentation = projectBinaryIdentityStatus(response('mismatch', reasons), 'managed');
    expect(presentation.reasons.length).toBeLessThanOrEqual(16);
    expect(presentation.reasons.every((reason) => /^[a-z][a-z0-9_]{0,63}$/.test(reason))).toBe(
      true,
    );
    expect(presentation.supportPacket).not.toContain('/etc/passwd');
    expect(presentation.detail.length).toBeLessThan(600);
  });

  test('support packet is a closed, redacted projection without paths or secrets', () => {
    const leaky = response('mismatch', ['version_mismatch']) as BinaryIdentityResponseV1 & {
      server: BinaryIdentityPacketV1 & { install_path: string };
      home: string;
    };
    leaky.server = { ...leaky.server, install_path: '/Users/alice/.vscode/perllsp' };
    leaky.server.build = { ...leaky.server.build, profile: 'C:\\Users\\alice\\target' };
    leaky.home = '/home/alice';
    leaky.limitations = [
      'cache at /home/alice/.cache/perl-lsp',
      'token=ghp_abcdef',
      'dap identity absent',
    ];

    const presentation = projectBinaryIdentityStatus(leaky, 'managed');
    const copied = JSON.parse(presentation.supportPacket) as Record<string, unknown>;

    expect(presentation.supportPacket).not.toContain('/Users/');
    expect(presentation.supportPacket).not.toContain('C:\\\\Users');
    expect(presentation.supportPacket).not.toContain('/home/alice');
    expect(presentation.supportPacket).not.toContain('ghp_abcdef');
    expect(presentation.supportPacket).not.toContain('install_path');
    expect(copied.redacted).toBe(true);
    expect(copied.redacted_fields).toBe(3);
    expect(copied.ui_state).toBe('update_or_repair_required');
    expect(copied.reasons).toEqual(['version_mismatch']);
    expect(copied.limitations).toEqual(['<redacted>', '<redacted>', 'dap identity absent']);
    // Repository slugs are the schema's only sanctioned slash-bearing values.
    expect(presentation.supportPacket).toContain('EffortlessMetrics/perl-lsp-swarm');
  });

  test('a clean packet reports zero redactions', () => {
    const presentation = projectBinaryIdentityStatus(
      response('exact_match', ['exact_identity_match']),
      'managed',
    );
    const copied = JSON.parse(presentation.supportPacket) as Record<string, unknown>;
    expect(copied.redacted_fields).toBe(0);
    expect(copied.server_reported_copy_safe).toBe(true);
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

  test('a restart that reproduces the same mismatch does not raise a second notice', () => {
    const tracker = new BinaryIdentityNoticeTracker();
    const first = projectBinaryIdentityStatus(
      response('mismatch', ['version_mismatch'], 'server-1'),
      'managed',
    );
    const afterRestart = projectBinaryIdentityStatus(
      response('mismatch', ['version_mismatch'], 'server-2'),
      'managed',
    );
    const changedCondition = projectBinaryIdentityStatus(
      response('mismatch', ['target_mismatch'], 'server-2'),
      'managed',
    );
    expect(tracker.shouldNotify(first)).toBe(true);
    expect(tracker.shouldNotify(afterRestart)).toBe(false);
    expect(tracker.shouldNotify(changedCondition)).toBe(true);
  });
});

describe('binary identity status without a running server', () => {
  test('managed selection with no resolved binary is managed_binary_missing with repair', () => {
    const presentation = projectUnavailableBinaryIdentity('managed', false);
    expect(presentation.state).toBe('managed_binary_missing');
    expect(presentation.action).toBe('repair_managed_pair');
    expect(presentation.quiet).toBe(false);
    expect(presentation.reasons).toEqual([CLIENT_REASON_MANAGED_BINARY_ABSENT]);
  });

  test.each<[Parameters<typeof projectUnavailableBinaryIdentity>[0], boolean | undefined]>([
    ['managed', undefined],
    ['managed', true],
    ['user_supplied', false],
    ['unknown', true],
  ])('role %s with resolved=%s is not_proven, not a diagnosis', (role, resolved) => {
    const presentation = projectUnavailableBinaryIdentity(role, resolved);
    expect(presentation.state).toBe('not_proven');
    expect(presentation.action).toBe('none');
    expect(presentation.reasons).toEqual([CLIENT_REASON_SERVER_NOT_RUNNING]);
  });
});
