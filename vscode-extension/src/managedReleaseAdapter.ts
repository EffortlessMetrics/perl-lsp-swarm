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
  ManagedReleaseChannel,
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

/**
 * Mirrors `managedReleaseSelector`'s SEMVER_PATTERN exactly. The adapter must
 * be at least as strict as the selector: a record this adapter judges
 * parseable reaches the selector's own parse, and any disagreement there
 * fails the whole input set closed.
 */
const SEMVER_PATTERN =
  /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

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
 * - anything other than an explicit `prerelease: false` counts as a claimed
 *   prerelease;
 * - target availability is evidence from the release's own asset list, not
 *   assumed: `available` only when a candidate asset name is present.
 *
 * Quarantine (real release history is unbounded, unlike the selector's
 * curated-input contract): a record whose prerelease claim disagrees with
 * its parsed semver — a historical mistag — has its flag aligned to the
 * parsed tag. On the recency-ordered channels (`stable`/`latest`) it is
 * additionally demoted to `not_proven` compatibility: never selectable, and
 * blocking selection only when actually newer than every proven candidate
 * (the selector's newer-unknown rule), so one bad historical record cannot
 * poison every managed route (#9925 hosted smoke: v0.13.1 mistagged
 * prerelease). On the exact-tag channel there is no recency ordering to
 * protect: an explicit pin is the user choosing one specific artifact, so a
 * mistagged pinned release keeps its train-lineage compatibility evidence
 * and remains installable, exactly as before this wiring.
 */
export function toManagedReleaseRecord(
  release: TransportRelease,
  expectation: ManagedReleaseExpectation,
  assetTargetCandidates: readonly string[],
  archiveExtension: string,
  findAsset: ReleaseAssetMatcher,
  channel: ManagedReleaseChannel,
): ManagedReleaseRecord {
  const matchedAsset = assetTargetCandidates
    .map((target) => findAsset(release.assets, release.tag_name, target, archiveExtension))
    .find((name) => name !== undefined);
  const targetState: ManagedTargetState = matchedAsset === undefined ? 'unavailable' : 'available';
  const parsed = SEMVER_PATTERN.exec(release.tag_name);
  const claimedPrerelease = release.prerelease !== false;
  const actualPrerelease = parsed?.[4] !== undefined;
  const mistagged = parsed !== null && claimedPrerelease !== actualPrerelease;
  const demote = mistagged && channel !== 'tag';
  return {
    releaseId: `release:${release.tag_name}`,
    candidateId: `candidate:${release.tag_name}`,
    tagName: release.tag_name,
    version: release.tag_name,
    prerelease: mistagged ? actualPrerelease : claimedPrerelease,
    draft: release.draft === true,
    compatibilityRequirement: expectation.compatibilityRequirement,
    compatibilityState: demote ? 'not_proven' : 'compatible',
    compatibilityEvidenceRef: demote ? undefined : `release-train:${release.tag_name}`,
    target: expectation.target,
    targetState,
    targetEvidenceRef: `release-assets:${release.tag_name}`,
  };
}

export interface ManagedReleaseRecordConversion {
  readonly records: ManagedReleaseRecord[];
  /** Tags quarantined out of the candidate set entirely (unparseable semver). */
  readonly droppedTags: string[];
}

export function toManagedReleaseRecords(
  releases: readonly TransportRelease[],
  expectation: ManagedReleaseExpectation,
  assetTargetCandidates: readonly string[],
  archiveExtension: string,
  findAsset: ReleaseAssetMatcher,
  channel: ManagedReleaseChannel,
): ManagedReleaseRecordConversion {
  const records: ManagedReleaseRecord[] = [];
  const droppedTags: string[] = [];
  for (const release of releases) {
    if (!SEMVER_PATTERN.test(release.tag_name)) {
      // Unparseable tags cannot be ordered by the selector at all; their
      // recency is unknowable, so they neither satisfy nor block selection.
      droppedTags.push(release.tag_name);
      continue;
    }
    records.push(
      toManagedReleaseRecord(
        release,
        expectation,
        assetTargetCandidates,
        archiveExtension,
        findAsset,
        channel,
      ),
    );
  }
  return { records, droppedTags };
}
