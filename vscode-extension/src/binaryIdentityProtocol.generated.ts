// Generated projection of the binary-identity v1 Rust/schema contract. Do not hand-edit.
// `binary_identity_protocol_contract.rs` verifies every current state/reason and literal.

export const BINARY_IDENTITY_METHOD = 'perl/binaryIdentity' as const;
export const BINARY_COMPATIBILITY_METHOD = 'perl/binaryCompatibility' as const;
export const BINARY_IDENTITY_FEATURE_VERSION = 1 as const;
export const CANONICAL_EXTENSION_PUBLISHER = 'EffortlessMetrics' as const;
export const CANONICAL_EXTENSION_PACKAGE = 'perl-lsp-rs' as const;
export const CANONICAL_EXTENSION_ID = 'EffortlessMetrics.perl-lsp-rs' as const;
export const CANONICAL_DAP_POSTURE = 'preview' as const;

export type BinaryRole = 'server' | 'dap';
export type BuildIdentityState = 'exact' | 'partial' | 'not_proven';
export type ArtifactRole = 'managed' | 'user_supplied' | 'package_install' | 'archive' | 'unknown';

export interface ProductIdentity {
  name: 'perl-lsp';
  public_repository: 'EffortlessMetrics/perl-lsp';
  development_repository: 'EffortlessMetrics/perl-lsp-swarm';
}

export interface ExecutableIdentity {
  executable: 'perllsp' | 'perl-dap' | string;
  cargo_package: 'perllsp' | 'perl-dap' | string;
  role: BinaryRole;
  version: string;
}

export interface BuildIdentity {
  source_revision?: string;
  source_tree_digest?: string;
  target?: string;
  profile?: string;
  identity_state: BuildIdentityState;
}

export interface ArtifactIdentity {
  role: ArtifactRole;
  digest?: string;
  candidate_identity?: string;
}

export interface BinaryIdentityPacketV1 {
  schema_version: string;
  product: ProductIdentity;
  binary: ExecutableIdentity;
  build: BuildIdentity;
  artifact: ArtifactIdentity;
  compatibility: {
    expected_product_identity_version: number;
    dap_posture: string;
  };
  limitations?: string[];
}

export type BinaryCompatibilityState =
  | 'exact_match'
  | 'compatible_partial'
  | 'mismatch'
  | 'unsupported'
  | 'stale'
  | 'not_proven';

export type KnownBinaryCompatibilityReason =
  | 'server_product_mismatch'
  | 'product_repository_mismatch'
  | 'packet_schema_unsupported'
  | 'product_identity_version_unsupported'
  | 'dap_posture_mismatch'
  | 'extension_publisher_mismatch'
  | 'extension_package_mismatch'
  | 'extension_identity_mismatch'
  | 'extension_authority_not_proven'
  | 'extension_package_digest_not_proven'
  | 'version_mismatch'
  | 'target_mismatch'
  | 'target_not_proven'
  | 'source_revision_mismatch'
  | 'source_revision_not_proven'
  | 'source_tree_digest_mismatch'
  | 'source_tree_digest_not_proven'
  | 'profile_mismatch'
  | 'profile_not_proven'
  | 'candidate_mismatch'
  | 'candidate_not_proven'
  | 'artifact_role_mismatch'
  | 'artifact_role_not_proven'
  | 'artifact_digest_mismatch'
  | 'artifact_digest_not_proven'
  | 'dap_role_mismatch'
  | 'dap_identity_absent'
  | 'build_identity_partial'
  | 'build_identity_not_proven'
  | 'payload_not_redacted'
  | 'server_instance_stale'
  | 'environment_snapshot_stale'
  | 'feature_version_unsupported'
  | 'exact_identity_match';

/**
 * Current values remain strongly typed while future bounded server reason codes
 * stay representable and are rendered by clients as an unknown/degraded state.
 */
export type BinaryCompatibilityReason = KnownBinaryCompatibilityReason | (string & {});

export interface BinaryIdentityCapabilityV1 {
  version: 1;
  supports_dap_identity: boolean;
  supports_compatibility: boolean;
}

export interface ExpectedExtensionIdentityV1 {
  publisher: string;
  package_name: string;
  id: string;
  version: string;
  candidate_identity?: string;
  target?: string;
  package_sha256?: string;
  server_sha256?: string;
  dap_sha256?: string;
  binary_artifact_role: ArtifactRole;
  authority_identity: string;
}

export interface BinaryIdentityRequestV1 {
  feature_version: number;
  expected_extension: ExpectedExtensionIdentityV1;
  expected_server_instance_id?: string;
  expected_environment_snapshot_id?: string;
}

export interface BinaryIdentityResponseV1 {
  feature_version: 1;
  server: BinaryIdentityPacketV1;
  dap?: BinaryIdentityPacketV1;
  expected_extension: ExpectedExtensionIdentityV1;
  server_instance_id: string;
  environment_snapshot_id: string;
  compatibility: BinaryCompatibilityState;
  reasons: BinaryCompatibilityReason[];
  /** true when NO field was removed or replaced: the payload is copy-safe.
   * false means at least one field was redacted and must not be trusted verbatim. */
  redacted: boolean;
  limitations?: string[];
}
