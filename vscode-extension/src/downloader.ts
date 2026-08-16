import * as vscode from 'vscode';
import * as https from 'https';
import * as http from 'http';
import * as fs from 'fs';
import * as path from 'path';
import * as crypto from 'crypto';
import * as os from 'os';
import { promisify } from 'util';
import * as child_process from 'child_process';
import * as tar from 'tar';
import AdmZip from 'adm-zip';
import {
  admissibleManagedCompatibilityKeys,
  buildManagedCompatibilityKey,
  classifyLegacyManagedCandidate,
  legacyManagedBaseDir,
  managedNamespaceDir,
  managedUpdateCheckStateKey,
  probeBinaryIdentity,
  LEGACY_UPDATE_CHECK_STATE_KEY,
  type ManagedEmulation,
} from './managedStorageIdentity';

const execFile = promisify(child_process.execFile);

interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}

interface Release {
  tag_name: string;
  prerelease?: boolean;
  /** Synthetic metadata produced for an internal download mirror. */
  internal?: boolean;
  assets: ReleaseAsset[];
}

// Retry budget for transient managed-install file locks. Total wait grows to
// ~31s so first-time Windows Defender signature scans of freshly extracted
// perllsp.exe (5–15s on cold caches) and end-of-life lock release for a
// running perllsp.exe both fit comfortably.
const MANAGED_INSTALL_RETRY_DELAYS_MS = [100, 250, 500, 1000, 2000, 4000, 8000, 16000];
const TRANSIENT_MANAGED_INSTALL_ERROR_CODES = new Set(['EBUSY', 'EPERM', 'EACCES', 'ETXTBSY']);
const WINDOWS_11_MIN_BUILD = 22000;

export type WindowsArm64Support =
  | 'not-applicable'
  | 'windows-11-or-newer'
  | 'windows-10-or-earlier'
  | 'unknown';

export function parseWindowsBuildNumber(release: string): number | null {
  const match = /^(\d+)\.(\d+)\.(\d+)/.exec(release);
  if (!match) {
    return null;
  }

  const major = Number(match[1]);
  const minor = Number(match[2]);
  const build = Number(match[3]);
  if (![major, minor, build].every(Number.isSafeInteger)) {
    return null;
  }

  // Keep future Windows versions fail-open for ARM64 x64 emulation while
  // preserving the Windows 10 build boundary. A newer major/minor version is
  // represented as at least the Windows 11 minimum for classification.
  if (major > 10 || (major === 10 && minor > 0)) {
    return Math.max(build, WINDOWS_11_MIN_BUILD);
  }

  return build;
}

export function classifyWindowsArm64Support(
  platform = process.platform,
  arch = process.arch,
  release = os.release(),
): WindowsArm64Support {
  if (platform !== 'win32' || arch !== 'arm64') {
    return 'not-applicable';
  }

  const build = parseWindowsBuildNumber(release);
  if (build === null) {
    return 'unknown';
  }

  return build >= WINDOWS_11_MIN_BUILD ? 'windows-11-or-newer' : 'windows-10-or-earlier';
}

export function getUnsupportedWindowsArm64Message(
  platform = process.platform,
  arch = process.arch,
  release = os.release(),
): string | undefined {
  const support = classifyWindowsArm64Support(platform, arch, release);
  if (support !== 'windows-10-or-earlier' && support !== 'unknown') {
    return undefined;
  }

  const detected = release
    ? `detected OS release ${release}`
    : 'the Windows build could not be detected';
  return (
    `Windows ARM64 x64 emulation requires Windows 11 (build ${WINDOWS_11_MIN_BUILD} or newer); ${detected}. ` +
    'Windows 10 ARM64 cannot run the published x86_64 binary. Build from source with ' +
    '`cargo install --locked --path crates/perllsp` and configure perl-lsp.serverPath instead.'
  );
}

export const WINDOWS_X64_TARGET = 'x86_64-pc-windows-msvc';
export const WINDOWS_ARM64_TARGET = 'aarch64-pc-windows-msvc';

export interface WindowsArm64Selection {
  /** The target triple to download. */
  target: string;
  /** True when the native ARM64 asset was absent and x64 emulation was chosen. */
  emulated: boolean;
  /** Set when neither path is usable; the caller must fail with this message. */
  error?: string;
  /** Human-readable reason, for the output channel. */
  reason: string;
}

/**
 * Choose the Windows ARM64 download target for one specific release.
 *
 * The release matrix builds a native `aarch64-pc-windows-msvc` since #5208, so
 * prefer it. This must stay a *preference* rather than a requirement: that
 * target was added on 2026-08-03 and no release has been cut since, so the
 * asset has never actually been produced, and every existing release carries
 * only the x64 archive. Requiring it would break installing any published tag.
 *
 * The Windows 11 build floor gates ONLY the emulation fallback. It is a
 * property of running x64 code on ARM64, not of ARM64 itself — a native ARM64
 * binary runs fine on Windows 10 ARM64. Applying it to the native path is the
 * defect this function exists to prevent: it refused an install that works and
 * sent the user to a source build they did not need (#6196).
 */
export function selectWindowsArm64Target(
  assets: ReadonlyArray<{ name: string }>,
  versionOrTag: string,
  ext: string,
  release = os.release(),
): WindowsArm64Selection {
  if (findReleaseAssetName(assets, versionOrTag, WINDOWS_ARM64_TARGET, ext)) {
    return {
      target: WINDOWS_ARM64_TARGET,
      emulated: false,
      reason: `Using the native ARM64 Windows build (${WINDOWS_ARM64_TARGET}).`,
    };
  }

  const unsupported = getUnsupportedWindowsArm64Message('win32', 'arm64', release);
  if (unsupported) {
    return {
      target: WINDOWS_X64_TARGET,
      emulated: true,
      error:
        `${versionOrTag} ships no native ARM64 Windows build (${WINDOWS_ARM64_TARGET}), ` +
        `so the x86_64 build would have to run under emulation. ${unsupported}`,
      reason: 'No native ARM64 asset, and this Windows build cannot emulate x64.',
    };
  }

  return {
    target: WINDOWS_X64_TARGET,
    emulated: true,
    reason:
      `${versionOrTag} ships no native ARM64 Windows build; using ${WINDOWS_X64_TARGET}, ` +
      'which runs under the x64 emulation in Windows 11 on ARM.',
  };
}

// Module-level singleflight: coalesce concurrent managed-install calls so
// activation auto-download, manual reinstall, and silent update-check do not
// race the same destination path. Multiple BinaryDownloader instances are
// constructed across call sites, so this state lives at module scope.
type ManagedInstallReason = 'force' | 'ensure';
interface ActiveManagedInstall {
  promise: Promise<string | null>;
  reason: ManagedInstallReason;
}
let activeManagedInstall: ActiveManagedInstall | undefined;

export function __resetManagedInstallSingleflightForTesting(): void {
  activeManagedInstall = undefined;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

export function isTransientManagedInstallError(error: unknown): boolean {
  const code =
    typeof error === 'object' && error !== null && 'code' in error
      ? String((error as { code?: unknown }).code ?? '')
      : '';
  return TRANSIENT_MANAGED_INSTALL_ERROR_CODES.has(code);
}

export async function copyManagedFileWithRetry(
  sourcePath: string,
  destinationPath: string,
  label: string,
  log: (message: string) => void,
  retryDelaysMs = MANAGED_INSTALL_RETRY_DELAYS_MS,
  copyFile: (source: string, destination: string) => void = fs.copyFileSync,
): Promise<void> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      copyFile(sourcePath, destinationPath);
      return;
    } catch (error: unknown) {
      const canRetry = attempt < retryDelaysMs.length && isTransientManagedInstallError(error);
      if (!canRetry) {
        throw error;
      }

      const message = error instanceof Error ? error.message : String(error);
      const delayMs = retryDelaysMs[attempt];
      if (delayMs === undefined) {
        throw new Error('Managed install retry delay is missing for a retryable failure.');
      }
      log(
        `Retrying ${label} install after transient file lock (${message}); waiting ${delayMs}ms.`,
      );
      await delay(delayMs);
    }
  }
}

