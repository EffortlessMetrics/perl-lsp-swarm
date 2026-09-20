import * as fs from 'fs';
import * as path from 'path';
import Ajv2020 from 'ajv/dist/2020';
import { expect, test } from '@jest/globals';
import {
  buildVsixCandidatePayloadManifest,
  canonicalVsixPayloadJson,
  deriveVsixTargetProjection,
  type ReleaseTopologyTargetRow,
  type VsixCandidatePayloadManifestInput,
} from '../vsixPackageProjection';
import { RELEASE_TOPOLOGY_MANAGED_TARGETS } from '../releaseTopologyTargets';

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

/**
 * Per-target fixture facts keyed by the canonical managed-target list from
 * releaseTopologyTargets (itself drift-pinned to release.yml). The binding test
 * below rejects any decay between this record and the watched list, so this
 * file never maintains an unobserved second target matrix.
 */
const TOPOLOGY_TARGET_FACTS: Record<
  string,
  {
    os: ReleaseTopologyTargetRow['os'];
    architecture: ReleaseTopologyTargetRow['architecture'];
    libc: ReleaseTopologyTargetRow['libc'];
  }
> = {
  'x86_64-unknown-linux-gnu': { os: 'linux', architecture: 'x86_64', libc: 'gnu' },
  'aarch64-unknown-linux-gnu': { os: 'linux', architecture: 'aarch64', libc: 'gnu' },
  'x86_64-unknown-linux-musl': { os: 'linux', architecture: 'x86_64', libc: 'musl' },
  'aarch64-unknown-linux-musl': { os: 'linux', architecture: 'aarch64', libc: 'musl' },
  'x86_64-apple-darwin': { os: 'macos', architecture: 'x86_64', libc: null },
  'aarch64-apple-darwin': { os: 'macos', architecture: 'aarch64', libc: null },
  'x86_64-pc-windows-msvc': { os: 'windows', architecture: 'x86_64', libc: null },
  'aarch64-pc-windows-msvc': { os: 'windows', architecture: 'aarch64', libc: null },
};

const topology: readonly ReleaseTopologyTargetRow[] = RELEASE_TOPOLOGY_MANAGED_TARGETS.map(
  (target) => {
    const facts = TOPOLOGY_TARGET_FACTS[target];
    if (!facts) {
      throw new Error(`No fixture facts recorded for canonical target ${target}.`);
    }
    return topologyRow(target, facts.os, facts.architecture, facts.libc);
  },
);

function rowFor(target: string): ReleaseTopologyTargetRow {
  const row = topology.find((candidate) => candidate.target === target);
  if (!row) {
    throw new Error(`Fixture topology is missing ${target}.`);
  }
  return row;
}

test('fixture topology stays bound to the canonical release topology list', () => {
  expect(Object.keys(TOPOLOGY_TARGET_FACTS).sort()).toEqual(
    [...RELEASE_TOPOLOGY_MANAGED_TARGETS].sort(),
  );
  expect(topology.map((row) => row.target)).toEqual([...RELEASE_TOPOLOGY_MANAGED_TARGETS]);
});

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
      targets: [rowFor('x86_64-unknown-linux-gnu'), rowFor('x86_64-unknown-linux-gnu')],
      includeUniversalManaged: false,
    }),
  ).toThrow(/Duplicate release topology target/);

  const sameArchive = {
    ...rowFor('aarch64-unknown-linux-gnu'),
    archiveName: rowFor('x86_64-unknown-linux-gnu').archiveName,
  };
  expect(() =>
    deriveVsixTargetProjection({
      releaseTopologySha256: digest,
      targets: [rowFor('x86_64-unknown-linux-gnu'), sameArchive],
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

test('missing required server cannot produce a target-specific manifest', () => {
  expect(() =>
    buildVsixCandidatePayloadManifest({ ...manifestInput(), server: undefined }),
  ).toThrow(/missing its required server payload/);
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
  const first = buildVsixCandidatePayloadManifest(manifestInput());
  const second = buildVsixCandidatePayloadManifest(manifestInput());
  expect(canonicalVsixPayloadJson(first)).toBe(canonicalVsixPayloadJson(second));
});

test('emitted manifests conform to the checked-in candidate payload schema', () => {
  const root = path.resolve(__dirname, '../../..');
  const schema = JSON.parse(
    fs.readFileSync(path.join(root, 'schemas/vsix_candidate_payload.v1.schema.json'), 'utf8'),
  ) as object;
  // strict:false silences ajv's strictTypes warnings: the schema's if/then
  // applicator blocks intentionally omit redundant `type: "object"` declarations,
  // which strict mode rejects even though the top-level object contract holds.
  const validate = new Ajv2020({ strict: false }).compile(schema);

  const targetSpecific = JSON.parse(
    canonicalVsixPayloadJson(buildVsixCandidatePayloadManifest(manifestInput())),
  ) as Record<string, unknown>;
  expect(validate(targetSpecific)).toBe(true);

  const universal = rows().find((row) => row.packageMode === 'universal_managed');
  if (!universal) {
    throw new Error('universal fixture is missing');
  }
  const universalManifest = JSON.parse(
    canonicalVsixPayloadJson(
      buildVsixCandidatePayloadManifest(
        manifestInput({ projection: universal, server: undefined, dap: undefined }),
      ),
    ),
  ) as Record<string, unknown>;
  expect(validate(universalManifest)).toBe(true);

  expect(validate({ ...targetSpecific, server: null })).toBe(false);
});
