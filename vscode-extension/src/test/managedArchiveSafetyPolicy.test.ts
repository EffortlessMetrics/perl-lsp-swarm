import {
  MANAGED_ARCHIVE_MAX_COMPRESSED_BYTES,
  MANAGED_ARCHIVE_MAX_ENTRIES,
  MANAGED_ARCHIVE_MAX_ENTRY_BYTES,
  MANAGED_ARCHIVE_MAX_PATH_BYTES,
  MANAGED_ARCHIVE_MAX_PATH_DEPTH,
  MANAGED_ARCHIVE_MAX_UNCOMPRESSED_BYTES,
  MANAGED_CHECKSUM_FILE_MAX_BYTES,
  VSCODE_MANAGED_ARCHIVE_SAFETY_POLICY_ID,
  VSCODE_MANAGED_ARCHIVE_SAFETY_SCHEMA_VERSION,
  caseFoldIdentity,
  classifyManagedExecutableBasename,
  managedArchiveSafetyLimits,
  normalizeManagedArchiveMemberPath,
} from '../managedArchiveSafetyPolicy';

describe('vscode-managed-archive-safety.v1', () => {
  test('pins a versioned policy identity so limit changes are reviewable', () => {
    expect(VSCODE_MANAGED_ARCHIVE_SAFETY_POLICY_ID).toBe('vscode-managed-archive-safety.v1');
    expect(VSCODE_MANAGED_ARCHIVE_SAFETY_SCHEMA_VERSION).toBe(1);
  });

  test('keeps production limits generous enough for current cargo-dist artifacts', () => {
    const limits = managedArchiveSafetyLimits();
    expect(limits.maxCompressedBytes).toBe(MANAGED_ARCHIVE_MAX_COMPRESSED_BYTES);
    expect(limits.maxUncompressedBytes).toBe(MANAGED_ARCHIVE_MAX_UNCOMPRESSED_BYTES);
    expect(limits.maxEntryBytes).toBe(MANAGED_ARCHIVE_MAX_ENTRY_BYTES);
    expect(limits.maxEntries).toBe(MANAGED_ARCHIVE_MAX_ENTRIES);
    expect(limits.maxPathBytes).toBe(MANAGED_ARCHIVE_MAX_PATH_BYTES);
    expect(limits.maxPathDepth).toBe(MANAGED_ARCHIVE_MAX_PATH_DEPTH);
    expect(limits.maxCompressedBytes).toBe(256 * 1024 * 1024);
    expect(limits.maxUncompressedBytes).toBe(512 * 1024 * 1024);
    expect(MANAGED_CHECKSUM_FILE_MAX_BYTES).toBe(1024 * 1024);
    expect(MANAGED_CHECKSUM_FILE_MAX_BYTES).toBeLessThan(limits.maxCompressedBytes);
  });

  test('classifies documented server and DAP names and ignores other files', () => {
    expect(classifyManagedExecutableBasename('perllsp', false)).toBe('server');
    expect(classifyManagedExecutableBasename('perl-lsp', false)).toBe('server');
    expect(classifyManagedExecutableBasename('perl-dap', false)).toBe('dap');
    expect(classifyManagedExecutableBasename('perllsp.exe', true)).toBe('server');
    expect(classifyManagedExecutableBasename('perl-lsp.exe', true)).toBe('server');
    expect(classifyManagedExecutableBasename('perl-dap.exe', true)).toBe('dap');
    expect(classifyManagedExecutableBasename('README.md', false)).toBeNull();
    expect(classifyManagedExecutableBasename('perllsp', true)).toBeNull();
    expect(classifyManagedExecutableBasename('perllsp.exe', false)).toBeNull();
  });

  test('accepts a cargo-dist nested package path', () => {
    expect(
      normalizeManagedArchiveMemberPath(
        'perllsp-0.17.0-x86_64-unknown-linux-gnu/perllsp',
        managedArchiveSafetyLimits(),
      ),
    ).toEqual(['perllsp-0.17.0-x86_64-unknown-linux-gnu', 'perllsp']);
  });

  test('rejects traversal, absolute, drive, backslash, ADS, reserved, and charset escapes', () => {
    const limits = managedArchiveSafetyLimits();
    expect(() => normalizeManagedArchiveMemberPath('../outside', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('/tmp/evil', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('C:/Windows/evil', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('dir\\perllsp', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('perllsp:stream', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('CON.txt', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('aux', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('perllsp.', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('perllsp ', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('perllsp/./bin', limits)).toThrow(
      /unsafe archive member path/,
    );
    expect(() => normalizeManagedArchiveMemberPath('ok/name with space', limits)).toThrow(
      /unsafe archive member path/,
    );
  });

  test('folds path identity case-insensitively', () => {
    expect(caseFoldIdentity(['Pkg', 'Perllsp'])).toBe('pkg/perllsp');
  });
});
