import * as fs from 'fs';
import * as path from 'path';
import { expect, test } from '@jest/globals';
import {
  decideManagedHostTarget,
  requireManagedHostTarget,
  type ManagedHostTargetDecisionInput,
} from '../managedHostTarget';
import {
  RELEASE_TOPOLOGY_MANAGED_TARGETS,
  RELEASE_TOPOLOGY_SOURCE,
} from '../releaseTopologyTargets';

function decide(overrides: Partial<ManagedHostTargetDecisionInput> = {}) {
  return decideManagedHostTarget({
    host: {
      platform: 'linux',
      arch: 'x64',
      linuxLibc: 'gnu',
      environment: 'ordinary',
    },
    ...overrides,
  });
}

function host(
  platform: string,
  arch: string,
  linuxLibc: 'gnu' | 'musl' | 'not_proven' | 'not_applicable' = 'not_applicable',
) {
  return { platform, arch, linuxLibc, environment: 'ordinary' as const };
}

test('the managed target projection exactly matches the canonical release workflow', () => {
  const root = path.resolve(__dirname, '../../..');
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/release.yml'), 'utf8');
  const targets = [...workflow.matchAll(/^\s*- target:\s*([A-Za-z0-9_-]+)\s*$/gm)].map(
    (match) => match[1],
  );
  expect([...new Set(targets)].sort()).toEqual([...RELEASE_TOPOLOGY_MANAGED_TARGETS].sort());
  expect(RELEASE_TOPOLOGY_SOURCE).toContain('release.yml');
});

test.each([
  ['x64', 'gnu', 'x86_64-unknown-linux-gnu'],
  ['x64', 'musl', 'x86_64-unknown-linux-musl'],
  ['arm64', 'gnu', 'aarch64-unknown-linux-gnu'],
  ['arm64', 'musl', 'aarch64-unknown-linux-musl'],
] as const)(
  'Linux %s %s selects only its exact target',
  (arch: 'x64' | 'arm64', libc: 'gnu' | 'musl', selectedTarget: string) => {
    expect(decide({ host: host('linux', arch, libc) })).toMatchObject({
      selectionKind: 'exact',
      selectedTarget,
      hostPlatform: 'linux',
      hostArch: arch,
      hostAbi: libc,
    });
  },
);

test.each(['arm', 'ia32', 'ppc64', 's390x', 'future-arch'])(
  'Linux %s never becomes x86_64',
  (arch: string) => {
    expect(decide({ host: host('linux', arch, 'gnu') })).toMatchObject({
      selectionKind: 'unsupported',
      selectedTarget: null,
      reason: 'unsupported_architecture',
    });
  },
);

test('Linux libc uncertainty remains not proven', () => {
  expect(decide({ host: host('linux', 'x64', 'not_proven') })).toMatchObject({
    selectionKind: 'not_proven',
    selectedTarget: null,
    reason: 'linux_libc_not_proven',
  });
});

test.each(['android', 'termux'] as const)(
  '%s is explicit unsupported environment',
  (environment: 'android' | 'termux') => {
    expect(
      decide({
        host: { platform: 'linux', arch: 'arm64', linuxLibc: 'not_proven', environment },
      }),
    ).toMatchObject({
      selectionKind: 'unsupported',
      selectedTarget: null,
      reason: 'unsupported_android_environment',
    });
  },
);

test.each([
  ['x64', 'x86_64-apple-darwin'],
  ['arm64', 'aarch64-apple-darwin'],
] as const)(
  'Darwin %s selects its exact topology row',
  (arch: 'x64' | 'arm64', selectedTarget: string) => {
    expect(decide({ host: host('darwin', arch) })).toMatchObject({
      selectionKind: 'exact',
      selectedTarget,
    });
  },
);

test('Darwin unknown architecture does not become x86_64', () => {
  expect(decide({ host: host('darwin', 'ppc64') })).toMatchObject({
    selectionKind: 'unsupported',
    selectedTarget: null,
  });
});

test.each([
  ['x64', 'x86_64-pc-windows-msvc'],
  ['arm64', 'aarch64-pc-windows-msvc'],
] as const)(
  'Windows %s selects its exact preferred topology row',
  (arch: 'x64' | 'arm64', selectedTarget: string) => {
    expect(decide({ host: host('win32', arch) })).toMatchObject({
      selectionKind: 'exact',
      selectedTarget,
    });
  },
);

test.each(['ia32', 'arm', 'ppc64', 'future-arch'])(
  'Windows %s never inherits x64 fallback',
  (arch: string) => {
    expect(decide({ host: host('win32', arch) })).toMatchObject({
      selectionKind: 'unsupported',
      selectedTarget: null,
      reason: 'unsupported_architecture',
    });
  },
);

test('a topology mutation removing a known target becomes unsupported', () => {
  expect(
    decide({
      host: host('linux', 'x64', 'gnu'),
      supportedTargets: new Set(['aarch64-unknown-linux-gnu']),
      topologyRef: 'fixture:target-removed',
    }),
  ).toMatchObject({
    selectionKind: 'unsupported',
    selectedTarget: null,
    reason: 'topology_target_absent',
    topologyRef: 'fixture:target-removed',
  });
});

test('an unsupported platform fails before a target can be required', () => {
  const decision = decide({ host: host('freebsd', 'x64') });
  expect(decision).toMatchObject({
    selectionKind: 'unsupported',
    reason: 'unsupported_platform',
  });
  expect(() => requireManagedHostTarget(decision)).toThrow(/No managed binary target/);
});

test('workspace-host identity is the only target-selection input', () => {
  const remoteHost = decide({ host: host('linux', 'arm64', 'musl') });
  expect(remoteHost).toMatchObject({
    selectedTarget: 'aarch64-unknown-linux-musl',
    hostPlatform: 'linux',
    hostArch: 'arm64',
  });
});
