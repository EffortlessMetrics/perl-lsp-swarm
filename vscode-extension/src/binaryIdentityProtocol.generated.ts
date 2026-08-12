// Generated from the binary-identity v1 protocol contract. Do not hand-edit.

export const BINARY_IDENTITY_METHOD = "perl/binaryIdentity" as const;
export const BINARY_COMPATIBILITY_METHOD = "perl/binaryCompatibility" as const;
export const BINARY_IDENTITY_FEATURE_VERSION = 1 as const;
export const CANONICAL_EXTENSION_ID = "EffortlessMetrics.perl-lsp-rs" as const;

export type BinaryRole = "server" | "dap";
export type BuildIdentityState = "exact" | "partial" | "not_proven";
export type ArtifactRole = "managed" | "user_supplied" | "package_install" | "archive" | "unknown";

export interface ProductIdentity {
  name: string;
  public_repository: string;
  development_repository: string;
}

export interface ExecutableIdentity {
  executable: string;
  cargo_package: string;
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
  schema_version: "perl_lsp.binary_identity.v1";
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
  | "exact_match"
  | "compatible_partial"
  | "mismatch"
  | "unsupported"
  | "stale"
  | "not_proven";

export type BinaryCompatibilityReason =
  | "server_product_mismatch"
  | "extension_identity_mismatch"
  | "version_mismatch"
  | "target_mismatch"
  | "source_revision_mismatch"
  | "candidate_mismatch"
  | "dap_role_mismatch"
  | "dap_identity_absent"
  | "build_identity_partial"
  | "server_instance_stale"
  | "environment_snapshot_stale"
  | "feature_version_unsupported"
  | "exact_identity_match";

export interface BinaryIdentityCapabilityV1 {
  version: 1;
  supports_dap_identity: boolean;
  supports_compatibility: boolean;
}

export interface ExpectedExtensionIdentityV1 {
  id: string;
  version: string;
  candidate_identity?: string;
  target?: string;
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
  redacted: true;
  limitations?: string[];
}
