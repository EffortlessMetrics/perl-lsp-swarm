/**
 * Unit tests for Perl debug adapter configuration and descriptor factory.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import type * as vscode from 'vscode';
import {
  PerlDebugAdapterDescriptorFactory,
  PerlDebugConfigurationProvider,
  activateDebugger,
  buildDapExecutableArgs as productionBuildDapExecutableArgs,
  buildLaunchJsonContent,
  canonicalizeWorkspaceRoot,
  debugConfigTemplateChoices,
  hasLaunchJson,
  offerDebugConfigOnFirstPerlOpen,
  parseDebugTestLaunchTarget,
  resetDebugConfigPromptFlag,
  rewriteTestLensCommand,
  VSCODE_DEBUG_TEST_COMMAND,
  VSCODE_RUN_TEST_COMMAND,
} from '../debugAdapter';
import * as downloader from '../downloader';
import { hostManagedCompatibilityKeys } from '../downloader';
import { managedNamespaceDir } from '../managedStorageIdentity';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
interface LaunchConfiguration {
  type: string;
  request: string;
  name?: string;
  program?: string;
  host?: string;
  port?: number;
  externalPeer?: string;
}

interface LaunchJson {
  version: string;
  configurations: LaunchConfiguration[];
}

function makeContext(storagePath?: string, extensionPath?: string): vscode.ExtensionContext {
  const dir = storagePath ?? fs.mkdtempSync(path.join(os.tmpdir(), 'dap-test-'));
  return {
    globalStorageUri: { fsPath: dir } as vscode.Uri,
    extensionPath: extensionPath ?? dir,
    subscriptions: [],
  } as unknown as vscode.ExtensionContext;
}

function asDebugConfiguration(value: Record<string, unknown>): vscode.DebugConfiguration {
  return value as unknown as vscode.DebugConfiguration;
}

function buildDapExecutableArgs(
  value: unknown,
  hostWorkspaceRoot?: string,
  trustedRoots?: readonly string[],
): string[] {
  return productionBuildDapExecutableArgs(
    value as unknown as vscode.DebugConfiguration | undefined,
    hostWorkspaceRoot,
    trustedRoots,
  );
}

function required<T>(value: T | undefined | null, label: string): T {
  if (value === undefined || value === null) {
    throw new Error(`Missing ${label}`);
  }
  return value;
}

function currentBundledDapDirectory(extensionDir: string): string {
  const platform =
    process.platform === 'linux'
      ? downloader.detectMusl()
        ? 'alpine'
        : 'linux'
      : process.platform;
  return path.join(extensionDir, 'bin', `${platform}-${process.arch}`);
}

// ---------------------------------------------------------------------------
// PerlDebugConfigurationProvider
// ---------------------------------------------------------------------------
describe('PerlDebugConfigurationProvider', () => {
  let provider: PerlDebugConfigurationProvider;

  beforeEach(() => {
    provider = new PerlDebugConfigurationProvider();
    // Launch resolution refuses folder-less sessions (#16554); the default
    // fixture below presents one open workspace folder. Folder-less behavior
    // is covered by the dedicated #16554 tests, which clear this.
    const vscodeGlobal = require('vscode');
    vscodeGlobal.workspace.workspaceFolders = [{ uri: { fsPath: '/ws' }, name: 'ws' }];
  });

  afterEach(() => {
    const vscodeGlobal = require('vscode');
    vscodeGlobal.workspace.workspaceFolders = undefined;
  });

  describe('resolveDebugConfiguration', () => {
    test('fills in defaults for empty config when active editor is Perl', () => {
      const vscode = require('vscode');
      vscode.window.activeTextEditor = {
        document: { languageId: 'perl', uri: { fsPath: '/test.pl' } },
      };

      const config = asDebugConfiguration({});
      provider.resolveDebugConfiguration(undefined, config);

      expect(config.type).toBe('perl');
      expect(config.name).toBe('Launch Perl');
      expect(config.request).toBe('launch');
      expect(config.program).toBe('${file}');

      vscode.window.activeTextEditor = undefined;
    });

    test('fills in the same defaults for a perl5 alias editor (#7699)', () => {
      const vscode = require('vscode');
      vscode.window.activeTextEditor = {
        document: { languageId: 'perl5', uri: { fsPath: '/test.pl' } },
      };

      const config = asDebugConfiguration({});
      provider.resolveDebugConfiguration(undefined, config);

      expect(config.type).toBe('perl');
      expect(config.name).toBe('Launch Perl');
      expect(config.request).toBe('launch');
      expect(config.program).toBe('${file}');

      vscode.window.activeTextEditor = undefined;
    });

    test('rewrites an explicit perl5 alias type onto the contributed perl debugger (#7699)', () => {
      // Only `perl` is a contributed debugger, and only its contributor may
      // register its descriptor factory: a `type: perl5` configuration must
      // resolve to `perl` here, before VS Code looks the debugger up.
      const config = asDebugConfiguration({
        type: 'perl5',
        request: 'launch',
        name: 'Alias Debug',
        program: '/my/script.pl',
      });
      provider.resolveDebugConfiguration(undefined, config);

      expect(config.type).toBe('perl');
      expect(config.request).toBe('launch');
      expect(config.name).toBe('Alias Debug');
      expect(config.program).toBe('/my/script.pl');
    });

    test('does not modify config with existing type/request/name', () => {
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'launch',
        name: 'Custom Debug',
        program: '/my/script.pl',
      });
      const result = provider.resolveDebugConfiguration(undefined, config);
      expect(result).toBeDefined();
      expect((result as vscode.DebugConfiguration).program).toBe('/my/script.pl');
    });

    test('sets attach defaults for TCP mode (no processId)', () => {
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'attach',
        name: 'Attach',
      });
      const result = provider.resolveDebugConfiguration(undefined, config);

      expect((result as vscode.DebugConfiguration).host).toBe('localhost');
      expect((result as vscode.DebugConfiguration).port).toBe(13603);
    });

    test('preserves user-supplied attach host and port', () => {
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'attach',
        name: 'Attach Custom',
        host: '10.0.0.1',
        port: 5000,
      });
      const result = provider.resolveDebugConfiguration(undefined, config);

      expect((result as vscode.DebugConfiguration).host).toBe('10.0.0.1');
      expect((result as vscode.DebugConfiguration).port).toBe(5000);
    });

    test('skips TCP defaults when processId is provided', () => {
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'attach',
        name: 'Attach PID',
        processId: 42,
      });
      const result = provider.resolveDebugConfiguration(undefined, config);

      expect((result as vscode.DebugConfiguration).host).toBeUndefined();
      expect((result as vscode.DebugConfiguration).port).toBeUndefined();
    });

    test('returns undefined when launch has no program', async () => {
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'launch',
        name: 'No Program',
      });
      const result = provider.resolveDebugConfiguration(undefined, config);

      if (result && typeof (result as PromiseLike<unknown>).then === 'function') {
        const resolved = await result;
        expect(resolved).toBeUndefined();
      }
    });

    // #16554: a folder-less launch can never obtain startup authority —
    // perl-dap refuses it over the wire with CLI flags the extension UI does
    // not expose. The provider must refuse before the adapter is spawned.
    test('refuses a folder-less launch with a warning instead of spawning the adapter (#16554)', async () => {
      const vscode = require('vscode');
      vscode.workspace.workspaceFolders = undefined;
      vscode.window.showWarningMessage.mockClear();
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'launch',
        name: 'Launch Perl',
        program: '/test.pl',
      });

      const result = await provider.resolveDebugConfiguration(undefined, config);

      expect(result).toBeUndefined();
      expect(vscode.window.showWarningMessage).toHaveBeenCalledTimes(1);
      expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
        'Perl debugging requires an open workspace folder.',
      );
    });

    test('refuses the synthesized folder-less launch the same way (#16554)', async () => {
      const vscode = require('vscode');
      vscode.workspace.workspaceFolders = undefined;
      vscode.window.showWarningMessage.mockClear();
      vscode.window.activeTextEditor = {
        document: { languageId: 'perl', uri: { fsPath: '/test.pl' } },
      };
      try {
        const result = await provider.resolveDebugConfiguration(
          undefined,
          asDebugConfiguration({}),
        );

        expect(result).toBeUndefined();
        expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
          'Perl debugging requires an open workspace folder.',
        );
      } finally {
        vscode.window.activeTextEditor = undefined;
      }
    });

    test('does not refuse attach requests without a workspace folder (#16554)', () => {
      const vscode = require('vscode');
      vscode.workspace.workspaceFolders = undefined;
      vscode.window.showWarningMessage.mockClear();
      const config = asDebugConfiguration({ type: 'perl', request: 'attach', name: 'Attach' });

      const result = provider.resolveDebugConfiguration(undefined, config);

      expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
      expect((result as vscode.DebugConfiguration).host).toBe('localhost');
      expect((result as vscode.DebugConfiguration).port).toBe(13603);
    });

    test('allows a folder-less launch when perl-lsp.debug.trustedRoots supplies authority (#16554)', async () => {
      const vscode = require('vscode') as {
        workspace: { workspaceFolders: unknown; getConfiguration: jest.Mock };
        window: { showWarningMessage: jest.Mock };
      };
      vscode.workspace.workspaceFolders = undefined;
      vscode.window.showWarningMessage.mockClear();
      const previousConfiguration = vscode.workspace.getConfiguration.getMockImplementation();
      vscode.workspace.getConfiguration.mockImplementation(() => ({
        get: (key: string, defaultValue?: unknown) =>
          key === 'debug.trustedRoots' ? ['/trusted/root-a'] : defaultValue,
      }));
      try {
        const config = asDebugConfiguration({
          type: 'perl',
          request: 'launch',
          name: 'Launch Perl',
          program: '/test.pl',
        });

        const result = await provider.resolveDebugConfiguration(undefined, config);

        // The configured trusted root is the launch authority the server
        // needs, so the launch must not be refused here.
        expect(result).toBe(config);
        expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
      } finally {
        if (previousConfiguration) {
          vscode.workspace.getConfiguration.mockImplementation(previousConfiguration);
        } else {
          vscode.workspace.getConfiguration.mockImplementation(
            () =>
              ({
                get: (_key: string, defaultValue?: unknown) => defaultValue,
                has: () => false,
                inspect: () => undefined,
                update: async () => undefined,
              }) as unknown as ReturnType<typeof vscode.workspace.getConfiguration>,
          );
        }
        vscode.workspace.workspaceFolders = undefined;
      }
    });

    test('allows a folder-less launch that carries external-peer authority (#16554)', () => {
      const vscode = require('vscode');
      vscode.workspace.workspaceFolders = undefined;
      vscode.window.showWarningMessage.mockClear();
      const config = asDebugConfiguration({
        type: 'perl',
        request: 'launch',
        name: 'Peer Launch',
        program: '/test.pl',
        externalPeer: '127.0.0.1:13604',
      });

      const result = provider.resolveDebugConfiguration(undefined, config);

      // perl-dap starts an external-peer adapter before its launch-authority
      // gate, so this folder-less launch works today and must stay allowed.
      expect(result).toBe(config);
      expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
    });
  });

  describe('provideDebugConfigurations', () => {
    test('provides at least 3 default configurations', () => {
      const configs = provider.provideDebugConfigurations(undefined);
      expect(Array.isArray(configs)).toBe(true);
      expect((configs as vscode.DebugConfiguration[]).length).toBeGreaterThanOrEqual(3);
    });

    test('includes launch and attach-by-TCP templates, and no PID template (#8109)', () => {
      const configs = provider.provideDebugConfigurations(undefined) as vscode.DebugConfiguration[];

      const hasLaunch = configs.some((c) => c.request === 'launch');
      const hasTCPAttach = configs.some((c) => c.request === 'attach' && c.port);
      const hasPIDAttach = configs.some((c) => c.request === 'attach' && c.processId);

      expect(hasLaunch).toBe(true);
      expect(hasTCPAttach).toBe(true);
      // #8109: the adapter refuses processId attach fail-closed, so no
      // template may advertise it.
      expect(hasPIDAttach).toBe(false);
    });

    test('all configurations have type "perl"', () => {
      const configs = provider.provideDebugConfigurations(undefined) as vscode.DebugConfiguration[];
      for (const config of configs) {
        expect(config.type).toBe('perl');
      }
    });
  });
});

// ---------------------------------------------------------------------------
// PerlDebugAdapterDescriptorFactory
// ---------------------------------------------------------------------------
describe('PerlDebugAdapterDescriptorFactory', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'dap-factory-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  test('returns undefined and shows an actionable warning when perl-dap is not found anywhere', () => {
    const ctx = makeContext(tmpDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const vscode = require('vscode');

    const origPath = process.env.PATH;
    const origHome = process.env.HOME;
    const origCargo = process.env.CARGO_HOME;
    process.env.PATH = tmpDir;
    process.env.HOME = tmpDir;
    process.env.CARGO_HOME = tmpDir;

    try {
      const result = factory.createDebugAdapterDescriptor(
        {} as unknown as vscode.DebugSession,
        undefined,
      );
      expect(result).toBeUndefined();
      expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
        expect.stringContaining('perl-dap'),
        'Reinstall',
        'Open Debugging Guide',
      );
    } finally {
      process.env.PATH = origPath;
      process.env.HOME = origHome;
      process.env.CARGO_HOME = origCargo;
    }
  });

  test('finds perl-dap in the auto-download directory', () => {
    const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(binDir, { recursive: true });
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const dapPath = path.join(binDir, dapName);
    fs.writeFileSync(dapPath, '#!/bin/sh\necho ok');
    if (process.platform !== 'win32') {
      fs.chmodSync(dapPath, 0o755);
    }

    const ctx = makeContext(tmpDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const result = factory.createDebugAdapterDescriptor(
      {} as unknown as vscode.DebugSession,
      undefined,
    ) as vscode.DebugAdapterExecutable;

    expect(result).toBeDefined();
    expect(result.command).toBe(dapPath);
  });

  test('prefers the packaged perl-dap over a stale ambient adapter', () => {
    const extensionDir = fs.mkdtempSync(path.join(tmpDir, 'extension-'));
    const bundledDir = currentBundledDapDirectory(extensionDir);
    const ambientDir = fs.mkdtempSync(path.join(tmpDir, 'ambient-'));
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const bundledPath = path.join(bundledDir, dapName);
    fs.mkdirSync(bundledDir, { recursive: true });
    fs.writeFileSync(bundledPath, 'bundled dap');
    fs.writeFileSync(path.join(ambientDir, dapName), 'stale ambient dap');
    const managedDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(managedDir, { recursive: true });
    const managedPath = path.join(managedDir, dapName);
    fs.writeFileSync(managedPath, 'stale managed dap');
    if (process.platform !== 'win32') {
      fs.chmodSync(bundledPath, 0o755);
      fs.chmodSync(path.join(ambientDir, dapName), 0o755);
      fs.chmodSync(managedPath, 0o755);
    }

    const ctx = makeContext(tmpDir, extensionDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const originalPath = process.env.PATH;
    const originalHome = process.env.HOME;
    const originalCargo = process.env.CARGO_HOME;
    process.env.PATH = ambientDir;
    process.env.HOME = tmpDir;
    process.env.CARGO_HOME = tmpDir;
    try {
      const result = factory.createDebugAdapterDescriptor(
        {} as unknown as vscode.DebugSession,
        undefined,
      ) as vscode.DebugAdapterExecutable;
      expect(result).toBeDefined();
      expect(result.command).toBe(bundledPath);
    } finally {
      for (const [name, value] of [
        ['PATH', originalPath],
        ['HOME', originalHome],
        ['CARGO_HOME', originalCargo],
      ] as const) {
        if (value === undefined) {
          delete process.env[name];
        } else {
          process.env[name] = value;
        }
      }
    }
  });

  const packagedHostCases = [
    {
      name: 'GNU x64',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: 'linux-x64',
      expected: 'linux-x64',
    },
    {
      name: 'GNU arm64',
      platform: 'linux',
      arch: 'arm64',
      musl: false,
      metadata: 'linux-arm64',
      expected: 'linux-arm64',
    },
    {
      name: 'Alpine x64',
      platform: 'linux',
      arch: 'x64',
      musl: true,
      metadata: 'alpine-x64',
      expected: 'alpine-x64',
    },
    {
      name: 'Alpine arm64',
      platform: 'linux',
      arch: 'arm64',
      musl: true,
      metadata: 'alpine-arm64',
      expected: 'alpine-arm64',
    },
    {
      name: 'Windows x64',
      platform: 'win32',
      arch: 'x64',
      musl: false,
      metadata: 'win32-x64',
      expected: 'win32-x64',
    },
    {
      name: 'Windows arm64',
      platform: 'win32',
      arch: 'arm64',
      musl: false,
      metadata: 'win32-arm64',
      expected: 'win32-arm64',
      windowsSupport: 'windows-11-or-newer',
    },
    {
      name: 'Windows arm64 selects emulated x64 on Windows 11',
      platform: 'win32',
      arch: 'arm64',
      musl: false,
      metadata: 'win32-x64',
      expected: 'win32-x64',
      windowsSupport: 'windows-11-or-newer',
    },
    {
      name: 'Windows arm64 rejects emulated x64 on Windows 10',
      platform: 'win32',
      arch: 'arm64',
      musl: false,
      metadata: 'win32-x64',
      expected: undefined,
      windowsSupport: 'windows-10-or-earlier',
    },
    {
      name: 'Windows arm64 rejects emulated x64 on unknown build',
      platform: 'win32',
      arch: 'arm64',
      musl: false,
      metadata: 'win32-x64',
      expected: undefined,
      windowsSupport: 'unknown',
    },
    {
      name: 'Darwin x64',
      platform: 'darwin',
      arch: 'x64',
      musl: false,
      metadata: 'darwin-x64',
      expected: 'darwin-x64',
    },
    {
      name: 'Darwin arm64',
      platform: 'darwin',
      arch: 'arm64',
      musl: false,
      metadata: 'darwin-arm64',
      expected: 'darwin-arm64',
    },
    {
      name: 'Alpine metadata on GNU',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: 'alpine-x64',
      expected: undefined,
    },
    {
      name: 'GNU metadata on Alpine',
      platform: 'linux',
      arch: 'x64',
      musl: true,
      metadata: 'linux-x64',
      expected: undefined,
    },
    {
      name: 'same-filename wrong OS',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: 'darwin-x64',
      expected: undefined,
    },
    {
      name: 'same-filename wrong architecture',
      platform: 'linux',
      arch: 'arm64',
      musl: false,
      metadata: 'linux-x64',
      expected: undefined,
    },
    {
      name: 'Android refuses ordinary Linux package',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: 'linux-x64',
      environment: 'android',
      expected: undefined,
    },
    {
      name: 'Termux refuses ordinary Linux package',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: 'linux-x64',
      environment: 'termux',
      expected: undefined,
    },
    {
      name: 'unsupported host architecture',
      platform: 'linux',
      arch: 'ia32',
      musl: false,
      metadata: 'linux-x64',
      expected: undefined,
    },
    {
      name: 'missing metadata uses GNU host',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: undefined,
      expected: 'linux-x64',
    },
    {
      name: 'local VSIX undefined target uses Alpine host',
      platform: 'linux',
      arch: 'x64',
      musl: true,
      metadata: 'undefined',
      expected: 'alpine-x64',
    },
    {
      name: 'Windows 11 arm64 missing metadata prefers native payload',
      platform: 'win32',
      arch: 'arm64',
      musl: false,
      metadata: undefined,
      expected: 'win32-arm64',
      windowsSupport: 'windows-11-or-newer',
    },
    {
      name: 'Windows arm64 missing native metadata falls back to emulated payload',
      platform: 'win32',
      arch: 'arm64',
      musl: false,
      metadata: undefined,
      expected: 'win32-x64',
      windowsSupport: 'windows-11-or-newer',
      removeTargets: ['win32-arm64'],
    },
    {
      name: 'malformed package JSON uses host payload',
      platform: 'linux',
      arch: 'x64',
      musl: false,
      metadata: undefined,
      malformed: true,
      expected: 'linux-x64',
    },
  ];

  test.each(packagedHostCases)('packaged factory: $name', (row) => {
    const originalPlatform = required(
      Object.getOwnPropertyDescriptor(process, 'platform'),
      'platform descriptor',
    );
    const originalArch = required(
      Object.getOwnPropertyDescriptor(process, 'arch'),
      'arch descriptor',
    );
    const extensionDir = path.join(tmpDir, 'extension');
    const targets = [
      'linux-x64',
      'linux-arm64',
      'alpine-x64',
      'alpine-arm64',
      'darwin-x64',
      'darwin-arm64',
      'win32-x64',
      'win32-arm64',
    ];
    const payloadPath = (target: string): string =>
      path.join(
        extensionDir,
        'bin',
        target,
        target.startsWith('win32-') ? 'perl-dap.exe' : 'perl-dap',
      );
    // Both compatible and incompatible files exist, including identical names
    // on Darwin/Linux. A missing wrong-target file cannot make refusal pass.
    for (const target of targets) {
      const file = payloadPath(target);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      if (!row.removeTargets?.includes(target)) {
        fs.writeFileSync(file, `${target} packaged adapter`);
        fs.chmodSync(file, 0o755);
      }
    }
    fs.writeFileSync(
      path.join(extensionDir, 'package.json'),
      row.malformed ? '{' : JSON.stringify({ __metadata: { targetPlatform: row.metadata } }),
    );
    const managedPath = path.join(
      tmpDir,
      row.platform === 'win32' ? 'managed-dap.exe' : 'managed-dap',
    );
    fs.writeFileSync(managedPath, 'compatible fallback adapter');
    fs.chmodSync(managedPath, 0o755);
    // Managed resolution is a separate contract. This fixture proves which
    // actual file the registered factory selects, not managed namespace policy.
    const managedSpy = jest
      .spyOn(downloader.BinaryDownloader, 'getLocalDapPath')
      .mockReturnValue(managedPath);
    const muslSpy = jest.spyOn(downloader, 'detectMusl').mockReturnValue(row.musl);
    const windowsSupportSpy = jest
      .spyOn(downloader, 'classifyWindowsArm64Support')
      .mockReturnValue((row.windowsSupport ?? 'not-applicable') as downloader.WindowsArm64Support);
    const androidSpy = jest
      .spyOn(downloader, 'isAndroidEnvironment')
      .mockReturnValue(row.environment === 'android');
    const termuxSpy = jest
      .spyOn(downloader, 'isTermuxEnvironment')
      .mockReturnValue(row.environment === 'termux');
    const vscodeApi = require('vscode') as { workspace: { getConfiguration: jest.Mock } };
    const getConfiguration = vscodeApi.workspace.getConfiguration;
    const previousConfiguration = getConfiguration.getMockImplementation();
    // A conflicting managed-download override must not select a packaged ABI.
    // `debug.trustedRoots` is the #16554 setting read by the descriptor
    // factory on every spawn; it stays registered here so the strictness of
    // this fixture (throw on anything else) keeps its force.
    getConfiguration.mockImplementation(() => ({
      get: (key: string) => {
        if (key === 'debug.trustedRoots') return [];
        if (key !== 'linuxLibc') throw new Error(`Unexpected configuration key: ${key}`);
        return row.musl ? 'gnu' : 'musl';
      },
    }));
    try {
      Object.defineProperty(process, 'platform', { value: row.platform, configurable: true });
      Object.defineProperty(process, 'arch', { value: row.arch, configurable: true });
      const factory = new PerlDebugAdapterDescriptorFactory(makeContext(tmpDir, extensionDir));
      const result = factory.createDebugAdapterDescriptor(
        {} as unknown as vscode.DebugSession,
        undefined,
      ) as vscode.DebugAdapterExecutable;
      expect(result.command).toBe(row.expected ? payloadPath(row.expected) : managedPath);
      if (row.expected) expect(managedSpy).not.toHaveBeenCalled();
      else expect(managedSpy).toHaveBeenCalledTimes(1);
    } finally {
      Object.defineProperty(process, 'platform', originalPlatform);
      Object.defineProperty(process, 'arch', originalArch);
      managedSpy.mockRestore();
      muslSpy.mockRestore();
      windowsSupportSpy.mockRestore();
      androidSpy.mockRestore();
      termuxSpy.mockRestore();
      if (previousConfiguration) getConfiguration.mockImplementation(previousConfiguration);
      else getConfiguration.mockReset();
    }
  });

  test('finds the packaged perl-dap with no ambient search path', () => {
    const extensionDir = fs.mkdtempSync(path.join(tmpDir, 'extension-'));
    const bundledDir = currentBundledDapDirectory(extensionDir);
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const bundledPath = path.join(bundledDir, dapName);
    fs.mkdirSync(bundledDir, { recursive: true });
    fs.writeFileSync(bundledPath, 'bundled dap');
    if (process.platform !== 'win32') {
      fs.chmodSync(bundledPath, 0o755);
    }

    const ctx = makeContext(tmpDir, extensionDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const originalPath = process.env.PATH;
    const originalHome = process.env.HOME;
    const originalCargo = process.env.CARGO_HOME;
    process.env.PATH = '';
    process.env.HOME = tmpDir;
    process.env.CARGO_HOME = tmpDir;
    try {
      const result = factory.createDebugAdapterDescriptor(
        {} as unknown as vscode.DebugSession,
        undefined,
      ) as vscode.DebugAdapterExecutable;
      expect(result).toBeDefined();
      expect(result.command).toBe(bundledPath);
    } finally {
      for (const [name, value] of [
        ['PATH', originalPath],
        ['HOME', originalHome],
        ['CARGO_HOME', originalCargo],
      ] as const) {
        if (value === undefined) {
          delete process.env[name];
        } else {
          process.env[name] = value;
        }
      }
    }
  });

  test('descriptor includes RUST_LOG=debug environment variable', () => {
    const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(binDir, { recursive: true });
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const dapPath = path.join(binDir, dapName);
    fs.writeFileSync(dapPath, '#!/bin/sh\necho ok');
    if (process.platform !== 'win32') {
      fs.chmodSync(dapPath, 0o755);
    }

    const ctx = makeContext(tmpDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const result = factory.createDebugAdapterDescriptor(
      {} as unknown as vscode.DebugSession,
      undefined,
    ) as vscode.DebugAdapterExecutable;

    const env = (result.options as { env?: NodeJS.ProcessEnv } | undefined)?.env;
    expect(env?.RUST_LOG).toBe('debug');
  });

  test('passes --external-peer through to the descriptor when the session sets externalPeer', () => {
    const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(binDir, { recursive: true });
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const dapPath = path.join(binDir, dapName);
    fs.writeFileSync(dapPath, '#!/bin/sh\necho ok');
    if (process.platform !== 'win32') {
      fs.chmodSync(dapPath, 0o755);
    }

    const ctx = makeContext(tmpDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const session = { configuration: { externalPeer: 'localhost:9000' } };
    const result = factory.createDebugAdapterDescriptor(
      session as unknown as vscode.DebugSession,
      undefined,
    ) as vscode.DebugAdapterExecutable;

    expect(result.args).toEqual(['--external-peer', 'localhost:9000']);
  });

  test('uses empty args for a plain launch session', () => {
    const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(binDir, { recursive: true });
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const dapPath = path.join(binDir, dapName);
    fs.writeFileSync(dapPath, '#!/bin/sh\necho ok');
    if (process.platform !== 'win32') {
      fs.chmodSync(dapPath, 0o755);
    }

    const ctx = makeContext(tmpDir);
    const factory = new PerlDebugAdapterDescriptorFactory(ctx);
    const session = { configuration: { request: 'launch', program: '/tmp/x.pl' } };
    const result = factory.createDebugAdapterDescriptor(
      session as unknown as vscode.DebugSession,
      undefined,
    ) as vscode.DebugAdapterExecutable;

    expect(result.args).toEqual([]);
  });

  test('descriptor forwards the session workspace folder as the trusted root', () => {
    const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(binDir, { recursive: true });
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const dapPath = path.join(binDir, dapName);
    fs.writeFileSync(dapPath, '#!/bin/sh\necho ok');
    if (process.platform !== 'win32') {
      fs.chmodSync(dapPath, 0o755);
    }

    const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'dap-ws-'));
    try {
      const ctx = makeContext(tmpDir);
      const factory = new PerlDebugAdapterDescriptorFactory(ctx);
      const session = {
        configuration: { request: 'launch', program: path.join(workspace, 'x.pl') },
        workspaceFolder: { uri: { fsPath: workspace } },
      };
      const result = factory.createDebugAdapterDescriptor(
        session as unknown as vscode.DebugSession,
        undefined,
      ) as vscode.DebugAdapterExecutable;

      expect(result.args).toEqual(['--trusted-root', fs.realpathSync(workspace)]);
    } finally {
      fs.rmSync(workspace, { recursive: true, force: true });
    }
  });

  // #16554: the descriptor factory reads perl-lsp.debug.trustedRoots and
  // forwards each entry, so the refusal advice is followable from settings
  // even when the session carries no workspace folder.
  test('descriptor forwards perl-lsp.debug.trustedRoots as --trusted-root arguments (#16554)', () => {
    const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
    fs.mkdirSync(binDir, { recursive: true });
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    fs.writeFileSync(path.join(binDir, dapName), '#!/bin/sh\necho ok');

    const vscodeApi = require('vscode') as { workspace: { getConfiguration: jest.Mock } };
    const previousConfiguration = vscodeApi.workspace.getConfiguration.getMockImplementation();
    vscodeApi.workspace.getConfiguration.mockImplementation(() => ({
      get: (key: string, defaultValue?: unknown) =>
        key === 'debug.trustedRoots' ? ['/trusted-a', '/trusted-b'] : defaultValue,
    }));
    try {
      const ctx = makeContext(tmpDir);
      const factory = new PerlDebugAdapterDescriptorFactory(ctx);
      const session = { configuration: { request: 'launch', program: '/tmp/x.pl' } };
      const result = factory.createDebugAdapterDescriptor(
        session as unknown as vscode.DebugSession,
        undefined,
      ) as vscode.DebugAdapterExecutable;

      expect(result.args).toEqual(['--trusted-root', '/trusted-a', '--trusted-root', '/trusted-b']);
    } finally {
      if (previousConfiguration) {
        vscodeApi.workspace.getConfiguration.mockImplementation(previousConfiguration);
      } else {
        vscodeApi.workspace.getConfiguration.mockImplementation(
          () =>
            ({
              get: (_key: string, defaultValue?: unknown) => defaultValue,
              has: () => false,
              inspect: () => undefined,
              update: async () => undefined,
            }) as unknown as ReturnType<typeof vscodeApi.workspace.getConfiguration>,
        );
      }
    }
  });

  // Mutation-think: if the guard at the top of createDebugAdapterDescriptor
  // were removed (or demoted to a warning that still spawns native), each case
  // below would return a DebugAdapterExecutable instead of undefined and fail
  // the `toBeUndefined()` assertion; if the typed reason were dropped from the
  // message, `stringContaining(reason)` would fail even though the descriptor
  // still refused.
  test.each([
    [
      {
        externalPeer: '127.0.0.1:13604',
        debuggerBackend: 'external',
        externalDebugger: { mode: 'connect', port: 13604 },
      },
      'not both',
    ],
    [
      {
        debuggerBackend: 'external',
        externalDebugger: { mode: 'connect', control: 'cooperative', port: 13604 },
      },
      'Only mirror control',
    ],
    [{ externalDebugger: { mode: 'connect', port: 13604 } }, 'requires debuggerBackend="external"'],
  ])(
    'factory refuses an invalid explicit backend selection end-to-end and spawns nothing %#',
    (configuration, reason) => {
      const binDir = managedNamespaceDir(tmpDir, hostManagedCompatibilityKeys()[0]!)!;
      fs.mkdirSync(binDir, { recursive: true });
      const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
      fs.writeFileSync(path.join(binDir, dapName), '#!/bin/sh\necho ok');

      const ctx = makeContext(tmpDir);
      const factory = new PerlDebugAdapterDescriptorFactory(ctx);
      const vscodeMock = require('vscode');
      const session = { configuration };

      const result = factory.createDebugAdapterDescriptor(
        session as unknown as vscode.DebugSession,
        undefined,
      );

      expect(result).toBeUndefined();
      expect(vscodeMock.window.showErrorMessage).toHaveBeenCalledWith(
        expect.stringContaining('Perl debugger configuration error'),
        // No action buttons: this refusal is terminal, not an install offer.
      );
      const [message] = vscodeMock.window.showErrorMessage.mock.calls.at(-1) as [string];
      expect(message).toContain(reason as string);
      expect(message).toContain('Native debugging was not started.');
    },
  );
});

// ---------------------------------------------------------------------------
// debug test command wiring
// ---------------------------------------------------------------------------
describe('debug test command helpers', () => {
  test('rewrites server debug-test code lenses to the VS Code command', () => {
    const lens = {
      command: {
        title: 'Debug Test',
        command: 'perl.debugTest',
        arguments: ['file:///tmp/basic.t::test_basic'],
      },
    };

    expect(rewriteTestLensCommand(lens).command.command).toBe(VSCODE_DEBUG_TEST_COMMAND);
  });

  test('rewrites server run-test code lenses to the VS Code command', () => {
    const lens = {
      command: {
        title: 'Run Test',
        command: 'perl.runTest',
        arguments: ['file:///tmp/basic.t::test_basic'],
      },
    };

    expect(rewriteTestLensCommand(lens).command.command).toBe(VSCODE_RUN_TEST_COMMAND);
  });

  test('leaves unrelated code lenses unchanged', () => {
    const lens = {
      command: {
        title: 'Go to definition',
        command: 'perl.goToDefinition',
      },
    };

    expect(rewriteTestLensCommand(lens)).toEqual(lens);
  });

  test('parses a code-lens test id into a launch target', () => {
    const fileUri = process.platform === 'win32' ? 'file:///C:/tmp/basic.t' : 'file:///tmp/basic.t';
    const expectedProgram =
      process.platform === 'win32' ? path.normalize('C:/tmp/basic.t') : '/tmp/basic.t';

    expect(parseDebugTestLaunchTarget(`${fileUri}::test_basic`)).toEqual({
      label: 'test_basic',
      program: expectedProgram,
      args: [],
    });
  });

  test('parses a TestItem-like object into a launch target', () => {
    expect(
      parseDebugTestLaunchTarget({
        label: 'constructor',
        uri: { fsPath: path.normalize('/workspace/t/basic.t') },
        args: ['--verbose'],
      }),
    ).toEqual({
      label: 'constructor',
      program: path.normalize('/workspace/t/basic.t'),
      args: ['--verbose'],
    });
  });

  test('returns undefined for an invalid debug target payload', () => {
    expect(parseDebugTestLaunchTarget(null)).toBeUndefined();
    expect(parseDebugTestLaunchTarget({ label: 'missing-uri' })).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// perl-lsp.debugTest command path (#16554)
// ---------------------------------------------------------------------------
describe('perl-lsp.debugTest command', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'dap-debugtest-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
    const vscode = require('vscode');
    vscode.workspace.workspaceFolders = undefined;
    (vscode.debug.startDebugging as jest.Mock).mockClear();
    vscode.window.showWarningMessage.mockClear();
  });

  function registerCommandHandler(): void {
    activateDebugger(makeContext(tmpDir));
  }

  test('refuses a folder-less debug test launch before startDebugging (#16554)', async () => {
    const vscode = require('vscode');
    vscode.workspace.workspaceFolders = undefined;
    registerCommandHandler();

    const result = await vscode.commands.executeCommand(
      VSCODE_DEBUG_TEST_COMMAND,
      'file:///tmp/basic.t::test_basic',
    );

    expect(result).toBeUndefined();
    expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
      'Perl debugging requires an open workspace folder.',
    );
    expect(vscode.debug.startDebugging).not.toHaveBeenCalled();
  });

  test('launches with an open workspace folder and no refusal (#16554)', async () => {
    const vscode = require('vscode');
    vscode.workspace.workspaceFolders = [{ uri: { fsPath: '/ws' }, name: 'ws' }];
    registerCommandHandler();

    const fileUri = process.platform === 'win32' ? 'file:///C:/tmp/basic.t' : 'file:///tmp/basic.t';
    await vscode.commands.executeCommand(VSCODE_DEBUG_TEST_COMMAND, `${fileUri}::test_basic`);

    expect(vscode.debug.startDebugging).toHaveBeenCalledTimes(1);
    const [passedFolder, config] = (vscode.debug.startDebugging as jest.Mock).mock.calls[0] as [
      unknown,
      Record<string, unknown>,
    ];
    // The session folder stays host-resolved: passing `workspaceFolders[0]`
    // here would pin a multiroot session to the first folder and mint a
    // trusted root that can exclude the debugged program.
    expect(passedFolder).toBeUndefined();
    expect(config).toMatchObject({
      type: 'perl',
      request: 'launch',
      name: 'Debug test_basic',
      program: process.platform === 'win32' ? path.normalize('C:/tmp/basic.t') : '/tmp/basic.t',
    });
    expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
  });

  test('an unresolvable test target still gets its specific error, not the folder warning (#16554)', async () => {
    const vscode = require('vscode');
    vscode.workspace.workspaceFolders = undefined;
    registerCommandHandler();

    const result = await vscode.commands.executeCommand(VSCODE_DEBUG_TEST_COMMAND, {
      label: 'unresolvable',
    });

    expect(result).toBeUndefined();
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining('Cannot debug this test'),
    );
    expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
    expect(vscode.debug.startDebugging).not.toHaveBeenCalled();
  });

  test('still launches folder-less when perl-lsp.debug.trustedRoots supplies authority (#16554)', async () => {
    const vscode = require('vscode') as {
      commands: { executeCommand: jest.Mock };
      workspace: { workspaceFolders: unknown; getConfiguration: jest.Mock };
      window: { showWarningMessage: jest.Mock };
      debug: { startDebugging: jest.Mock };
    };
    vscode.workspace.workspaceFolders = undefined;
    const previousConfiguration = vscode.workspace.getConfiguration.getMockImplementation();
    vscode.workspace.getConfiguration.mockImplementation(() => ({
      get: (key: string, defaultValue?: unknown) =>
        key === 'debug.trustedRoots' ? ['/trusted/root-a'] : defaultValue,
    }));
    try {
      registerCommandHandler();

      await vscode.commands.executeCommand(
        VSCODE_DEBUG_TEST_COMMAND,
        'file:///tmp/basic.t::test_basic',
      );

      expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
      expect(vscode.debug.startDebugging).toHaveBeenCalledWith(
        undefined,
        expect.objectContaining({ type: 'perl', request: 'launch' }),
      );
    } finally {
      if (previousConfiguration) {
        vscode.workspace.getConfiguration.mockImplementation(previousConfiguration);
      } else {
        vscode.workspace.getConfiguration.mockImplementation(
          () =>
            ({
              get: (_key: string, defaultValue?: unknown) => defaultValue,
              has: () => false,
              inspect: () => undefined,
              update: async () => undefined,
            }) as unknown as ReturnType<typeof vscode.workspace.getConfiguration>,
        );
      }
    }
  });
});

// ---------------------------------------------------------------------------
// buildLaunchJsonContent
// ---------------------------------------------------------------------------
describe('buildLaunchJsonContent', () => {
  test('launch-script template produces valid JSON with perl type', () => {
    const content = buildLaunchJsonContent('launch-script');
    const parsed = JSON.parse(content) as LaunchJson;
    expect(parsed.version).toBe('0.2.0');
    expect(Array.isArray(parsed.configurations)).toBe(true);
    const cfg = required(parsed.configurations[0], 'launch-script configuration');
    expect(cfg.type).toBe('perl');
    expect(cfg.request).toBe('launch');
    expect(cfg.program).toBe('${file}');
  });

  test('package launch defaults target the active file, while attach stays TCP', () => {
    const manifest = JSON.parse(
      fs.readFileSync(path.join(__dirname, '..', '..', 'package.json'), 'utf8'),
    ) as {
      contributes: {
        debuggers: Array<{
          configurationAttributes: { launch: { properties: { program: { default: string } } } };
          initialConfigurations: LaunchConfiguration[];
        }>;
      };
    };
    const debuggerContribution = required(manifest.contributes.debuggers[0], 'Perl debugger');
    expect(debuggerContribution.configurationAttributes.launch.properties.program.default).toBe(
      '${file}',
    );
    const launch = required(debuggerContribution.initialConfigurations[0], 'initial launch');
    expect(launch.request).toBe('launch');
    expect(launch.program).toBe('${file}');
    const attach = required(debuggerContribution.initialConfigurations[1], 'initial attach');
    expect(attach.request).toBe('attach');
    expect(attach.host).toBe('localhost');
  });

  test('attach-process template produces attach config with host and port', () => {
    const content = buildLaunchJsonContent('attach-process');
    const parsed = JSON.parse(content) as LaunchJson;
    const cfg = required(parsed.configurations[0], 'attach-process configuration');
    expect(cfg.type).toBe('perl');
    expect(cfg.request).toBe('attach');
    expect(cfg.host).toBe('localhost');
    expect(cfg.port).toBe(13603);
  });

  test('remote-tcp-attach template produces a normal TCP attach config (#9868)', () => {
    const content = buildLaunchJsonContent('remote-tcp-attach');
    const parsed = JSON.parse(content) as LaunchJson;
    const cfg = required(parsed.configurations[0], 'remote-tcp-attach configuration');
    expect(cfg.type).toBe('perl');
    expect(cfg.request).toBe('attach');
    // Safe default endpoint compatible with an existing local port forward,
    // not a placeholder remote SSH target.
    expect(cfg.name).toBe('Perl: Remote TCP Attach');
    expect(cfg.host).toBe('localhost');
    expect(cfg.port).toBe(13603);
  });

  test('legacy remote-ssh selector aliases the same honest TCP attach config (#9868)', () => {
    const legacy = buildLaunchJsonContent('remote-ssh');
    expect(legacy).toBe(buildLaunchJsonContent('remote-tcp-attach'));
  });

  test('all template uses the corrected remote TCP attach name and copy (#9868)', () => {
    const content = buildLaunchJsonContent('all');
    const parsed = JSON.parse(content) as LaunchJson;
    const names = parsed.configurations.map((config) => config.name);
    expect(names).toContain('Perl: Remote TCP Attach');
    expect(content).not.toContain('Remote (SSH)');
    expect(content).not.toContain('remote-host');
  });

  test('wizard copy never claims built-in SSH or tunnel ownership (#9868)', () => {
    const choices = debugConfigTemplateChoices();
    expect(choices.length).toBeGreaterThan(0);
    for (const choice of choices) {
      const copy = `${choice.label} ${choice.description} ${choice.detail}`;
      expect(copy).not.toMatch(/ssh/i);
      expect(copy).not.toMatch(/\btunnel\b/i);
    }
    // Every generated configuration stays free of SSH/tunnel claims too.
    const templates = [
      'launch-script',
      'attach-process',
      'remote-tcp-attach',
      'external-peer',
      'all',
    ];
    for (const template of templates) {
      expect(buildLaunchJsonContent(template)).not.toMatch(/ssh/i);
      expect(buildLaunchJsonContent(template)).not.toMatch(/\btunnel\b/i);
    }
  });

  test('remote TCP attach copy stays inside the accepted attach surface (#9868, #5257)', () => {
    const choices = debugConfigTemplateChoices();
    const remote = choices.find((choice) => choice.template === 'remote-tcp-attach');
    expect(remote).toBeDefined();
    const copy = `${required(remote, 'remote-tcp-attach choice').label} ${
      required(remote, 'remote-tcp-attach choice').description
    } ${required(remote, 'remote-tcp-attach choice').detail}`;
    // Adapter validation resolves the host and refuses private/link-local
    // addresses, so the copy must not invite a "direct" non-loopback endpoint.
    expect(copy).not.toMatch(/\bdirect(ly)?\b/i);
    // The supported remote route is a user-run port forward to loopback, and
    // the non-loopback private-host refusal is stated, not implied.
    expect(copy).toMatch(/port forward/i);
    expect(copy).toMatch(/loopback/i);
    expect(copy).toMatch(/private\/link-local/i);
  });

  test('remote-tcp-attach wizard default round-trips resolveDebugConfiguration unchanged (#9868)', () => {
    const content = buildLaunchJsonContent('remote-tcp-attach');
    const parsed = JSON.parse(content) as LaunchJson;
    const config = asDebugConfiguration(
      parsed.configurations[0] as unknown as Record<string, unknown>,
    );
    const provider = new PerlDebugConfigurationProvider();
    const result = provider.resolveDebugConfiguration(undefined, config);
    // The attach path must resolve synchronously to the same config object.
    if (!result || typeof (result as { then?: unknown }).then === 'function') {
      throw new Error('resolveDebugConfiguration must resolve synchronously for attach configs');
    }
    const resolved = result as vscode.DebugConfiguration;
    // The wizard default is a loopback endpoint and the provider passes it
    // through unchanged; any private-host refusal happens at the adapter
    // boundary (pinned Rust-side by crates/perl-dap/tests/tcp_attach_tests.rs).
    expect(resolved).toBe(config);
    expect(resolved.type).toBe('perl');
    expect(resolved.request).toBe('attach');
    expect(resolved.host).toBe('localhost');
    expect(resolved.port).toBe(13603);
  });

  test('all template produces multiple configurations', () => {
    const content = buildLaunchJsonContent('all');
    const parsed = JSON.parse(content) as LaunchJson;
    expect(parsed.configurations.length).toBeGreaterThanOrEqual(3);
    const types = parsed.configurations.map((config) => config.type);
    expect(types.every((type) => type === 'perl')).toBe(true);
  });

  test('unknown template falls back to launch-script', () => {
    const content = buildLaunchJsonContent('unknown-template');
    const parsed = JSON.parse(content) as LaunchJson;
    const cfg = required(parsed.configurations[0], 'fallback configuration');
    expect(cfg.type).toBe('perl');
    expect(cfg.request).toBe('launch');
  });

  test('external-peer template carries the externalPeer field', () => {
    const content = buildLaunchJsonContent('external-peer');
    const parsed = JSON.parse(content) as LaunchJson;
    const cfg = required(parsed.configurations[0], 'external-peer configuration');
    expect(cfg.type).toBe('perl');
    expect(cfg.request).toBe('attach');
    expect(typeof cfg.externalPeer).toBe('string');
    expect(cfg.externalPeer).toMatch(/^[^\s:]+:\d+$/);
  });

  test('all template includes the external-peer configuration', () => {
    const content = buildLaunchJsonContent('all');
    const parsed = JSON.parse(content) as LaunchJson;
    const hasPeer = parsed.configurations.some((config) => typeof config.externalPeer === 'string');
    expect(hasPeer).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// buildDapExecutableArgs
// ---------------------------------------------------------------------------
describe('buildDapExecutableArgs', () => {
  test('passes --external-peer through for a valid host:port', () => {
    expect(buildDapExecutableArgs({ externalPeer: 'localhost:9000' })).toEqual([
      '--external-peer',
      'localhost:9000',
    ]);
  });

  test('trims surrounding whitespace on the peer address', () => {
    expect(buildDapExecutableArgs({ externalPeer: '  127.0.0.1:13700  ' })).toEqual([
      '--external-peer',
      '127.0.0.1:13700',
    ]);
  });

  test('returns no args when externalPeer is absent', () => {
    expect(buildDapExecutableArgs({ request: 'launch' })).toEqual([]);
    expect(buildDapExecutableArgs(undefined)).toEqual([]);
  });

  test('ignores a malformed peer address rather than passing it through', () => {
    expect(buildDapExecutableArgs({ externalPeer: 'not-a-peer' })).toEqual([]);
    expect(buildDapExecutableArgs({ externalPeer: 'host:' })).toEqual([]);
    expect(buildDapExecutableArgs({ externalPeer: 42 })).toEqual([]);
  });

  test('falls back to native for a non-connectable port in the flat shape', () => {
    // `host:0` (0 = "allocate", not connectable) and out-of-range ports must
    // fall back to the native adapter, consistent with the structured shape —
    // not spawn an unconnectable `--external-peer host:0`.
    expect(buildDapExecutableArgs({ externalPeer: 'localhost:0' })).toEqual([]);
    expect(buildDapExecutableArgs({ externalPeer: 'localhost:70000' })).toEqual([]);
  });

  test('falls back to the native adapter for a bracketed IPv6 peer address', () => {
    // The validator requires the host segment to contain no ':' (so a plain
    // "host:port" split is unambiguous), so a bracketed IPv6 literal like
    // "[::1]:9000" does not match and the adapter falls back to native mode
    // rather than passing an unvalidated value through. This documents a
    // known scope limit, not a crash or injection risk.
    expect(buildDapExecutableArgs({ externalPeer: '[::1]:9000' })).toEqual([]);
    expect(buildDapExecutableArgs({ externalPeer: '::1:9000' })).toEqual([]);
  });

  test('rejects a peer address with embedded whitespace instead of splitting on it', () => {
    // Guards against argv smuggling: a value like "host --some-flag:9000"
    // must not turn into a second, attacker-controlled CLI argument for the
    // spawned perl-dap process.
    expect(buildDapExecutableArgs({ externalPeer: 'host --flag:9000' })).toEqual([]);
    expect(buildDapExecutableArgs({ externalPeer: 'host:9000 --flag' })).toEqual([]);
  });

  test('translates the shipped structured externalDebugger (connect) shape', () => {
    const config = {
      debuggerBackend: 'external',
      externalDebugger: {
        kind: 'ptkdb',
        mode: 'connect',
        control: 'mirror',
        host: '127.0.0.1',
        port: 13604,
      },
    };
    expect(buildDapExecutableArgs(config)).toEqual(['--external-peer', '127.0.0.1:13604']);
  });

  test('defaults host to 127.0.0.1 and mode to connect for the structured shape', () => {
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { port: 9001 },
      }),
    ).toEqual(['--external-peer', '127.0.0.1:9001']);
  });

  test('wires listen mode to --external-peer-listen (port 0 = allocate ephemeral)', () => {
    // A concrete port binds it; port 0 / absent asks perl-dap to allocate one,
    // so only the host is passed as the bind spec.
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'listen', control: 'mirror', host: '127.0.0.1', port: 13604 },
      }),
    ).toEqual(['--external-peer-listen', '127.0.0.1:13604']);
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'listen', control: 'mirror', host: '127.0.0.1', port: 0 },
      }),
    ).toEqual(['--external-peer-listen', '127.0.0.1']);
    // Defaults host to 127.0.0.1 when omitted.
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'listen' },
      }),
    ).toEqual(['--external-peer-listen', '127.0.0.1']);
  });

  test('does not fabricate an address for unimplemented modes or connect port 0', () => {
    // launchPeer is not wired; connect requires a concrete port — fall back to native.
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'launchPeer', port: 13604 },
      }),
    ).toEqual([]);
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'connect', port: 0 },
      }),
    ).toEqual([]);
  });

  test('rejects a listen host that could smuggle extra argv tokens', () => {
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'listen', host: 'host --flag' },
      }),
    ).toEqual([]);
    expect(
      buildDapExecutableArgs({
        debuggerBackend: 'external',
        externalDebugger: { mode: 'listen', host: 'a:b' },
      }),
    ).toEqual([]);
  });

  test('the native backend (or absent debuggerBackend) yields no bridge args', () => {
    expect(buildDapExecutableArgs({ debuggerBackend: 'native', program: '/x.pl' })).toEqual([]);
    expect(buildDapExecutableArgs({ request: 'launch', program: '/x.pl' })).toEqual([]);
  });

  test('native editor sessions receive host-owned workspace authority', () => {
    expect(buildDapExecutableArgs({ request: 'launch', program: '/x.pl' }, '/workspace')).toEqual([
      '--trusted-root',
      '/workspace',
    ]);
  });

  test('a symlinked workspace root is canonicalized before handoff', () => {
    const real = fs.mkdtempSync(path.join(os.tmpdir(), 'dap-real-'));
    const link = `${real}-link`;
    try {
      fs.symlinkSync(real, link, 'dir');
    } catch {
      // Windows CI without symlink privilege cannot create the link; the
      // fallback path (unresolvable input passes through) is covered below.
      expect(canonicalizeWorkspaceRoot(`${real}-missing`)).toBe(`${real}-missing`);
      return;
    }
    try {
      expect(canonicalizeWorkspaceRoot(link)).toBe(fs.realpathSync(real));
      expect(buildDapExecutableArgs({ request: 'launch' }, link)).toEqual([
        '--trusted-root',
        fs.realpathSync(real),
      ]);
    } finally {
      fs.rmSync(link, { recursive: true, force: true });
      fs.rmSync(real, { recursive: true, force: true });
    }
  });

  // #16554: perl-dap declares --trusted-root repeatable (Vec<PathBuf>), so a
  // configured perl-lsp.debug.trustedRoots entry must become one flag each.
  test('forwards one --trusted-root per perl-lsp.debug.trustedRoots entry (#16554)', () => {
    const rootA = fs.mkdtempSync(path.join(os.tmpdir(), 'dap-trusted-a-'));
    const rootB = fs.mkdtempSync(path.join(os.tmpdir(), 'dap-trusted-b-'));
    try {
      expect(buildDapExecutableArgs({ request: 'launch' }, undefined, [rootA, rootB])).toEqual([
        '--trusted-root',
        fs.realpathSync(rootA),
        '--trusted-root',
        fs.realpathSync(rootB),
      ]);
    } finally {
      fs.rmSync(rootA, { recursive: true, force: true });
      fs.rmSync(rootB, { recursive: true, force: true });
    }
  });

  test('forwards the session workspace root together with configured roots (#16554)', () => {
    expect(buildDapExecutableArgs({ request: 'launch' }, '/workspace', ['/trusted'])).toEqual([
      '--trusted-root',
      '/workspace',
      '--trusted-root',
      '/trusted',
    ]);
  });

  test('ignores blank and non-string trusted root entries instead of emitting broken argv (#16554)', () => {
    expect(
      buildDapExecutableArgs({ request: 'launch' }, undefined, [
        '',
        '   ',
        42 as unknown as string,
        null as unknown as string,
      ]),
    ).toEqual([]);
  });

  test('an empty trustedRoots list still yields no flags without a workspace root', () => {
    expect(buildDapExecutableArgs({ request: 'launch' }, undefined, [])).toEqual([]);
  });

  test('never emits an editor --socket or --port flag', () => {
    const configs: Array<Record<string, unknown> | undefined> = [
      undefined,
      { request: 'launch', program: '/x.pl' },
      { externalPeer: 'localhost:9000' },
      {
        debuggerBackend: 'external',
        externalDebugger: { host: '127.0.0.1', port: 13604 },
      },
      {
        debuggerBackend: 'external',
        externalDebugger: { mode: 'listen', host: '127.0.0.1' },
      },
      {
        debuggerBackend: 'external',
        externalDebugger: {
          mode: 'listen',
          host: '127.0.0.1',
          port: 0,
        },
      },
    ];
    for (const config of configs) {
      const args = buildDapExecutableArgs(config);
      expect(args).not.toContain('--socket');
      expect(args).not.toContain('--port');
    }
  });
});

// ---------------------------------------------------------------------------
// hasLaunchJson
// ---------------------------------------------------------------------------
describe('hasLaunchJson', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'launch-json-test-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  test('returns false when .vscode/launch.json does not exist', () => {
    expect(hasLaunchJson(tmpDir)).toBe(false);
  });

  test('returns false when .vscode directory is missing', () => {
    expect(hasLaunchJson(path.join(tmpDir, 'nonexistent'))).toBe(false);
  });

  test('returns true when .vscode/launch.json exists', () => {
    const vscodDir = path.join(tmpDir, '.vscode');
    fs.mkdirSync(vscodDir);
    fs.writeFileSync(path.join(vscodDir, 'launch.json'), '{}');
    expect(hasLaunchJson(tmpDir)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// offerDebugConfigOnFirstPerlOpen
// ---------------------------------------------------------------------------
describe('offerDebugConfigOnFirstPerlOpen', () => {
  const vscode = require('vscode');

  beforeEach(() => {
    resetDebugConfigPromptFlag();
    jest.clearAllMocks();
    vscode.workspace.workspaceFolders = undefined;
  });

  afterEach(() => {
    vscode.workspace.workspaceFolders = undefined;
  });

  test('does nothing for non-perl documents', async () => {
    const doc = { languageId: 'javascript' };
    await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
    expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
  });

  test('does nothing when no workspace folders are open', async () => {
    vscode.workspace.workspaceFolders = [];
    const doc = { languageId: 'perl' };
    await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
    expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
  });

  test('shows the same onboarding prompt for a perl5 alias document (#7699)', async () => {
    // Runs before any test that trips the once-per-session prompt flag, so the
    // alias itself must pass the language gate for the prompt to appear.
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'onboard-perl5-'));
    try {
      vscode.workspace.workspaceFolders = [{ uri: { fsPath: tmpDir }, name: 'test' }];
      const doc = { languageId: 'perl5' };
      await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
      expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
        expect.stringContaining('debug configuration'),
        expect.any(String),
        expect.any(String),
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  test('shows onboarding prompt for perl document in workspace without launch.json', async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'onboard-test-'));
    try {
      vscode.workspace.workspaceFolders = [{ uri: { fsPath: tmpDir }, name: 'test' }];
      const doc = { languageId: 'perl' };
      await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
      expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
        expect.stringContaining('debug configuration'),
        expect.any(String),
        expect.any(String),
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  test('does not show prompt when launch.json already exists', async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'onboard-exists-'));
    try {
      const vscodDir = path.join(tmpDir, '.vscode');
      fs.mkdirSync(vscodDir);
      fs.writeFileSync(path.join(vscodDir, 'launch.json'), '{}');
      vscode.workspace.workspaceFolders = [{ uri: { fsPath: tmpDir }, name: 'test' }];
      const doc = { languageId: 'perl' };
      await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
      expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  test('shows prompt only once per session even with multiple perl opens', async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'onboard-once-'));
    try {
      vscode.workspace.workspaceFolders = [{ uri: { fsPath: tmpDir }, name: 'test' }];
      const doc = { languageId: 'perl' };
      await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
      await offerDebugConfigOnFirstPerlOpen(doc as vscode.TextDocument);
      expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(1);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});
