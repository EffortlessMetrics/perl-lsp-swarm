/**
 * Two-host falsifiers for compatibility-scoped managed storage (#9847).
 *
 * These drive the real `BinaryDownloader` resolution path against ONE shared
 * `globalStorageUri` while simulating more than one host compatibility
 * identity — the situation a shared home directory, roaming profile, or
 * Remote-SSH/WSL setup actually produces. The defect being falsified is a host
 * observing, advancing, or consuming another host's `current` selection.
 *
 * Concurrency inside a namespace is not in scope here: #7816/#7859 own
 * cross-process coordination, and namespace isolation is not a substitute for
 * it.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import type * as vscode from 'vscode';
import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals';
import { BinaryDownloader, MANAGED_INSTALL_TARGET_FILE } from '../downloader';
import { legacyManagedBaseDir, managedNamespaceDir } from '../managedStorageIdentity';

interface TestDownloader {
  getLocalBinaryPath(): string;
  commitVersionedInstall(installDirName: string, compatibilityKey?: string): void;
}

const LINUX_GNU = 'x86_64-unknown-linux-gnu';
const LINUX_MUSL = 'x86_64-unknown-linux-musl';
const WIN_ARM64 = 'aarch64-pc-windows-msvc';
const WIN_X64_EMULATED = 'x86_64-pc-windows-msvc__windows-arm64-emulation';

const ELF_MACHINE_X86_64 = 0x3e;
const GLIBC_INTERP = '/lib64/ld-linux-x86-64.so.2';
const MUSL_INTERP = '/lib/ld-musl-x86_64.so.1';

/** A structurally real dynamically linked ELF64 naming a specific loader. */
function elfWithInterpreter(interpreter: string | null): Buffer {
  const programHeaderOffset = 64;
  const entrySize = 56;
  const interpOffset = programHeaderOffset + entrySize;
  const interpBytes = interpreter === null ? Buffer.alloc(0) : Buffer.from(`${interpreter}\0`);
  const buffer = Buffer.alloc(interpOffset + interpBytes.length);

  buffer.writeUInt32LE(0x464c457f, 0);
  buffer[4] = 2;
  buffer[5] = 1;
  buffer[6] = 1;
  buffer.writeUInt16LE(2, 16);
  buffer.writeUInt16LE(ELF_MACHINE_X86_64, 18);
  buffer.writeBigUInt64LE(BigInt(programHeaderOffset), 0x20);
  buffer.writeUInt16LE(64, 0x34);
  buffer.writeUInt16LE(entrySize, 0x36);
  buffer.writeUInt16LE(interpreter === null ? 0 : 1, 0x38);
  if (interpreter !== null) {
    buffer.writeUInt32LE(3, programHeaderOffset);
    buffer.writeUInt32LE(4, programHeaderOffset + 4);
    buffer.writeBigUInt64LE(BigInt(interpOffset), programHeaderOffset + 8);
    buffer.writeBigUInt64LE(BigInt(interpBytes.length), programHeaderOffset + 32);
    interpBytes.copy(buffer, interpOffset);
  }
  return buffer;
}

function peImage(machine: number): Buffer {
  const peOffset = 0x80;
  const buffer = Buffer.alloc(peOffset + 24);
  buffer.write('MZ', 0, 'latin1');
  buffer.writeUInt32LE(peOffset, 0x3c);
  buffer.writeUInt32LE(0x00004550, peOffset);
  buffer.writeUInt16LE(machine, peOffset + 4);
  return buffer;
}

