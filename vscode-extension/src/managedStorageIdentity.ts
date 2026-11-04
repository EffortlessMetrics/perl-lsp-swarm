/**
 * Compatibility-scoped identity for managed (auto-downloaded) server state.
 *
 * Managed installs used to live under
 * `globalStorageUri/bin/<process.platform>-<process.arch>/`, with a single
 * `current` pointer selecting the active install. That key is weaker than the
 * artifact compatibility identity it is supposed to name: `linux-x64` covers
 * both GNU and musl targets, and `win32-arm64` covers both a native ARM64
 * candidate and an explicitly x64-emulated one. Any environment that shares a
 * profile or extension global storage across hosts — Remote-SSH, WSL, roaming
 * home directories — can therefore have one host advance `current` to bytes a
 * second host cannot execute (#9847).
 *
 * This module owns exactly one thing: projecting an accepted compatibility
 * target into the storage namespace and the state keys that depend on it. It
 * is deliberately pure and filesystem-free apart from the byte probes at the
 * bottom, which read only fixed-size headers.
 *
 * Explicit non-ownership:
 * - which target a host may use (#9844/#10073),
 * - which release/candidate is selected (#9098/#9924/#9925/#9927),
 * - candidate publication and selection schema (#7857/#7858),
 * - cross-process coordination inside a namespace (#7816/#7859).
 */

import * as fs from 'fs';
import * as path from 'path';

/**
 * A host compatibility shim that makes a foreign-architecture target runnable.
 * Emulated candidates are never interchangeable with native ones, so the shim
 * is part of the compatibility identity rather than a property of the host.
 */
export type ManagedEmulation = 'windows-arm64-emulation';

const EMULATIONS: ReadonlySet<string> = new Set<ManagedEmulation>(['windows-arm64-emulation']);

/**
 * Cargo-dist style target triples: lowercase alphanumeric groups joined by
 * single `-`, `_`, or `.` separators. This deliberately rejects `__`, which is
 * reserved below as the emulation separator, and rejects anything that could
 * escape a path segment.
 */
const TARGET = /^[a-z0-9]+(?:[._-][a-z0-9]+)*$/;

const EMULATION_SEPARATOR = '__';

export interface ManagedCompatibilityIdentity {
  /** The canonical target triple of the candidate that will actually run. */
  readonly target: string;
  /** The shim required to run it here, or null when it runs natively. */
  readonly emulation: ManagedEmulation | null;
}

/**
 * The canonical compatibility key.
 *
 * One spelling serves as both the logical key and the on-disk path segment, so
 * a namespace can never disagree with the identity that named it. The key is
 * `<target>` natively and `<target>__<emulation>` under a shim — for example
 * `x86_64-unknown-linux-musl` or
 * `x86_64-pc-windows-msvc__windows-arm64-emulation`.
 */
export function buildManagedCompatibilityKey(
  identity: ManagedCompatibilityIdentity,
): string | null {
  if (!TARGET.test(identity.target)) {
    return null;
  }
  if (identity.emulation === null) {
    return identity.target;
  }
  if (!EMULATIONS.has(identity.emulation)) {
    return null;
  }
  return `${identity.target}${EMULATION_SEPARATOR}${identity.emulation}`;
}

/** Inverse of {@link buildManagedCompatibilityKey}; null when not canonical. */
export function parseManagedCompatibilityKey(key: string): ManagedCompatibilityIdentity | null {
  const separatorIndex = key.indexOf(EMULATION_SEPARATOR);
  if (separatorIndex < 0) {
    return TARGET.test(key) ? { target: key, emulation: null } : null;
  }
  const target = key.slice(0, separatorIndex);
  const emulation = key.slice(separatorIndex + EMULATION_SEPARATOR.length);
  if (!TARGET.test(target) || !EMULATIONS.has(emulation)) {
    return null;
  }
  return { target, emulation: emulation as ManagedEmulation };
}

/**
 * Root of every compatibility-scoped namespace.
 *
 * Distinct from the legacy `bin/` root so an old install is never mistaken for
 * a compatibility-scoped one purely by path shape.
 */
export function managedNamespaceRoot(globalStorageFsPath: string): string {
  return path.join(globalStorageFsPath, 'managed');
}

