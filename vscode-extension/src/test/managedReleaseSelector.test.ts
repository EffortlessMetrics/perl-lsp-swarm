import {
  selectManagedRelease,
  type ManagedReleaseExpectation,
  type ManagedReleaseRecord,
  type ManagedReleaseSelectionInput,
} from '../managedReleaseSelector';

const expectation: ManagedReleaseExpectation = {
  extensionCandidateId: 'vscode-perl-lsp@0.18.0',
  compatibilityRequirement: 'extension-protocol.v2',
  target: 'x86_64-unknown-linux-gnu',
  extensionTrack: 'stable',
};

function release(
  version: string,
  overrides: Partial<ManagedReleaseRecord> = {},
): ManagedReleaseRecord {
  const tagName = `v${version}`;
  return {
    releaseId: `release:${tagName}`,
    candidateId: `candidate:${tagName}`,
    tagName,
    version,
    prerelease: version.includes('-'),
    draft: false,
    compatibilityRequirement: expectation.compatibilityRequirement,
    compatibilityState: 'compatible',
    compatibilityEvidenceRef: `compat:${tagName}`,
    target: expectation.target,
    targetState: 'available',
    targetEvidenceRef: `target:${tagName}`,
    ...overrides,
  };
}

function select(
  overrides: Partial<ManagedReleaseSelectionInput> = {},
): ReturnType<typeof selectManagedRelease> {
  return selectManagedRelease({
    expectation,
    channel: 'stable',
    releases: [],
    ...overrides,
  });
}

test('stable selects the newest proven compatible non-prerelease', () => {
  const result = select({
    releases: [
      release('0.19.0', {
        compatibilityState: 'incompatible',
        compatibilityEvidenceRef: 'compat:0.19.0:incompatible',
      }),
      release('0.18.0'),
      release('0.17.0'),
    ],
  });

  expect(result).toMatchObject({
    kind: 'selected',
    reason: 'stable_newest_compatible',
    release: { tagName: 'v0.18.0' },
  });
});

test('stable excludes prereleases while latest admits compatible prereleases', () => {
  const releases = [release('0.19.0-rc.1'), release('0.18.0')];

  expect(select({ channel: 'stable', releases })).toMatchObject({
    kind: 'selected',
    release: { tagName: 'v0.18.0' },
  });
  expect(select({ channel: 'latest', releases })).toMatchObject({
    kind: 'selected',
    reason: 'latest_newest_compatible',
    release: { tagName: 'v0.19.0-rc.1' },
  });
});

test('tag resolves the exact compatible release without substitution', () => {
  const result = select({
    channel: 'tag',
    explicitTag: 'v0.17.0',
    releases: [release('0.18.0'), release('0.17.0')],
  });

  expect(result).toMatchObject({
    kind: 'selected',
    reason: 'tag_exact_compatible',
    release: { tagName: 'v0.17.0' },
  });
});

test('an exact incompatible tag remains configured incompatibility', () => {
  const result = select({
    channel: 'tag',
    explicitTag: 'v0.19.0',
    releases: [
      release('0.19.0', {
        compatibilityState: 'incompatible',
        compatibilityEvidenceRef: 'compat:0.19.0:incompatible',
      }),
      release('0.18.0'),
    ],
  });

  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'configured_incompatible',
    configuredTag: 'v0.19.0',
  });
});

test('tag never substitutes a nearby release when the exact tag is absent', () => {
  const result = select({
    channel: 'tag',
    explicitTag: 'v0.17.1',
    releases: [release('0.18.0'), release('0.17.0')],
  });

  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'no_compatible_release',
    configuredTag: 'v0.17.1',
  });
});

test('a newer not-proven release blocks recency selection behind it', () => {
  const result = select({
    releases: [
      release('0.19.0', {
        compatibilityState: 'not_proven',
        compatibilityEvidenceRef: undefined,
      }),
      release('0.18.0'),
    ],
  });

  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.19.0',
  });
});

test('an older not-proven release does not block a newer proven release', () => {
  const result = select({
    releases: [
      release('0.18.0'),
      release('0.17.0', {
        targetState: 'not_proven',
        targetEvidenceRef: undefined,
      }),
    ],
  });

  expect(result).toMatchObject({
    kind: 'selected',
    release: { tagName: 'v0.18.0' },
  });
});

test('target absence is ineligible rather than satisfied by an asset-like version', () => {
  const result = select({
    releases: [
      release('0.19.0', {
        targetState: 'unavailable',
        targetEvidenceRef: 'target:0.19.0:unavailable',
      }),
      release('0.18.0'),
    ],
  });

  expect(result).toMatchObject({
    kind: 'selected',
    release: { tagName: 'v0.18.0' },
  });
});

