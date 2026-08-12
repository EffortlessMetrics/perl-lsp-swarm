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
import { createBinaryIdentityCommand } from '../extension';

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
      id: 'EffortlessMetrics.perl-lsp-rs',
      version: '0.18.0',
      candidate_identity: 'rc1',
      target: 'x86_64-unknown-linux-gnu',
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

    const command = createBinaryIdentityCommand(() => client, '0.18.0', 'managed', host);
    const result = await command();

    expect(request).toHaveBeenCalledWith('perl/binaryIdentity', expect.anything());
    expect(show).toHaveBeenCalledTimes(1);
    expect(result).toEqual(expect.objectContaining({ state: 'update_or_repair_required' }));
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
      '0.18.0',
      'managed',
      host,
      reportError,
    )();

    expect(result).toEqual({ status: 'error', message: 'method not found' });
    expect(reportError).toHaveBeenCalledWith('method not found');
  });
});
