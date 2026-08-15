import { expect, test } from '@jest/globals';
import {
  buildVsixCandidatePayloadManifest,
  canonicalVsixPayloadJson,
  deriveVsixTargetProjection,
  type ReleaseTopologyTargetRow,
  type VsixCandidatePayloadManifestInput,
} from '../vsixPackageProjection';

const digest = 'a'.repeat(64);
const sourceSha = 'b'.repeat(40);

function topologyRow(
  target: string,
  os: ReleaseTopologyTargetRow['os'],
  architecture: ReleaseTopologyTargetRow['architecture'],
  libc: ReleaseTopologyTargetRow['libc'],
): ReleaseTopologyTargetRow {
  const suffix = os === 'windows' ? '.zip' : '.tar.gz';
  const executableSuffix = os === 'windows' ? '.exe' : '';
  return {
    target,
    os,
    architecture,
    libc,
    archiveName: `perllsp-0.18.0-${target}${suffix}`,
    requiredMembers: [
      `perllsp${executableSuffix}`,
      `perl-dap${executableSuffix}`,
      'README.md',
      'LICENSE-APACHE',
      'LICENSE-MIT',
      'SHA256SUMS.txt',
    ],
  };
}

const topology = [
  topologyRow('x86_64-unknown-linux-gnu', 'linux', 'x86_64', 'gnu'),
  topologyRow('aarch64-unknown-linux-gnu', 'linux', 'aarch64', 'gnu'),
  topologyRow('x86_64-unknown-linux-musl', 'linux', 'x86_64', 'musl'),
  topologyRow('aarch64-unknown-linux-musl', 'linux', 'aarch64', 'musl'),
  topologyRow('x86_64-apple-darwin', 'macos', 'x86_64', null),
  topologyRow('aarch64-apple-darwin', 'macos', 'aarch64', null),
  topologyRow('x86_64-pc-windows-msvc', 'windows', 'x86_64', null),
  topologyRow('aarch64-pc-windows-msvc', 'windows', 'aarch64', null),
] as const;

function rows(includeUniversalManaged = true) {
  return deriveVsixTargetProjection({
    releaseTopologySha256: digest,
    targets: topology,
    includeUniversalManaged,
  });
}

function manifestInput(
  overrides: Partial<VsixCandidatePayloadManifestInput> = {},
): VsixCandidatePayloadManifestInput {
  const projection = rows(false).find((row) => row.rustTarget === 'x86_64-unknown-linux-gnu');
  if (!projection) {
    throw new Error('fixture projection is missing');
  }
  return {
    extension: { id: 'EffortlessMetrics.perl-lsp-rs', version: '0.18.0', sourceSha },
    candidate: { id: 'candidate:v0.18.0', release: 'v0.18.0', sourceSha },
    releaseTopologySha256: digest,
    packageInventorySha256: 'c'.repeat(64),
    projection,
    server: {
      candidateId: 'candidate:v0.18.0',
      target: 'x86_64-unknown-linux-gnu',
      member: 'perllsp',
      sha256: 'd'.repeat(64),
      identityRef: 'identity:server',
    },
    dap: {
      candidateId: 'candidate:v0.18.0',
      target: 'x86_64-unknown-linux-gnu',
      member: 'perl-dap',
      sha256: 'e'.repeat(64),
      identityRef: 'identity:dap',
    },
    ...overrides,
  };
}

test('derives exact VS Code target IDs without collapsing GNU and musl', () => {
  expect(rows().map((row) => row.vscodeTargetId)).toEqual([
    'alpine-arm64',
    'alpine-x64',
    'darwin-arm64',
    'darwin-x64',
    'linux-arm64',
    'linux-x64',
    'universal',
    'win32-arm64',
    'win32-x64',
  ]);
});

test('universal managed package is explicitly binaryless', () => {
  const universal = rows().find((row) => row.packageMode === 'universal_managed');
  expect(universal).toMatchObject({
    vscodeTargetId: 'universal',
    rustTarget: null,
    serverMember: null,
    dapMember: null,
    dapDisposition: 'not_required',
  });
});

test('target-specific rows require one server and DAP member', () => {
  for (const row of rows(false)) {
    expect(row.serverMember).toMatch(/^perllsp(?:\.exe)?$/);
    expect(row.dapMember).toMatch(/^perl-dap(?:\.exe)?$/);
    expect(row.dapDisposition).toBe('required_present');
  }
});