/**
 * Directory owning the candidate generations, `current` pointer, and
 * previous-known-good fallback for exactly one compatibility key.
 *
 * Returns null for a non-canonical key: callers must fail closed rather than
 * write managed state into an unverified location.
 */
export function managedNamespaceDir(globalStorageFsPath: string, key: string): string | null {
  if (parseManagedCompatibilityKey(key) === null) {
    return null;
  }
  return path.join(managedNamespaceRoot(globalStorageFsPath), key);
}

/** The pre-#9847 managed root, retained for read-only legacy adoption. */
export function legacyManagedBaseDir(
  globalStorageFsPath: string,
  platform: string,
  arch: string,
): string {
  return path.join(globalStorageFsPath, 'bin', `${platform}-${arch}`);
}

/**
 * `globalState` key for the last managed update check.
 *
 * Scoped so a GNU host cannot suppress a musl host's check while both share
 * one extension global state object.
 */
export function managedUpdateCheckStateKey(key: string): string | null {
  if (parseManagedCompatibilityKey(key) === null) {
    return null;
  }
  return `${LEGACY_UPDATE_CHECK_STATE_KEY}.${key}`;
}

/** The unscoped update-check key written before #9847. */
export const LEGACY_UPDATE_CHECK_STATE_KEY = 'perl-lsp.lastUpdateCheck';

/**
 * Compatibility keys this host may legitimately consume, most preferred first.
 *
 * Windows ARM64 is the only host with more than one admissible key: a release
 * may ship a native ARM64 candidate or only the x64 one, and which was
 * installed is a property of the install, not of the host. Every other host
 * resolves to exactly one key, which is what keeps GNU and musl apart.
 */
export function admissibleManagedCompatibilityKeys(
  platform: string,
  arch: string,
  preferredTarget: string,
): string[] {
  const keys: string[] = [];
  const preferred = buildManagedCompatibilityKey({ target: preferredTarget, emulation: null });
  if (preferred !== null) {
    keys.push(preferred);
  }
  if (platform === 'win32' && arch === 'arm64') {
    const emulated = buildManagedCompatibilityKey({
      target: WINDOWS_X64_COMPATIBILITY_TARGET,
      emulation: 'windows-arm64-emulation',
    });
    if (emulated !== null && !keys.includes(emulated)) {
      keys.push(emulated);
    }
  }
  return keys;
}

const WINDOWS_X64_COMPATIBILITY_TARGET = 'x86_64-pc-windows-msvc';

// ---------------------------------------------------------------------------
// Legacy adoption
// ---------------------------------------------------------------------------

/**
 * Executable-format facts read directly from candidate bytes.
 *
 * Only 64-bit images are classified. A 32-bit candidate (i686/armv7 Android)
 * probes as unknown and is never adopted, which costs one redownload and never
 * risks running the wrong bytes.
 */
export interface ObservedBinaryIdentity {
  /**
   * Android is its own OS here even though its images are ELF: bionic is not
   * interchangeable with glibc or musl, so folding it into `linux` would let
   * an Android candidate satisfy a desktop Linux key.
   */
  readonly os: 'linux' | 'android' | 'windows' | 'macos';
  readonly arch: 'x86_64' | 'aarch64';
  /**
   * Desktop Linux only. `null` when the interpreter is absent (static link) or
   * unrecognized — that is missing evidence, not proof of either libc.
   */
  readonly libc: 'gnu' | 'musl' | null;
}

export type LegacyAdoption =
  /** Bytes were read and match the expected compatibility key exactly. */
  | 'adopt'
  /** Bytes were read and contradict the expected key. */
  | 'reject_mismatch'
  /** Bytes were absent, unreadable, or carried no discriminating evidence. */
  | 'reject_unknown';

/**
 * Decide whether a legacy candidate may be consumed under `expectedKey`.
 *
 * Path placement is not evidence: a `bin/linux-x64` directory says nothing
 * about GNU versus musl, and that ambiguity is the whole defect. Adoption
 * therefore requires the candidate's own bytes to agree with the key on every
 * axis the key distinguishes, and missing evidence stays `reject_unknown`
 * rather than being resolved optimistically.
 */
