/**
 * Checked VS Code projection of the canonical release workflow target matrix.
 *
 * The source of truth remains `.github/workflows/release.yml`. The focused
 * projection test parses that matrix and rejects any drift, so runtime target
 * decisions never maintain an unobserved second list inside downloader.ts.
 */
export const RELEASE_TOPOLOGY_SOURCE =
  '.github/workflows/release.yml#jobs.build.strategy.matrix.include';

export const RELEASE_TOPOLOGY_MANAGED_TARGETS = [
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
  'x86_64-unknown-linux-musl',
  'aarch64-unknown-linux-musl',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
  'x86_64-pc-windows-msvc',
  'aarch64-pc-windows-msvc',
] as const;

export type ReleaseTopologyManagedTarget = (typeof RELEASE_TOPOLOGY_MANAGED_TARGETS)[number];

export const RELEASE_TOPOLOGY_MANAGED_TARGET_SET: ReadonlySet<string> = new Set(
  RELEASE_TOPOLOGY_MANAGED_TARGETS,
);
