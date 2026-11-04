import {
  BINARY_IDENTITY_FEATURE_VERSION,
  BINARY_IDENTITY_METHOD,
  CANONICAL_EXTENSION_ID,
  CANONICAL_EXTENSION_PACKAGE,
  CANONICAL_EXTENSION_PUBLISHER,
  type BinaryIdentityRequestV1,
  type BinaryIdentityResponseV1,
} from './binaryIdentityProtocol.generated';
import {
  type BinaryIdentityAction,
  type BinaryIdentityPresentation,
  type SelectedBinaryRole,
  projectBinaryIdentityStatus,
} from './binaryIdentityStatus';

export const SHOW_BINARY_IDENTITY_COMMAND = 'perl-lsp.showBinaryIdentity' as const;

export interface BinaryIdentityRequestClient {
  sendRequest<TResponse>(method: string, params: unknown): Promise<TResponse>;
}

export interface BinaryIdentityCommandHost {
  show(presentation: BinaryIdentityPresentation): Promise<BinaryIdentityAction | undefined>;
  refreshIdentity(): Promise<void>;
  repairManagedPair(): Promise<void>;
  inspectConfiguredBinary(): Promise<void>;
  copySupportPacket(packet: string): Promise<void>;
}

export interface BinaryIdentityCommandInput {
  extensionVersion: string;
  extensionCandidate?: string;
  expectedTarget?: string;
  selectedRole: SelectedBinaryRole;
  expectedServerInstanceId?: string;
  expectedEnvironmentSnapshotId?: string;
}

function request(input: BinaryIdentityCommandInput): BinaryIdentityRequestV1 {
  return {
    feature_version: BINARY_IDENTITY_FEATURE_VERSION,
    expected_extension: {
      publisher: CANONICAL_EXTENSION_PUBLISHER,
      package_name: CANONICAL_EXTENSION_PACKAGE,
      id: CANONICAL_EXTENSION_ID,
      version: input.extensionVersion,
      // The selected channel is the artifact role the server must agree with.
      binary_artifact_role: input.selectedRole,
      // The installed VSIX is the authority that produced this expectation.
      authority_identity: `vsix:${input.extensionVersion}`,
      ...(input.extensionCandidate === undefined
        ? {}
        : { candidate_identity: input.extensionCandidate }),
      ...(input.expectedTarget === undefined ? {} : { target: input.expectedTarget }),
    },
    ...(input.expectedServerInstanceId === undefined
      ? {}
      : { expected_server_instance_id: input.expectedServerInstanceId }),
    ...(input.expectedEnvironmentSnapshotId === undefined
      ? {}
      : { expected_environment_snapshot_id: input.expectedEnvironmentSnapshotId }),
  };
}

async function applyAction(
  action: BinaryIdentityAction | undefined,
  presentation: BinaryIdentityPresentation,
  host: BinaryIdentityCommandHost,
): Promise<void> {
  switch (action) {
    case undefined:
    case 'none':
      return;
    case 'refresh_identity':
      return host.refreshIdentity();
    case 'repair_managed_pair':
      return host.repairManagedPair();
    case 'inspect_configured_binary':
      return host.inspectConfiguredBinary();
    case 'copy_support_packet':
      return host.copySupportPacket(presentation.supportPacket);
  }
}

export async function showBinaryIdentityStatus(
  client: BinaryIdentityRequestClient,
  host: BinaryIdentityCommandHost,
  input: BinaryIdentityCommandInput,
): Promise<BinaryIdentityPresentation> {
  const response = await client.sendRequest<BinaryIdentityResponseV1>(
    BINARY_IDENTITY_METHOD,
    request(input),
  );
  const presentation = projectBinaryIdentityStatus(response, input.selectedRole);
  const action = await host.show(presentation);
  await applyAction(action, presentation, host);
  return presentation;
}
