/**
 * Unit tests for OnboardingManager.
 *
 * Tests cover:
 * - First-run detection via globalState
 * - checkPerlInstalled: detects Perl presence and version
 * - checkPerltidyInstalled: detects perltidy on PATH
 * - checkBinaryDownloaded: confirms server path present
 * - runSetupHealthCheck: full check sequence returning HealthCheckResult
 * - shouldShowWelcome: returns false after welcome flag is set
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  OnboardingManager,
  HealthCheckStatus,
  selectWindowsCommandCandidate,
  resolveUnixShellInvocationFallback,
  toPosixShellCommand,
  classifyStartupFailure,
  PERL_MISSING_MESSAGE,
  STARTUP_DIAGNOSTICS_UNAVAILABLE_MESSAGE,
} from '../onboarding';
import type { HealthCheckResult } from '../onboarding';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeContext(opts?: { welcomed?: boolean; storagePath?: string }): vscode.ExtensionContext {
  const store = new Map<string, unknown>();
  if (opts?.welcomed) {
    store.set('perl-lsp.welcomed', true);
  }
  const dir = opts?.storagePath ?? fs.mkdtempSync(path.join(os.tmpdir(), 'onboarding-test-'));
  return {
    globalStorageUri: { fsPath: dir },
    extensionPath: dir,
    subscriptions: [],
    globalState: {
      get<T>(key: string, defaultValue?: T): T | undefined {
        return (store.has(key) ? store.get(key) : defaultValue) as T | undefined;
      },
      update: jest.fn(async (key: string, value: unknown): Promise<void> => {
        store.set(key, value);
      }),
    },
  } as unknown as vscode.ExtensionContext;
}

function makeOutputChannel(): vscode.OutputChannel {
  return {
    appendLine: jest.fn(),
    show: jest.fn(),
    dispose: jest.fn(),
  } as unknown as vscode.OutputChannel;
}

function mockExecCheck(
  manager: OnboardingManager,
  implementation: (cmd: string, args: string[]) => Promise<{ stdout: string; stderr: string }>,
): void {
  manager._execCheck = jest.fn(implementation);
}

function setWorkspaceFolders(folders: readonly vscode.WorkspaceFolder[] | undefined): void {
  Object.defineProperty(vscode.workspace, 'workspaceFolders', {
    configurable: true,
    value: folders,
  });
}

function findRequired<T>(
  values: readonly T[],
  predicate: (value: T) => boolean,
  description: string,
): T {
  const value = values.find(predicate);
  if (!value) {
    throw new Error(`Expected ${description} in package manifest`);
  }
  return value;
}

// ---------------------------------------------------------------------------
// shouldShowWelcome
// ---------------------------------------------------------------------------

describe('OnboardingManager.shouldShowWelcome', () => {
  test('returns true on first run (welcomed flag not set)', () => {
    const ctx = makeContext();
    const mgr = new OnboardingManager(ctx, makeOutputChannel());
    expect(mgr.shouldShowWelcome()).toBe(true);
  });

  test('returns false if welcomed flag is already set', () => {
    const ctx = makeContext({ welcomed: true });
    const mgr = new OnboardingManager(ctx, makeOutputChannel());
    expect(mgr.shouldShowWelcome()).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// markWelcomed
// ---------------------------------------------------------------------------

describe('OnboardingManager.markWelcomed', () => {
  test('sets the welcomed flag in globalState', async () => {
    const ctx = makeContext();
    const mgr = new OnboardingManager(ctx, makeOutputChannel());
    expect(mgr.shouldShowWelcome()).toBe(true);
    await mgr.markWelcomed();
    expect(ctx.globalState.update).toHaveBeenCalledWith('perl-lsp.welcomed', true);
  });
});

// ---------------------------------------------------------------------------
// checkPerlInstalled
// ---------------------------------------------------------------------------

describe('OnboardingManager.checkPerlInstalled', () => {
  test('returns ok status with version when perl is available', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    // Inject a mock that simulates `perl -e 'print $]'` returning a version
    mockExecCheck(mgr, (_cmd: string, _args: string[]) =>
      Promise.resolve({ stdout: '5.036000', stderr: '' }),
    );
    const result = await mgr.checkPerlInstalled();
    expect(result.ok).toBe(true);
    expect(result.detail).toContain('5.036000');
  });

  test('returns warning status when optional Perl tooling is not found', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, () => Promise.reject(new Error('perl: command not found')));
    const result = await mgr.checkPerlInstalled();
    expect(result.ok).toBe(false);
    expect(result.status).toBe(HealthCheckStatus.Warning);
    expect(result.detail).toContain('strawberryperl.com');
    expect(result.detail).toContain('brew install perl');
    expect(result.detail).toContain('package manager');
    expect(result.detail).toContain('core language server does not require Perl');
    expect(result.detail).not.toContain('command not found');
  });
});

// ---------------------------------------------------------------------------
// checkPerltidyInstalled
// ---------------------------------------------------------------------------

describe('OnboardingManager.checkPerltidyInstalled', () => {
  test('returns ok status when perltidy is available', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, (_cmd: string, _args: string[]) =>
      Promise.resolve({ stdout: 'perltidy, v20230309', stderr: '' }),
    );
    const result = await mgr.checkPerltidyInstalled();
    expect(result.ok).toBe(true);
    expect(result.detail).toContain('perltidy');
  });

  test('returns warning (not error) when perltidy is absent', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, () => Promise.reject(new Error('perltidy: command not found')));
    const result = await mgr.checkPerltidyInstalled();
    expect(result.ok).toBe(false);
    expect(result.status).toBe(HealthCheckStatus.Warning);
  });
});

describe('selectWindowsCommandCandidate', () => {
  test('prefers executable or wrapper paths over extensionless shims', () => {
    const selected = selectWindowsCommandCandidate(
      [
        'C:\\Strawberry\\perl\\bin\\perltidy',
        'C:\\Strawberry\\perl\\bin\\perltidy.bat',
        'C:\\tools\\perltidy.exe',
      ].join('\r\n'),
    );

    expect(selected).toBe('C:\\tools\\perltidy.exe');
  });

  test('returns null for empty where output', () => {
    expect(selectWindowsCommandCandidate(' \r\n \r\n')).toBeNull();
  });
});

describe('toPosixShellCommand', () => {
  test('quotes command and arguments for safe shell execution', () => {
    const command = toPosixShellCommand('perl', ['-e', "print q{can't fail}"]);

    expect(command).toBe("'perl' '-e' 'print q{can'\\''t fail}'");
  });
});

describe('resolveUnixShellInvocationFallback', () => {
  const originalPlatform = process.platform;
  const originalShell = process.env.SHELL;

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform });
    if (originalShell === undefined) {
      delete process.env.SHELL;
    } else {
      process.env.SHELL = originalShell;
    }
  });

  test('returns shell fallback invocation on unix ENOENT errors', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' });
    process.env.SHELL = '/bin/zsh';

    const fallback = resolveUnixShellInvocationFallback(
      { command: 'perl', args: ['-e', 'print $]'] },
      { code: 'ENOENT' },
    );

    expect(fallback).toEqual({
      command: '/bin/zsh',
      args: ['-lc', "'perl' '-e' 'print $]'"],
    });
  });

  test('returns null when no shell is available', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' });
    delete process.env.SHELL;

    const fallback = resolveUnixShellInvocationFallback(
      { command: 'perl', args: ['-e', 'print $]'] },
      { code: 'ENOENT' },
    );

    expect(fallback).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// checkPerlcriticSetup
// ---------------------------------------------------------------------------

describe('OnboardingManager.checkPerlcriticSetup', () => {
  afterEach(() => {
    jest.restoreAllMocks();
    setWorkspaceFolders(undefined);
  });

  test.each([
    ['default', {}],
    ['native', { 'critic.engine': 'native', 'critic.enabled': true }],
    ['disabled', { 'critic.enabled': false, perlcritic: { enabled: false } }],
    ['old enabled alias', { perlcritic: { enabled: true } }],
    ['legacy engine', { 'critic.engine': 'legacy' }],
    ['external engine', { 'critic.engine': 'external' }],
    ['perlcritic engine', { 'critic.engine': 'perlcritic' }],
    ['existing profile', { perlcritic: { profile: __filename } }],
    ['absolute profile', { perlcritic: { profile: 'C:/missing/critic.rc' } }],
    ['relative profile', { perlcritic: { profile: 'config/perlcriticrc' } }],
    ['tilde profile', { perlcritic: { profile: '~/critic.rc' } }],
    [
      'canonical native plus stale alias',
      {
        'critic.engine': 'native',
        'critic.enabled': true,
        perlcritic: { enabled: true },
      },
    ],
  ])('never executes perlcritic or probes profiles: %s', async (_name, settings) => {
    const values = settings as Record<string, unknown>;
    setWorkspaceFolders(undefined);
    const update = jest.fn();
    jest.spyOn(vscode.workspace, 'getConfiguration').mockReturnValue({
      get: jest.fn((key: string, fallback: unknown) => values[key] ?? fallback),
      update,
    } as unknown as vscode.WorkspaceConfiguration);
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, () => Promise.resolve({ stdout: 'perlcritic available', stderr: '' }));
    const profileProbe = jest.spyOn(jest.requireActual<typeof fs>('fs'), 'existsSync');
    const result = await mgr.checkPerlcriticSetup();
    expect(mgr._execCheck).not.toHaveBeenCalled();
    expect(profileProbe).not.toHaveBeenCalled();
    expect(update).not.toHaveBeenCalled();
    expect(result.detail).toContain('Native Critic does not require an external binary');
    expect(result.detail).not.toContain('cpanm');
    expect(result.detail).not.toContain('Perl::Critic is disabled');
    const old = values.perlcritic as { enabled?: boolean; profile?: string } | undefined;
    const needsReview =
      old?.enabled === true ||
      Boolean(old?.profile) ||
      ['legacy', 'external', 'perlcritic'].includes(String(values['critic.engine']));
    expect(result.status).toBe(needsReview ? HealthCheckStatus.Warning : HealthCheckStatus.Ok);
    expect(result.ok).toBe(!needsReview);
  });

  test.each([false, true])(
    'keeps folder observations separate when reversed=%s',
    async (reversed) => {
      const folders = ['A', 'B'].map((name, index) => ({
        name,
        index,
        uri: vscode.Uri.file(path.join(os.tmpdir(), name)),
      }));
      const rootA = folders[0];
      if (reversed) folders.reverse();
      setWorkspaceFolders(folders);
      const scopes: unknown[] = [];
      jest.spyOn(vscode.workspace, 'getConfiguration').mockImplementation((_section, scope) => {
        scopes.push(scope);
        return {
          get: (key: string, fallback: unknown) =>
            scope === rootA?.uri && key === 'critic.engine' ? 'legacy' : fallback,
        } as vscode.WorkspaceConfiguration;
      });
      const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
      mockExecCheck(mgr, () => Promise.reject(new Error('forbidden subprocess')));
      const result = await mgr.checkPerlcriticSetup();
      expect(scopes).toEqual([undefined, folders[0]?.uri, folders[1]?.uri]);
      expect(result.detail).toContain('folder A');
      expect(result.detail).not.toContain('folder B');
      expect(result.status).toBe(HealthCheckStatus.Warning);
      expect(mgr._execCheck).not.toHaveBeenCalled();
    },
  );

  test('composite health retains Perl/perltidy/binary checks without perlcritic execution', async () => {
    setWorkspaceFolders(undefined);
    jest.spyOn(vscode.workspace, 'getConfiguration').mockReturnValue({
      get: jest.fn((key: string, fallback: unknown) =>
        key === 'perlcritic' ? { enabled: true, profile: '~/missing.rc' } : fallback,
      ),
    } as unknown as vscode.WorkspaceConfiguration);
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, (command) =>
      command === 'perlcritic'
        ? Promise.reject(new Error('forbidden subprocess'))
        : Promise.resolve({ stdout: 'available', stderr: '' }),
    );
    const result = await mgr.runSetupHealthCheck(null);
    expect(mgr._execCheck).toHaveBeenCalledWith('perl', ['-e', 'print $]']);
    expect(mgr._execCheck).toHaveBeenCalledWith('perltidy', ['--version']);
    expect(mgr._execCheck).toHaveBeenCalledTimes(2);
    expect(result.find((row) => row.label === 'LSP binary')?.status).toBe(HealthCheckStatus.Error);
    expect(result.find((row) => row.label === 'perlcritic')?.status).toBe(
      HealthCheckStatus.Warning,
    );
  });
});

// ---------------------------------------------------------------------------
// checkBinaryDownloaded
// ---------------------------------------------------------------------------

describe('OnboardingManager.checkBinaryDownloaded', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ob-bin-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  test('returns ok when server path exists', () => {
    const binPath = path.join(tmpDir, 'perl-lsp');
    fs.writeFileSync(binPath, '#!/bin/sh\necho ok');
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    const result = mgr.checkBinaryDownloaded(binPath);
    expect(result.ok).toBe(true);
  });

  test('returns error when server path is null', () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    const result = mgr.checkBinaryDownloaded(null);
    expect(result.ok).toBe(false);
    expect(result.status).toBe(HealthCheckStatus.Error);
  });

  test('returns error when server path does not exist on disk', () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    const result = mgr.checkBinaryDownloaded('/nonexistent/perl-lsp');
    expect(result.ok).toBe(false);
    expect(result.status).toBe(HealthCheckStatus.Error);
  });
});

// ---------------------------------------------------------------------------
// runSetupHealthCheck
// ---------------------------------------------------------------------------

describe('OnboardingManager.runSetupHealthCheck', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ob-health-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  test('returns a HealthCheckResult array with one entry per check', async () => {
    const binPath = path.join(tmpDir, 'perl-lsp');
    fs.writeFileSync(binPath, '#!/bin/sh\necho ok');

    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, (_cmd: string, _args: string[]) =>
      Promise.resolve({ stdout: '5.036000', stderr: '' }),
    );
    const results: HealthCheckResult[] = await mgr.runSetupHealthCheck(binPath);
    expect(Array.isArray(results)).toBe(true);
    expect(results.length).toBeGreaterThanOrEqual(3);
  });

  test('each result has label, ok, status, and detail properties', async () => {
    const binPath = path.join(tmpDir, 'perl-lsp');
    fs.writeFileSync(binPath, '#!/bin/sh\necho ok');

    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, (_cmd: string, _args: string[]) =>
      Promise.resolve({ stdout: '5.036000', stderr: '' }),
    );
    const results: HealthCheckResult[] = await mgr.runSetupHealthCheck(binPath);
    for (const r of results) {
      expect(typeof r.label).toBe('string');
      expect(typeof r.ok).toBe('boolean');
      expect(Object.values(HealthCheckStatus)).toContain(r.status);
      expect(typeof r.detail).toBe('string');
    }
  });

  test('binary check fails when server path is null', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, (_cmd: string, _args: string[]) =>
      Promise.resolve({ stdout: '5.036000', stderr: '' }),
    );
    const results: HealthCheckResult[] = await mgr.runSetupHealthCheck(null);
    const binCheck = results.find((r) => r.label === 'LSP binary');
    expect(binCheck).toBeDefined();
    expect(binCheck!.ok).toBe(false);
  });

  test('all checks pass on a fully healthy environment', async () => {
    const binPath = path.join(tmpDir, 'perl-lsp');
    fs.writeFileSync(binPath, '#!/bin/sh\necho ok');

    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    // Simulate both perl and perltidy available
    mockExecCheck(mgr, (cmd: string, _args: string[]) => {
      if (cmd === 'perl') return Promise.resolve({ stdout: '5.036000', stderr: '' });
      if (cmd === 'perltidy') return Promise.resolve({ stdout: 'perltidy, v20230309', stderr: '' });
      return Promise.reject(new Error('unknown'));
    });
    const results: HealthCheckResult[] = await mgr.runSetupHealthCheck(binPath);
    const errors = results.filter((r) => r.status === HealthCheckStatus.Error);
    expect(errors).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// package.json contract: healthCheck command registered
// ---------------------------------------------------------------------------

describe('package.json health check command', () => {
  const EXT_ROOT = path.resolve(__dirname, '..', '..');
  type CommandContribution = { command: string; category: string; title: string };
  type CommandPaletteEntry = { command: string; when?: string };
  type PackageManifest = {
    contributes: {
      commands: CommandContribution[];
      menus: { commandPalette: CommandPaletteEntry[] };
    };
  };
  let pkg: PackageManifest;

  beforeAll(() => {
    pkg = JSON.parse(
      fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'),
    ) as PackageManifest;
  });

  test('registers perl-lsp.runHealthCheck command', () => {
    const commandIds = pkg.contributes.commands.map((c: CommandContribution) => c.command);
    expect(commandIds).toContain('perl-lsp.runHealthCheck');
  });

  test('health check command has Perl category', () => {
    const cmd = findRequired(
      pkg.contributes.commands,
      (c: CommandContribution) => c.command === 'perl-lsp.runHealthCheck',
      'perl-lsp.runHealthCheck command',
    );
    expect(cmd.category).toBe('Perl');
  });

  test('health check command title is user-friendly', () => {
    const cmd = findRequired(
      pkg.contributes.commands,
      (c: CommandContribution) => c.command === 'perl-lsp.runHealthCheck',
      'perl-lsp.runHealthCheck command',
    );
    expect(cmd.title).toBeTruthy();
    expect(cmd.title.toLowerCase()).toContain('health');
  });

  test('runHealthCheck is declared as a command so VSCode auto-activates the extension', () => {
    // runHealthCheck is palette-global (no when clause restricting to editorLangId == perl).
    // VSCode >= 1.75 automatically activates an extension when any of its declared commands are
    // triggered — explicit onCommand:* activationEvents entries are redundant and have been
    // removed. The guarantee is that the command exists in contributes.commands.
    const commands = pkg.contributes.commands as Array<{ command: string; title: string }>;
    const cmd = commands.find((c) => c.command === 'perl-lsp.runHealthCheck');
    expect(cmd).toBeDefined();
  });

  test('runHealthCheck is listed in commandPalette without a language restriction', () => {
    const palette = pkg.contributes.menus.commandPalette;
    const entry = findRequired(
      palette,
      (e: CommandPaletteEntry) => e.command === 'perl-lsp.runHealthCheck',
      'perl-lsp.runHealthCheck command palette entry',
    );
    // No editorLangId restriction — the health check must be reachable from any context.
    expect(entry.when ?? '').not.toMatch(/editorLangId/);
  });
});

// ---------------------------------------------------------------------------
// classifyStartupFailure
// ---------------------------------------------------------------------------

describe('classifyStartupFailure', () => {
  function makeResult(
    label: string,
    ok: boolean,
    status: HealthCheckStatus,
    detail: string,
  ): HealthCheckResult {
    return { label, ok, status, detail };
  }

  test('returns Perl-missing message when the Perl check explicitly failed', () => {
    const results: HealthCheckResult[] = [
      makeResult('Perl interpreter', false, HealthCheckStatus.Error, 'perl: command not found'),
      makeResult('perltidy', true, HealthCheckStatus.Ok, 'perltidy found'),
      makeResult('LSP binary', true, HealthCheckStatus.Ok, 'Binary found: /usr/bin/perllsp'),
    ];
    const msg = classifyStartupFailure(results);
    expect(msg).toBe(PERL_MISSING_MESSAGE);
    expect(msg).toContain('5.10');
    expect(msg).toContain('strawberryperl.com');
    expect(msg).toContain('brew install perl');
    expect(msg).toContain('package manager');
    expect(msg).toMatch(/install|Install/);
    expect(msg).not.toContain('Restart the server');
  });

  test('returns binary-missing message when binary check failed and Perl is present', () => {
    const results: HealthCheckResult[] = [
      makeResult('Perl interpreter', true, HealthCheckStatus.Ok, 'Perl 5.036000 found'),
      makeResult('perltidy', true, HealthCheckStatus.Ok, 'perltidy found'),
      makeResult('LSP binary', false, HealthCheckStatus.Error, 'perl-lsp binary not found'),
    ];
    const msg = classifyStartupFailure(results);
    expect(msg).not.toContain('Install Perl');
    expect(msg).toMatch(/binary|perllsp/i);
  });

  test('returns generic message when all checks pass (unknown crash)', () => {
    const results: HealthCheckResult[] = [
      makeResult('Perl interpreter', true, HealthCheckStatus.Ok, 'Perl 5.036000 found'),
      makeResult('perltidy', true, HealthCheckStatus.Ok, 'perltidy found'),
      makeResult('LSP binary', true, HealthCheckStatus.Ok, 'Binary found: /usr/bin/perllsp'),
    ];
    const msg = classifyStartupFailure(results);
    expect(msg).toMatch(/Output panel|output/i);
    expect(msg).not.toContain('Install Perl');
    expect(msg).not.toContain('perl-lsp binary not found');
  });

  test('returns unavailable diagnostics when required result rows are absent', () => {
    const msg = classifyStartupFailure([]);
    expect(msg).toBe(STARTUP_DIAGNOSTICS_UNAVAILABLE_MESSAGE);
    expect(msg).not.toContain('Install Perl');
    expect(msg).not.toContain('5.10');
  });
});

// ---------------------------------------------------------------------------
// showWelcomeNotification
// ---------------------------------------------------------------------------

describe('OnboardingManager.showWelcomeNotification', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  test('markWelcomed is NOT called before the notification resolves', async () => {
    const ctx = makeContext();
    const mgr = new OnboardingManager(ctx, makeOutputChannel());
    const updateSpy = ctx.globalState.update as jest.Mock;

    let welcomedDuringNotification = false;
    (vscode.window.showInformationMessage as jest.Mock).mockImplementation(async () => {
      welcomedDuringNotification = updateSpy.mock.calls.some(
        (call) => call[0] === 'perl-lsp.welcomed',
      );
      return undefined;
    });

    await mgr.showWelcomeNotification(null);

    expect(welcomedDuringNotification).toBe(false);
    expect(updateSpy).toHaveBeenCalledWith('perl-lsp.welcomed', true);
  });

  test('markWelcomed is called even when the user dismisses the notification', async () => {
    const ctx = makeContext();
    const mgr = new OnboardingManager(ctx, makeOutputChannel());
    const updateSpy = ctx.globalState.update as jest.Mock;

    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue(undefined);

    await mgr.showWelcomeNotification(null);

    expect(updateSpy).toHaveBeenCalledWith('perl-lsp.welcomed', true);
  });

  test('executes health check command when user clicks "Run Health Check"', async () => {
    const ctx = makeContext();
    const mgr = new OnboardingManager(ctx, makeOutputChannel());

    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Run Health Check');
    const executeCommandSpy = jest.spyOn(vscode.commands, 'executeCommand');

    await mgr.showWelcomeNotification('/path/to/perllsp');

    expect(executeCommandSpy).toHaveBeenCalledWith('perl-lsp.runHealthCheck', '/path/to/perllsp');
  });
});

// ---------------------------------------------------------------------------
// OnboardingManager.runStartupDiagnostics
// ---------------------------------------------------------------------------

describe('OnboardingManager.runStartupDiagnostics', () => {
  test('does not blame optional Perl when the explicit binary check failed', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, () => Promise.reject(new Error('command not found')));

    const msg = await mgr.runStartupDiagnostics(null);

    expect(msg).toMatch(/binary|perllsp/i);
    expect(msg).not.toContain('Install Perl 5.10');
  });

  test('returns binary-missing message when Perl is present but binary not found', async () => {
    const mgr = new OnboardingManager(makeContext(), makeOutputChannel());
    mockExecCheck(mgr, (_cmd: string) => Promise.resolve({ stdout: '5.036000', stderr: '' }));
    const msg = await mgr.runStartupDiagnostics(null);
    expect(msg).toMatch(/binary|perllsp/i);
    expect(msg).not.toContain('Install Perl 5.10');
  });

  test('keeps diagnostic instrument failure distinct from product root causes', async () => {
    const outputChannel = makeOutputChannel();
    const mgr = new OnboardingManager(makeContext(), outputChannel);
    jest
      .spyOn(mgr, 'runSetupHealthCheck')
      .mockRejectedValue(new Error('/private/workspace diagnostic instrument failed'));

    const msg = await mgr.runStartupDiagnostics('/path/to/perllsp');

    expect(msg).toBe(STARTUP_DIAGNOSTICS_UNAVAILABLE_MESSAGE);
    expect(msg).not.toContain('Install Perl');
    expect(msg).not.toContain('/private/workspace');
    expect(outputChannel.appendLine).toHaveBeenCalledWith(
      '[onboarding] Startup diagnostics failed: /private/workspace diagnostic instrument failed',
    );
  });
});
