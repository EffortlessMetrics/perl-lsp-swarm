/**
 * Proof for compatibility-scoped managed storage identity (#9847).
 *
 * The defect under test is that `bin/<process.platform>-<process.arch>` is a
 * weaker key than the artifact compatibility identity it names, so two hosts
 * sharing one global storage can fight over one `current` pointer. These tests
 * are written against the falsifiers in #9847: they fail if platform/arch
 * alone can name a namespace, if GNU and musl can collide, if native and
 * emulated Windows ARM64 candidates can be confused, or if path placement
 * alone promotes legacy bytes.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import {
  admissibleManagedCompatibilityKeys,
  buildManagedCompatibilityKey,
  classifyElfInterpreter,
  classifyLegacyManagedCandidate,
  isAndroidLinker,
  legacyManagedBaseDir,
  managedNamespaceDir,
  managedNamespaceRoot,
  managedUpdateCheckStateKey,
  parseManagedCompatibilityKey,
  probeBinaryIdentity,
  type ObservedBinaryIdentity,
} from '../managedStorageIdentity';

const LINUX_GNU = 'x86_64-unknown-linux-gnu';
const LINUX_MUSL = 'x86_64-unknown-linux-musl';
const WIN_ARM64 = 'aarch64-pc-windows-msvc';
const WIN_X64 = 'x86_64-pc-windows-msvc';
const WIN_X64_EMULATED = 'x86_64-pc-windows-msvc__windows-arm64-emulation';

describe('compatibility key', () => {
  test('a native target is its own key', () => {
    expect(buildManagedCompatibilityKey({ target: LINUX_GNU, emulation: null })).toBe(LINUX_GNU);
  });

  test('an emulated candidate is a distinct key from the same native target', () => {
    const emulated = buildManagedCompatibilityKey({
      target: WIN_X64,
      emulation: 'windows-arm64-emulation',
    });
    expect(emulated).toBe(WIN_X64_EMULATED);
    expect(emulated).not.toBe(WIN_X64);
  });

  test('GNU and musl never produce the same key', () => {
    expect(buildManagedCompatibilityKey({ target: LINUX_GNU, emulation: null })).not.toBe(
      buildManagedCompatibilityKey({ target: LINUX_MUSL, emulation: null }),
    );
  });

  test.each([
    ['a path separator', 'x86_64/unknown-linux-gnu'],
    ['a parent traversal', '..'],
    ['a traversal inside a triple', 'x86_64-../-linux-gnu'],
    ['an absolute path', '/etc'],
    ['an empty target', ''],
    ['uppercase', 'X86_64-UNKNOWN-LINUX-GNU'],
    ['a windows drive', 'c:'],
    ['the reserved emulation separator', 'x86_64__linux'],
  ])('rejects %s as a target', (_label, target) => {
    expect(buildManagedCompatibilityKey({ target, emulation: null })).toBeNull();
  });

  test('rejects an unknown emulation', () => {
    expect(
      buildManagedCompatibilityKey({
        target: WIN_X64,
        emulation: 'rosetta' as never,
      }),
    ).toBeNull();
  });

  test('round-trips every canonical key', () => {
    for (const key of [LINUX_GNU, LINUX_MUSL, WIN_ARM64, WIN_X64_EMULATED]) {
      const identity = parseManagedCompatibilityKey(key);
      expect(identity).not.toBeNull();
      expect(buildManagedCompatibilityKey(identity!)).toBe(key);
    }
  });

  test('parsing rejects a non-canonical key', () => {
    expect(parseManagedCompatibilityKey('x86_64-pc-windows-msvc__rosetta')).toBeNull();
    expect(parseManagedCompatibilityKey('../escape')).toBeNull();
  });
});

describe('namespace projection', () => {
  const storage = path.join('/tmp', 'global-storage');

  test('each compatibility key gets its own directory under one storage root', () => {
    const gnu = managedNamespaceDir(storage, LINUX_GNU);
    const musl = managedNamespaceDir(storage, LINUX_MUSL);
    expect(gnu).toBe(path.join(managedNamespaceRoot(storage), LINUX_GNU));
    expect(musl).toBe(path.join(managedNamespaceRoot(storage), LINUX_MUSL));
    expect(gnu).not.toBe(musl);
  });

  test('native and emulated Windows ARM64 candidates get distinct directories', () => {
    expect(managedNamespaceDir(storage, WIN_ARM64)).not.toBe(
      managedNamespaceDir(storage, WIN_X64_EMULATED),
    );
  });

  test('the namespace root is disjoint from the legacy root', () => {
    const legacy = legacyManagedBaseDir(storage, 'linux', 'x64');
    expect(legacy.startsWith(managedNamespaceRoot(storage))).toBe(false);
  });

  test('a non-canonical key yields no directory rather than an unnamed one', () => {
    expect(managedNamespaceDir(storage, '../escape')).toBeNull();
    expect(managedNamespaceDir(storage, 'x86_64-pc-windows-msvc__rosetta')).toBeNull();
  });

  test('update-check state is keyed per compatibility target', () => {
    const gnu = managedUpdateCheckStateKey(LINUX_GNU);
    const musl = managedUpdateCheckStateKey(LINUX_MUSL);
    expect(gnu).toBe('perl-lsp.lastUpdateCheck.x86_64-unknown-linux-gnu');
    expect(gnu).not.toBe(musl);
    expect(managedUpdateCheckStateKey('../escape')).toBeNull();
  });
});

describe('admissible host keys', () => {
  test('a Linux GNU host admits only the GNU key', () => {
    expect(admissibleManagedCompatibilityKeys('linux', 'x64', LINUX_GNU)).toEqual([LINUX_GNU]);
  });

  test('a Linux musl host admits only the musl key', () => {
    expect(admissibleManagedCompatibilityKeys('linux', 'x64', LINUX_MUSL)).toEqual([LINUX_MUSL]);
  });

  test('Windows ARM64 admits the native key first and the emulated key second', () => {
    expect(admissibleManagedCompatibilityKeys('win32', 'arm64', WIN_ARM64)).toEqual([
      WIN_ARM64,
      WIN_X64_EMULATED,
    ]);
  });

  test('Windows x64 never admits the emulation namespace', () => {
    expect(admissibleManagedCompatibilityKeys('win32', 'x64', WIN_X64)).toEqual([WIN_X64]);
  });

  test('a non-canonical preferred target admits nothing', () => {
    expect(admissibleManagedCompatibilityKeys('linux', 'x64', '../escape')).toEqual([]);
  });
});

describe('legacy adoption', () => {
  const gnuBytes: ObservedBinaryIdentity = { os: 'linux', arch: 'x86_64', libc: 'gnu' };
  const muslBytes: ObservedBinaryIdentity = { os: 'linux', arch: 'x86_64', libc: 'musl' };
  const staticBytes: ObservedBinaryIdentity = { os: 'linux', arch: 'x86_64', libc: null };

  test('adopts a legacy candidate whose bytes match the key exactly', () => {
    expect(classifyLegacyManagedCandidate(gnuBytes, LINUX_GNU)).toBe('adopt');
  });

  test('refuses to promote musl bytes into the GNU namespace', () => {
    expect(classifyLegacyManagedCandidate(muslBytes, LINUX_GNU)).toBe('reject_mismatch');
  });

  test('refuses to promote GNU bytes into the musl namespace', () => {
    expect(classifyLegacyManagedCandidate(gnuBytes, LINUX_MUSL)).toBe('reject_mismatch');
  });

  test('missing libc evidence is not resolved optimistically', () => {
    expect(classifyLegacyManagedCandidate(staticBytes, LINUX_GNU)).toBe('reject_unknown');
    expect(classifyLegacyManagedCandidate(staticBytes, LINUX_MUSL)).toBe('reject_unknown');
  });

  test('unreadable bytes are unknown, never adopted', () => {
    expect(classifyLegacyManagedCandidate(null, LINUX_GNU)).toBe('reject_unknown');
  });

  test('x64 Windows bytes cannot become a native ARM64 candidate', () => {
    const x64 = { os: 'windows', arch: 'x86_64', libc: null } as const;
    expect(classifyLegacyManagedCandidate(x64, WIN_ARM64)).toBe('reject_mismatch');
    expect(classifyLegacyManagedCandidate(x64, WIN_X64_EMULATED)).toBe('adopt');
  });

  test('native ARM64 bytes cannot become an emulated x64 candidate', () => {
    const arm = { os: 'windows', arch: 'aarch64', libc: null } as const;
    expect(classifyLegacyManagedCandidate(arm, WIN_X64_EMULATED)).toBe('reject_mismatch');
    expect(classifyLegacyManagedCandidate(arm, WIN_ARM64)).toBe('adopt');
  });

  test('a Linux binary cannot satisfy a macOS key', () => {
    expect(classifyLegacyManagedCandidate(gnuBytes, 'x86_64-apple-darwin')).toBe('reject_mismatch');
  });

  test('a desktop Linux candidate cannot satisfy an Android key', () => {
    expect(classifyLegacyManagedCandidate(gnuBytes, 'x86_64-linux-android')).toBe(
      'reject_mismatch',
    );
  });

  test('an Android candidate is adopted only under its own key', () => {
    const bionic = { os: 'android', arch: 'aarch64', libc: null } as const;
    expect(classifyLegacyManagedCandidate(bionic, 'aarch64-linux-android')).toBe('adopt');
    expect(classifyLegacyManagedCandidate(bionic, 'aarch64-unknown-linux-gnu')).toBe(
      'reject_mismatch',
    );
    expect(classifyLegacyManagedCandidate(bionic, 'aarch64-unknown-linux-musl')).toBe(
      'reject_mismatch',
    );
  });

  test('an unrecognized target adopts nothing', () => {
    expect(classifyLegacyManagedCandidate(gnuBytes, 'x86_64-unknown-freebsd')).toBe(
      'reject_unknown',
    );
  });
});

describe('ELF interpreter classification', () => {
  test.each([
    ['/lib/ld-musl-x86_64.so.1', 'musl'],
    ['/lib/ld-musl-aarch64.so.1', 'musl'],
    ['/lib64/ld-linux-x86-64.so.2', 'gnu'],
    ['/lib/ld-linux-aarch64.so.1', 'gnu'],
  ])('%s is %s', (interpreter, expected) => {
    expect(classifyElfInterpreter(interpreter)).toBe(expected);
  });

  test('an unrecognized loader is unknown rather than assumed glibc', () => {
    expect(classifyElfInterpreter('/opt/custom/loader.so')).toBeNull();
    expect(classifyElfInterpreter('')).toBeNull();
  });

  test('bionic loaders are recognized and never read as a desktop libc', () => {
    for (const linker of ['/system/bin/linker64', '/system/bin/linker']) {
      expect(isAndroidLinker(linker)).toBe(true);
      expect(classifyElfInterpreter(linker)).toBeNull();
    }
    expect(isAndroidLinker('/lib64/ld-linux-x86-64.so.2')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Byte probes against synthesized headers
// ---------------------------------------------------------------------------

const ELF_MACHINE_X86_64 = 0x3e;
const ELF_MACHINE_AARCH64 = 0xb7;

/**
 * Build a minimal but structurally real ELF64: a valid header, one program
 * header table entry, and (optionally) the PT_INTERP string it points at.
 */