test('Windows ARM64 remains its own target rather than inheriting x64', () => {
  const arm = rows(false).find((row) => row.vscodeTargetId === 'win32-arm64');
  expect(arm).toMatchObject({
    rustTarget: 'aarch64-pc-windows-msvc',
    architecture: 'arm64',
  });
});

test('target identity contradictions fail instead of being normalized', () => {
  const wrong = topologyRow('x86_64-unknown-linux-gnu', 'linux', 'aarch64', 'gnu');
  expect(() =>
    deriveVsixTargetProjection({
      releaseTopologySha256: digest,
      targets: [wrong],
      includeUniversalManaged: false,
    }),
  ).toThrow(/contradicts/);
});

test('duplicate target, archive, and VS Code target identities fail', () => {
  expect(() =>
    deriveVsixTargetProjection({
      releaseTopologySha256: digest,
      targets: [topology[0], topology[0]],
      includeUniversalManaged: false,
    }),
  ).toThrow(/Duplicate release topology target/);

  const sameArchive = {
    ...topology[1],
    archiveName: topology[0].archiveName,
  };
  expect(() =>
    deriveVsixTargetProjection({
      releaseTopologySha256: digest,
      targets: [topology[0], sameArchive],
      includeUniversalManaged: false,
    }),
  ).toThrow(/Duplicate release topology archive/);
});

test('builds a same-candidate target-specific payload manifest', () => {
  expect(buildVsixCandidatePayloadManifest(manifestInput())).toMatchObject({
    schema: 'vsix_candidate_payload.v1',
    package: {
      vscodeTargetId: 'linux-x64',
      rustTarget: 'x86_64-unknown-linux-gnu',
      mode: 'target_specific',
    },
    server: { candidateId: 'candidate:v0.18.0', member: 'perllsp' },
    dap: {
      disposition: 'required_present',
      payload: { candidateId: 'candidate:v0.18.0', member: 'perl-dap' },
    },
  });
});

test('mixed candidate and mixed target payloads fail', () => {
  const base = manifestInput();
  expect(() =>
    buildVsixCandidatePayloadManifest({
      ...base,
      dap: base.dap ? { ...base.dap, candidateId: 'candidate:other' } : undefined,
    }),
  ).toThrow(/another candidate/);
  expect(() =>
    buildVsixCandidatePayloadManifest({
      ...base,
      server: base.server ? { ...base.server, target: 'aarch64-unknown-linux-gnu' } : undefined,
    }),
  ).toThrow(/another target/);
});

test('missing required DAP cannot produce a target-specific manifest', () => {
  expect(() => buildVsixCandidatePayloadManifest({ ...manifestInput(), dap: undefined })).toThrow(
    /missing its required DAP payload/,
  );
});

test('universal manifests reject any host-native payload', () => {
  const universal = rows().find((row) => row.packageMode === 'universal_managed');
  if (!universal) {
    throw new Error('universal fixture is missing');
  }
  const base = manifestInput({ projection: universal, server: undefined, dap: undefined });
  expect(buildVsixCandidatePayloadManifest(base)).toMatchObject({
    package: { mode: 'universal_managed' },
    server: null,
    dap: { disposition: 'not_required', payload: null },
  });
  expect(() =>
    buildVsixCandidatePayloadManifest({ ...base, server: manifestInput().server }),
  ).toThrow(/binaryless/);
});

test('unsupported projection cannot become a package manifest', () => {
  const unsupported = deriveVsixTargetProjection({
    releaseTopologySha256: digest,
    targets: [],
    includeUniversalManaged: false,
    unsupportedTargets: ['freebsd-x64'],
  })[0];
  if (!unsupported) {
    throw new Error('unsupported fixture is missing');
  }
  expect(() =>
    buildVsixCandidatePayloadManifest(
      manifestInput({ projection: unsupported, server: undefined, dap: undefined }),
    ),
  ).toThrow(/unsupported target/);
});

test('source and topology identity changes alter canonical manifest bytes', () => {
  const first = buildVsixCandidatePayloadManifest(manifestInput());
  const second = buildVsixCandidatePayloadManifest(
    manifestInput({
      extension: {
        id: first.extension.id,
        version: first.extension.version,
        sourceSha: 'f'.repeat(40),
      },
    }),
  );
  expect(canonicalVsixPayloadJson(first)).not.toBe(canonicalVsixPayloadJson(second));
});

test('fixed structured input produces byte-equivalent canonical JSON', () => {
  const manifest = buildVsixCandidatePayloadManifest(manifestInput());
  expect(canonicalVsixPayloadJson(manifest)).toBe(canonicalVsixPayloadJson(manifest));
});
