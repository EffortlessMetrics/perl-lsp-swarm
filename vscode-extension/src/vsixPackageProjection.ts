export type VsixPackageMode = 'target_specific' | 'universal_managed' | 'unsupported';
export type VsixDapDisposition = 'required_present' | 'preview_unavailable' | 'not_required';

export interface ReleaseTopologyTargetRow {
  readonly target: string;
  readonly os: 'linux' | 'macos' | 'windows';
  readonly architecture: 'x86_64' | 'aarch64';
  readonly libc: 'gnu' | 'musl' | null;
  readonly archiveName: string;
  readonly requiredMembers: readonly string[];
}

export interface VsixProjectionInput {
  readonly releaseTopologySha256: string;
  readonly targets: readonly ReleaseTopologyTargetRow[];
  readonly includeUniversalManaged: boolean;
  readonly unsupportedTargets?: readonly string[] | undefined;
}

export interface VsixTargetProjectionRow {
  readonly vscodeTargetId: string;
  readonly rustTarget: string | null;
  readonly platform: 'linux' | 'macos' | 'windows' | 'universal' | 'unsupported';
  readonly architecture: 'x64' | 'arm64' | 'universal' | 'unsupported';
  readonly libc: 'gnu' | 'musl' | null;
  readonly packageMode: VsixPackageMode;
  readonly archiveName: string | null;
  readonly serverMember: string | null;
  readonly dapMember: string | null;
  readonly dapDisposition: VsixDapDisposition;
}

export interface VsixExtensionIdentity {
  readonly id: string;
  readonly version: string;
  readonly sourceSha: string;
}

export interface VsixCandidateIdentity {
  readonly id: string;
  readonly release: string;
  readonly sourceSha: string;
}

export interface VsixNativePayloadIdentity {
  readonly candidateId: string;
  readonly target: string;
  readonly member: string;
  readonly sha256: string;
  readonly identityRef: string;
}

export interface VsixCandidatePayloadManifestInput {
  readonly extension: VsixExtensionIdentity;
  readonly candidate: VsixCandidateIdentity;
  readonly releaseTopologySha256: string;
  readonly projection: VsixTargetProjectionRow;
  readonly packageInventorySha256: string;
  readonly server?: VsixNativePayloadIdentity | undefined;
  readonly dap?: VsixNativePayloadIdentity | undefined;
}

export interface VsixCandidatePayloadManifest {
  readonly schema: 'vsix_candidate_payload.v1';
  readonly extension: VsixExtensionIdentity;
  readonly candidate: VsixCandidateIdentity;
  readonly releaseTopologySha256: string;
  readonly package: {
    readonly vscodeTargetId: string;
    readonly rustTarget: string | null;
    readonly mode: VsixPackageMode;
    readonly inventorySha256: string;
  };
  readonly server: VsixNativePayloadIdentity | null;
  readonly dap: {
    readonly disposition: VsixDapDisposition;
    readonly payload: VsixNativePayloadIdentity | null;
  };
}

const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const SOURCE_SHA_PATTERN = /^[0-9a-f]{40}$/;
const MEMBER_PATTERN = /^[A-Za-z0-9._/-]+$/;
const PACKAGE_ID_PATTERN = /^[A-Za-z0-9._-]+$/;

function requireNonEmpty(value: string, field: string): void {
  if (value.trim().length === 0) {
    throw new Error(`${field} must be non-empty.`);
  }
}

function requireSha256(value: string, field: string): void {
  if (!SHA256_PATTERN.test(value)) {
    throw new Error(`${field} must be a lowercase SHA-256 digest.`);
  }
}

function requireSourceSha(value: string, field: string): void {
  if (!SOURCE_SHA_PATTERN.test(value)) {
    throw new Error(`${field} must be a full lowercase source commit SHA.`);
  }
}

function validateMember(member: string, field: string): void {
  if (
    !MEMBER_PATTERN.test(member) ||
    member.startsWith('/') ||
    member.includes('..') ||
    member.includes('\\')
  ) {
    throw new Error(`${field} is not a safe canonical archive member.`);
  }
}

