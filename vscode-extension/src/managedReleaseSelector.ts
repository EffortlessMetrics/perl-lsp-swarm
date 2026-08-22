export type ManagedReleaseChannel = 'stable' | 'latest' | 'tag';
export type ManagedReleaseTrack = 'stable' | 'prerelease';
export type ManagedCompatibilityState = 'compatible' | 'incompatible' | 'not_proven';
export type ManagedTargetState = 'available' | 'unavailable' | 'not_proven';

export interface ManagedReleaseExpectation {
  readonly extensionCandidateId: string;
  readonly compatibilityRequirement: string;
  readonly target: string;
  readonly extensionTrack: ManagedReleaseTrack;
}

export interface ManagedReleaseRecord {
  readonly releaseId: string;
  readonly candidateId: string;
  readonly tagName: string;
  readonly version: string;
  readonly prerelease: boolean;
  readonly draft: boolean;
  readonly compatibilityRequirement: string;
  readonly compatibilityState: ManagedCompatibilityState;
  readonly compatibilityEvidenceRef?: string | undefined;
  readonly target: string;
  readonly targetState: ManagedTargetState;
  readonly targetEvidenceRef?: string | undefined;
}

export interface ManagedReleaseSelectionInput {
  readonly expectation: ManagedReleaseExpectation;
  readonly channel: ManagedReleaseChannel;
  readonly explicitTag?: string | undefined;
  readonly releases: readonly ManagedReleaseRecord[];
}

export type ManagedReleaseSelectionReason =
  | 'stable_newest_compatible'
  | 'latest_newest_compatible'
  | 'tag_exact_compatible';

export type ManagedReleaseRefusalReason =
  | 'configured_incompatible'
  | 'no_compatible_release'
  | 'release_metadata_not_proven'
  | 'invalid_policy';

export interface SelectedManagedRelease {
  readonly kind: 'selected';
  readonly reason: ManagedReleaseSelectionReason;
  readonly expectation: ManagedReleaseExpectation;
  readonly release: ManagedReleaseRecord;
}

export interface RefusedManagedRelease {
  readonly kind: 'refused';
  readonly reason: ManagedReleaseRefusalReason;
  readonly detail: string;
  readonly expectation: ManagedReleaseExpectation;
  readonly blockingReleaseId?: string | undefined;
  readonly configuredTag?: string | undefined;
}

export type ManagedReleaseSelection = SelectedManagedRelease | RefusedManagedRelease;

interface ParsedSemver {
  readonly major: number;
  readonly minor: number;
  readonly patch: number;
  readonly prerelease: readonly string[];
}

interface PreparedRelease {
  readonly release: ManagedReleaseRecord;
  readonly version: ParsedSemver;
}

const SEMVER_PATTERN =
  /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

function nonEmpty(value: string): boolean {
  return value.trim().length > 0;
}

function cloneExpectation(expectation: ManagedReleaseExpectation): ManagedReleaseExpectation {
  return {
    extensionCandidateId: expectation.extensionCandidateId,
    compatibilityRequirement: expectation.compatibilityRequirement,
    target: expectation.target,
    extensionTrack: expectation.extensionTrack,
  };
}

function cloneRelease(release: ManagedReleaseRecord): ManagedReleaseRecord {
  return {
    releaseId: release.releaseId,
    candidateId: release.candidateId,
    tagName: release.tagName,
    version: release.version,
    prerelease: release.prerelease,
    draft: release.draft,
    compatibilityRequirement: release.compatibilityRequirement,
    compatibilityState: release.compatibilityState,
    compatibilityEvidenceRef: release.compatibilityEvidenceRef,
    target: release.target,
    targetState: release.targetState,
    targetEvidenceRef: release.targetEvidenceRef,
  };
}

function refuse(
  expectation: ManagedReleaseExpectation,
  reason: ManagedReleaseRefusalReason,
  detail: string,
  options: {
    readonly blockingReleaseId?: string | undefined;
    readonly configuredTag?: string | undefined;
  } = {},
): RefusedManagedRelease {
  return {
    kind: 'refused',
    reason,
    detail,
    expectation: cloneExpectation(expectation),
    blockingReleaseId: options.blockingReleaseId,
    configuredTag: options.configuredTag,
  };
}