function githubApiHeaders(url: string, includeAuth = true): Record<string, string> {
  const headers: Record<string, string> = {
    'User-Agent': 'vscode-perl-lsp',
    Accept: 'application/vnd.github+json',
  };

  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  if (includeAuth && token && url.startsWith('https://api.github.com/')) {
    headers.Authorization = `Bearer ${token}`;
  }

  return headers;
}

export function buildBinaryAssetCandidateNames(
  versionOrTag: string,
  target: string,
  ext: string,
): string[] {
  const normalizedVersion = versionOrTag.replace(/^v/, '');
  const tagVersion = `v${normalizedVersion}`;
  const candidates = [
    `perllsp-${normalizedVersion}-${target}${ext}`,
    `perllsp-${versionOrTag}-${target}${ext}`,
    `perllsp-${tagVersion}-${target}${ext}`,
    `perllsp-${target}${ext}`,
    `perl-lsp-${normalizedVersion}-${target}${ext}`,
    `perl-lsp-${versionOrTag}-${target}${ext}`,
    `perl-lsp-${tagVersion}-${target}${ext}`,
    `perl-lsp-${target}${ext}`,
  ];

  return [...new Set(candidates)];
}

export function findReleaseAssetName(
  assets: ReadonlyArray<{ name: string }>,
  versionOrTag: string,
  target: string,
  ext: string,
): string | undefined {
  for (const name of buildBinaryAssetCandidateNames(versionOrTag, target, ext)) {
    if (assets.some((asset) => asset.name === name)) {
      return name;
    }
  }

  return undefined;
}

/**
 * Parse the local version string from `perllsp --version` stdout.
 *
 * The binary prints three lines; only the first is needed:
 *   perllsp 0.12.0
 *   Git tag: v0.12.0
 *   Perl Language Server using perl-parser v3
 *
 * Returns the semver string (e.g. "0.12.0") or null if the format is unexpected.
 */
export function parseLocalVersion(versionOutput: string): string | null {
  const firstLine = versionOutput.split('\n')[0]?.trim() ?? '';
  const match = /^(?:perllsp|perl-lsp)\s+(\S+)/.exec(firstLine);
  return match?.[1] ?? null;
}

/**
 * Numeric semver comparison. Strips a leading 'v' from either argument.
 * Returns -1 if a < b, 0 if equal, 1 if a > b.
 */
export function compareVersions(a: string, b: string): -1 | 0 | 1 {
  const normalize = (v: string) =>
    v
      .replace(/^v/, '')
      .split('.')
      .map((n) => parseInt(n, 10));
  const [aMaj, aMin, aPat] = normalize(a);
  const [bMaj, bMin, bPat] = normalize(b);
  for (const [x, y] of [
    [aMaj, bMaj],
    [aMin, bMin],
    [aPat, bPat],
  ] as [number, number][]) {
    if (x < y) {
      return -1;
    }
    if (x > y) {
      return 1;
    }
  }
  return 0;
}

export function isTermuxEnvironment(): boolean {
  return Boolean(
    process.env.TERMUX_VERSION ||
    process.env.PREFIX?.includes('/com.termux/') ||
    fs.existsSync('/data/data/com.termux/files/usr'),
  );
}

export function isAndroidEnvironment(): boolean {
  if (process.platform !== 'linux') {
    return false;
  }

  return (
    typeof process.env.ANDROID_ROOT === 'string' ||
    typeof process.env.ANDROID_DATA === 'string' ||
    typeof process.env.TERMUX_VERSION === 'string' ||
    os.release().toLowerCase().includes('android')
  );
}

export function detectMusl(): boolean {
  const ldd = child_process.spawnSync('ldd', ['--version'], {
    encoding: 'utf8',
    timeout: 1000,
  });
  const lddOutput = `${ldd.stdout ?? ''}${ldd.stderr ?? ''}`.toLowerCase();
  if (lddOutput.includes('musl')) {
    return true;
  }
  if (lddOutput.includes('glibc') || lddOutput.includes('gnu libc')) {
    return false;
  }

  const getconf = child_process.spawnSync('getconf', ['GNU_LIBC_VERSION'], {
    encoding: 'utf8',
    timeout: 1000,
  });
  if (getconf.status === 0) {
    return false;
  }

  // Check for Alpine or musl when active-libc probes are unavailable.
  if (fs.existsSync('/etc/alpine-release')) {
    return true;
  }

  // Check for musl libc
  const muslLibs = [
    '/lib/libc.musl-x86_64.so.1',
    '/lib/libc.musl-aarch64.so.1',
    '/lib/ld-musl-x86_64.so.1',
    '/lib/ld-musl-aarch64.so.1',
  ];

  return muslLibs.some((lib) => fs.existsSync(lib));
}

type TargetLog = (message: string) => void;

/**
 * Environment probes used by target resolution.
 *
 * Kept injectable because target resolution is the one place where a test must
 * be able to describe a host it is not running on — a GNU host proving musl
 * behavior, for instance.
 */
export interface PlatformDetectionSeams {
  isTermux(): boolean;
  isAndroid(): boolean;
  detectMusl(): boolean;
}

const DEFAULT_PLATFORM_DETECTION: PlatformDetectionSeams = {
  isTermux: isTermuxEnvironment,
  isAndroid: isAndroidEnvironment,
  detectMusl,
};

export function resolveLinuxLibcTarget(
  log: TargetLog,
  seams: PlatformDetectionSeams = DEFAULT_PLATFORM_DETECTION,
): 'gnu' | 'musl' {
  const config = vscode.workspace.getConfiguration('perl-lsp');
  const rawValue = config.get<string>('linuxLibc', 'auto');
  const value = rawValue.trim().toLowerCase();

  if (value === 'gnu' || value === 'glibc') {
    return 'gnu';
  }

  if (value === 'musl') {
    return 'musl';
  }

  if (value !== 'auto') {
    log(`Unknown perl-lsp.linuxLibc value "${rawValue}", falling back to auto`);
  }

  return seams.detectMusl() ? 'musl' : 'gnu';
}

/**
 * The target triple this host prefers, independent of any release's asset list.
 *
 * Module-level because managed-state resolution needs it from static call
 * sites that have no downloader instance: the storage namespace is a property
 * of the host's compatibility identity, not of who happens to be asking.
 */