export function classifyLegacyManagedCandidate(
  observed: ObservedBinaryIdentity | null,
  expectedKey: string,
): LegacyAdoption {
  if (observed === null) {
    return 'reject_unknown';
  }
  const identity = parseManagedCompatibilityKey(expectedKey);
  if (identity === null) {
    return 'reject_unknown';
  }
  const expected = describeCompatibilityTarget(identity.target);
  if (expected === null) {
    return 'reject_unknown';
  }
  if (expected.os !== observed.os || expected.arch !== observed.arch) {
    return 'reject_mismatch';
  }
  // libc only discriminates on desktop Linux, where GNU and musl candidates
  // are both published for the same arch.
  if (expected.os === 'linux') {
    if (observed.libc === null) {
      return 'reject_unknown';
    }
    if (observed.libc !== expected.libc) {
      return 'reject_mismatch';
    }
  }
  return 'adopt';
}

interface CompatibilityTargetFacts {
  readonly os: ObservedBinaryIdentity['os'];
  readonly arch: ObservedBinaryIdentity['arch'];
  readonly libc: 'gnu' | 'musl' | null;
}

/**
 * The executable facts a target triple asserts. Unknown triples return null so
 * an unrecognized target can never satisfy an adoption check by default.
 */
function describeCompatibilityTarget(target: string): CompatibilityTargetFacts | null {
  const arch = target.startsWith('x86_64-')
    ? 'x86_64'
    : target.startsWith('aarch64-')
      ? 'aarch64'
      : null;
  if (arch === null) {
    return null;
  }
  if (target.endsWith('-linux-android')) {
    return { os: 'android', arch, libc: null };
  }
  if (target.endsWith('-unknown-linux-gnu')) {
    return { os: 'linux', arch, libc: 'gnu' };
  }
  if (target.endsWith('-unknown-linux-musl')) {
    return { os: 'linux', arch, libc: 'musl' };
  }
  if (target.endsWith('-pc-windows-msvc')) {
    return { os: 'windows', arch, libc: null };
  }
  if (target.endsWith('-apple-darwin')) {
    return { os: 'macos', arch, libc: null };
  }
  return null;
}

// ---------------------------------------------------------------------------
// Byte probes
// ---------------------------------------------------------------------------

const ELF_MAGIC = 0x464c457f; // "\x7fELF", little-endian u32
const PE_MACHINE_X86_64 = 0x8664;
const PE_MACHINE_AARCH64 = 0xaa64;
const ELF_MACHINE_X86_64 = 0x3e;
const ELF_MACHINE_AARCH64 = 0xb7;
const MACHO_CPU_X86_64 = 0x01000007;
const MACHO_CPU_ARM64 = 0x0100000c;
const PT_INTERP = 3;

/** Largest PT_INTERP string we will read; real interpreters are far shorter. */
const MAX_INTERP_BYTES = 256;
/** Program-header table entries we will walk before giving up. */
const MAX_PROGRAM_HEADERS = 128;

/**
 * Read the executable identity of a candidate binary from its own headers.
 *
 * Only fixed-size header regions are read — never the whole multi-megabyte
 * binary. Any malformed, truncated, or unrecognized input yields null, which
 * {@link classifyLegacyManagedCandidate} treats as missing evidence.
 */
export function probeBinaryIdentity(binaryPath: string): ObservedBinaryIdentity | null {
  let handle: number;
  try {
    handle = fs.openSync(binaryPath, 'r');
  } catch {
    return null;
  }
  try {
    return probeOpenBinaryIdentity(handle);
  } catch {
    return null;
  } finally {
    try {
      fs.closeSync(handle);
    } catch {
      /* the probe result does not depend on a clean close */
    }
  }
}

function readAt(handle: number, offset: number, length: number): Buffer | null {
  const buffer = Buffer.alloc(length);
  const read = fs.readSync(handle, buffer, 0, length, offset);
  return read === length ? buffer : null;
}

function probeOpenBinaryIdentity(handle: number): ObservedBinaryIdentity | null {
  const head = readAt(handle, 0, 64);
  if (head === null) {
    return null;
  }
  if (head.readUInt32LE(0) === ELF_MAGIC) {
    return probeElf(handle, head);
  }
  if (head[0] === 0x4d && head[1] === 0x5a) {
    return probePe(handle, head);
  }
  return probeMachO(head);
}