function parseSemver(value: string): ParsedSemver | null {
  const match = SEMVER_PATTERN.exec(value);
  if (!match) {
    return null;
  }

  const majorText = match[1];
  const minorText = match[2];
  const patchText = match[3];
  if (majorText === undefined || minorText === undefined || patchText === undefined) {
    return null;
  }

  const major = Number(majorText);
  const minor = Number(minorText);
  const patch = Number(patchText);
  if (![major, minor, patch].every(Number.isSafeInteger)) {
    return null;
  }

  const prereleaseText = match[4];
  const prerelease = prereleaseText ? prereleaseText.split('.') : [];
  if (
    prerelease.some(
      (identifier) =>
        /^\d+$/.test(identifier) && identifier.length > 1 && identifier.startsWith('0'),
    )
  ) {
    return null;
  }

  return { major, minor, patch, prerelease };
}

function comparePrereleaseIdentifiers(left: string, right: string): number {
  const leftNumeric = /^\d+$/.test(left);
  const rightNumeric = /^\d+$/.test(right);
  if (leftNumeric && rightNumeric) {
    if (left.length !== right.length) {
      return Math.sign(left.length - right.length);
    }
    return left < right ? -1 : left > right ? 1 : 0;
  }
  if (leftNumeric !== rightNumeric) {
    return leftNumeric ? -1 : 1;
  }
  return left < right ? -1 : left > right ? 1 : 0;
}

function compareSemver(left: ParsedSemver, right: ParsedSemver): number {
  for (const [leftPart, rightPart] of [
    [left.major, right.major],
    [left.minor, right.minor],
    [left.patch, right.patch],
  ] as const) {
    if (leftPart !== rightPart) {
      return Math.sign(leftPart - rightPart);
    }
  }

  if (left.prerelease.length === 0 || right.prerelease.length === 0) {
    if (left.prerelease.length === right.prerelease.length) {
      return 0;
    }
    return left.prerelease.length === 0 ? 1 : -1;
  }

  const sharedLength = Math.min(left.prerelease.length, right.prerelease.length);
  for (let index = 0; index < sharedLength; index += 1) {
    const leftIdentifier = left.prerelease[index];
    const rightIdentifier = right.prerelease[index];
    if (leftIdentifier === undefined || rightIdentifier === undefined) {
      return 0;
    }
    const comparison = comparePrereleaseIdentifiers(leftIdentifier, rightIdentifier);
    if (comparison !== 0) {
      return comparison;
    }
  }

  return Math.sign(left.prerelease.length - right.prerelease.length);
}

function validatePolicy(input: ManagedReleaseSelectionInput): string | null {
  const { expectation, channel, explicitTag } = input;
  if (
    !nonEmpty(expectation.extensionCandidateId) ||
    !nonEmpty(expectation.compatibilityRequirement) ||
    !nonEmpty(expectation.target)
  ) {
    return 'The extension candidate, compatibility requirement, and target must be explicit.';
  }

  if (!['stable', 'prerelease'].includes(expectation.extensionTrack)) {
    return `Unsupported extension track: ${String(expectation.extensionTrack)}.`;
  }

  if (!['stable', 'latest', 'tag'].includes(channel)) {
    return `Unsupported managed release channel: ${String(channel)}.`;
  }

  if (channel === 'tag') {
    if (!explicitTag || !nonEmpty(explicitTag)) {
      return 'The tag channel requires one exact non-empty tag.';
    }
  } else if (explicitTag !== undefined && nonEmpty(explicitTag)) {
    return `The ${channel} channel cannot carry an explicit tag.`;
  }

  return null;
}

function prepareReleases(
  input: ManagedReleaseSelectionInput,
): { readonly releases: readonly PreparedRelease[] } | { readonly error: RefusedManagedRelease } {
  const seenReleaseIds = new Set<string>();
  const seenTags = new Set<string>();
  const seenPrecedence = new Set<string>();
  const prepared: PreparedRelease[] = [];

  for (const release of input.releases) {
    if (
      !nonEmpty(release.releaseId) ||
      !nonEmpty(release.candidateId) ||
      !nonEmpty(release.tagName) ||
      !nonEmpty(release.version)
    ) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          'A release record is missing its release, candidate, tag, or version identity.',
          { blockingReleaseId: release.releaseId || undefined },
        ),
      };
    }

    if (seenReleaseIds.has(release.releaseId) || seenTags.has(release.tagName)) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Release metadata contains a duplicate release or tag identity: ${release.tagName}.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }
    seenReleaseIds.add(release.releaseId);
    seenTags.add(release.tagName);

    if (
      release.compatibilityRequirement !== input.expectation.compatibilityRequirement ||
      release.target !== input.expectation.target
    ) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Release ${release.tagName} was evaluated against another compatibility requirement or target.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }

    if (
      release.compatibilityState !== 'not_proven' &&
      (!release.compatibilityEvidenceRef || !nonEmpty(release.compatibilityEvidenceRef))
    ) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Release ${release.tagName} has no compatibility evidence reference.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }

    if (
      release.targetState !== 'not_proven' &&
      (!release.targetEvidenceRef || !nonEmpty(release.targetEvidenceRef))
    ) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Release ${release.tagName} has no target evidence reference.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }

    const version = parseSemver(release.version);
    if (!version) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Release ${release.tagName} has an invalid semantic version: ${release.version}.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }

    if (version.prerelease.length > 0 !== release.prerelease) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Release ${release.tagName} disagrees with its semantic-version prerelease identity.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }

    const precedenceKey = [version.major, version.minor, version.patch, ...version.prerelease].join(
      '.',
    );
    if (seenPrecedence.has(precedenceKey)) {
      return {
        error: refuse(
          input.expectation,
          'release_metadata_not_proven',
          `Multiple release records have equal semantic-version precedence at ${release.version}.`,
          { blockingReleaseId: release.releaseId },
        ),
      };
    }
    seenPrecedence.add(precedenceKey);
    prepared.push({ release, version });
  }

  return { releases: prepared };
}

