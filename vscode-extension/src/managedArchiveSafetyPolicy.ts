/**
 * Versioned resource envelope for VS Code managed-install archives (#7432).
 *
 * Numeric limits match the standalone installer sibling
 * (`policy/standalone-archive-safety.v1.toml`, #8352) so currently supported
 * cargo-dist artifacts fit, but this module is the managed-extension authority.
 * Transports stay uncoupled. Changing a limit is a reviewable policy edit.
 *
 * Does not cover release-JSON metadata (#6018), provenance (#7425),
 * transaction schemas (#11099), or owned-state manifests (#11470).
 */

export const VSCODE_MANAGED_ARCHIVE_SAFETY_POLICY_ID = 'vscode-managed-archive-safety.v1';
export const VSCODE_MANAGED_ARCHIVE_SAFETY_SCHEMA_VERSION = 1;

/** 256 MiB. Independent of the download timeout. */
export const MANAGED_ARCHIVE_MAX_COMPRESSED_BYTES = 268435456;
/** 512 MiB cumulative uncompressed payload. */
export const MANAGED_ARCHIVE_MAX_UNCOMPRESSED_BYTES = 536870912;
/** 256 MiB per archive member. */
export const MANAGED_ARCHIVE_MAX_ENTRY_BYTES = 268435456;
/** cargo-dist package trees are far smaller; keep a hard inode/work cap. */
export const MANAGED_ARCHIVE_MAX_ENTRIES = 32;
export const MANAGED_ARCHIVE_MAX_PATH_BYTES = 255;
export const MANAGED_ARCHIVE_MAX_PATH_DEPTH = 3;

/**
 * SHA256SUMS is a short text catalog, not an archive. Cap it separately so a
 * huge checksum response cannot fill disk under the archive ceiling.
 */
export const MANAGED_CHECKSUM_FILE_MAX_BYTES = 1024 * 1024;

const WINDOWS_RESERVED_DEVICE = /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$/i;
const ALLOWED_COMPONENT = /^[A-Za-z0-9._-]+$/;

export interface ManagedArchiveSafetyLimits {
  maxCompressedBytes: number;
  maxUncompressedBytes: number;
  maxEntryBytes: number;
  maxEntries: number;
  maxPathBytes: number;
  maxPathDepth: number;
}

export function managedArchiveSafetyLimits(): ManagedArchiveSafetyLimits {
  return {
    maxCompressedBytes: MANAGED_ARCHIVE_MAX_COMPRESSED_BYTES,
    maxUncompressedBytes: MANAGED_ARCHIVE_MAX_UNCOMPRESSED_BYTES,
    maxEntryBytes: MANAGED_ARCHIVE_MAX_ENTRY_BYTES,
    maxEntries: MANAGED_ARCHIVE_MAX_ENTRIES,
    maxPathBytes: MANAGED_ARCHIVE_MAX_PATH_BYTES,
    maxPathDepth: MANAGED_ARCHIVE_MAX_PATH_DEPTH,
  };
}

const POSIX_SERVER_NAMES = new Set(['perllsp', 'perl-lsp']);
const WINDOWS_SERVER_NAMES = new Set(['perllsp.exe', 'perl-lsp.exe']);
const POSIX_DAP_NAMES = new Set(['perl-dap']);
const WINDOWS_DAP_NAMES = new Set(['perl-dap.exe']);

export type ManagedExecutableKind = 'server' | 'dap';

export function classifyManagedExecutableBasename(
  basename: string,
  windows: boolean,
): ManagedExecutableKind | null {
  const folded = basename.toLowerCase();
  if (windows) {
    if (WINDOWS_SERVER_NAMES.has(folded)) {
      return 'server';
    }
    if (WINDOWS_DAP_NAMES.has(folded)) {
      return 'dap';
    }
    return null;
  }
  if (POSIX_SERVER_NAMES.has(folded)) {
    return 'server';
  }
  if (POSIX_DAP_NAMES.has(folded)) {
    return 'dap';
  }
  return null;
}

export function installedServerBasename(windows: boolean): string {
  return windows ? 'perllsp.exe' : 'perllsp';
}

export function installedDapBasename(windows: boolean): string {
  return windows ? 'perl-dap.exe' : 'perl-dap';
}

/**
 * Normalize one archive member path under the managed-install envelope.
 * Returns POSIX components contained under the extraction root.
 */
export function normalizeManagedArchiveMemberPath(
  raw: string,
  limits: ManagedArchiveSafetyLimits,
): string[] {
  if (raw.includes('\\')) {
    throw new Error(`unsafe archive member path: ${raw}`);
  }
  if (raw.includes('\0')) {
    throw new Error(`unsafe archive member path: ${raw}`);
  }
  const trimmed = raw.replace(/\/+$/, '');
  if (!trimmed) {
    throw new Error(`unsafe archive member path: ${raw}`);
  }
  if (trimmed.startsWith('/') || trimmed.startsWith('//')) {
    throw new Error(`unsafe archive member path: ${raw}`);
  }
  if (/^[A-Za-z]:/.test(trimmed) || trimmed.startsWith('\\\\')) {
    throw new Error(`unsafe archive member path: ${raw}`);
  }
  if (Buffer.byteLength(trimmed, 'utf8') > limits.maxPathBytes) {
    throw new Error(`archive member path exceeds ${limits.maxPathBytes} bytes: ${raw}`);
  }

  const components = trimmed.split('/');
  if (components.length > limits.maxPathDepth) {
    throw new Error(`archive member path exceeds depth ${limits.maxPathDepth}: ${raw}`);
  }

  for (const component of components) {
    if (component === '' || component === '.' || component === '..') {
      throw new Error(`unsafe archive member path: ${raw}`);
    }
    if (component.includes(':')) {
      throw new Error(`unsafe archive member path: ${raw}`);
    }
    if (component.endsWith('.') || component.endsWith(' ')) {
      throw new Error(`unsafe archive member path: ${raw}`);
    }
    if (!ALLOWED_COMPONENT.test(component)) {
      throw new Error(`unsafe archive member path: ${raw}`);
    }
    if (WINDOWS_RESERVED_DEVICE.test(component)) {
      throw new Error(`unsafe archive member path: ${raw}`);
    }
  }

  return components;
}

export function caseFoldIdentity(components: readonly string[]): string {
  return components.map((part) => part.toLowerCase()).join('/');
}
