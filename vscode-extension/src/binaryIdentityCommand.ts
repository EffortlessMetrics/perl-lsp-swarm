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
  projectUnavailableBinaryIdentity,
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
  /**
   * Whether the host resolved any server binary. `false` with a managed
   * selection is the only no-server state attributed as a missing managed
   * artifact; `undefined` means the host does not know.
   */
  serverBinaryResolved?: boolean;
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
      // A binary the host cannot attribute is sent as `unknown`, never as
      // `managed`, so the server cannot be asked to confirm a guessed role.
      binary_artifact_role: input.selectedRole,
      // The installed VSIX is the authority that produced this expectation.
      // The token carries the packaged extension identity rather than the
      // installed version string, so it stays stable across upgrades; the
      // version itself is compared in its dedicated `version` field above.
      authority_identity: `vsix:${CANONICAL_EXTENSION_ID}`,
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
  // The response is untrusted wire data; the projection validates it and maps
  // anything it cannot read to `not_proven`.
  const response = await client.sendRequest<BinaryIdentityResponseV1 | null>(
    BINARY_IDENTITY_METHOD,
    request(input),
  );
  const presentation = projectBinaryIdentityStatus(response, input.selectedRole);
  const action = await host.show(presentation);
  await applyAction(action, presentation, host);
  return presentation;
}

/**
 * Presents identity status when no language client is running. The user
 * asked for status, so a bounded explanation is always shown rather than
 * returning silently.
 */
export async function showUnavailableBinaryIdentityStatus(
  host: BinaryIdentityCommandHost,
  input: Pick<BinaryIdentityCommandInput, 'selectedRole' | 'serverBinaryResolved'>,
): Promise<BinaryIdentityPresentation> {
  const presentation = projectUnavailableBinaryIdentity(
    input.selectedRole,
    input.serverBinaryResolved,
  );
  const action = await host.show(presentation);
  await applyAction(action, presentation, host);
  return presentation;
}