function probeElf(handle: number, head: Buffer): ObservedBinaryIdentity | null {
  // Only 64-bit little-endian ELF is a target we publish.
  if (head[4] !== 2 || head[5] !== 1) {
    return null;
  }
  const machine = head.readUInt16LE(18);
  const arch =
    machine === ELF_MACHINE_X86_64 ? 'x86_64' : machine === ELF_MACHINE_AARCH64 ? 'aarch64' : null;
  if (arch === null) {
    return null;
  }
  const interpreter = readElfInterpreter(handle, head);
  if (interpreter !== null && isAndroidLinker(interpreter)) {
    return { os: 'android', arch, libc: null };
  }
  return { os: 'linux', arch, libc: classifyElfInterpreter(interpreter ?? '') };
}

/**
 * Read the ELF interpreter, the one place Linux flavours are distinguishable
 * without executing anything.
 *
 * A statically linked binary has no PT_INTERP and yields `null`: musl is the
 * common static case, but glibc can be statically linked too, so the absence
 * of an interpreter is not evidence for either.
 */
function readElfInterpreter(handle: number, head: Buffer): string | null {
  const programHeaderOffset = Number(head.readBigUInt64LE(0x20));
  const entrySize = head.readUInt16LE(0x36);
  const entryCount = head.readUInt16LE(0x38);
  if (
    !Number.isSafeInteger(programHeaderOffset) ||
    programHeaderOffset <= 0 ||
    entrySize < 56 ||
    entryCount === 0
  ) {
    return null;
  }
  const walked = Math.min(entryCount, MAX_PROGRAM_HEADERS);
  for (let index = 0; index < walked; index += 1) {
    const entry = readAt(handle, programHeaderOffset + index * entrySize, 56);
    if (entry === null) {
      return null;
    }
    if (entry.readUInt32LE(0) !== PT_INTERP) {
      continue;
    }
    const interpOffset = Number(entry.readBigUInt64LE(8));
    const interpSize = Number(entry.readBigUInt64LE(32));
    if (
      !Number.isSafeInteger(interpOffset) ||
      !Number.isSafeInteger(interpSize) ||
      interpSize <= 0 ||
      interpSize > MAX_INTERP_BYTES
    ) {
      return null;
    }
    const interp = readAt(handle, interpOffset, interpSize);
    if (interp === null) {
      return null;
    }
    return interp.toString('latin1').replace(/\0+$/, '');
  }
  return null;
}

/** Bionic's loader is unambiguous and shipped at a fixed system path. */
export function isAndroidLinker(interpreter: string): boolean {
  return interpreter === '/system/bin/linker64' || interpreter === '/system/bin/linker';
}

/** musl and glibc name their loaders distinctly; anything else is unknown. */
export function classifyElfInterpreter(interpreter: string): 'gnu' | 'musl' | null {
  const name = path.posix.basename(interpreter);
  if (name.startsWith('ld-musl-')) {
    return 'musl';
  }
  if (name.startsWith('ld-linux') || name.startsWith('ld.so')) {
    return 'gnu';
  }
  return null;
}

function probePe(handle: number, head: Buffer): ObservedBinaryIdentity | null {
  const peOffset = head.readUInt32LE(0x3c);
  if (!Number.isSafeInteger(peOffset) || peOffset <= 0) {
    return null;
  }
  const coff = readAt(handle, peOffset, 6);
  if (coff === null) {
    return null;
  }
  if (coff.readUInt32LE(0) !== 0x00004550) {
    return null;
  }
  const machine = coff.readUInt16LE(4);
  const arch =
    machine === PE_MACHINE_X86_64 ? 'x86_64' : machine === PE_MACHINE_AARCH64 ? 'aarch64' : null;
  if (arch === null) {
    return null;
  }
  return { os: 'windows', arch, libc: null };
}

function probeMachO(head: Buffer): ObservedBinaryIdentity | null {
  // 64-bit Mach-O only: 0xfeedfacf little-endian, or byte-swapped.
  const magic = head.readUInt32LE(0);
  const swapped = magic === 0xcffaedfe;
  if (magic !== 0xfeedfacf && !swapped) {
    return null;
  }
  const cpuType = swapped ? head.readUInt32BE(4) : head.readUInt32LE(4);
  const arch =
    cpuType === MACHO_CPU_X86_64 ? 'x86_64' : cpuType === MACHO_CPU_ARM64 ? 'aarch64' : null;
  if (arch === null) {
    return null;
  }
  return { os: 'macos', arch, libc: null };
}