export function resolvePlatformTarget(
  log: TargetLog,
  seams: PlatformDetectionSeams = DEFAULT_PLATFORM_DETECTION,
): string {
  const platform = process.platform;
  const arch = process.arch;

  // Map Node.js platform/arch to exact cargo-dist target triples
  if (platform === 'darwin') {
    return arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  } else if (platform === 'linux') {
    // Check Termux first: isTermuxEnvironment() is the authoritative Termux
    // detector.  isAndroidEnvironment() also matches TERMUX_VERSION and would
    // shadow this branch if checked first, routing to the old arch-map path
    // instead of the uniform `${archPrefix}-linux-android` form.
    const archPrefix = arch === 'arm64' ? 'aarch64' : 'x86_64';
    if (seams.isTermux()) {
      return `${archPrefix}-linux-android`;
    }
    if (seams.isAndroid()) {
      const androidArchMap: Record<string, string> = {
        arm64: 'aarch64-linux-android',
        x64: 'x86_64-linux-android',
        ia32: 'i686-linux-android',
        arm: 'armv7-linux-androideabi',
      };
      return androidArchMap[arch] ?? `${arch}-linux-android`;
    }
    const libc = resolveLinuxLibcTarget(log, seams);
    log(`Linux binary target libc: ${libc}`);
    return `${archPrefix}-unknown-linux-${libc}`;
  } else if (platform === 'win32') {
    // The *preferred* target, which is not necessarily the one downloaded.
    // Windows builds both x86_64-pc-windows-msvc and, since #5208, a native
    // aarch64-pc-windows-msvc, so ARM64 prefers the native build here.
    //
    // Whether that asset exists in a given release is decided later by
    // selectWindowsArm64Target, against that release's actual asset list.
    // This function has no asset list, so it must not reject anything: the
    // Windows 10 ARM64 rejection belongs on the emulation fallback path
    // only, and applying it here refused installs that work (#6196).
    if (arch === 'arm64') {
      return WINDOWS_ARM64_TARGET;
    }
    return WINDOWS_X64_TARGET;
  }

  // Fallback to the old logic
  const platformMap: Record<string, string> = {
    darwin: 'apple-darwin',
    linux: 'unknown-linux-gnu',
    win32: 'pc-windows-msvc',
  };

  const archMap: Record<string, string> = {
    x64: 'x86_64',
    arm64: 'aarch64',
  };

  const rustPlatform = platformMap[platform] || platform;
  const rustArch = archMap[arch] || arch;

  return `${rustArch}-${rustPlatform}`;
}

/**
 * Compatibility keys this host may consume, most preferred first.
 *
 * Resolution walks this list; installation writes into exactly one of its
 * entries. A host with more than one admissible key (Windows ARM64) can hold a
 * native and an emulated candidate side by side without either overwriting the
 * other's `current` pointer.
 */
export function hostManagedCompatibilityKeys(
  log: TargetLog = () => {},
  seams: PlatformDetectionSeams = DEFAULT_PLATFORM_DETECTION,
): string[] {
  return admissibleManagedCompatibilityKeys(
    process.platform,
    process.arch,
    resolvePlatformTarget(log, seams),
  );
}

/** Schema for the per-install record binding bytes to the namespace holding them. */
export interface ManagedInstallTargetRecord {
  schema_version: 'managed_install_target.v1';
  compatibility_key: string;
  target: string;
  emulation: ManagedEmulation | null;
}

export const MANAGED_INSTALL_TARGET_FILE = 'target.json';

/**
 * Namespace for a host whose target triple is not canonical.
 *
 * Such a host cannot share bytes with anything, so it gets its own quarantined
 * namespace rather than being folded into a neighbour's.
 */
export const UNSUPPORTED_COMPATIBILITY_KEY = 'unsupported-host-target';