function expectedTargetIdentity(row: ReleaseTopologyTargetRow): {
  readonly platform: 'linux' | 'macos' | 'windows';
  readonly architecture: 'x64' | 'arm64';
  readonly vscodeTargetId: string;
} {
  const architecture = row.architecture === 'aarch64' ? 'arm64' : 'x64';
  let vscodePlatform: 'linux' | 'alpine' | 'darwin' | 'win32';
  switch (row.os) {
    case 'linux':
      if (row.libc === 'gnu') {
        vscodePlatform = 'linux';
      } else if (row.libc === 'musl') {
        vscodePlatform = 'alpine';
      } else {
        throw new Error(`Linux target ${row.target} has no explicit gnu/musl libc.`);
      }
      break;
    case 'macos':
      if (row.libc !== null) {
        throw new Error(`macOS target ${row.target} must not declare a libc row.`);
      }
      vscodePlatform = 'darwin';
      break;
    case 'windows':
      if (row.libc !== null) {
        throw new Error(`Windows target ${row.target} must not declare a libc row.`);
      }
      vscodePlatform = 'win32';
      break;
  }

  const expectedArchitecture = row.architecture === 'aarch64' ? 'aarch64' : 'x86_64';
  const expectedTarget =
    row.os === 'linux'
      ? `${expectedArchitecture}-unknown-linux-${row.libc}`
      : row.os === 'macos'
        ? `${expectedArchitecture}-apple-darwin`
        : `${expectedArchitecture}-pc-windows-msvc`;
  if (row.target !== expectedTarget) {
    throw new Error(
      `Topology target ${row.target} contradicts its ${row.os}/${row.architecture}/${String(row.libc)} identity; expected ${expectedTarget}.`,
    );
  }

  return {
    platform: row.os,
    architecture,
    vscodeTargetId: `${vscodePlatform}-${architecture}`,
  };
}

function requiredBinaryMember(
  row: ReleaseTopologyTargetRow,
  basename: 'perllsp' | 'perl-dap',
): string {
  const expected = row.os === 'windows' ? `${basename}.exe` : basename;
  const matches = row.requiredMembers.filter((member) => member === expected);
  if (matches.length !== 1) {
    throw new Error(
      `Topology target ${row.target} must declare exactly one required ${expected} member.`,
    );
  }
  validateMember(expected, `${row.target}.${basename}`);
  return expected;
}

function validateProjectionInput(input: VsixProjectionInput): void {
  requireSha256(input.releaseTopologySha256, 'releaseTopologySha256');
  const targets = new Set<string>();
  const archives = new Set<string>();
  for (const row of input.targets) {
    requireNonEmpty(row.target, 'target');
    requireNonEmpty(row.archiveName, `${row.target}.archiveName`);
    validateMember(row.archiveName, `${row.target}.archiveName`);
    if (targets.has(row.target)) {
      throw new Error(`Duplicate release topology target: ${row.target}.`);
    }
    if (archives.has(row.archiveName)) {
      throw new Error(`Duplicate release topology archive: ${row.archiveName}.`);
    }
    targets.add(row.target);
    archives.add(row.archiveName);
  }
}

export function deriveVsixTargetProjection(
  input: VsixProjectionInput,
): readonly VsixTargetProjectionRow[] {
  validateProjectionInput(input);
  const rows: VsixTargetProjectionRow[] = input.targets.map((row) => {
    const identity = expectedTargetIdentity(row);
    return {
      vscodeTargetId: identity.vscodeTargetId,
      rustTarget: row.target,
      platform: identity.platform,
      architecture: identity.architecture,
      libc: row.libc,
      packageMode: 'target_specific',
      archiveName: row.archiveName,
      serverMember: requiredBinaryMember(row, 'perllsp'),
      dapMember: requiredBinaryMember(row, 'perl-dap'),
      dapDisposition: 'required_present',
    };
  });

  for (const target of input.unsupportedTargets ?? []) {
    requireNonEmpty(target, 'unsupported target');
    rows.push({
      vscodeTargetId: target,
      rustTarget: null,
      platform: 'unsupported',
      architecture: 'unsupported',
      libc: null,
      packageMode: 'unsupported',
      archiveName: null,
      serverMember: null,
      dapMember: null,
      dapDisposition: 'preview_unavailable',
    });
  }

  if (input.includeUniversalManaged) {
    rows.push({
      vscodeTargetId: 'universal',
      rustTarget: null,
      platform: 'universal',
      architecture: 'universal',
      libc: null,
      packageMode: 'universal_managed',
      archiveName: null,
      serverMember: null,
      dapMember: null,
      dapDisposition: 'not_required',
    });
  }

  const ids = new Set<string>();
  for (const row of rows) {
    if (ids.has(row.vscodeTargetId)) {
      throw new Error(`Duplicate VS Code target projection: ${row.vscodeTargetId}.`);
    }
    ids.add(row.vscodeTargetId);
  }

  return rows.sort((left, right) => left.vscodeTargetId.localeCompare(right.vscodeTargetId));
}