describe('shared global storage across incompatible hosts', () => {
  let storageRoot: string;

  beforeEach(() => {
    storageRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'managed-namespace-'));
  });

  afterEach(() => {
    fs.rmSync(storageRoot, { recursive: true, force: true });
    jest.restoreAllMocks();
  });

  function context(): vscode.ExtensionContext {
    return {
      globalStorageUri: { fsPath: storageRoot } as vscode.Uri,
      extensionPath: storageRoot,
      subscriptions: [],
    } as unknown as vscode.ExtensionContext;
  }

  function outputChannel(): vscode.OutputChannel {
    return {
      appendLine: jest.fn(),
      show: jest.fn(),
      dispose: jest.fn(),
    } as unknown as vscode.OutputChannel;
  }

  /**
   * Runs `fn` as a specific host: `process.platform`/`process.arch` are the
   * host's Node identity, and `linuxLibc` pins the libc the host resolves to.
   * Everything else — namespace selection, pointer reads, adoption — is the
   * production code path under test.
   */
  function asHost<T>(
    options: { platform: string; arch: string; linuxLibc?: 'gnu' | 'musl' },
    fn: () => T,
  ): T {
    const vscodeMock = require('vscode');
    const previousConfig = vscodeMock.workspace.getConfiguration;
    vscodeMock.workspace.getConfiguration = jest.fn(() => ({
      get: (key: string, defaultValue?: unknown) =>
        key === 'linuxLibc' ? (options.linuxLibc ?? 'gnu') : defaultValue,
      update: jest.fn(),
    }));
    const platformDescriptor = Object.getOwnPropertyDescriptor(process, 'platform');
    const archDescriptor = Object.getOwnPropertyDescriptor(process, 'arch');
    Object.defineProperty(process, 'platform', {
      value: options.platform,
      configurable: true,
    });
    Object.defineProperty(process, 'arch', { value: options.arch, configurable: true });
    try {
      return fn();
    } finally {
      if (platformDescriptor) {
        Object.defineProperty(process, 'platform', platformDescriptor);
      }
      if (archDescriptor) {
        Object.defineProperty(process, 'arch', archDescriptor);
      }
      vscodeMock.workspace.getConfiguration = previousConfig;
    }
  }

  const linuxGnu = { platform: 'linux', arch: 'x64', linuxLibc: 'gnu' } as const;
  const linuxMusl = { platform: 'linux', arch: 'x64', linuxLibc: 'musl' } as const;

  function downloader(): TestDownloader {
    return new BinaryDownloader(context(), outputChannel()) as unknown as TestDownloader;
  }

  /** Publish a candidate into `key`'s namespace and select it. */
  function publish(key: string, installDirName: string, host: Parameters<typeof asHost>[0]): void {
    const baseDir = managedNamespaceDir(storageRoot, key)!;
    const installDir = path.join(baseDir, installDirName);
    fs.mkdirSync(installDir, { recursive: true });
    fs.writeFileSync(path.join(installDir, 'perllsp'), `bytes for ${key}`);
    fs.writeFileSync(
      path.join(installDir, MANAGED_INSTALL_TARGET_FILE),
      JSON.stringify({
        schema_version: 'managed_install_target.v1',
        compatibility_key: key,
        target: key.split('__')[0],
        emulation: key.includes('__') ? 'windows-arm64-emulation' : null,
      }),
    );
    asHost(host, () => downloader().commitVersionedInstall(installDirName, key));
  }

  function resolve(host: Parameters<typeof asHost>[0]): string {
    return asHost(host, () => downloader().getLocalBinaryPath());
  }

  // 1. Distinct namespaces per libc.
  test('GNU and musl hosts resolve into distinct managed namespaces', () => {
    const gnuPath = resolve(linuxGnu);
    const muslPath = resolve(linuxMusl);

    expect(gnuPath.startsWith(managedNamespaceDir(storageRoot, LINUX_GNU)!)).toBe(true);
    expect(muslPath.startsWith(managedNamespaceDir(storageRoot, LINUX_MUSL)!)).toBe(true);
    expect(path.dirname(gnuPath)).not.toBe(path.dirname(muslPath));
  });

  // 2/3. Publication in one namespace cannot move the other's selection.
  test('publishing a GNU candidate does not move the musl selection', () => {
    publish(LINUX_MUSL, 'v1-musl', linuxMusl);
    const muslBefore = resolve(linuxMusl);

    publish(LINUX_GNU, 'v2-gnu', linuxGnu);

    expect(resolve(linuxMusl)).toBe(muslBefore);
    expect(resolve(linuxMusl)).toContain(path.join(LINUX_MUSL, 'v1-musl'));
    expect(resolve(linuxGnu)).toContain(path.join(LINUX_GNU, 'v2-gnu'));
  });

  test('publishing a musl candidate does not move the GNU selection', () => {
    publish(LINUX_GNU, 'v1-gnu', linuxGnu);
    const gnuBefore = resolve(linuxGnu);

    publish(LINUX_MUSL, 'v2-musl', linuxMusl);

    expect(resolve(linuxGnu)).toBe(gnuBefore);
  });

  // 4. Compatible hosts share one namespace.
  test('a second host with the same compatibility key consumes the same candidate', () => {
    publish(LINUX_GNU, 'v1-gnu', linuxGnu);

    // Same key, different Node-visible host instance.
    const secondHost = resolve({ platform: 'linux', arch: 'x64', linuxLibc: 'gnu' });

    expect(secondHost).toBe(resolve(linuxGnu));
    expect(secondHost).toContain(path.join(LINUX_GNU, 'v1-gnu'));
  });

  // 9. Rollback and pruning are target-scoped.
  test('rolling a target back cannot switch another compatibility row', () => {
    publish(LINUX_GNU, 'v1-gnu', linuxGnu);
    publish(LINUX_MUSL, 'v1-musl', linuxMusl);
    publish(LINUX_GNU, 'v2-gnu', linuxGnu);

    // Roll GNU back to its previous known-good generation.
    asHost(linuxGnu, () => downloader().commitVersionedInstall('v1-gnu', LINUX_GNU));

    expect(resolve(linuxGnu)).toContain(path.join(LINUX_GNU, 'v1-gnu'));
    expect(resolve(linuxMusl)).toContain(path.join(LINUX_MUSL, 'v1-musl'));
  });

  // 5. Native and emulated Windows ARM64 candidates stay distinguishable.
  describe('Windows ARM64', () => {
    const winArm = { platform: 'win32', arch: 'arm64' } as const;
    const winX64 = { platform: 'win32', arch: 'x64' } as const;

    test('prefers a native candidate over an emulated one', () => {
      publish(WIN_X64_EMULATED, 'v1-emulated', winArm);
      expect(resolve(winArm)).toContain(path.join(WIN_X64_EMULATED, 'v1-emulated'));

      publish(WIN_ARM64, 'v1-native', winArm);
      expect(resolve(winArm)).toContain(path.join(WIN_ARM64, 'v1-native'));
    });

    test('an emulated candidate is never visible to a plain x64 host', () => {
      publish(WIN_X64_EMULATED, 'v1-emulated', winArm);

      const x64Path = resolve(winX64);
      expect(x64Path).not.toContain(WIN_X64_EMULATED);
      expect(x64Path).toContain(path.join('managed', 'x86_64-pc-windows-msvc', 'perllsp.exe'));
    });

    test('an install whose record names another key is not selected', () => {
      // A candidate record and the namespace holding it must agree; a
      // disagreement means one of them is lying about the bytes.
      const baseDir = managedNamespaceDir(storageRoot, WIN_ARM64)!;
      const installDir = path.join(baseDir, 'v1-mislabelled');
      fs.mkdirSync(installDir, { recursive: true });
      fs.writeFileSync(path.join(installDir, 'perllsp.exe'), 'bytes');
      fs.writeFileSync(
        path.join(installDir, MANAGED_INSTALL_TARGET_FILE),
        JSON.stringify({
          schema_version: 'managed_install_target.v1',
          compatibility_key: WIN_X64_EMULATED,
          target: 'x86_64-pc-windows-msvc',
          emulation: 'windows-arm64-emulation',
        }),
      );
      asHost(winArm, () => downloader().commitVersionedInstall('v1-mislabelled', WIN_ARM64));

      expect(resolve(winArm)).not.toContain('v1-mislabelled');
    });
  });

  // 6/7. Legacy adoption revalidates bytes.
  describe('legacy bin/<platform>-<arch> installs', () => {
    function seedLegacy(bytes: Buffer, name = 'perllsp'): string {
      const legacy = legacyManagedBaseDir(storageRoot, 'linux', 'x64');
      fs.mkdirSync(legacy, { recursive: true });
      const file = path.join(legacy, name);
      fs.writeFileSync(file, bytes);
      return file;
    }

    test('adopts a legacy candidate once its bytes revalidate against the key', () => {
      const legacyBinary = seedLegacy(elfWithInterpreter(GLIBC_INTERP));

      expect(resolve(linuxGnu)).toBe(legacyBinary);
    });

    test('does not promote a legacy musl candidate for a GNU host', () => {
      seedLegacy(elfWithInterpreter(MUSL_INTERP));

      const resolved = resolve(linuxGnu);
      expect(resolved.startsWith(managedNamespaceDir(storageRoot, LINUX_GNU)!)).toBe(true);
      expect(fs.existsSync(resolved)).toBe(false);
    });

    test('does not promote a legacy GNU candidate for a musl host', () => {
      seedLegacy(elfWithInterpreter(GLIBC_INTERP));

      const resolved = resolve(linuxMusl);
      expect(resolved.startsWith(managedNamespaceDir(storageRoot, LINUX_MUSL)!)).toBe(true);
      expect(fs.existsSync(resolved)).toBe(false);
    });

    test('path placement alone never promotes unreadable bytes', () => {
      seedLegacy(Buffer.from('#!/bin/sh\nexec perllsp "$@"\n'));

      const resolved = resolve(linuxGnu);
      expect(fs.existsSync(resolved)).toBe(false);
    });

    test('a rejected legacy candidate is left intact for the host that owns it', () => {
      const legacyBinary = seedLegacy(elfWithInterpreter(MUSL_INTERP));

      resolve(linuxGnu);

      expect(fs.existsSync(legacyBinary)).toBe(true);
      // And the host it really belongs to can still adopt it.
      expect(resolve(linuxMusl)).toBe(legacyBinary);
    });

    test('a compatibility-scoped candidate wins over an adoptable legacy one', () => {
      seedLegacy(elfWithInterpreter(GLIBC_INTERP));
      publish(LINUX_GNU, 'v1-gnu', linuxGnu);

      expect(resolve(linuxGnu)).toContain(path.join(LINUX_GNU, 'v1-gnu'));
    });

    test('a legacy versioned install is adopted through its own pointer', () => {
      const legacy = legacyManagedBaseDir(storageRoot, 'linux', 'x64');
      const versioned = path.join(legacy, 'v0.13.3-stamp');
      fs.mkdirSync(versioned, { recursive: true });
      fs.writeFileSync(path.join(versioned, 'perllsp'), elfWithInterpreter(GLIBC_INTERP));
      fs.writeFileSync(path.join(legacy, 'current'), 'v0.13.3-stamp\n');

      expect(resolve(linuxGnu)).toBe(path.join(versioned, 'perllsp'));
    });

    test('a legacy Windows x64 install is adopted only under the emulation key', () => {
      const legacy = legacyManagedBaseDir(storageRoot, 'win32', 'arm64');
      fs.mkdirSync(legacy, { recursive: true });
      const legacyBinary = path.join(legacy, 'perllsp.exe');
      fs.writeFileSync(legacyBinary, peImage(0x8664));

      // The ARM64 host prefers native, finds no native namespace, and falls
      // through to the emulated key — which these bytes do satisfy.
      expect(resolve({ platform: 'win32', arch: 'arm64' })).toBe(legacyBinary);
    });
  });
});
