import type { BinaryIdentityResponseV1 } from '../binaryIdentityProtocol.generated';

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {},
  State: { Starting: 'starting', Running: 'running', Stopped: 'stopped' },
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));

import {
  SHOW_BINARY_IDENTITY_COMMAND,
  showBinaryIdentityStatus,
  type BinaryIdentityCommandHost,
  type BinaryIdentityRequestClient,
} from '../binaryIdentityCommand';
import {
  createBinaryIdentityCommand,
  createBinaryIdentityDialogShow,
  installedIdentityFromManifest,
} from '../extension';

function response(): BinaryIdentityResponseV1 {
  return {
    feature_version: 1,
    server: {
      schema_version: 'perl_lsp.binary_identity.v1',
      product: {
        name: 'perl-lsp',
        public_repository: 'EffortlessMetrics/perl-lsp',
        development_repository: 'EffortlessMetrics/perl-lsp-swarm',
      },
      binary: {
        executable: 'perllsp',
        cargo_package: 'perllsp',
        role: 'server',
        version: '0.17.0',
      },
      build: {
        source_revision: 'old',
        target: 'x86_64-unknown-linux-gnu',
        identity_state: 'exact',
      },
      artifact: { role: 'managed', candidate_identity: 'old' },
      compatibility: {
        expected_product_identity_version: 1,
        dap_posture: 'preview',
      },
    },
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
    server_instance_id: 'server-1',
    environment_snapshot_id: 'env-1',
    compatibility: 'mismatch',
    reasons: ['version_mismatch', 'candidate_mismatch'],
    redacted: true,
  };
}