function validatePayload(
  payload: VsixNativePayloadIdentity,
  field: 'server' | 'dap',
  input: VsixCandidatePayloadManifestInput,
): void {
  if (payload.candidateId !== input.candidate.id) {
    throw new Error(`${field} payload belongs to another candidate.`);
  }
  if (payload.target !== input.projection.rustTarget) {
    throw new Error(`${field} payload belongs to another target.`);
  }
  requireNonEmpty(payload.identityRef, `${field}.identityRef`);
  requireSha256(payload.sha256, `${field}.sha256`);
  validateMember(payload.member, `${field}.member`);
}

function clonePayload(payload: VsixNativePayloadIdentity): VsixNativePayloadIdentity {
  return {
    candidateId: payload.candidateId,
    target: payload.target,
    member: payload.member,
    sha256: payload.sha256,
    identityRef: payload.identityRef,
  };
}

export function buildVsixCandidatePayloadManifest(
  input: VsixCandidatePayloadManifestInput,
): VsixCandidatePayloadManifest {
  requireNonEmpty(input.extension.id, 'extension.id');
  if (!PACKAGE_ID_PATTERN.test(input.extension.id)) {
    throw new Error('extension.id contains unsupported characters.');
  }
  requireNonEmpty(input.extension.version, 'extension.version');
  requireSourceSha(input.extension.sourceSha, 'extension.sourceSha');
  requireNonEmpty(input.candidate.id, 'candidate.id');
  requireNonEmpty(input.candidate.release, 'candidate.release');
  requireSourceSha(input.candidate.sourceSha, 'candidate.sourceSha');
  requireSha256(input.releaseTopologySha256, 'releaseTopologySha256');
  requireSha256(input.packageInventorySha256, 'packageInventorySha256');

  if (input.projection.packageMode === 'unsupported') {
    throw new Error(
      `Cannot build a VSIX payload manifest for unsupported target ${input.projection.vscodeTargetId}.`,
    );
  }

  if (input.projection.packageMode === 'universal_managed') {
    if (input.server || input.dap || input.projection.serverMember || input.projection.dapMember) {
      throw new Error('Universal managed VSIX packages must be binaryless.');
    }
  } else {
    if (!input.projection.rustTarget || !input.projection.serverMember || !input.server) {
      throw new Error('Target-specific VSIX package is missing its required server payload.');
    }
    validatePayload(input.server, 'server', input);
    if (input.server.member !== input.projection.serverMember) {
      throw new Error('Server payload member disagrees with the topology projection.');
    }

    if (input.projection.dapDisposition === 'required_present') {
      if (!input.projection.dapMember || !input.dap) {
        throw new Error('Target-specific VSIX package is missing its required DAP payload.');
      }
      validatePayload(input.dap, 'dap', input);
      if (input.dap.member !== input.projection.dapMember) {
        throw new Error('DAP payload member disagrees with the topology projection.');
      }
    } else if (input.dap) {
      validatePayload(input.dap, 'dap', input);
    }
  }

  return {
    schema: 'vsix_candidate_payload.v1',
    extension: {
      id: input.extension.id,
      version: input.extension.version,
      sourceSha: input.extension.sourceSha,
    },
    candidate: {
      id: input.candidate.id,
      release: input.candidate.release,
      sourceSha: input.candidate.sourceSha,
    },
    releaseTopologySha256: input.releaseTopologySha256,
    package: {
      vscodeTargetId: input.projection.vscodeTargetId,
      rustTarget: input.projection.rustTarget,
      mode: input.projection.packageMode,
      inventorySha256: input.packageInventorySha256,
    },
    server: input.server ? clonePayload(input.server) : null,
    dap: {
      disposition: input.projection.dapDisposition,
      payload: input.dap ? clonePayload(input.dap) : null,
    },
  };
}

function canonicalize(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, item]) => [key, canonicalize(item)]),
    );
  }
  return value;
}

export function canonicalVsixPayloadJson(manifest: VsixCandidatePayloadManifest): string {
  return `${JSON.stringify(canonicalize(manifest), null, 2)}\n`;
}
