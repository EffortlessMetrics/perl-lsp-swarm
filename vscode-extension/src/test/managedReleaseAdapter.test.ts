import { expect, test } from '@jest/globals';
import {
  buildManagedReleaseExpectation,
  toManagedReleaseRecords,
  MANAGED_RELEASE_COMPATIBILITY_REQUIREMENT,
  type TransportRelease,
} from '../managedReleaseAdapter';
import { selectManagedRelease, type ManagedReleaseSelectionInput } from '../managedReleaseSelector';

const HOST_TARGET = 'x86_64-unknown-linux-gnu';

const expectation = buildManagedReleaseExpectation({
  extensionId: 'EffortlessMetrics.perl-lsp-rs',
  extensionVersion: '0.18.0',
  hostTarget: HOST_TARGET,
});

function findAsset(
  assets: ReadonlyArray<{ readonly name: string }>,
  versionOrTag: string,
  target: string,
  archiveExtension: string,
): string | undefined {
  const name = `perllsp-${versionOrTag.replace(/^v/, '')}-${target}${archiveExtension}`;
  return assets.some((asset) => asset.name === name) ? name : undefined;
}

function release(tag: string, overrides: Partial<TransportRelease> = {}): TransportRelease {
  return {
    tag_name: tag,
    prerelease: tag.includes('-'),
    draft: false,
    assets: [{ name: `perllsp-${tag.replace(/^v/, '')}-${HOST_TARGET}.tar.gz` }],
    ...overrides,
  };
}

function select(
  overrides: Partial<ManagedReleaseSelectionInput> = {},
  releases: TransportRelease[] = [release('v0.18.0')],
) {
  const channel = overrides.channel ?? 'stable';
  return selectManagedRelease({
    expectation,
    channel,
    releases: toManagedReleaseRecords(
      releases,
      expectation,
      [HOST_TARGET],
      '.tar.gz',
      findAsset,
      channel,
    ).records,
    ...overrides,
  });
}

test('expectation carries extension identity, target, and the train contract', () => {
  expect(expectation).toEqual({
    extensionCandidateId: 'EffortlessMetrics.perl-lsp-rs@0.18.0',
    compatibilityRequirement: MANAGED_RELEASE_COMPATIBILITY_REQUIREMENT,
    target: HOST_TARGET,
    extensionTrack: 'stable',
  });
  expect(
    buildManagedReleaseExpectation({
      extensionId: 'ext',
      extensionVersion: '0.19.0-rc.1',
      hostTarget: HOST_TARGET,
    }).extensionTrack,
  ).toBe('prerelease');
});

test('stable selection requires an asset carrying the host target', () => {
  const withAsset = select({}, [release('v0.18.0')]);
  expect(withAsset).toMatchObject({
    kind: 'selected',
    reason: 'stable_newest_compatible',
    release: { tagName: 'v0.18.0' },
  });

  // A release without a host-target asset is ineligible: an unsupported host
  // target must be refused here, never reach artifact download.
  const withoutAsset = select({}, [release('v0.18.0', { assets: [] })]);
  expect(withoutAsset).toMatchObject({
    kind: 'refused',
    reason: 'no_compatible_release',
  });
});

test('a newer incompatible-for-target release never defeats an older servable one', () => {
  const result = select({}, [release('v0.19.0', { assets: [] }), release('v0.18.0')]);
  expect(result).toMatchObject({
    kind: 'selected',
    release: { tagName: 'v0.18.0' },
  });
});

test('the latest channel admits an explicit prerelease; stable excludes it', () => {
  const releases = [release('v0.19.0-rc.1', { prerelease: true }), release('v0.18.0')];
  expect(select({ channel: 'latest' }, releases)).toMatchObject({
    kind: 'selected',
    reason: 'latest_newest_compatible',
    release: { tagName: 'v0.19.0-rc.1' },
  });
  expect(select({ channel: 'stable' }, releases)).toMatchObject({
    kind: 'selected',
    release: { tagName: 'v0.18.0' },
  });
});

test('an omitted prerelease flag is fail-closed, not stable evidence', () => {
  // The fixture carries a matching asset, so targetState is available and the
  // refusal can only come from the omitted prerelease flag — an adapter that
  // wrongly mapped it to false would select this record instead.
  const result = select({}, [
    {
      tag_name: 'v0.18.0',
      assets: [{ name: `perllsp-0.18.0-${HOST_TARGET}.tar.gz` }],
    },
  ]);
  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });
});

