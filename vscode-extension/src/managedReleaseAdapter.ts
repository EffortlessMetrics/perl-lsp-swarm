/**
 * Bounded-transport → managed-release-selector adapter (#9925).
 *
 * Owns exactly one thing: converting release metadata fetched through the
 * downloader's bounded transport into the selector's closed input model, so
 * first-use, repair, and update routes all select through
 * `selectManagedRelease` instead of route-local `/releases/latest`,
 * first-element, or convenience-endpoint semantics.
 *
 * Explicit non-ownership:
 * - HTTP/proxy/TLS transport stays in `downloader.ts` (#6018/#7851);
 * - artifact/member resolution stays with the asset-selection path (#9020);
 * - candidate publication/cache schema is #7857/#7858;
 * - per-release protocol compatibility data is #6854. Until it lands, the
 *   honest compatibility evidence is same-train release lineage: the release
 *   is published by the same repository train that built this extension. It
 *   is never inferred from version equality.
 *
 * Windows ARM64 note: `targetState` there means "this release can serve this
 * host" — native ARM64 asset or the documented x64-emulation asset. The
 * per-release native-versus-emulation choice stays with
 * `selectWindowsArm64Target` after selection (#9844/#10001 boundary).
 */

import type {
  ManagedReleaseExpectation,
  ManagedReleaseRecord,
  ManagedTargetState,
} from './managedReleaseSelector';

/**
 * The compatibility contract today's managed train can honestly assert: the
 * server release is published by the extension's own release pipeline. Real
 * per-release protocol compatibility rows arrive with #6854.
 */
export const MANAGED_RELEASE_COMPATIBILITY_REQUIREMENT = 'perl-lsp-managed-train.v1';

/** Minimal transport shape the adapter consumes (a subset of Release). */
export interface TransportRelease {
  readonly tag_name: string;
  readonly prerelease?: boolean | undefined;
  readonly draft?: boolean | undefined;
  readonly assets: ReadonlyArray<{ readonly name: string }>;
}

/** Asset-name lookup injected by the caller so this module stays pure. */
export type ReleaseAssetMatcher = (
  assets: ReadonlyArray<{ readonly name: string }>,
  versionOrTag: string,
  target: string,
  archiveExtension: string,
) => string | undefined;

export interface ManagedReleaseExpectationFacts {
  readonly extensionId: string;
  readonly extensionVersion: string;
  readonly hostTarget: string;
}

export function buildManagedReleaseExpectation(
  facts: ManagedReleaseExpectationFacts,
): ManagedReleaseExpectation {
  return {
    extensionCandidateId: `${facts.extensionId}@${facts.extensionVersion}`,
    compatibilityRequirement: MANAGED_RELEASE_COMPATIBILITY_REQUIREMENT,
    target: facts.hostTarget,
    extensionTrack: facts.extensionVersion.includes('-') ? 'prerelease' : 'stable',
  };
}

/**
 * Convert one transport release into the selector's record model.
 *
 * Fail-closed mappings:
 * - anything other than an explicit `prerelease: false` counts as a
 *   prerelease (a stable-version record then disagrees with its parsed
 *   semver and the selector refuses the metadata, exactly the fail-closed
 *   behavior the stable channel has always had);
 * - target availability is evidence from the release's own asset list, not
 *   assumed: `available` only when a candidate asset name is present.
 */
export function toManagedReleaseRecord(
  release: TransportRelease,
  expectation: ManagedReleaseExpectation,
  assetTargetCandidates: readonly string[],
  archiveExtension: string,
  findAsset: ReleaseAssetMatcher,
): ManagedReleaseRecord {
  const matchedAsset = assetTargetCandidates
    .map((target) => findAsset(release.assets, release.tag_name, target, archiveExtension))
    .find((name) => name !== undefined);
  const targetState: ManagedTargetState = matchedAsset === undefined ? 'unavailable' : 'available';
  return {
    releaseId: `release:${release.tag_name}`,
    candidateId: `candidate:${release.tag_name}`,
    tagName: release.tag_name,
    version: release.tag_name,
    prerelease: release.prerelease !== false,
    draft: release.draft === true,
    compatibilityRequirement: expectation.compatibilityRequirement,
    compatibilityState: 'compatible',
    compatibilityEvidenceRef: `release-train:${release.tag_name}`,
    target: expectation.target,
    targetState,
    targetEvidenceRef: `release-assets:${release.tag_name}`,
  };
}

export function toManagedReleaseRecords(
  releases: readonly TransportRelease[],
  expectation: ManagedReleaseExpectation,
  assetTargetCandidates: readonly string[],
  archiveExtension: string,
  findAsset: ReleaseAssetMatcher,
): ManagedReleaseRecord[] {
  return releases.map((release) =>
    toManagedReleaseRecord(
      release,
      expectation,
      assetTargetCandidates,
      archiveExtension,
      findAsset,
    ),
  );
}