test('version equality without compatibility evidence cannot create compatibility', () => {
  const result = select({
    releases: [
      release('0.18.0', {
        compatibilityState: 'not_proven',
        compatibilityEvidenceRef: undefined,
      }),
    ],
  });

  expect(result).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
  });
});

test('draft releases never satisfy managed selection', () => {
  const result = select({
    channel: 'latest',
    releases: [release('0.19.0', { draft: true }), release('0.18.0')],
  });

  expect(result).toMatchObject({
    kind: 'selected',
    release: { tagName: 'v0.18.0' },
  });
});

test('invalid or ambiguous release metadata fails closed', () => {
  const invalidVersion = select({ releases: [release('0.18')] });
  expect(invalidVersion).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18',
  });

  const equalPrecedence = select({
    releases: [
      release('0.18.0', { releaseId: 'release:a', tagName: 'v0.18.0+a' }),
      release('0.18.0+build.2', { releaseId: 'release:b', tagName: 'v0.18.0+b' }),
    ],
  });
  expect(equalPrecedence).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:b',
  });
});

test('release records from another requirement or target fail closed', () => {
  const wrongRequirement = select({
    releases: [release('0.18.0', { compatibilityRequirement: 'extension-protocol.v1' })],
  });
  expect(wrongRequirement).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });

  const wrongTarget = select({
    releases: [release('0.18.0', { target: 'aarch64-apple-darwin' })],
  });
  expect(wrongTarget).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });
});

test('resolved compatibility or target states require evidence references', () => {
  const missingCompatibilityEvidence = select({
    releases: [release('0.18.0', { compatibilityEvidenceRef: '' })],
  });
  expect(missingCompatibilityEvidence).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });

  const missingTargetEvidence = select({
    releases: [release('0.18.0', { targetEvidenceRef: '' })],
  });
  expect(missingTargetEvidence).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });
});

test('duplicate release or tag identity fails closed', () => {
  const duplicateReleaseId = select({
    releases: [release('0.18.0'), release('0.18.1', { releaseId: 'release:v0.18.0' })],
  });
  expect(duplicateReleaseId).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });

  const duplicateTag = select({
    releases: [release('0.18.0'), release('0.18.1', { tagName: 'v0.18.0' })],
  });
  expect(duplicateTag).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.1',
  });
});

test('prerelease flag must agree with the parsed semantic version', () => {
  const flaggedStable = select({
    releases: [release('0.19.0-rc.1', { prerelease: false })],
  });
  expect(flaggedStable).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.19.0-rc.1',
  });

  const flaggedPrerelease = select({
    releases: [release('0.18.0', { prerelease: true })],
  });
  expect(flaggedPrerelease).toMatchObject({
    kind: 'refused',
    reason: 'release_metadata_not_proven',
    blockingReleaseId: 'release:v0.18.0',
  });
});

test('stable and latest refuse when no compatible release exists', () => {
  expect(select({ releases: [] })).toMatchObject({
    kind: 'refused',
    reason: 'no_compatible_release',
  });

  expect(
    select({
      releases: [
        release('0.18.0', {
          compatibilityState: 'incompatible',
          compatibilityEvidenceRef: 'compat:0.18.0:incompatible',
        }),
      ],
    }),
  ).toMatchObject({
    kind: 'refused',
    reason: 'no_compatible_release',
  });
});

test('tag policy requires exactly one tag and other channels reject one', () => {
  expect(select({ channel: 'tag', explicitTag: undefined })).toMatchObject({
    kind: 'refused',
    reason: 'invalid_policy',
  });
  expect(select({ channel: 'stable', explicitTag: 'v0.18.0' })).toMatchObject({
    kind: 'refused',
    reason: 'invalid_policy',
  });
});

test('fixed input produces byte-equivalent canonical result content', () => {
  const input: ManagedReleaseSelectionInput = {
    expectation,
    channel: 'latest',
    releases: [release('0.18.0'), release('0.19.0-rc.1')],
  };

  expect(selectManagedRelease(input)).toEqual({
    kind: 'selected',
    reason: 'latest_newest_compatible',
    expectation: { ...expectation },
    release: release('0.19.0-rc.1'),
  });

  const permuted: ManagedReleaseSelectionInput = {
    ...input,
    releases: [release('0.19.0-rc.1'), release('0.18.0')],
  };
  expect(JSON.stringify(selectManagedRelease(permuted))).toBe(
    JSON.stringify(selectManagedRelease(input)),
  );
});