test('a release servable through any candidate target is available', () => {
  // Windows ARM64 production path: native asset absent, x64 emulation asset
  // present — the second candidate target must satisfy availability.
  const conversion = toManagedReleaseRecords(
    [release('v0.18.0', { assets: [{ name: 'perllsp-0.18.0-x86_64-pc-windows-msvc.zip' }] })],
    buildManagedReleaseExpectation({
      extensionId: 'ext',
      extensionVersion: '0.18.0',
      hostTarget: 'aarch64-pc-windows-msvc',
    }),
    ['aarch64-pc-windows-msvc', 'x86_64-pc-windows-msvc'],
    '.zip',
    findAsset,
    'stable',
  );
  expect(conversion.records[0]).toMatchObject({
    target: 'aarch64-pc-windows-msvc',
    targetState: 'available',
  });
  const nativeOnly = toManagedReleaseRecords(
    [release('v0.18.0', { assets: [{ name: 'perllsp-0.18.0-aarch64-pc-windows-msvc.zip' }] })],
    buildManagedReleaseExpectation({
      extensionId: 'ext',
      extensionVersion: '0.18.0',
      hostTarget: 'aarch64-pc-windows-msvc',
    }),
    ['aarch64-pc-windows-msvc', 'x86_64-pc-windows-msvc'],
    '.zip',
    findAsset,
    'stable',
  );
  expect(nativeOnly.records[0]).toMatchObject({ targetState: 'available' });
  const unservable = toManagedReleaseRecords(
    [release('v0.18.0', { assets: [] })],
    expectation,
    [HOST_TARGET, 'x86_64-unknown-linux-musl'],
    '.tar.gz',
    findAsset,
    'stable',
  );
  expect(unservable.records[0]).toMatchObject({ targetState: 'unavailable' });
});

test('draft releases never satisfy any channel', () => {
  const result = select({ channel: 'latest' }, [release('v0.18.0', { draft: true })]);
  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'no_compatible_release',
  });
});

test('tag channel requires the exact configured tag without substitution', () => {
  const releases = [release('v0.18.0')];
  expect(select({ channel: 'tag', explicitTag: 'v0.18.0' }, releases)).toMatchObject({
    kind: 'selected',
    reason: 'tag_exact_compatible',
  });
  expect(select({ channel: 'tag', explicitTag: 'v0.17.9' }, releases)).toMatchObject({
    kind: 'refused',
    reason: 'no_compatible_release',
    configuredTag: 'v0.17.9',
  });
  expect(select({ channel: 'tag' }, releases)).toMatchObject({
    kind: 'refused',
    reason: 'invalid_policy',
  });
});

test('duplicate transport records fail closed', () => {
  expect(select({}, [release('v0.18.0'), release('v0.18.0')])).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
  });
});

test('a mistagged historical record older than a valid release cannot poison selection', () => {
  // Hosted-smoke regression: GitHub release v0.13.1 carries prerelease:true
  // on a stable-semver tag. Quarantined to not_proven, it must not refuse the
  // whole route when a valid newer release exists.
  const result = select({}, [release('v0.13.1', { prerelease: true }), release('v0.18.0')]);
  expect(result).toMatchObject({
    kind: 'selected',
    reason: 'stable_newest_compatible',
    release: { tagName: 'v0.18.0' },
  });
});

test('a mistagged record newer than every valid release blocks selection', () => {
  const result = select({}, [release('v0.99.0', { prerelease: true }), release('v0.18.0')]);
  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.99.0',
  });
});

test('a mistagged record is never selected on recency channels but remains pinnable', () => {
  const mistagged = [release('v0.13.1', { prerelease: true })];
  // stable/latest: quarantined to not_proven, never selected.
  expect(select({}, mistagged)).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
  });
  expect(select({ channel: 'latest' }, mistagged)).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
  });
  // Exact-tag: an explicit user pin of one specific artifact overrides the
  // channel-level metadata anomaly — pinning a mistagged historical release
  // keeps working, as it did before the selector wiring (hosted smoke pins
  // exactly v0.13.1).
  expect(select({ channel: 'tag', explicitTag: 'v0.13.1' }, mistagged)).toMatchObject({
    kind: 'selected',
    reason: 'tag_exact_compatible',
    release: { tagName: 'v0.13.1' },
  });
  expect(select({ channel: 'tag', explicitTag: 'v0.18.0' }, [release('v0.18.0')])).toMatchObject({
    kind: 'selected',
    reason: 'tag_exact_compatible',
  });
});

test('unparseable tags are quarantined out without blocking or satisfying selection', () => {
  const conversion = toManagedReleaseRecords(
    [
      { tag_name: 'nightly-build', prerelease: false, draft: false, assets: [] },
      release('v0.18.0'),
    ],
    expectation,
    [HOST_TARGET],
    '.tar.gz',
    findAsset,
    'stable',
  );
  expect(conversion.droppedTags).toEqual(['nightly-build']);
  expect(
    selectManagedRelease({ expectation, channel: 'stable', releases: conversion.records }),
  ).toMatchObject({ kind: 'selected', release: { tagName: 'v0.18.0' } });

  const onlyGarbage = select({}, [{ tag_name: 'nightly-build', prerelease: false, assets: [] }]);
  expect(onlyGarbage).toMatchObject({ kind: 'refused', reason: 'no_compatible_release' });
});
