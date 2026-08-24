import * as assert from 'assert';
import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { parsePackagedServerVersionStdout } from '../../packagedServerVersion';
import { runBoundedProcess } from '../../testAdapter';

/**
 * Shared primitives for the published-smoke journeys (packaged bundle journey
 * #4346/#6056, packaged activation-failure journey #7856): candidate-bound
 * receipt directories, packaged-server identity probes, bounded waits, and
 * provider smoke helpers that all run inside the real extension host against
 * the installed VSIX.
 */

export type ReceiptValue = Record<string, unknown>;

export function platformLabel(): string {
  switch (process.platform) {
    case 'win32':
      return 'windows';
    case 'darwin':
      return 'macos';
    case 'linux':
      return 'linux';
    default:
      return process.platform;
  }
}

export function receiptsDir(): string {
  const root =
    process.env.PERL_LSP_SMOKE_RECEIPTS_DIR ??
    path.resolve(__dirname, '..', '..', '..', '..', 'target', 'receipts', 'vscode-smoke');
  const label = process.env.PERL_LSP_SMOKE_SOURCE_LABEL ?? 'packaged-bundle';
  assert.match(
    label,
    /^[A-Za-z0-9_-]+$/,
    'smoke receipt label must be a single safe path component',
  );
  const directory = path.join(root, label, platformLabel());
  fs.mkdirSync(directory, { recursive: true });
  return directory;
}

