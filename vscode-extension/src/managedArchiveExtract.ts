import * as fs from 'fs';
import * as path from 'path';
import { pipeline } from 'stream/promises';
import { Transform } from 'stream';
import * as zlib from 'zlib';
import AdmZip from 'adm-zip';
import { Parser } from 'tar';
import type { ReadEntry } from 'tar';
import type { CancellationTokenLike } from './boundedHttpJson';
import {
  caseFoldIdentity,
  classifyManagedExecutableBasename,
  installedDapBasename,
  installedServerBasename,
  managedArchiveSafetyLimits,
  normalizeManagedArchiveMemberPath,
  type ManagedArchiveSafetyLimits,
  type ManagedExecutableKind,
} from './managedArchiveSafetyPolicy';

const TAR_REGULAR_TYPES = new Set(['File', 'OldFile']);
const TAR_DIRECTORY_TYPES = new Set(['Directory']);
const ZIP_STORE = 0;
const ZIP_DEFLATE = 8;
const UNIX_S_IFMT = 0xf000;
const UNIX_S_IFDIR = 0x4000;
const UNIX_S_IFREG = 0x8000;
const UNIX_MADE_UNIX = 3;

export interface ExtractedManagedArchive {
  serverPath: string;
  dapPath: string | null;
  serverMember: string;
  dapMember: string | null;
}

export interface ExtractManagedArchiveOptions {
  archivePath: string;
  extractDir: string;
  format: 'tar.gz' | 'zip';
  windows: boolean;
  limits?: ManagedArchiveSafetyLimits;
  cancellationToken?: CancellationTokenLike;
}

interface InspectedMember {
  originalName: string;
  components: string[];
  size: number;
  kind: 'file' | 'directory';
  executable: ManagedExecutableKind | null;
}

interface InspectedArchive {
  members: InspectedMember[];
  server: InspectedMember;
  dap: InspectedMember | null;
}

class ByteCeilingTransform extends Transform {
  private seen = 0;

  constructor(
    private readonly maxBytes: number,
    private readonly message: string,
  ) {
    super();
  }

  override _transform(
    chunk: Buffer,
    _encoding: BufferEncoding,
    callback: (error?: Error | null, data?: Buffer) => void,
  ): void {
    this.seen += chunk.length;
    if (this.seen > this.maxBytes) {
      callback(new Error(this.message));
      return;
    }
    callback(null, chunk);
  }
}

function throwIfCancelled(token: CancellationTokenLike | undefined, message: string): void {
  if (token?.isCancellationRequested) {
    throw new Error(message);
  }
}

function removeExtractTree(extractDir: string): void {
  fs.rmSync(extractDir, { recursive: true, force: true });
}

function inflatedTarCeiling(limits: ManagedArchiveSafetyLimits): number {
  // ustar spends a 512-byte header plus up to 511 bytes of padding per member
  // in addition to declared file sizes. Keep that overhead inside the gunzip
  // ceiling so inspect can still observe one extra member for the count cap.
  return limits.maxUncompressedBytes + 512 * (limits.maxEntries * 2 + 4);
}

function inspectMember(
  originalName: string,
  size: number,
  kind: 'file' | 'directory' | 'link' | 'special',
  windows: boolean,
  limits: ManagedArchiveSafetyLimits,
): InspectedMember {
  if (kind === 'link') {
    throw new Error(`archive links are not accepted: ${originalName}`);
  }
  if (kind === 'special') {
    throw new Error(`archive special entry types are not accepted: ${originalName}`);
  }
  const components = normalizeManagedArchiveMemberPath(originalName, limits);
  if (kind === 'file') {
    if (size > limits.maxEntryBytes) {
      throw new Error(`archive entry exceeds ${limits.maxEntryBytes} bytes: ${originalName}`);
    }
  }
  const basename = components[components.length - 1];
  if (basename === undefined) {
    throw new Error(`unsafe archive member path: ${originalName}`);
  }
  return {
    originalName,
    components,
    size: kind === 'directory' ? 0 : size,
    kind,
    executable: kind === 'file' ? classifyManagedExecutableBasename(basename, windows) : null,
  };
}