describe('binary identity command', () => {
  test('exports the stable command identifier', () => {
    expect(SHOW_BINARY_IDENTITY_COMMAND).toBe('perl-lsp.showBinaryIdentity');
  });

  test('requests canonical method and routes managed repair', async () => {
    const request = jest.fn().mockResolvedValue(response());
    const repairManagedPair = jest.fn().mockResolvedValue(undefined);
    const host: BinaryIdentityCommandHost = {
      show: jest.fn().mockResolvedValue('repair_managed_pair'),
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair,
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket: jest.fn().mockResolvedValue(undefined),
    };
    const client: BinaryIdentityRequestClient = { sendRequest: request };

    const presentation = await showBinaryIdentityStatus(client, host, {
      extensionVersion: '0.18.0',
      extensionCandidate: 'rc1',
      expectedTarget: 'x86_64-unknown-linux-gnu',
      selectedRole: 'managed',
      expectedServerInstanceId: 'server-1',
      expectedEnvironmentSnapshotId: 'env-1',
    });

    expect(request).toHaveBeenCalledWith(
      'perl/binaryIdentity',
      expect.objectContaining({
        feature_version: 1,
        expected_extension: expect.objectContaining({
          id: 'EffortlessMetrics.perl-lsp-rs',
          version: '0.18.0',
          // The packaged authority token is version-independent (#10307).
          authority_identity: 'vsix:EffortlessMetrics.perl-lsp-rs',
          candidate_identity: 'rc1',
          target: 'x86_64-unknown-linux-gnu',
        }),
      }),
    );
    expect(presentation.state).toBe('update_or_repair_required');
    expect(repairManagedPair).toHaveBeenCalledTimes(1);
  });

  test('copy action receives only the redacted support packet', async () => {
    const copySupportPacket = jest.fn().mockResolvedValue(undefined);
    const host: BinaryIdentityCommandHost = {
      show: jest.fn().mockResolvedValue('copy_support_packet'),
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair: jest.fn().mockResolvedValue(undefined),
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket,
    };
    const client: BinaryIdentityRequestClient = {
      sendRequest: jest.fn().mockResolvedValue(response()),
    };

    await showBinaryIdentityStatus(client, host, {
      extensionVersion: '0.18.0',
      selectedRole: 'managed',
    });

    expect(copySupportPacket).toHaveBeenCalledTimes(1);
    expect(copySupportPacket.mock.calls[0][0]).toContain('"redacted": true');
  });

  test('production composition delegates the registered command to the identity adapter', async () => {
    const request = jest.fn().mockResolvedValue(response());
    const show = jest.fn().mockResolvedValue(undefined);
    const client: BinaryIdentityRequestClient = { sendRequest: request };
    const host: BinaryIdentityCommandHost = {
      show,
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair: jest.fn().mockResolvedValue(undefined),
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket: jest.fn().mockResolvedValue(undefined),
    };

    const command = createBinaryIdentityCommand(
      () => client,
      () => ({ extensionVersion: '0.18.0', selectedRole: 'managed' }),
      host,
    );
    const result = await command();

    expect(request).toHaveBeenCalledWith(
      'perl/binaryIdentity',
      expect.objectContaining({
        expected_extension: expect.objectContaining({
          authority_identity: 'vsix:EffortlessMetrics.perl-lsp-rs',
          version: '0.18.0',
        }),
      }),
    );
    expect(show).toHaveBeenCalledTimes(1);
    expect(result).toEqual(expect.objectContaining({ state: 'update_or_repair_required' }));
  });

  test('installed candidate and target resolve per invocation and omit when unavailable', async () => {
    const request = jest.fn().mockResolvedValue(response());
    const client: BinaryIdentityRequestClient = { sendRequest: request };
    const host: BinaryIdentityCommandHost = {
      show: jest.fn().mockResolvedValue(undefined),
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair: jest.fn().mockResolvedValue(undefined),
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket: jest.fn().mockResolvedValue(undefined),
    };

    let installed: { candidate: string; target: string } | undefined = {
      candidate: 'rc1',
      target: 'x86_64-unknown-linux-gnu',
    };
    const command = createBinaryIdentityCommand(
      () => client,
      () => ({
        extensionVersion: '0.18.0',
        selectedRole: 'managed' as const,
        ...(installed === undefined
          ? {}
          : { extensionCandidate: installed.candidate, expectedTarget: installed.target }),
      }),
      host,
    );

    await command();
    expect(request).toHaveBeenCalledWith(
      'perl/binaryIdentity',
      expect.objectContaining({
        expected_extension: expect.objectContaining({
          candidate_identity: 'rc1',
          target: 'x86_64-unknown-linux-gnu',
        }),
      }),
    );

    // After a reinstall or on a user-supplied install the manifest is gone;
    // the request omits the expectations instead of carrying stale ones.
    installed = undefined;
    await command();
    const second = request.mock.calls[1][1] as {
      expected_extension: Record<string, unknown>;
    };
    expect(second.expected_extension).not.toHaveProperty('candidate_identity');
    expect(second.expected_extension).not.toHaveProperty('target');
  });

  test('installed identity carries the release-tag token, not the hash-derived cache id', () => {
    // The server compares `candidate_identity` against its build-embedded
    // release tag (`PERL_LSP_CANDIDATE_ID`), so the manifest's `subject.release`
    // — the same tag — is the only comparable value. The manifest's hash-derived
    // `candidate_id` is a local managed-cache key that can never match it.
    const manifest = {
      schema_version: 'managed_candidate_manifest.v1' as const,
      candidate_id: `candidate-${'a'.repeat(64)}`,
      subject: {
        release: 'v0.18.0',
        version: 'v0.18.0',
        target: 'x86_64-unknown-linux-gnu',
        topology_digest: 'sha256:0'.padEnd(71, '0'),
        perllsp_digest: 'sha256:0'.padEnd(71, '0'),
        perl_dap_digest: null,
      },
      verification: {
        perllsp: 'verified' as const,
        perl_dap: 'not_present' as const,
        topology: 'verified' as const,
        provenance: 'verified' as const,
      },
    };
    expect(installedIdentityFromManifest(manifest)).toEqual({
      candidate: 'v0.18.0',
      target: 'x86_64-unknown-linux-gnu',
    });
  });

  test('installed identity omits the expectations for a missing or mistyped subject', () => {
    expect(installedIdentityFromManifest(null)).toBeUndefined();
    expect(installedIdentityFromManifest({})).toBeUndefined();
    expect(
      installedIdentityFromManifest({
        schema_version: 'managed_candidate_manifest.v1',
        candidate_id: `candidate-${'a'.repeat(64)}`,
        subject: { release: 'v0.18.0' },
      }),
    ).toBeUndefined();
    expect(
      installedIdentityFromManifest({
        schema_version: 'managed_candidate_manifest.v1',
        candidate_id: `candidate-${'a'.repeat(64)}`,
        subject: { release: 'v0.18.0', target: 42 },
      }),
    ).toBeUndefined();
    expect(
      installedIdentityFromManifest({
        schema_version: 'managed_candidate_manifest.v1',
        candidate_id: `candidate-${'a'.repeat(64)}`,
        subject: { release: 7, target: 'x86_64-unknown-linux-gnu' },
      }),
    ).toBeUndefined();
  });

  test('production dialog maps the selected action to the governed callback', async () => {
    const request = jest.fn().mockResolvedValue(response());
    const repairManagedPair = jest.fn().mockResolvedValue(undefined);
    const refreshIdentity = jest.fn().mockResolvedValue(undefined);
    const showMessage = jest.fn().mockResolvedValue('Repair managed binary');
    const host: BinaryIdentityCommandHost = {
      show: createBinaryIdentityDialogShow(showMessage),
      refreshIdentity,
      repairManagedPair,
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket: jest.fn().mockResolvedValue(undefined),
    };

    await createBinaryIdentityCommand(
      () => ({ sendRequest: request }),
      () => ({ extensionVersion: '0.18.0', selectedRole: 'managed' }),
      host,
    )();

    expect(showMessage).toHaveBeenCalledWith(
      expect.stringContaining('repair required'),
      'Repair managed binary',
      'Copy support packet',
    );
    expect(repairManagedPair).toHaveBeenCalledTimes(1);
    expect(refreshIdentity).not.toHaveBeenCalled();
  });

  test('dismissing the production dialog dispatches no action', async () => {
    const request = jest.fn().mockResolvedValue(response());
    const repairManagedPair = jest.fn().mockResolvedValue(undefined);
    const copySupportPacket = jest.fn().mockResolvedValue(undefined);
    const showMessage = jest.fn().mockResolvedValue(undefined);
    const host: BinaryIdentityCommandHost = {
      show: createBinaryIdentityDialogShow(showMessage),
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair,
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket,
    };

    await createBinaryIdentityCommand(
      () => ({ sendRequest: request }),
      () => ({ extensionVersion: '0.18.0', selectedRole: 'managed' }),
      host,
    )();

    expect(showMessage).toHaveBeenCalledTimes(1);
    expect(repairManagedPair).not.toHaveBeenCalled();
    expect(copySupportPacket).not.toHaveBeenCalled();
  });

  test('quiet exact-match presentation offers no buttons', async () => {
    const request = jest.fn().mockResolvedValue({
      ...response(),
      compatibility: 'exact_match',
      reasons: ['exact_identity_match'],
    });
    const showMessage = jest.fn().mockResolvedValue(undefined);
    const host: BinaryIdentityCommandHost = {
      show: createBinaryIdentityDialogShow(showMessage),
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair: jest.fn().mockResolvedValue(undefined),
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket: jest.fn().mockResolvedValue(undefined),
    };

    await createBinaryIdentityCommand(
      () => ({ sendRequest: request }),
      () => ({ extensionVersion: '0.18.0', selectedRole: 'managed' }),
      host,
    )();

    expect(showMessage).toHaveBeenCalledWith(expect.stringContaining('identity verified'));
  });

  test('reports an identity request failure instead of returning unsupported', async () => {
    const reportError = jest.fn();
    const host: BinaryIdentityCommandHost = {
      show: jest.fn().mockResolvedValue(undefined),
      refreshIdentity: jest.fn().mockResolvedValue(undefined),
      repairManagedPair: jest.fn().mockResolvedValue(undefined),
      inspectConfiguredBinary: jest.fn().mockResolvedValue(undefined),
      copySupportPacket: jest.fn().mockResolvedValue(undefined),
    };
    const client: BinaryIdentityRequestClient = {
      sendRequest: jest.fn().mockRejectedValue(new Error('method not found')),
    };

    const result = await createBinaryIdentityCommand(
      () => client,
      () => ({ extensionVersion: '0.18.0', selectedRole: 'managed' }),
      host,
      reportError,
    )();

    expect(result).toEqual({ status: 'error', message: 'method not found' });
    expect(reportError).toHaveBeenCalledWith('method not found');
  });
});