export function sha256(filePath: string): string {
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

export type BundledServerVersion =
  | {
      status: 'ok';
      version: string;
      stdout: string;
      stderr: string;
      outcome: 'completed';
      output_truncated: boolean;
      termination_confirmed: boolean;
    }
  | {
      status: 'error';
      version?: never;
      stdout: string;
      stderr: string;
      outcome: string;
      output_truncated: boolean;
      termination_confirmed: boolean;
      message: string;
    };

const VERSION_PROBE_TIMEOUT_MS = 15_000;
const VERSION_PROBE_OUTPUT_MAX_BYTES = 64 * 1024;

export async function bundledServerVersion(binaryPath: string): Promise<BundledServerVersion> {
  const result = await runBoundedProcess(binaryPath, ['--version'], {
    shell: false,
    timeoutMs: VERSION_PROBE_TIMEOUT_MS,
    maxOutputBytes: VERSION_PROBE_OUTPUT_MAX_BYTES,
    terminationGraceMs: 500,
    terminationWatchdogMs: 5_000,
    windowsHide: true,
  });
  const terminationConfirmed =
    result.outcome !== 'spawn_error' && result.outcome !== 'termination_failed';
  const base = {
    stdout: result.stdout,
    stderr: result.stderr,
    outcome: result.outcome,
    output_truncated: result.outcome === 'output_limit',
    termination_confirmed: terminationConfirmed,
  };
  if (result.outcome !== 'completed') {
    return {
      status: 'error',
      ...base,
      message:
        result.diagnostic ??
        `bundled server --version ended with ${result.outcome} before a clean completion`,
    };
  }
  const completedBase = { ...base, outcome: 'completed' as const };
  try {
    const version = parsePackagedServerVersionStdout(result.stdout);
    if (!version) {
      return {
        status: 'error',
        ...completedBase,
        message: 'bundled server --version did not contain a semantic version',
      };
    }
    return {
      status: 'ok',
      version,
      ...completedBase,
    };
  } catch (error: unknown) {
    return {
      status: 'error',
      ...completedBase,
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

export async function withTimeout<T>(
  label: string,
  operation: PromiseLike<T>,
  timeoutMs: number,
): Promise<T> {
  let timeout: NodeJS.Timeout | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timeout = setTimeout(
      () => reject(new Error(`${label} timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([Promise.resolve(operation), timeoutPromise]);
  } finally {
    if (timeout) {
      clearTimeout(timeout);
    }
  }
}

export async function waitForStartupMetrics(
  getMetrics: () => ReceiptValue,
  timeoutMs: number,
): Promise<ReceiptValue> {
  const deadline = Date.now() + timeoutMs;
  let metrics = getMetrics();
  while (
    Date.now() < deadline &&
    [metrics.binary_resolution_status, metrics.server_start_status, metrics.initialize_status].some(
      (status) => status === 'running',
    )
  ) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    metrics = getMetrics();
  }
  return metrics;
}

export function bundledBinaryPath(extensionPath: string): string {
  const directory = path.join(extensionPath, 'bin', `${process.platform}-${process.arch}`);
  const names =
    process.platform === 'win32' ? ['perllsp.exe', 'perl-lsp.exe'] : ['perllsp', 'perl-lsp'];
  const binary = names
    .map((name) => path.join(directory, name))
    .find((candidate) => fs.existsSync(candidate));
  assert.ok(binary, `packaged VSIX must contain a bundled server in ${directory}`);
  return binary;
}

export function pathsEquivalent(left: unknown, right: string): boolean {
  if (typeof left !== 'string' || left.length === 0) {
    return false;
  }
  const normalizedLeft = path.resolve(left);
  const normalizedRight = path.resolve(right);
  if (process.platform === 'win32' || process.platform === 'darwin') {
    return normalizedLeft.toLowerCase() === normalizedRight.toLowerCase();
  }
  return normalizedLeft === normalizedRight;
}

export async function providerResult(
  label: string,
  command: string,
  ...args: unknown[]
): Promise<ReceiptValue> {
  const started = performance.now();
  try {
    const result = await withTimeout(
      label,
      vscode.commands.executeCommand(command, ...args),
      15_000,
    );
    const record: ReceiptValue = {
      status: 'ok',
      duration_ms: Math.round(performance.now() - started),
    };
    if (Array.isArray(result)) {
      record.item_count = result.length;
    } else if (result && typeof result === 'object' && 'items' in result) {
      const items = (result as { items?: unknown }).items;
      record.item_count = Array.isArray(items) ? items.length : 0;
    } else if (result === undefined || result === null) {
      record.result = 'empty';
    } else {
      record.result = 'present';
    }
    return record;
  } catch (error: unknown) {
    return {
      status: 'error',
      duration_ms: Math.round(performance.now() - started),
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

export function providerPosition(document: vscode.TextDocument): vscode.Position {
  const offset = document.getText().indexOf('$value');
  assert.notEqual(offset, -1, 'packaged journey fixture must contain the $value probe');
  return document.positionAt(offset);
}

export function assertProviderSucceeded(label: string, result: ReceiptValue): void {
  assert.notEqual(result.status, 'error', `${label}: ${JSON.stringify(result)}`);
}

/**
 * Observable command-registry truth for the activation-failure journey
 * (#7856): the mandatory command registrations a committed activation owns,
 * and the retained support surfaces that must survive a rolled-back attempt
 * (#7854 wiring contract).
 */
export const MANDATORY_COMMAND_IDS = [
  'perl-lsp.showOutput',
  'perl-lsp.reinstall',
  'perl-lsp.restart',
  'perl-lsp.showBinaryIdentity',
  'perl-lsp.runHealthCheck',
  'perl-lsp.runAllTests',
] as const;

export const RETAINED_SUPPORT_COMMAND_IDS = [
  'perl-lsp.showWhatsNew',
  'perl-lsp.openConfigurationGuide',
  'perl-lsp.checkForUpdate',
  'perl-lsp.reportIssue',
] as const;

const PROCESS_SCAN_TIMEOUT_MS = 20_000;
const PROCESS_SCAN_OUTPUT_MAX_BYTES = 4 * 1024 * 1024;

/**
 * Enumerate running OS processes launched from `directory` (the installed
 * extension's bundled-server directory). The scan is a bounded child process:
 * PowerShell `Get-Process` on Windows, `ps` elsewhere. The scan is fail-closed:
 * a nonzero exit code, scanner diagnostics on stderr, or a bounded-run failure
 * all throw — a broken scanner must never masquerade as "no processes".
 */
export async function scanProcessesUnderDirectory(directory: string): Promise<string[]> {
  const resolved = path.resolve(directory);
  let command: string;
  let args: string[];
  if (process.platform === 'win32') {
    command = 'powershell.exe';
    args = [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '(Get-Process -Name perllsp,perl-lsp -ErrorAction SilentlyContinue).Path',
    ];
  } else {
    command = 'ps';
    args = ['-eo', 'args='];
  }
  const result = await runBoundedProcess(command, args, {
    shell: false,
    timeoutMs: PROCESS_SCAN_TIMEOUT_MS,
    maxOutputBytes: PROCESS_SCAN_OUTPUT_MAX_BYTES,
    terminationGraceMs: 1_000,
    terminationWatchdogMs: 5_000,
    windowsHide: true,
  });
  if (result.outcome !== 'completed') {
    throw new Error(
      `bundled-server process scan did not complete (${result.outcome}): ${result.diagnostic ?? ''}`,
    );
  }
  if (result.exitCode !== 0) {
    throw new Error(
      `bundled-server process scan exited ${result.exitCode}: ${(result.stderr || '').slice(0, 300)}`,
    );
  }
  const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin';
  const needle = caseInsensitive ? resolved.toLowerCase() : resolved;
  return result.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => {
      if (line.length === 0) {
        return false;
      }
      const haystack = caseInsensitive ? line.toLowerCase() : line;
      return haystack.startsWith(needle);
    });
}
