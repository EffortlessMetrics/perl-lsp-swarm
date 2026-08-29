import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as zlib from 'zlib';
import { extractManagedArchive } from '../managedArchiveExtract';
import type { CancellationTokenLike, DisposableLike } from '../boundedHttpJson';
import type { ManagedArchiveSafetyLimits } from '../managedArchiveSafetyPolicy';

class TestCancellationToken implements CancellationTokenLike {
  isCancellationRequested = false;
  private readonly listeners = new Set<() => void>();

  onCancellationRequested(listener: () => void): DisposableLike {
    this.listeners.add(listener);
    return {
      dispose: () => {
        this.listeners.delete(listener);
      },
    };
  }

  cancel(): void {
    this.isCancellationRequested = true;
    for (const listener of [...this.listeners]) {
      listener();
    }
  }
}

const TEST_LIMITS: ManagedArchiveSafetyLimits = {
  maxCompressedBytes: 64 * 1024,
  maxUncompressedBytes: 64,
  maxEntryBytes: 48,
  maxEntries: 4,
  maxPathBytes: 255,
  maxPathDepth: 3,
};

function octalField(value: number, length: number): string {
  return `${value.toString(8).padStart(length - 1, '0')}\0`;
}

function checksumUstar(header: Buffer): number {
  let sum = 0;
  for (const byte of header) {
    sum += byte;
  }
  return sum;
}

function ustarHeader(opts: {
  name: string;
  size: number;
  type: string;
  linkname?: string;
}): Buffer {
  const header = Buffer.alloc(512);
  header.write(opts.name, 0, 100, 'utf8');
  header.write(octalField(0o644, 8), 100, 8, 'ascii');
  header.write(octalField(0, 8), 108, 8, 'ascii');
  header.write(octalField(0, 8), 116, 8, 'ascii');
  header.write(octalField(opts.size, 12), 124, 12, 'ascii');
  header.write(octalField(Math.floor(Date.now() / 1000), 12), 136, 12, 'ascii');
  header.fill(0x20, 148, 156);
  header.write(opts.type, 156, 1, 'ascii');
  if (opts.linkname !== undefined) {
    header.write(opts.linkname, 157, 100, 'utf8');
  }
  header.write('ustar\0', 257, 6, 'ascii');
  header.write('00', 263, 2, 'ascii');
  const sum = checksumUstar(header);
  header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8, 'ascii');
  return header;
}

function tarBytes(
  entries: ReadonlyArray<{ name: string; type: string; content?: string; linkname?: string }>,
): Buffer {
  const parts: Buffer[] = [];
  for (const entry of entries) {
    const content = Buffer.from(entry.content ?? '', 'utf8');
    const size = entry.type === '0' ? content.length : 0;
    const headerArgs =
      entry.linkname === undefined
        ? { name: entry.name, size, type: entry.type }
        : { name: entry.name, size, type: entry.type, linkname: entry.linkname };
    parts.push(ustarHeader(headerArgs));
    if (entry.type === '0') {
      parts.push(content);
      const pad = (512 - (content.length % 512)) % 512;
      if (pad > 0) {
        parts.push(Buffer.alloc(pad));
      }
    }
  }
  parts.push(Buffer.alloc(1024));
  return Buffer.concat(parts);
}

function writeTarGz(
  dest: string,
  entries: ReadonlyArray<{ name: string; type: string; content?: string; linkname?: string }>,
): void {
  fs.writeFileSync(dest, zlib.gzipSync(tarBytes(entries)));
}

function storedZip(entries: ReadonlyArray<[string, string]>, uncompressedLies?: number[]): Buffer {
  const locals: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  entries.forEach(([name, contents], index) => {
    const nameBytes = Buffer.from(name, 'utf8');
    const data = Buffer.from(contents, 'utf8');
    const crc = zlib.crc32(data);
    const declaredUncompressed = uncompressedLies?.[index] ?? data.length;

    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, data);

    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt32LE(crc, 16);
    header.writeUInt32LE(data.length, 20);
    header.writeUInt32LE(declaredUncompressed, 24);
    header.writeUInt16LE(nameBytes.length, 28);
    header.writeUInt32LE(offset, 42);
    central.push(header, nameBytes);
    offset += local.length + nameBytes.length + data.length;
  });

  const centralBytes = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, centralBytes, end]);
}