function selectExecutables(
  members: InspectedMember[],
  limits: ManagedArchiveSafetyLimits,
): { server: InspectedMember; dap: InspectedMember | null } {
  if (members.length > limits.maxEntries) {
    throw new Error(`archive exceeds ${limits.maxEntries} entries`);
  }

  let cumulative = 0;
  const normalized = new Set<string>();
  const folded = new Set<string>();
  const servers: InspectedMember[] = [];
  const daps: InspectedMember[] = [];

  for (const member of members) {
    cumulative += member.size;
    if (cumulative > limits.maxUncompressedBytes) {
      throw new Error(`archive exceeds ${limits.maxUncompressedBytes} uncompressed bytes`);
    }
    const joined = member.components.join('/');
    if (normalized.has(joined)) {
      throw new Error(`duplicate archive member: ${joined}`);
    }
    normalized.add(joined);
    const fold = caseFoldIdentity(member.components);
    if (folded.has(fold)) {
      throw new Error(`case-colliding archive member: ${joined}`);
    }
    folded.add(fold);
    if (member.executable === 'server') {
      servers.push(member);
    } else if (member.executable === 'dap') {
      daps.push(member);
    }
  }

  if (servers.length === 0) {
    throw new Error('Binary not found in archive');
  }
  if (servers.length > 1) {
    throw new Error(
      `ambiguous executable identity: ${servers.map((member) => member.originalName).join(', ')}`,
    );
  }
  if (daps.length > 1) {
    throw new Error(
      `ambiguous executable identity: ${daps.map((member) => member.originalName).join(', ')}`,
    );
  }

  const server = servers[0];
  if (server === undefined) {
    throw new Error('Binary not found in archive');
  }
  return { server, dap: daps[0] ?? null };
}

function zipUnixMode(entry: { header: { made: number }; attr: number }): number | null {
  const made = Math.floor(entry.header.made / 256);
  if (made !== UNIX_MADE_UNIX) {
    return null;
  }
  return (entry.attr >>> 16) & 0xffff;
}

function classifyZipEntry(entry: {
  isDirectory: boolean;
  header: { made: number };
  attr: number;
}): 'file' | 'directory' | 'link' | 'special' {
  if (entry.isDirectory) {
    return 'directory';
  }
  const mode = zipUnixMode(entry);
  if (mode === null) {
    return 'file';
  }
  const type = mode & UNIX_S_IFMT;
  if (type === UNIX_S_IFDIR) {
    return 'directory';
  }
  if (type === UNIX_S_IFREG || type === 0) {
    return 'file';
  }
  if (type === 0xa000) {
    return 'link';
  }
  return 'special';
}

function inspectZip(
  archivePath: string,
  windows: boolean,
  limits: ManagedArchiveSafetyLimits,
): InspectedArchive {
  const zip = new AdmZip(archivePath);
  const members: InspectedMember[] = [];
  for (const entry of zip.getEntries()) {
    members.push(
      inspectMember(entry.entryName, entry.header.size, classifyZipEntry(entry), windows, limits),
    );
  }
  const selected = selectExecutables(members, limits);
  return { members, server: selected.server, dap: selected.dap };
}

async function inspectTar(
  archivePath: string,
  windows: boolean,
  limits: ManagedArchiveSafetyLimits,
  token: CancellationTokenLike | undefined,
): Promise<InspectedArchive> {
  const members: InspectedMember[] = [];
  await walkTar(archivePath, limits, token, (entry) => {
    const kind = classifyTarEntry(entry);
    members.push(inspectMember(entry.path, entry.size, kind, windows, limits));
    entry.resume();
  });
  const selected = selectExecutables(members, limits);
  return { members, server: selected.server, dap: selected.dap };
}