function synthesizeElf(machine: number, interpreter: string | null): Buffer {
  const headerSize = 64;
  const programHeaderOffset = 64;
  const entrySize = 56;
  const interpOffset = programHeaderOffset + entrySize;
  const interpBytes = interpreter === null ? Buffer.alloc(0) : Buffer.from(`${interpreter}\0`);

  const buffer = Buffer.alloc(interpOffset + interpBytes.length);
  buffer.writeUInt32LE(0x464c457f, 0); // \x7fELF
  buffer[4] = 2; // ELFCLASS64
  buffer[5] = 1; // ELFDATA2LSB
  buffer[6] = 1; // EV_CURRENT
  buffer.writeUInt16LE(2, 16); // ET_EXEC
  buffer.writeUInt16LE(machine, 18);
  buffer.writeBigUInt64LE(BigInt(programHeaderOffset), 0x20);
  buffer.writeUInt16LE(headerSize, 0x34);
  buffer.writeUInt16LE(entrySize, 0x36);
  buffer.writeUInt16LE(interpreter === null ? 0 : 1, 0x38);

  if (interpreter !== null) {
    buffer.writeUInt32LE(3, programHeaderOffset); // PT_INTERP
    buffer.writeUInt32LE(4, programHeaderOffset + 4); // PF_R
    buffer.writeBigUInt64LE(BigInt(interpOffset), programHeaderOffset + 8);
    buffer.writeBigUInt64LE(BigInt(interpBytes.length), programHeaderOffset + 32);
    interpBytes.copy(buffer, interpOffset);
  }
  return buffer;
}