function isProvenCompatible(release: ManagedReleaseRecord): boolean {
  return release.compatibilityState === 'compatible' && release.targetState === 'available';
}

function isNotProven(release: ManagedReleaseRecord): boolean {
  return release.compatibilityState === 'not_proven' || release.targetState === 'not_proven';
}

function selected(
  input: ManagedReleaseSelectionInput,
  release: ManagedReleaseRecord,
  reason: ManagedReleaseSelectionReason,
): SelectedManagedRelease {
  return {
    kind: 'selected',
    reason,
    expectation: cloneExpectation(input.expectation),
    release: cloneRelease(release),
  };
}

export function selectManagedRelease(input: ManagedReleaseSelectionInput): ManagedReleaseSelection {
  const policyError = validatePolicy(input);
  if (policyError) {
    return refuse(input.expectation, 'invalid_policy', policyError, {
      configuredTag: input.explicitTag,
    });
  }

  const preparedResult = prepareReleases(input);
  if ('error' in preparedResult) {
    return preparedResult.error;
  }

  if (input.channel === 'tag') {
    const configuredTag = input.explicitTag;
    if (!configuredTag) {
      return refuse(input.expectation, 'invalid_policy', 'The tag channel requires one exact tag.');
    }
    const exact = preparedResult.releases.find(
      ({ release }) => !release.draft && release.tagName === configuredTag,
    );
    if (!exact) {
      return refuse(
        input.expectation,
        'no_compatible_release',
        `No public release record exists for the exact configured tag ${configuredTag}.`,
        { configuredTag },
      );
    }
    if (isNotProven(exact.release)) {
      return refuse(
        input.expectation,
        'release_metadata_not_proven',
        `Compatibility or target availability is not proven for ${configuredTag}.`,
        { blockingReleaseId: exact.release.releaseId, configuredTag },
      );
    }
    if (!isProvenCompatible(exact.release)) {
      return refuse(
        input.expectation,
        'configured_incompatible',
        `The exact configured tag ${configuredTag} is not compatible with this extension candidate and target.`,
        { blockingReleaseId: exact.release.releaseId, configuredTag },
      );
    }
    return selected(input, exact.release, 'tag_exact_compatible');
  }

  const eligible = preparedResult.releases
    .filter(({ release }) => !release.draft)
    .filter(({ release }) => input.channel === 'latest' || !release.prerelease)
    .sort((left, right) => compareSemver(right.version, left.version));

  let newerUnknown: ManagedReleaseRecord | undefined;
  for (const { release } of eligible) {
    if (isNotProven(release)) {
      newerUnknown ??= release;
      continue;
    }
    if (!isProvenCompatible(release)) {
      continue;
    }
    if (newerUnknown) {
      return refuse(
        input.expectation,
        'release_metadata_not_proven',
        `A newer release (${newerUnknown.tagName}) has unresolved compatibility or target evidence.`,
        { blockingReleaseId: newerUnknown.releaseId },
      );
    }
    return selected(
      input,
      release,
      input.channel === 'stable' ? 'stable_newest_compatible' : 'latest_newest_compatible',
    );
  }

  if (newerUnknown) {
    return refuse(
      input.expectation,
      'release_metadata_not_proven',
      `Release ${newerUnknown.tagName} has unresolved compatibility or target evidence.`,
      { blockingReleaseId: newerUnknown.releaseId },
    );
  }

  return refuse(
    input.expectation,
    'no_compatible_release',
    `No compatible ${input.channel} release is available for target ${input.expectation.target}.`,
  );
}