export class BinaryDownloader {
  private static readonly REPO_OWNER = 'EffortlessMetrics';
  private static readonly REPO_NAME = 'perl-lsp';
  private static readonly BINARY_NAME = 'perllsp';
  private lastErrorMessage: string | undefined;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly outputChannel: vscode.OutputChannel,
  ) {}

  getTargetTriple(): string {
    return this.getPlatformTarget();
  }

  getLastErrorMessage(): string | undefined {
    return this.lastErrorMessage;
  }

  async ensureBinary(forceDownload = false): Promise<string | null> {
    this.lastErrorMessage = undefined;
    const myReason: ManagedInstallReason = forceDownload ? 'force' : 'ensure';

    // Singleflight: if an install is already running, decide whether to
    // join it or run our own afterward. A pending force install always
    // satisfies all callers; an ensure call always joins; a force call
    // that arrives while an ensure is in flight waits for it then runs
    // its own to honor the explicit reinstall intent.
    if (activeManagedInstall) {
      const activeReason = activeManagedInstall.reason;
      this.outputChannel.appendLine(
        `Managed install already in progress (${activeReason}); ${myReason} call will join.`,
      );
      const joined = await activeManagedInstall.promise.catch(() => null);
      if (!forceDownload || activeReason === 'force') {
        return joined;
      }
      this.outputChannel.appendLine(
        'Joined ensure-install completed; running explicit force-reinstall now.',
      );
    }

    const promise = this.runEnsureBinary(forceDownload);
    activeManagedInstall = { promise, reason: myReason };
    try {
      return await promise;
    } finally {
      if (activeManagedInstall && activeManagedInstall.promise === promise) {
        activeManagedInstall = undefined;
      }
    }
  }

  private async runEnsureBinary(forceDownload: boolean): Promise<string | null> {
    const config = vscode.workspace.getConfiguration('perl-lsp');
    const channel = config.get<string>('channel', 'latest');
    const versionTag = config.get<string>('versionTag', '');

    // If channel is 'tag' and versionTag is specified, use that specific version
    if (channel === 'tag' && versionTag) {
      this.outputChannel.appendLine(`Using specific version: ${versionTag}`);
    }

    // Check if binary already exists
    const existingPath = this.getLocalBinaryPath();
    if (!forceDownload && existingPath && fs.existsSync(existingPath)) {
      this.outputChannel.appendLine(`Using existing binary: ${existingPath}`);
      return existingPath;
    }

    if (forceDownload && existingPath && fs.existsSync(existingPath)) {
      this.outputChannel.appendLine(`Refreshing existing binary: ${existingPath}`);
    }

    // Show status bar while downloading
    const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBar.text = '$(sync~spin) Perl LSP: downloading binary...';
    statusBar.tooltip = 'Downloading Perl Language Server... Click to show logs';
    statusBar.command = 'perl-lsp.showOutput';
    statusBar.show();

    // Download binary
    try {
      // No eager Windows 10 ARM64 rejection here. Whether emulation is even
      // needed depends on whether the target release carries a native ARM64
      // asset, which is not known until the release is fetched; rejecting up
      // front refused installs that would have succeeded natively (#6196).
      return await this.downloadWithProgress();
    } catch (error: unknown) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      this.lastErrorMessage = errorMsg;
      this.outputChannel.appendLine(`Failed to download binary: ${errorMsg}`);

      const manualInstallUrl = 'https://github.com/EffortlessMetrics/perl-lsp#install';
      const manualInstallNote =
        'To use a manually installed binary, set the "perl-lsp.serverPath" setting to its path.';

      let message: string;
      let buttons: string[];

      if (errorMsg.includes('Windows ARM64 x64 emulation')) {
        message = `perl-lsp: ${errorMsg} ${manualInstallNote}`;
        buttons = ['Install Manually', 'View Logs'];
      } else if (
        errorMsg.includes('ECONNREFUSED') ||
        errorMsg.includes('ETIMEDOUT') ||
        errorMsg.includes('timeout')
      ) {
        // Network connectivity failure — proxy, VPN, or firewall
        message =
          'perl-lsp: Binary download failed — network error ' +
          `(${errorMsg.split('\n')[0]}). ` +
          'Check your proxy/VPN settings (http.proxy in VS Code settings). ' +
          manualInstallNote;
        buttons = ['Open Proxy Settings', 'Install Manually'];
      } else if (errorMsg.includes('No binary found for platform')) {
        // Architecture or OS not supported by the release
        const platformMatch = /platform:\s*([^\s.]+)/.exec(errorMsg);
        const platformStr = platformMatch?.[1] ?? 'your platform';
        if (this.isTermuxEnvironment() || platformStr.includes('linux-android')) {
          message =
            `perl-lsp: No pre-built binary for ${platformStr} (Termux/Android). ` +
            'Install from source in Termux (for example: pkg install rust && cargo install --locked --path crates/perllsp), then configure perl-lsp.serverPath. ' +
            manualInstallNote;
        } else {
          message =
            `perl-lsp: No pre-built binary for ${platformStr}. ` +
            'Build from source or download a compatible binary manually. ' +
            manualInstallNote;
        }
        buttons = ['Install Manually'];
      } else if (errorMsg.includes('HTTP 403')) {
        // GitHub rate limit or auth failure
        message =
          'perl-lsp: Download blocked (HTTP 403 — GitHub rate limit). ' +
          'Wait a few minutes, or set the GITHUB_TOKEN environment variable to increase your rate limit. ' +
          manualInstallNote;
        buttons = ['Install Manually', 'View Logs'];
      } else if (errorMsg.includes('HTTP 404')) {
        // Release or asset not found
        message =
          'perl-lsp: Binary not found (HTTP 404). ' +
          'The release asset may not exist yet for this platform. ' +
          manualInstallNote;
        buttons = ['Install Manually', 'View Logs'];
      } else if (errorMsg.toLowerCase().includes('checksum') || errorMsg.includes('SHA256SUMS')) {
        // Corrupted or tampered download, or missing checksum file
        message =
          'perl-lsp: Checksum verification failed — download may be corrupted. ' +
          'Please retry. If this persists, install manually. ' +
          manualInstallNote;
        buttons = ['Install Manually', 'View Logs'];
      } else if (
        errorMsg.includes('tar') ||
        errorMsg.includes('unzip') ||
        errorMsg.includes('extract')
      ) {
        // Archive extraction failure
        message =
          'perl-lsp: Archive extraction failed. ' +
          'Ensure tar (Linux/macOS) or the built-in zip support (Windows) is working. ' +
          manualInstallNote;
        buttons = ['Install Manually', 'View Logs'];
      } else {
        // Generic fallback — always surface the manual install path
        message =
          `perl-lsp: Binary download failed — ${errorMsg.split('\n')[0]}. ` + manualInstallNote;
        buttons = ['Install Manually', 'View Logs'];
      }

      vscode.window.showErrorMessage(message, ...buttons).then((choice: string | undefined) => {
        if (choice === 'Install Manually') {
          vscode.env.openExternal(vscode.Uri.parse(manualInstallUrl));
        } else if (choice === 'Open Proxy Settings') {
          vscode.commands.executeCommand('workbench.action.openSettings', 'http.proxy');
        } else if (choice === 'View Logs') {
          this.outputChannel.show();
        }
      });

      return null;
    } finally {
      statusBar.dispose();
    }
  }

  private async downloadWithProgress(): Promise<string> {
    return vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: 'Downloading Perl Language Server',
        cancellable: true,
      },
      async (progress, token) => {
        // Get latest release info
        progress.report({ increment: 0, message: 'Fetching release information...' });
        const release = await this.getLatestRelease();

        if (token.isCancellationRequested) {
          throw new Error('Download cancelled');
        }

        // Determine platform and architecture
        let target = this.getPlatformTarget();
        // Non-null only when the selected target needs a host compatibility
        // shim to run. It is part of the candidate's storage identity, because
        // an emulated candidate is not interchangeable with a native one.
        let emulation: ManagedEmulation | null = null;

        // Try multiple naming patterns for our release format
        const ext = process.platform === 'win32' ? '.zip' : '.tar.gz';

        // Windows ARM64 is the one target whose choice depends on what this
        // release actually carries: prefer the native build, fall back to x64
        // emulation when it is absent. Resolved here because this is the first
        // point with the release's asset list in hand.
        if (release.internal && process.platform === 'win32' && process.arch === 'arm64') {
          // Internal mirrors historically served the x64 archive for ARM64.
          // Synthetic metadata cannot establish which mirror files really
          // exist, so do not let the native preference turn a working mirror
          // into an unverified ARM64 URL.
          target = WINDOWS_X64_TARGET;
          emulation = 'windows-arm64-emulation';
        } else if (process.platform === 'win32' && process.arch === 'arm64') {
          const selection = selectWindowsArm64Target(release.assets, release.tag_name, ext);
          this.outputChannel.appendLine(`Windows ARM64: ${selection.reason}`);
          if (selection.error) {
            throw new Error(selection.error);
          }
          target = selection.target;
          emulation = selection.emulated ? 'windows-arm64-emulation' : null;
        }

        const compatibilityKey = buildManagedCompatibilityKey({ target, emulation });
        if (compatibilityKey === null) {
          throw new Error(
            `Refusing to install: "${target}" is not a canonical compatibility target, ` +
              'so managed state cannot be namespaced safely.',
          );
        }

        const assetName = findReleaseAssetName(release.assets, release.tag_name, target, ext);
        const asset = assetName ? release.assets.find((a) => a.name === assetName) : undefined;

        if (!asset || !assetName) {
          const availableAssets = release.assets.map((a) => a.name).join(', ');
          this.outputChannel.appendLine(`Target platform: ${target}`);
          this.outputChannel.appendLine(`Available assets: ${availableAssets}`);
          if (target.includes('unknown-linux-')) {
            this.outputChannel.appendLine(
              'Linux binary selection: most distributions use gnu/glibc; Alpine Linux and musl containers use musl. ' +
                'Override with perl-lsp.linuxLibc if auto-detection chose the wrong target.',
            );
          }
          throw new Error(
            `No binary found for platform: ${target}. Available assets: ${availableAssets}`,
          );
        }

        // Security check: Validate asset name to prevent path traversal
        if (!/^[a-zA-Z0-9_.-]+$/.test(assetName) || assetName.includes('..')) {
          throw new Error(`Invalid asset name detected: ${assetName}`);
        }

        this.outputChannel.appendLine(`Found matching asset: ${assetName}`);

        // Find checksum file (SHA256SUMS file contains all checksums)
        const checksumAsset = release.assets.find((a) => a.name === 'SHA256SUMS');

        // Download to temp directory
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-'));
        const archivePath = path.join(tempDir, assetName);

        try {
          // Download binary archive
          progress.report({ increment: 10, message: 'Downloading binary...' });
          await this.downloadFile(asset.browser_download_url, archivePath);

          if (token.isCancellationRequested) {
            throw new Error('Download cancelled');
          }

          // Download and verify checksum (required for security)
          if (!checksumAsset) {
            throw new Error('Security check failed: No SHA256SUMS file found in release assets.');
          }

          progress.report({ increment: 40, message: 'Verifying checksum...' });
          const checksumPath = path.join(tempDir, 'SHA256SUMS');
          await this.downloadFile(checksumAsset.browser_download_url, checksumPath);

          // Find the checksum line for our file
          const checksums = fs.readFileSync(checksumPath, 'utf8');
          const lines = checksums.split('\n');
          const checksumLine = lines.find((line) => line.includes(assetName));

          if (!checksumLine) {
            throw new Error(
              `Security check failed: Checksum for ${assetName} not found in SHA256SUMS file.`,
            );
          }

          const expectedChecksum = checksumLine.split(/\s+/)[0]?.toLowerCase();
          if (!expectedChecksum) {
            throw new Error(
              `Security check failed: Checksum for ${assetName} is malformed in SHA256SUMS.`,
            );
          }
          const actualChecksum = await this.calculateSHA256(archivePath);

          if (expectedChecksum !== actualChecksum) {
            throw new Error(
              'Security check failed: Checksum verification failed (file may be corrupted or tampered with).',
            );
          }
          this.outputChannel.appendLine('Checksum verified successfully');

          // Extract archive
          progress.report({ increment: 30, message: 'Extracting binary...' });
          const extractDir = path.join(tempDir, 'extracted');
          fs.mkdirSync(extractDir);

          // Choose extraction method based on file extension
          if (assetName.endsWith('.tar.gz')) {
            await tar.x({
              file: archivePath,
              cwd: extractDir,
            });
          } else if (assetName.endsWith('.zip')) {
            await new Promise<void>((resolve, reject) => {
              const zip = new AdmZip(archivePath);
              zip.extractAllToAsync(extractDir, true, true, (error) => {
                if (error) {
                  reject(error);
                } else {
                  resolve();
                }
              });
            });
          } else if (assetName.endsWith('.tar.xz')) {
            // Fallback to system tar for .tar.xz (node-tar doesn't support xz)
            await execFile('tar', ['-xJf', archivePath, '-C', extractDir]);
          } else {
            throw new Error(`Unsupported archive format: ${assetName}`);
          }

          // Find the binary
          const binaryNames =
            process.platform === 'win32'
              ? ['perllsp.exe', 'perl-lsp.exe']
              : ['perllsp', 'perl-lsp'];
          const extractedBinary =
            binaryNames.map((name) => this.findBinary(extractDir, name)).find(Boolean) ?? null;

          if (!extractedBinary) {
            throw new Error('Binary not found in archive');
          }

          // Move to final location. Each install lands in a unique
          // versioned dir so a forced reinstall while perllsp.exe is
          // running does not have to overwrite the running file. The
          // active install is selected by an atomically-committed
          // pointer file at the base dir.
          progress.report({ increment: 15, message: 'Installing binary...' });
          const baseDir = this.getManagedBaseDirForKey(compatibilityKey);
          if (!fs.existsSync(baseDir)) {
            fs.mkdirSync(baseDir, { recursive: true });
          }
          const installDirName = this.buildVersionedInstallDirName(release.tag_name);
          const installDir = path.join(baseDir, installDirName);
          fs.mkdirSync(installDir, { recursive: true });
          this.writeInstallTargetRecord(installDir, compatibilityKey, target, emulation);

          const binaryName = process.platform === 'win32' ? 'perllsp.exe' : 'perllsp';
          const finalPath = path.join(installDir, binaryName);

          await copyManagedFileWithRetry(extractedBinary, finalPath, 'perllsp', (message) =>
            this.outputChannel.appendLine(message),
          );

          // Make executable on Unix
          if (process.platform !== 'win32') {
            fs.chmodSync(finalPath, 0o755);
          }

          // Best-effort: copy perl-dap if found in archive
          const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
          const extractedDap = this.findBinary(extractDir, dapName);
          if (extractedDap) {
            const dapDest = path.join(installDir, dapName);
            try {
              await copyManagedFileWithRetry(extractedDap, dapDest, 'perl-dap', (message) =>
                this.outputChannel.appendLine(message),
              );
              if (process.platform !== 'win32') {
                fs.chmodSync(dapDest, 0o755);
              }
              this.outputChannel.appendLine(`Debug adapter installed to: ${dapDest}`);
            } catch (e) {
              this.outputChannel.appendLine(`Note: could not install perl-dap: ${e}`);
            }
          }

          // Atomically activate the new install. Old install dirs stay
          // on disk for one generation as a fallback, then get pruned.
          // Both the pointer and the prune are scoped to this compatibility
          // key, so rollback and GC can only ever touch this target's row.
          this.commitVersionedInstall(installDirName, compatibilityKey);
          this.outputChannel.appendLine(`Active managed install: ${installDirName}`);
          this.pruneOldVersionedInstalls(baseDir, installDirName);

          progress.report({ increment: 5, message: 'Complete!' });
          this.outputChannel.appendLine(`Binary installed to: ${finalPath}`);

          return finalPath;
        } finally {
          // Clean up temp directory
          try {
            fs.rmSync(tempDir, { recursive: true, force: true });
          } catch (e) {
            this.outputChannel.appendLine(`Failed to clean up temp dir: ${e}`);
          }
        }
      },
    );
  }

  private async getLatestRelease(timeoutMs = 30000): Promise<Release> {
    const config = vscode.workspace.getConfiguration('perl-lsp');
    const channel = config.get<string>('channel', 'latest');
    const versionTag = config.get<string>('versionTag', '');
    const downloadBaseUrl = config.get<string>('downloadBaseUrl', '');

    // Handle internal base URL hosting
    if (downloadBaseUrl) {
      return this.getInternalRelease(downloadBaseUrl, versionTag || 'latest');
    }

    let url: string;
    if (channel === 'tag' && versionTag) {
      // Get specific release by tag
      url = `https://api.github.com/repos/${BinaryDownloader.REPO_OWNER}/${BinaryDownloader.REPO_NAME}/releases/tags/${versionTag}`;
    } else if (channel === 'stable') {
      // Get latest non-prerelease
      url = `https://api.github.com/repos/${BinaryDownloader.REPO_OWNER}/${BinaryDownloader.REPO_NAME}/releases`;
    } else {
      // Get latest release (including prereleases)
      url = `https://api.github.com/repos/${BinaryDownloader.REPO_OWNER}/${BinaryDownloader.REPO_NAME}/releases/latest`;
    }

    return new Promise((resolve, reject) => {
      const isHttps = url.startsWith('https:');
      let timedOut = false;
      let timeout: NodeJS.Timeout | undefined;
      let request: http.ClientRequest | undefined;

      const httpConfig = vscode.workspace.getConfiguration('http');
      const proxyStrictSSL = httpConfig.get<boolean>('proxyStrictSSL', true);
      const options = {
        headers: githubApiHeaders(url, proxyStrictSSL),
        rejectUnauthorized: proxyStrictSSL,
      };

      timeout = setTimeout(() => {
        timedOut = true;
        if (request) {
          request.destroy();
        }
        reject(new Error(`Release fetch timeout after ${timeoutMs / 1000} seconds`));
      }, timeoutMs);

      try {
        request = this.httpGet(isHttps, url, options, (res) => {
          let data = '';
          res.on('data', (chunk) => (data += chunk));
          res.on('end', () => {
            if (timedOut) {
              return;
            }
            if (timeout) {
              clearTimeout(timeout);
            }
            try {
              const parsed: unknown = JSON.parse(data);
              if (
                parsed &&
                typeof parsed === 'object' &&
                !Array.isArray(parsed) &&
                'message' in parsed
              ) {
                const msg = parsed as { message: string };
                if (msg.message.includes('Not Found')) {
                  reject(new Error('No releases found'));
                  return;
                }
                reject(new Error(`GitHub API error: ${msg.message}`));
                return;
              }
              if (Array.isArray(parsed)) {
                // For stable channel, find first non-prerelease
                const releases = parsed as Release[];
                const stableRelease = releases.find((r) => !r.prerelease);
                if (stableRelease) {
                  resolve(stableRelease);
                } else {
                  const fallbackRelease = releases[0];
                  if (fallbackRelease) {
                    resolve(fallbackRelease); // Fall back to latest
                  } else {
                    reject(new Error('No releases found'));
                  }
                }
              } else {
                resolve(parsed as Release);
              }
            } catch (e) {
              reject(e);
            }
          });
          res.on('error', (err) => {
            if (timeout) {
              clearTimeout(timeout);
            }
            if (!timedOut) {
              reject(err);
            }
          });
        });

        request.on('error', (err) => {
          if (timeout) {
            clearTimeout(timeout);
          }
          if (!timedOut) {
            reject(err);
          }
        });
      } catch (err) {
        if (timeout) {
          clearTimeout(timeout);
        }
        reject(err);
      }
    });
  }

  private async getInternalRelease(baseUrl: string, version: string): Promise<Release> {
    // For internal hosting, create a synthetic release object
    // This assumes the internal server hosts files directly without GitHub API
    const normalizedBaseUrl = baseUrl.endsWith('/') ? baseUrl.slice(0, -1) : baseUrl;
    const ext = process.platform === 'win32' ? '.zip' : '.tar.gz';
    const isWindowsArm64 = process.platform === 'win32' && process.arch === 'arm64';
    const targets = isWindowsArm64
      ? [WINDOWS_ARM64_TARGET, WINDOWS_X64_TARGET]
      : [this.getPlatformTarget()];

    // Try multiple naming patterns that might be used internally
    const possibleFilenames = [
      ...targets.flatMap((target) => buildBinaryAssetCandidateNames(version, target, ext)),
      `perllsp${ext}`,
      `perl-lsp${ext}`,
    ];

    // Create synthetic release with all possible asset URLs
    const assets: ReleaseAsset[] = possibleFilenames.map((filename) => ({
      name: filename,
      browser_download_url: `${normalizedBaseUrl}/${filename}`,
    }));

    // Add potential checksum file
    assets.push({
      name: 'SHA256SUMS',
      browser_download_url: `${normalizedBaseUrl}/SHA256SUMS`,
    });

    return {
      tag_name: version,
      internal: true,
      assets,
    };
  }

  private async downloadFile(
    url: string,
    dest: string,
    timeoutMs = 30000,
    maxRedirects = 5,
  ): Promise<void> {
    return new Promise((resolve, reject) => {
      // Security check: Enforce HTTPS for remote URLs to prevent MITM attacks
      try {
        const parsedUrl = new URL(url);

        // Only allow http: and https: protocols
        if (parsedUrl.protocol !== 'http:' && parsedUrl.protocol !== 'https:') {
          reject(
            new Error(
              `Unsupported protocol: ${parsedUrl.protocol}. Only HTTP and HTTPS are allowed.`,
            ),
          );
          return;
        }

        // Check for local addresses (full IPv4 loopback range 127.0.0.0/8)
        // Note: URL.hostname normalizes IPv6 addresses and never includes brackets
        const ipv4LoopbackRegex = /^127(?:\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)){3}$/;
        const isLocal =
          ['localhost', '::1'].includes(parsedUrl.hostname) ||
          parsedUrl.hostname.endsWith('.localhost') ||
          ipv4LoopbackRegex.test(parsedUrl.hostname);

        if (parsedUrl.protocol === 'http:' && !isLocal) {
          reject(
            new Error(
              `Security violation: Insecure HTTP download prevented for remote host: ${parsedUrl.hostname}. Use HTTPS or a local server.`,
            ),
          );
          return;
        }
      } catch (e) {
        reject(
          new Error(
            `Invalid URL format: ${url}. Error: ${e instanceof Error ? e.message : String(e)}`,
          ),
        );
        return;
      }

      const file = this.createWriteStream(dest);
      let timedOut = false;
      let timeout: NodeJS.Timeout | undefined;

      // Install the listener before any request activity. A refused request
      // can destroy the stream before a response callback runs; leaving the
      // stream unobserved turns a normal download failure into an unhandled
      // ENOENT when a caller cleans up its temporary directory.
      file.once('error', (err: NodeJS.ErrnoException) => {
        if (timeout) {
          clearTimeout(timeout);
        }
        this.removePartialFile(dest);
        reject(err);
      });

      // Set timeout
      timeout = setTimeout(() => {
        timedOut = true;
        file.destroy();
        reject(new Error(`Download timeout after ${timeoutMs / 1000} seconds`));
      }, timeoutMs);

      // Honor VS Code proxy settings
      const httpConfig = vscode.workspace.getConfiguration('http');
      const proxyStrictSSL = httpConfig.get<boolean>('proxyStrictSSL', true);

      const options = {
        headers: { 'User-Agent': 'vscode-perl-lsp' },
        rejectUnauthorized: proxyStrictSSL,
      };

      // Use appropriate module based on URL protocol
      const isHttps = url.startsWith('https:');
      const request = this.httpGet(isHttps, url, options, (response) => {
        // Handle redirects
        if (response.statusCode === 301 || response.statusCode === 302) {
          clearTimeout(timeout);
          file.destroy();
          const newUrl = response.headers.location;
          if (newUrl) {
            // Security check: Prevent downgrade from HTTPS to HTTP
            if (
              isHttps &&
              newUrl.toLowerCase().startsWith('http:') &&
              !newUrl.toLowerCase().startsWith('https:')
            ) {
              reject(new Error('Security violation: Redirect from HTTPS to HTTP prevented'));
              return;
            }
            if (maxRedirects <= 0) {
              reject(new Error('Too many redirects'));
              return;
            }
            this.downloadFile(newUrl, dest, timeoutMs, maxRedirects - 1)
              .then(resolve)
              .catch(reject);
            return;
          }
        }

        if (response.statusCode !== 200) {
          clearTimeout(timeout);
          file.destroy();
          reject(new Error(`Failed to download: HTTP ${response.statusCode}`));
          return;
        }

        response.pipe(file);

        file.on('finish', () => {
          if (!timedOut) {
            clearTimeout(timeout);
            file.close();
            resolve();
          }
        });
      });

      request.on('error', (err) => {
        clearTimeout(timeout);
        file.destroy();
        reject(err);
      });

      request.on('timeout', () => {
        request.destroy();
        reject(new Error('Request timeout'));
      });
    });
  }

  /**
   * Transport seam: the actual network GET, extracted so tests can stub it
   * without mocking Node's `http`/`https` core modules (whose `get` exports
   * are non-configurable). Behaviour is identical to calling the module's
   * `get` directly.
   */
  private httpGet(
    isHttps: boolean,
    url: string,
    options: https.RequestOptions,
    callback: (response: http.IncomingMessage) => void,
  ): http.ClientRequest {
    return isHttps ? https.get(url, options, callback) : http.get(url, options, callback);
  }

  private createWriteStream(dest: string): fs.WriteStream {
    return fs.createWriteStream(dest);
  }

  private removePartialFile(dest: string): void {
    fs.unlink(dest, () => {});
  }

  private async calculateSHA256(filePath: string): Promise<string> {
    return new Promise((resolve, reject) => {
      const hash = crypto.createHash('sha256');
      const stream = fs.createReadStream(filePath);

      stream.on('data', (data) => hash.update(data));
      stream.on('end', () => resolve(hash.digest('hex')));
      stream.on('error', reject);
    });
  }

  // No `release` parameter: target selection no longer depends on the OS
  // build. The Windows 11 floor moved to selectWindowsArm64Target, where it
  // gates only the x64 emulation fallback (#6196).
  private getPlatformTarget(): string {
    return resolvePlatformTarget(
      (message) => this.outputChannel.appendLine(message),
      this.platformDetectionSeams(),
    );
  }

  private getLinuxLibcTarget(): 'gnu' | 'musl' {
    return resolveLinuxLibcTarget(
      (message) => this.outputChannel.appendLine(message),
      this.platformDetectionSeams(),
    );
  }

  /**
   * Routes module-level target resolution back through this instance's own
   * probe methods, so overriding a probe overrides every decision that
   * consumes it.
   */
  private platformDetectionSeams(): PlatformDetectionSeams {
    return {
      isTermux: () => this.isTermuxEnvironment(),
      isAndroid: () => this.isAndroidEnvironment(),
      detectMusl: () => this.detectMusl(),
    };
  }

  private isAndroidEnvironment(): boolean {
    return isAndroidEnvironment();
  }

  private detectMusl(): boolean {
    return detectMusl();
  }

  private isTermuxEnvironment(): boolean {
    return isTermuxEnvironment();
  }

  /**
   * Root for managed installs owned by this host's preferred compatibility key:
   * `globalStorage/.../managed/<compatibility-key>/`. Inside it, the active
   * install is selected by the `current` pointer file.
   *
   * This is the *write* root. Resolution walks every admissible key, because a
   * Windows ARM64 host may legitimately hold a native or an emulated candidate.
   */
  private getManagedBaseDir(): string {
    return this.getManagedBaseDirForKey(this.getHostCompatibilityKey());
  }

  private getHostCompatibilityKey(): string {
    const keys = hostManagedCompatibilityKeys(
      (message) => this.outputChannel.appendLine(message),
      this.platformDetectionSeams(),
    );
    // resolvePlatformTarget always yields a canonical triple, so the preferred
    // key is present for every host the extension supports. The fallback keeps
    // an exotic platform string out of the shared namespace root rather than
    // silently writing managed state somewhere unnamed.
    return keys[0] ?? UNSUPPORTED_COMPATIBILITY_KEY;
  }

  private getManagedBaseDirForKey(key: string): string {
    return (
      managedNamespaceDir(this.context.globalStorageUri.fsPath, key) ??
      path.join(this.context.globalStorageUri.fsPath, 'managed', UNSUPPORTED_COMPATIBILITY_KEY)
    );
  }

  /**
   * Resolves the active managed install directory for this host.
   *
   * Admissible keys are walked in preference order and the first namespace
   * with a valid, self-consistent `current` pointer wins. When no
   * compatibility-scoped namespace is populated, a pre-#9847 install may be
   * adopted, but only after its own bytes are revalidated against the key —
   * path shape alone never promotes legacy bytes (#9847).
   */
  private static readActiveManagedInstallDir(context: vscode.ExtensionContext): string | null {
    const keys = hostManagedCompatibilityKeys();
    for (const key of keys) {
      const baseDir = managedNamespaceDir(context.globalStorageUri.fsPath, key);
      if (baseDir === null) {
        continue;
      }
      const active = BinaryDownloader.readPointedInstallDir(baseDir);
      if (active !== null && BinaryDownloader.installMatchesKey(active, key)) {
        return active;
      }
    }
    return BinaryDownloader.adoptLegacyManagedInstallDir(context, keys);
  }

  /**
   * Reads a `current` pointer and returns the directory it names.
   *
   * Pointer content is restricted to a single dir name with no separators or
   * '..' components.
   */
  private static readPointedInstallDir(baseDir: string): string | null {
    const pointerPath = path.join(baseDir, 'current');
    if (!fs.existsSync(pointerPath)) {
      return null;
    }
    let content = '';
    try {
      content = fs.readFileSync(pointerPath, 'utf8').trim();
    } catch {
      return null;
    }
    if (!content) {
      return null;
    }
    if (!/^[A-Za-z0-9._-]+$/.test(content) || content.includes('..')) {
      return null;
    }
    const candidate = path.join(baseDir, content);
    if (!fs.existsSync(candidate)) {
      return null;
    }
    return candidate;
  }

  /**
   * Rejects an install whose own record disagrees with the namespace holding
   * it. A namespace is only meaningful if nothing inside it can claim to be a
   * different target; an install written before this record existed carries no
   * claim and is accepted on the strength of its namespace.
   */
  private static installMatchesKey(installDir: string, key: string): boolean {
    const recordPath = path.join(installDir, MANAGED_INSTALL_TARGET_FILE);
    if (!fs.existsSync(recordPath)) {
      return true;
    }
    try {
      const record = JSON.parse(fs.readFileSync(recordPath, 'utf8')) as ManagedInstallTargetRecord;
      return record.compatibility_key === key;
    } catch {
      return false;
    }
  }

  /**
   * Revalidates a pre-#9847 `bin/<platform>-<arch>` install and returns it only
   * when its bytes prove it is interchangeable with a candidate this host would
   * install today.
   *
   * The legacy directory is never moved or deleted: a host that cannot prove
   * compatibility simply downloads its own candidate, leaving the only
   * known-good install intact for whichever host it actually belongs to.
   */
  private static adoptLegacyManagedInstallDir(
    context: vscode.ExtensionContext,
    keys: readonly string[],
  ): string | null {
    const legacyBase = legacyManagedBaseDir(
      context.globalStorageUri.fsPath,
      process.platform,
      process.arch,
    );
    if (!fs.existsSync(legacyBase)) {
      return null;
    }
    const binaryName = process.platform === 'win32' ? 'perllsp.exe' : 'perllsp';
    const pointed = BinaryDownloader.readPointedInstallDir(legacyBase);
    const legacyDirs = pointed === null ? [legacyBase] : [pointed, legacyBase];
    for (const legacyDir of legacyDirs) {
      const binaryPath = path.join(legacyDir, binaryName);
      if (!fs.existsSync(binaryPath)) {
        continue;
      }
      const observed = probeBinaryIdentity(binaryPath);
      for (const key of keys) {
        if (classifyLegacyManagedCandidate(observed, key) === 'adopt') {
          return legacyDir;
        }
      }
      // The first readable candidate decides: a second directory under the same
      // legacy root holds the same host's bytes, so re-probing cannot change
      // the verdict.
      return null;
    }
    return null;
  }

  /**
   * Builds a unique install dir name from a release tag plus an ISO timestamp.
   * Uniqueness lets a forced reinstall of the same version land in a fresh
   * directory instead of overwriting the running binary on Windows.
   */
  private buildVersionedInstallDirName(versionTag: string): string {
    const sanitizedTag = (versionTag || 'unknown').replace(/[^A-Za-z0-9._-]/g, '_');
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    return `${sanitizedTag}-${stamp}`;
  }

  /**
   * Records which compatibility key owns this install.
   *
   * The namespace already encodes the key; this record is the cross-check that
   * makes a namespace/candidate disagreement detectable instead of silent.
   * Failure to write it is not fatal — an absent record simply leaves the
   * namespace as the only claim.
   */
  private writeInstallTargetRecord(
    installDir: string,
    compatibilityKey: string,
    target: string,
    emulation: ManagedEmulation | null,
  ): void {
    const record: ManagedInstallTargetRecord = {
      schema_version: 'managed_install_target.v1',
      compatibility_key: compatibilityKey,
      target,
      emulation,
    };
    try {
      fs.writeFileSync(
        path.join(installDir, MANAGED_INSTALL_TARGET_FILE),
        `${JSON.stringify(record, null, 2)}\n`,
        { encoding: 'utf8' },
      );
    } catch (e) {
      this.outputChannel.appendLine(`Note: could not record managed install target: ${e}`);
    }
  }

  /**
   * Atomically updates the `current` pointer to a freshly populated install
   * dir. The temp + rename pattern is the strongest form of "commit on
   * success" we can use with no extra dependencies.
   *
   * The pointer lives inside the compatibility namespace, so committing here
   * cannot move another target's selection.
   */
  private commitVersionedInstall(installDirName: string, compatibilityKey?: string): void {
    const baseDir =
      compatibilityKey === undefined
        ? this.getManagedBaseDir()
        : this.getManagedBaseDirForKey(compatibilityKey);
    const pointerPath = path.join(baseDir, 'current');
    const tmpPath = `${pointerPath}.tmp`;
    fs.writeFileSync(tmpPath, `${installDirName}\n`, { encoding: 'utf8' });
    fs.renameSync(tmpPath, pointerPath);
  }

  /**
   * Removes versioned install dirs older than the most recent two. Best
   * effort only — failure to prune is logged but never propagates so it
   * cannot mask install success.
   *
   * Keeps `currentName` plus one prior install for fallback recovery.
   */
  private pruneOldVersionedInstalls(baseDir: string, currentName: string): void {
    let entries: { name: string; mtime: number }[] = [];
    try {
      entries = fs
        .readdirSync(baseDir, { withFileTypes: true })
        .filter((d) => d.isDirectory() && d.name !== currentName)
        .map((d) => {
          const full = path.join(baseDir, d.name);
          let mtime = 0;
          try {
            mtime = fs.statSync(full).mtimeMs;
          } catch {
            /* ignore */
          }
          return { name: d.name, mtime };
        })
        .sort((a, b) => b.mtime - a.mtime);
    } catch {
      return;
    }
    // Keep most recent prior install; remove anything older.
    for (const entry of entries.slice(1)) {
      const target = path.join(baseDir, entry.name);
      try {
        fs.rmSync(target, { recursive: true, force: true });
        this.outputChannel.appendLine(`Removed stale managed install: ${entry.name}`);
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        this.outputChannel.appendLine(`Could not remove stale install ${entry.name}: ${msg}`);
      }
    }
  }

  private getLocalBinaryPath(): string {
    const binaryName = process.platform === 'win32' ? 'perllsp.exe' : 'perllsp';
    const activeDir = BinaryDownloader.readActiveManagedInstallDir(this.context);
    if (activeDir) {
      return path.join(activeDir, binaryName);
    }
    // Legacy flat layout — pre-versioned installs grandfathered.
    return path.join(this.getManagedBaseDir(), binaryName);
  }

  /**
   * Returns the path where perl-dap would be placed inside the auto-download
   * directory.  Used by debugAdapter.ts to locate the binary without
   * duplicating path logic.
   */
  static getLocalDapPath(context: vscode.ExtensionContext): string {
    const dapName = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const activeDir = BinaryDownloader.readActiveManagedInstallDir(context);
    if (activeDir) {
      return path.join(activeDir, dapName);
    }
    return path.join(BinaryDownloader.hostManagedBaseDir(context), dapName);
  }

  /** The write root for this host, usable from static call sites. */
  private static hostManagedBaseDir(context: vscode.ExtensionContext): string {
    const key = hostManagedCompatibilityKeys()[0] ?? UNSUPPORTED_COMPATIBILITY_KEY;
    return (
      managedNamespaceDir(context.globalStorageUri.fsPath, key) ??
      path.join(context.globalStorageUri.fsPath, 'managed', UNSUPPORTED_COMPATIBILITY_KEY)
    );
  }

  /**
   * Silent background update check. Fire-and-forget from activate().
   *
   * No-ops when:
   * - channel === 'tag' (user has pinned a version)
   * - serverPath is user-configured (downloader doesn't own the binary)
   * - binary is bundled (under extensionPath, not globalStorageUri)
   * - updateCheckInterval === 0 (user disabled checks)
   * - not enough time has elapsed since the last check
   * - versions are equal or local is ahead
   *
   * All errors are logged to the output channel; none are shown to the user.
   */
  async checkForUpdateSilent(): Promise<void> {
    const config = vscode.workspace.getConfiguration('perl-lsp');

    // Guard: skip if user pinned a specific version
    const channel = config.get<string>('channel', 'latest');
    if (channel === 'tag') {
      return;
    }

    // Guard: skip if user manages their own binary
    const userPath = config.get<string>('serverPath', '');
    if (userPath) {
      return;
    }

    // Guard: only applies to auto-downloaded binaries (under globalStorageUri)
    const binaryPath = this.getLocalBinaryPath();
    const storagePath = this.context.globalStorageUri.fsPath;
    if (!binaryPath.startsWith(storagePath)) {
      return;
    }
    if (!fs.existsSync(binaryPath)) {
      return;
    }

    // Guard: check interval (treat negative values same as 0 — disabled)
    const intervalHours = config.get<number>('updateCheckInterval', 24);
    if (intervalHours <= 0) {
      return;
    }
    // The check interval is a property of one target's managed row. A GNU host
    // must not suppress a musl host's check merely because both hosts share
    // one extension global state object (#9847). The unscoped pre-#9847 value
    // is read once as a seed so upgrading does not force an immediate check.
    const stateKey =
      managedUpdateCheckStateKey(this.getHostCompatibilityKey()) ?? LEGACY_UPDATE_CHECK_STATE_KEY;
    const scopedCheck = this.context.globalState.get<number>(stateKey, 0);
    const lastCheck =
      scopedCheck > 0
        ? scopedCheck
        : this.context.globalState.get<number>(LEGACY_UPDATE_CHECK_STATE_KEY, 0);
    const elapsedHours = (Date.now() - lastCheck) / (1000 * 60 * 60);
    if (elapsedHours < intervalHours) {
      return;
    }

    // Record that we checked (even if the check fails) to avoid hammering
    await this.context.globalState.update(stateKey, Date.now());

    try {
      const localVersion = await this.getLocalVersion(binaryPath);
      if (!localVersion) {
        this.outputChannel.appendLine('[update-check] Could not read local version — skipping');
        return;
      }

      const release = await this.getLatestRelease();
      const remoteVersion = release.tag_name.replace(/^v/, '');

      if (compareVersions(localVersion, remoteVersion) >= 0) {
        this.outputChannel.appendLine(`[update-check] Up to date (${localVersion})`);
        return;
      }

      this.outputChannel.appendLine(
        `[update-check] New version available: ${remoteVersion} (installed: ${localVersion})`,
      );

      const autoUpdate = config.get<boolean>('autoUpdate', false);
      if (autoUpdate) {
        this.outputChannel.appendLine(`[update-check] Auto-updating to ${remoteVersion}`);
        await this.ensureBinary(true);
        return;
      }

      const choice = await vscode.window.showInformationMessage(
        `perllsp ${remoteVersion} is available (installed: ${localVersion})`,
        'Update',
        'Dismiss',
        "Don't ask again",
      );

      if (choice === 'Update') {
        await this.ensureBinary(true);
      } else if (choice === "Don't ask again") {
        await config.update('updateCheckInterval', 0, vscode.ConfigurationTarget.Global);
      }
      // 'Dismiss' is a no-op — will check again next interval
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      this.outputChannel.appendLine(`[update-check] Skipping: ${msg}`);
    }
  }

  private async getLocalVersion(binaryPath: string): Promise<string | null> {
    return new Promise((resolve) => {
      child_process.execFile(binaryPath, ['--version'], { timeout: 5000 }, (err, stdout) => {
        if (err) {
          resolve(null);
          return;
        }
        resolve(parseLocalVersion(stdout));
      });
    });
  }

  private findBinary(dir: string, name: string): string | null {
    const entries = fs.readdirSync(dir, { withFileTypes: true });

    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);

      if (entry.isDirectory()) {
        const found = this.findBinary(fullPath, name);
        if (found) return found;
      } else if (entry.name === name) {
        return fullPath;
      }
    }

    return null;
  }
}