function synthesizePe(machine: number): Buffer {
  const peOffset = 0x80;
  const buffer = Buffer.alloc(peOffset + 24);
  buffer.write('MZ', 0, 'latin1');
  buffer.writeUInt32LE(peOffset, 0x3c);
  buffer.writeUInt32LE(0x00004550, peOffset); // "PE\0\0"
  buffer.writeUInt16LE(machine, peOffset + 4);
  return buffer;
}

function synthesizeMachO(cpuType: number): Buffer {
  const buffer = Buffer.alloc(64);
  buffer.writeUInt32LE(0xfeedfacf, 0);
  buffer.writeUInt32LE(cpuType, 4);
  return buffer;
}

describe('binary identity probe', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-probe-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  function write(name: string, bytes: Buffer): string {
    const file = path.join(tmpDir, name);
    fs.writeFileSync(file, bytes);
    return file;
  }

  test('reads glibc x86_64 from a dynamically linked ELF', () => {
    const file = write(
      'perllsp-gnu',
      synthesizeElf(ELF_MACHINE_X86_64, '/lib64/ld-linux-x86-64.so.2'),
    );
    expect(probeBinaryIdentity(file)).toEqual({ os: 'linux', arch: 'x86_64', libc: 'gnu' });
  });

  test('reads musl x86_64 from a dynamically linked ELF', () => {
    const file = write(
      'perllsp-musl',
      synthesizeElf(ELF_MACHINE_X86_64, '/lib/ld-musl-x86_64.so.1'),
    );
    expect(probeBinaryIdentity(file)).toEqual({ os: 'linux', arch: 'x86_64', libc: 'musl' });
  });

  test('reads aarch64 from an ELF and keeps libc evidence separate', () => {
    const file = write(
      'perllsp-arm',
      synthesizeElf(ELF_MACHINE_AARCH64, '/lib/ld-musl-aarch64.so.1'),
    );
    expect(probeBinaryIdentity(file)).toEqual({ os: 'linux', arch: 'aarch64', libc: 'musl' });
  });

  test('an Android image is classified as bionic, not a desktop Linux build', () => {
    const file = write(
      'perllsp-android',
      synthesizeElf(ELF_MACHINE_AARCH64, '/system/bin/linker64'),
    );
    expect(probeBinaryIdentity(file)).toEqual({ os: 'android', arch: 'aarch64', libc: null });
    expect(
      classifyLegacyManagedCandidate(probeBinaryIdentity(file), 'aarch64-unknown-linux-gnu'),
    ).toBe('reject_mismatch');
    expect(classifyLegacyManagedCandidate(probeBinaryIdentity(file), 'aarch64-linux-android')).toBe(
      'adopt',
    );
  });

  test('a static ELF yields an unknown libc rather than a guess', () => {
    const file = write('perllsp-static', synthesizeElf(ELF_MACHINE_X86_64, null));
    expect(probeBinaryIdentity(file)).toEqual({ os: 'linux', arch: 'x86_64', libc: null });
  });

  test('the GNU/musl distinction survives the whole probe-to-decision path', () => {
    const musl = write('musl', synthesizeElf(ELF_MACHINE_X86_64, '/lib/ld-musl-x86_64.so.1'));
    expect(classifyLegacyManagedCandidate(probeBinaryIdentity(musl), LINUX_GNU)).toBe(
      'reject_mismatch',
    );
    expect(classifyLegacyManagedCandidate(probeBinaryIdentity(musl), LINUX_MUSL)).toBe('adopt');
  });

  test('reads machine from a PE image', () => {
    expect(probeBinaryIdentity(write('a.exe', synthesizePe(0x8664)))).toEqual({
      os: 'windows',
      arch: 'x86_64',
      libc: null,
    });
    expect(probeBinaryIdentity(write('b.exe', synthesizePe(0xaa64)))).toEqual({
      os: 'windows',
      arch: 'aarch64',
      libc: null,
    });
  });

  test('reads cputype from a 64-bit Mach-O image', () => {
    expect(probeBinaryIdentity(write('mac-x64', synthesizeMachO(0x01000007)))).toEqual({
      os: 'macos',
      arch: 'x86_64',
      libc: null,
    });
    expect(probeBinaryIdentity(write('mac-arm', synthesizeMachO(0x0100000c)))).toEqual({
      os: 'macos',
      arch: 'aarch64',
      libc: null,
    });
  });

  test.each([
    ['a missing file', null],
    ['an empty file', Buffer.alloc(0)],
    ['a shell script', Buffer.from('#!/bin/sh\necho hi\n')],
    ['a truncated ELF header', synthesizeElf(ELF_MACHINE_X86_64, null).subarray(0, 20)],
    [
      'a 32-bit ELF',
      (() => {
        const b = synthesizeElf(ELF_MACHINE_X86_64, null);
        b[4] = 1; // ELFCLASS32
        return b;
      })(),
    ],
    ['an unknown ELF machine', synthesizeElf(0x28, null)],
    ['an unknown PE machine', synthesizePe(0x014c)],
    [
      'a 32-bit Mach-O',
      (() => {
        const b = Buffer.alloc(64);
        b.writeUInt32LE(0xfeedface, 0);
        return b;
      })(),
    ],
  ])('%s probes as unknown', (_label, bytes) => {
    const file = bytes === null ? path.join(tmpDir, 'absent') : write('candidate', bytes as Buffer);
    expect(probeBinaryIdentity(file)).toBeNull();
  });

  test('a directory in place of a binary probes as unknown', () => {
    const dir = path.join(tmpDir, 'perllsp');
    fs.mkdirSync(dir);
    expect(probeBinaryIdentity(dir)).toBeNull();
  });

  test('an ELF whose PT_INTERP points past EOF probes without a libc claim', () => {
    const bytes = synthesizeElf(ELF_MACHINE_X86_64, '/lib/ld-musl-x86_64.so.1');
    bytes.writeBigUInt64LE(BigInt(0xffffff), 64 + 8); // p_offset beyond the file
    const file = write('corrupt', bytes);
    expect(probeBinaryIdentity(file)).toEqual({ os: 'linux', arch: 'x86_64', libc: null });
  });
});