function unixSymlinkZip(name: string, target: string): Buffer {
  const nameBytes = Buffer.from(name, 'utf8');
  const data = Buffer.from(target, 'utf8');
  const crc = zlib.crc32(data);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(data.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(nameBytes.length, 26);
  const header = Buffer.alloc(46);
  header.writeUInt32LE(0x02014b50, 0);
  header.writeUInt16LE((3 << 8) | 20, 4); // UNIX made-by
  header.writeUInt16LE(20, 6);
  header.writeUInt32LE(crc, 16);
  header.writeUInt32LE(data.length, 20);
  header.writeUInt32LE(data.length, 24);
  header.writeUInt16LE(nameBytes.length, 28);
  header.writeUInt32LE(0xa0000000, 38); // S_IFLNK in high 16 bits
  header.writeUInt32LE(0, 42);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(1, 8);
  end.writeUInt16LE(1, 10);
  end.writeUInt32LE(header.length + nameBytes.length, 12);
  end.writeUInt32LE(local.length + nameBytes.length + data.length, 16);
  return Buffer.concat([local, nameBytes, data, header, nameBytes, end]);
}

function windowsReparseZip(name: string, target: string): Buffer {
  const nameBytes = Buffer.from(name, 'utf8');
  const data = Buffer.from(target, 'utf8');
  const crc = zlib.crc32(data);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(data.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(nameBytes.length, 26);
  const header = Buffer.alloc(46);
  header.writeUInt32LE(0x02014b50, 0);
  header.writeUInt16LE((11 << 8) | 20, 4); // NTFS made-by
  header.writeUInt16LE(20, 6);
  header.writeUInt32LE(crc, 16);
  header.writeUInt32LE(data.length, 20);
  header.writeUInt32LE(data.length, 24);
  header.writeUInt16LE(nameBytes.length, 28);
  header.writeUInt32LE(0x00000400, 38); // FILE_ATTRIBUTE_REPARSE_POINT
  header.writeUInt32LE(0, 42);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(1, 8);
  end.writeUInt16LE(1, 10);
  end.writeUInt32LE(header.length + nameBytes.length, 12);
  end.writeUInt32LE(local.length + nameBytes.length + data.length, 16);
  return Buffer.concat([local, nameBytes, data, header, nameBytes, end]);
}

const VALID_POSIX = [
  { name: 'perllsp-0.17.0-x86_64-unknown-linux-gnu/', type: '5' },
  {
    name: 'perllsp-0.17.0-x86_64-unknown-linux-gnu/perllsp',
    type: '0',
    content: 'server-bytes',
  },
  {
    name: 'perllsp-0.17.0-x86_64-unknown-linux-gnu/perl-dap',
    type: '0',
    content: 'dap-bytes',
  },
  { name: 'perllsp-0.17.0-x86_64-unknown-linux-gnu/README.md', type: '0', content: 'docs' },
];

describe('extractManagedArchive', () => {
  let tmpDir: string;
  let extractDir: string;
  let sentinelPath: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'managed-ar-'));
    extractDir = path.join(tmpDir, 'extracted');
    sentinelPath = path.join(tmpDir, 'outside-sentinel');
    fs.writeFileSync(sentinelPath, 'untouched');
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  function assertOutsideUnchanged(): void {
    expect(fs.readFileSync(sentinelPath, 'utf8')).toBe('untouched');
    expect(fs.existsSync(path.join(tmpDir, 'outside'))).toBe(false);
    expect(fs.existsSync(extractDir)).toBe(false);
  }

  test('extracts only the documented executables from a nested tar.gz', async () => {
    const archivePath = path.join(tmpDir, 'ok.tar.gz');
    writeTarGz(archivePath, VALID_POSIX);
    const result = await extractManagedArchive({
      archivePath,
      extractDir,
      format: 'tar.gz',
      windows: false,
      limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntryBytes: 256, maxEntries: 8 },
    });
    expect(fs.readFileSync(result.serverPath, 'utf8')).toBe('server-bytes');
    expect(result.dapPath && fs.readFileSync(result.dapPath, 'utf8')).toBe('dap-bytes');
    expect(fs.existsSync(path.join(extractDir, 'README.md'))).toBe(false);
    expect(fs.readdirSync(extractDir).sort()).toEqual(['perl-dap', 'perllsp']);
    expect(fs.readFileSync(sentinelPath, 'utf8')).toBe('untouched');
  });

  test('extracts only the documented executables from a windows zip', async () => {
    const archivePath = path.join(tmpDir, 'ok.zip');
    fs.writeFileSync(
      archivePath,
      storedZip([
        ['perllsp.exe', 'win-server'],
        ['perl-dap.exe', 'win-dap'],
        ['README.md', 'docs'],
      ]),
    );
    const result = await extractManagedArchive({
      archivePath,
      extractDir,
      format: 'zip',
      windows: true,
      limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntryBytes: 256, maxEntries: 8 },
    });
    expect(fs.readFileSync(result.serverPath, 'utf8')).toBe('win-server');
    expect(result.dapPath && fs.readFileSync(result.dapPath, 'utf8')).toBe('win-dap');
    expect(fs.readdirSync(extractDir).sort()).toEqual(['perl-dap.exe', 'perllsp.exe']);
  });

  test('rejects a tiny tar.gz that expands past the cumulative uncompressed ceiling', async () => {
    const archivePath = path.join(tmpDir, 'bomb.tar.gz');
    writeTarGz(archivePath, [{ name: 'perllsp', type: '0', content: 'x'.repeat(65) }]);
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: TEST_LIMITS,
      }),
    ).rejects.toThrow(/exceeds 64 uncompressed bytes|exceeds 48 bytes/);
    assertOutsideUnchanged();
  });

  test('rejects an archive past the entry-count ceiling', async () => {
    const archivePath = path.join(tmpDir, 'many.tar.gz');
    writeTarGz(archivePath, [
      { name: 'perllsp', type: '0', content: 'srv' },
      { name: 'a', type: '0', content: '1' },
      { name: 'b', type: '0', content: '2' },
      { name: 'c', type: '0', content: '3' },
      { name: 'd', type: '0', content: '4' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 4 },
      }),
    ).rejects.toThrow('exceeds 4 entries');
    assertOutsideUnchanged();
  });

  test('rejects a zip past the entry-count ceiling before extracting members', async () => {
    const archivePath = path.join(tmpDir, 'many.zip');
    fs.writeFileSync(
      archivePath,
      storedZip([
        ['perllsp.exe', 'srv'],
        ['a.txt', '1'],
        ['b.txt', '2'],
        ['c.txt', '3'],
        ['d.txt', '4'],
      ]),
    );
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'zip',
        windows: true,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 4 },
      }),
    ).rejects.toThrow('exceeds 4 entries');
    assertOutsideUnchanged();
  });

  test('rejects zip64 entry-count sentinels before AdmZip materializes the table', async () => {
    const archivePath = path.join(tmpDir, 'zip64.zip');
    const bytes = Buffer.from(
      storedZip([
        ['perllsp.exe', 'srv'],
        ['README.md', 'docs'],
      ]),
    );
    bytes.writeUInt16LE(0xffff, bytes.length - 14);
    bytes.writeUInt16LE(0xffff, bytes.length - 12);
    fs.writeFileSync(archivePath, bytes);
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'zip',
        windows: true,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow('exceeds 8 entries');
    assertOutsideUnchanged();
  });

  test('rejects parent-path and absolute-path tar members before writing outside the root', async () => {
    const parentArchive = path.join(tmpDir, 'parent.tar.gz');
    writeTarGz(parentArchive, [
      { name: '../outside', type: '0', content: 'escaped' },
      { name: 'perllsp', type: '0', content: 'srv' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath: parentArchive,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/unsafe archive member path/);
    assertOutsideUnchanged();

    const absArchive = path.join(tmpDir, 'abs.tar.gz');
    writeTarGz(absArchive, [
      { name: '/tmp/evil', type: '0', content: 'escaped' },
      { name: 'perllsp', type: '0', content: 'srv' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath: absArchive,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/unsafe archive member path/);
    assertOutsideUnchanged();
  });

  test('rejects a zip member that escapes with ..', async () => {
    const archivePath = path.join(tmpDir, 'escape.zip');
    fs.writeFileSync(
      archivePath,
      storedZip([
        ['perllsp.exe', 'srv'],
        ['../outside', 'escaped'],
      ]),
    );
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'zip',
        windows: true,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/unsafe archive member path/);
    assertOutsideUnchanged();
  });

  test('rejects tar symlinks and hardlinks that point outside the extraction root', async () => {
    const symlinkArchive = path.join(tmpDir, 'sym.tar.gz');
    writeTarGz(symlinkArchive, [
      { name: 'perllsp', type: '0', content: 'srv' },
      { name: 'link-out', type: '2', linkname: '../outside-sentinel' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath: symlinkArchive,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/archive links are not accepted/);
    assertOutsideUnchanged();

    const hardlinkArchive = path.join(tmpDir, 'hard.tar.gz');
    writeTarGz(hardlinkArchive, [
      { name: 'perllsp', type: '0', content: 'srv' },
      { name: 'alias', type: '1', linkname: '../outside-sentinel' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath: hardlinkArchive,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/archive links are not accepted/);
    assertOutsideUnchanged();
  });

  test('rejects a zip unix symlink entry', async () => {
    const archivePath = path.join(tmpDir, 'sym.zip');
    fs.writeFileSync(archivePath, unixSymlinkZip('perllsp.exe', '../outside-sentinel'));
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'zip',
        windows: true,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/archive links are not accepted/);
    assertOutsideUnchanged();
  });

  test('rejects a zip windows reparse/junction member before writing', async () => {
    const archivePath = path.join(tmpDir, 'reparse.zip');
    fs.writeFileSync(archivePath, windowsReparseZip('perllsp.exe', '../outside-sentinel'));
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'zip',
        windows: true,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/archive links are not accepted/);
    assertOutsideUnchanged();
  });

  test('rejects extractDir when it is a host symlink or junction alias', async () => {
    const decoy = path.join(tmpDir, 'decoy');
    fs.mkdirSync(decoy);
    try {
      fs.symlinkSync(decoy, extractDir, process.platform === 'win32' ? 'junction' : undefined);
    } catch (error) {
      const err = error as NodeJS.ErrnoException;
      if (process.platform === 'win32' && (err.code === 'EPERM' || err.errno === 1314)) {
        process.stderr.write(
          'skipping Windows junction fixture: SeCreateSymbolicLinkPrivilege is not held\n',
        );
        return;
      }
      throw error;
    }
    expect(fs.lstatSync(extractDir).isSymbolicLink()).toBe(true);

    const archivePath = path.join(tmpDir, 'ok.tar.gz');
    writeTarGz(archivePath, VALID_POSIX);
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntryBytes: 256, maxEntries: 8 },
      }),
    ).rejects.toThrow(/symlink or reparse point/);
    expect(fs.readFileSync(sentinelPath, 'utf8')).toBe('untouched');
    expect(fs.existsSync(path.join(decoy, 'perllsp'))).toBe(false);
    expect(fs.existsSync(path.join(decoy, 'perl-dap'))).toBe(false);
  });

  test('rejects a FIFO tar entry', async () => {
    const archivePath = path.join(tmpDir, 'fifo.tar.gz');
    writeTarGz(archivePath, [
      { name: 'perllsp', type: '0', content: 'srv' },
      { name: 'pipe', type: '6' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/archive special entry types are not accepted/);
    assertOutsideUnchanged();
  });

  test('fails duplicate and case-colliding perllsp identities instead of traversal order', async () => {
    const duplicateArchive = path.join(tmpDir, 'dup.tar.gz');
    writeTarGz(duplicateArchive, [
      { name: 'pkg/perllsp', type: '0', content: 'one' },
      { name: 'other/perl-lsp', type: '0', content: 'two' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath: duplicateArchive,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/ambiguous executable identity/);
    assertOutsideUnchanged();

    const caseArchive = path.join(tmpDir, 'case.tar.gz');
    writeTarGz(caseArchive, [
      { name: 'pkg/perllsp', type: '0', content: 'one' },
      { name: 'pkg/Perllsp', type: '0', content: 'two' },
    ]);
    await expect(
      extractManagedArchive({
        archivePath: caseArchive,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntries: 8 },
      }),
    ).rejects.toThrow(/case-colliding archive member|ambiguous executable identity/);
    assertOutsideUnchanged();
  });

  test('rejects cancellation during extraction and leaves no extracted candidate', async () => {
    const archivePath = path.join(tmpDir, 'ok.tar.gz');
    writeTarGz(archivePath, VALID_POSIX);
    const token = new TestCancellationToken();
    token.cancel();
    await expect(
      extractManagedArchive({
        archivePath,
        extractDir,
        format: 'tar.gz',
        windows: false,
        limits: { ...TEST_LIMITS, maxUncompressedBytes: 1024, maxEntryBytes: 256, maxEntries: 8 },
        cancellationToken: token,
      }),
    ).rejects.toThrow('Archive extraction cancelled');
    assertOutsideUnchanged();
  });

  test('admits a fixture at the injected uncompressed ceiling', async () => {
    const archivePath = path.join(tmpDir, 'near.tar.gz');
    writeTarGz(archivePath, [{ name: 'perllsp', type: '0', content: 'x'.repeat(48) }]);
    const result = await extractManagedArchive({
      archivePath,
      extractDir,
      format: 'tar.gz',
      windows: false,
      limits: { ...TEST_LIMITS, maxUncompressedBytes: 48, maxEntryBytes: 48, maxEntries: 4 },
    });
    expect(fs.readFileSync(result.serverPath, 'utf8')).toHaveLength(48);
    expect(result.dapPath).toBeNull();
  });
});