function classifyTarEntry(entry: ReadEntry): 'file' | 'directory' | 'link' | 'special' {
  if (TAR_REGULAR_TYPES.has(entry.type)) {
    return 'file';
  }
  if (TAR_DIRECTORY_TYPES.has(entry.type)) {
    return 'directory';
  }
  if (entry.type === 'SymbolicLink' || entry.type === 'Link') {
    return 'link';
  }
  return 'special';
}

async function walkTar(
  archivePath: string,
  limits: ManagedArchiveSafetyLimits,
  token: CancellationTokenLike | undefined,
  onEntry: (entry: ReadEntry) => void,
): Promise<void> {
  throwIfCancelled(token, 'Archive extraction cancelled');
  const parser = new Parser({
    strict: true,
    maxDecompressionRatio: 32,
  });

  parser.on('entry', (entry: ReadEntry) => {
    try {
      throwIfCancelled(token, 'Archive extraction cancelled');
      onEntry(entry);
    } catch (error) {
      const failure = error instanceof Error ? error : new Error(String(error));
      parser.abort(failure);
    }
  });

  const ceiling = new ByteCeilingTransform(
    inflatedTarCeiling(limits),
    `archive exceeds ${limits.maxUncompressedBytes} uncompressed bytes`,
  );

  try {
    await pipeline(fs.createReadStream(archivePath), zlib.createGunzip(), ceiling, parser);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (
      message.includes('cancelled') ||
      message.includes('exceeds') ||
      message.includes('unsafe') ||
      message.includes('accepted') ||
      message.includes('ambiguous') ||
      message.includes('duplicate') ||
      message.includes('case-colliding') ||
      message.includes('Binary not found')
    ) {
      throw error instanceof Error ? error : new Error(message);
    }
    throw new Error(`malformed tar.gz archive: ${message}`);
  }
}

function writeBoundedBuffer(
  destPath: string,
  data: Buffer,
  remaining: number,
  maxEntryBytes: number,
): number {
  if (data.length > maxEntryBytes) {
    throw new Error(`archive entry exceeds ${maxEntryBytes} bytes: ${path.basename(destPath)}`);
  }
  if (data.length > remaining) {
    throw new Error(`archive exceeds remaining uncompressed budget (${remaining} bytes)`);
  }
  fs.mkdirSync(path.dirname(destPath), { recursive: true });
  fs.writeFileSync(destPath, data);
  return data.length;
}

function inflateZipEntry(
  entry: { entryName: string; header: { method: number }; getCompressedData: () => Buffer },
  remaining: number,
  maxEntryBytes: number,
): Buffer {
  const method = entry.header.method;
  const compressed = entry.getCompressedData();
  const budget = Math.min(remaining, maxEntryBytes);
  if (method === ZIP_STORE) {
    if (compressed.length > budget) {
      throw new Error(`archive entry exceeds ${maxEntryBytes} bytes: ${entry.entryName}`);
    }
    return compressed;
  }
  if (method === ZIP_DEFLATE) {
    try {
      return zlib.inflateRawSync(compressed, { maxOutputLength: budget });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(
        `archive entry exceeds ${maxEntryBytes} bytes: ${entry.entryName} (${message})`,
      );
    }
  }
  throw new Error(`unsupported zip compression method ${method}: ${entry.entryName}`);
}

function extractZipMembers(
  archivePath: string,
  extractDir: string,
  inspected: InspectedArchive,
  windows: boolean,
  limits: ManagedArchiveSafetyLimits,
): ExtractedManagedArchive {
  const zip = new AdmZip(archivePath);
  const byName = new Map(zip.getEntries().map((entry) => [entry.entryName, entry]));
  let remaining = limits.maxUncompressedBytes;

  const writeMember = (member: InspectedMember, destName: string): string => {
    const entry = byName.get(member.originalName);
    if (entry === undefined) {
      throw new Error(`archive member missing at extract time: ${member.originalName}`);
    }
    const data = inflateZipEntry(entry, remaining, limits.maxEntryBytes);
    remaining -= writeBoundedBuffer(
      path.join(extractDir, destName),
      data,
      remaining,
      limits.maxEntryBytes,
    );
    return path.join(extractDir, destName);
  };

  const serverPath = writeMember(inspected.server, installedServerBasename(windows));
  const dapPath =
    inspected.dap === null ? null : writeMember(inspected.dap, installedDapBasename(windows));
  return {
    serverPath,
    dapPath,
    serverMember: inspected.server.originalName,
    dapMember: inspected.dap?.originalName ?? null,
  };
}

async function extractTarMembers(
  archivePath: string,
  extractDir: string,
  inspected: InspectedArchive,
  windows: boolean,
  limits: ManagedArchiveSafetyLimits,
  token: CancellationTokenLike | undefined,
): Promise<ExtractedManagedArchive> {
  const wanted = new Map<string, string>();
  wanted.set(inspected.server.originalName, installedServerBasename(windows));
  if (inspected.dap !== null) {
    wanted.set(inspected.dap.originalName, installedDapBasename(windows));
  }

  const pending: Promise<void>[] = [];
  let remaining = limits.maxUncompressedBytes;

  await walkTar(archivePath, limits, token, (entry) => {
    const destName = wanted.get(entry.path);
    if (destName === undefined) {
      entry.resume();
      return;
    }
    const destPath = path.join(extractDir, destName);
    fs.mkdirSync(extractDir, { recursive: true });
    const out = fs.createWriteStream(destPath);
    let written = 0;
    pending.push(
      new Promise<void>((resolve, reject) => {
        const fail = (error: Error): void => {
          out.destroy();
          reject(error);
        };
        entry.on('data', (chunk: Buffer) => {
          written += chunk.length;
          if (written > limits.maxEntryBytes || written > remaining) {
            fail(new Error(`archive entry exceeds ${limits.maxEntryBytes} bytes: ${entry.path}`));
          }
        });
        entry.once('error', (error) => {
          fail(error instanceof Error ? error : new Error(String(error)));
        });
        out.once('error', (error) => {
          fail(error instanceof Error ? error : new Error(String(error)));
        });
        out.once('finish', () => {
          remaining -= written;
          resolve();
        });
        entry.pipe(out);
      }),
    );
  });

  await Promise.all(pending);

  const serverPath = path.join(extractDir, installedServerBasename(windows));
  if (!fs.existsSync(serverPath)) {
    throw new Error('Binary not found in archive');
  }
  const dapDest = path.join(extractDir, installedDapBasename(windows));
  const dapPath = inspected.dap !== null && fs.existsSync(dapDest) ? dapDest : null;
  return {
    serverPath,
    dapPath,
    serverMember: inspected.server.originalName,
    dapMember: inspected.dap?.originalName ?? null,
  };
}

/**
 * Inspect a managed-install archive against the versioned envelope, then extract
 * only the unique server executable and optional DAP into `extractDir`.
 *
 * Failure deletes `extractDir`. Callers still own the parent temp directory.
 */
export async function extractManagedArchive(
  options: ExtractManagedArchiveOptions,
): Promise<ExtractedManagedArchive> {
  const limits = options.limits ?? managedArchiveSafetyLimits();
  const { archivePath, extractDir, format, windows, cancellationToken } = options;

  throwIfCancelled(cancellationToken, 'Archive extraction cancelled');
  fs.mkdirSync(extractDir, { recursive: true });

  try {
    const inspected =
      format === 'zip'
        ? inspectZip(archivePath, windows, limits)
        : await inspectTar(archivePath, windows, limits, cancellationToken);

    throwIfCancelled(cancellationToken, 'Archive extraction cancelled');

    if (format === 'zip') {
      return extractZipMembers(archivePath, extractDir, inspected, windows, limits);
    }
    return await extractTarMembers(
      archivePath,
      extractDir,
      inspected,
      windows,
      limits,
      cancellationToken,
    );
  } catch (error) {
    removeExtractTree(extractDir);
    throw error;
  }
}
