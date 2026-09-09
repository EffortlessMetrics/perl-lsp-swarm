import * as assert from 'assert';
import { spawn, type ChildProcessWithoutNullStreams } from 'child_process';
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

export async function waitForActiveDocumentGeneration(
  getReadiness: (() => ReceiptValue) | undefined,
  initialGeneration: number | undefined,
  timeoutMs: number,
): Promise<ReceiptValue | undefined> {
  if (!getReadiness || initialGeneration === undefined || initialGeneration > 0) {
    return getReadiness?.();
  }
  const deadline = Date.now() + timeoutMs;
  let snapshot = getReadiness();
  while (
    Date.now() < deadline &&
    (typeof snapshot.generation !== 'number' || snapshot.generation <= initialGeneration)
  ) {
    await new Promise((resolve) => setTimeout(resolve, 25));
    snapshot = getReadiness();
  }
  if (typeof snapshot.generation === 'number' && snapshot.generation > initialGeneration) {
    return snapshot;
  }
  return {
    ...snapshot,
    status: 'not_proven',
    reason: `startup generation did not advance beyond ${initialGeneration} within ${timeoutMs}ms`,
  };
}

export async function observeActiveDocumentReadiness(
  waitForReady: ((uri: string, timeoutMs?: number) => Promise<void>) | undefined,
  uri: string,
  timeoutMs: number,
): Promise<ReceiptValue> {
  if (!waitForReady) {
    return {
      status: 'not_proven',
      reason: 'installed extension did not expose waitForActiveDocumentReady',
    };
  }
  try {
    await withTimeout('active-document readiness', waitForReady(uri, timeoutMs), timeoutMs);
  } catch (error: unknown) {
    return {
      status: 'not_proven',
      reason: error instanceof Error ? error.message : String(error),
    };
  }
  return { status: 'ready' };
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

export function providerPosition(
  document: vscode.TextDocument,
  probe: string = '$value',
): vscode.Position {
  const offset = document.getText().indexOf(probe);
  assert.notEqual(offset, -1, `packaged journey fixture must contain the ${probe} probe`);
  return document.positionAt(offset);
}

export function assertProviderSucceeded(label: string, result: ReceiptValue): void {
  assert.notEqual(result.status, 'error', `${label}: ${JSON.stringify(result)}`);
}

/**
 * Observable command-registry truth for the activation-failure journey
 * (#7856): the retained support surfaces that must survive a rolled-back
 * attempt (#7854 wiring contract), plus the internal commands activation
 * registers without a manifest contribution.
 */
export const RETAINED_SUPPORT_COMMAND_IDS = [
  'perl-lsp.showWhatsNew',
  'perl-lsp.openConfigurationGuide',
  'perl-lsp.checkForUpdate',
  'perl-lsp.reportIssue',
  'perl-lsp.showCoexistenceStatus',
] as const;

/** Activation-registered commands that carry no `contributes.commands` entry. */
export const INTERNAL_ACTIVATION_COMMAND_IDS = ['perl-lsp.showBinaryIdentity'] as const;

/**
 * Every command id a committed activation is expected to register, derived
 * from the installed manifest so a newly contributed command joins the check
 * automatically: all contributed commands plus the known internal ones, minus
 * the retained support surfaces. The failure leg additionally proves the
 * COMPLETE claim — nothing beyond the retained set survives rollback — through
 * a before/after registry diff, so an unlisted or future registration cannot
 * slip past the sampled rows.
 */
export function mandatoryActivationCommandIds(packageJSON: {
  contributes?: { commands?: Array<{ command?: unknown }> };
}): string[] {
  const contributed = (packageJSON.contributes?.commands ?? [])
    .map((entry) => entry.command)
    .filter((command): command is string => typeof command === 'string' && command.length > 0);
  const retained = new Set<string>(RETAINED_SUPPORT_COMMAND_IDS);
  return [...new Set([...contributed, ...INTERNAL_ACTIVATION_COMMAND_IDS])].filter(
    (command) => !retained.has(command),
  );
}

const PROCESS_SCAN_TIMEOUT_MS = 20_000;
const PROCESS_SCAN_OUTPUT_MAX_BYTES = 4 * 1024 * 1024;

/**
 * Enumerate running OS processes launched from `directory` (the installed
 * extension's bundled-server directory). The scan is a bounded child process:
 * PowerShell `Get-Process` on Windows, `ps` elsewhere. The scan is fail-closed
 * on the scanner's own signals: a bounded-run failure or a nonzero exit code
 * throws, so a broken scanner never masquerades as "no processes". Stderr
 * text with a zero exit code is deliberately ignored — advisory output (for
 * example a shell deprecation notice) is not evidence about the process
 * table, and failing on it would make the scan spuriously fragile.
 */
export interface BundledServerProcessIdentity {
  pid: number;
  path: string;
  creationTimeFileTime?: string;
}

/**
 * Enumerate running OS server processes launched from `directory` WITH their
 * process identities (pid + executable path). The crash-recovery journey
 * (#7848) terminates the exact server process from the harness — never
 * through the extension's user restart command — so it needs the pid, not
 * just the path rows `scanProcessesUnderDirectory` returns. Fail-closed on
 * scanner signals, exactly like the path-only scan.
 */
export async function scanServerProcessIdentities(
  directory: string,
): Promise<BundledServerProcessIdentity[]> {
  const resolved = path.resolve(directory);
  let command: string;
  let args: string[];
  if (process.platform === 'win32') {
    command = 'powershell.exe';
    args = [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '(Get-Process -Name perllsp,perl-lsp -ErrorAction SilentlyContinue) | ' +
        'ForEach-Object { if ($_.Path) { "$($_.Id)`t$($_.Path)`t$($_.StartTime.ToFileTimeUtc())" } }',
    ];
  } else {
    command = 'ps';
    args = ['-eo', 'pid=,args='];
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
      `bundled-server pid scan did not complete (${result.outcome}): ${result.diagnostic ?? ''}`,
    );
  }
  if (result.exitCode !== 0) {
    throw new Error(
      `bundled-server pid scan exited ${result.exitCode}: ${(result.stderr || '').slice(0, 300)}`,
    );
  }
  const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin';
  // Match the directory boundary, not a bare prefix: `/path/to/dir-other`
  // must not match a scan for `/path/to/dir`.
  const separator = process.platform === 'win32' ? '\\' : '/';
  const bounded = resolved.endsWith(separator) ? resolved : resolved + separator;
  const needle = caseInsensitive ? bounded.toLowerCase() : bounded;
  // `ps -eo pid=,args=` pads its columns with spaces while the PowerShell
  // probe emits id/path tab-separated: accept any whitespace separator so
  // both hosts parse (a tab-only parser silently drops every Linux row).
  return result.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const match = /^(\d+)[ \t]+(.+)$/.exec(line);
      const windowsMatch = process.platform === 'win32' ? /^(\d+)\t(.+)\t(\d+)$/.exec(line) : null;
      const pidText = windowsMatch?.[1] ?? match?.[1];
      const executable = (windowsMatch?.[2] ?? match?.[2])?.trim();
      if (pidText === undefined || executable === undefined) {
        return null;
      }
      const pid = Number.parseInt(pidText, 10);
      if (pid <= 0 || executable.length === 0) {
        return null;
      }
      const haystack = caseInsensitive ? executable.toLowerCase() : executable;
      if (!haystack.startsWith(needle)) {
        return null;
      }
      // The PowerShell probe emits the pure executable path, while POSIX
      // `ps args` appends the server arguments (`perllsp --stdio`): identity
      // and digest checks need the real file, so reduce the POSIX row to the
      // invoked binary — the scanned directory plus the binary name.
      if (process.platform === 'win32') {
        const creationTimeFileTime = windowsMatch?.[3];
        return creationTimeFileTime === undefined
          ? { pid, path: executable }
          : { pid, path: executable, creationTimeFileTime };
      }
      const remainder = executable.slice(resolved.length);
      const binaryName = remainder.split(/[ \t]/, 1)[0];
      return binaryName === undefined || binaryName.length === 0
        ? { pid, path: executable }
        : { pid, path: resolved + binaryName };
    })
    .filter((entry): entry is BundledServerProcessIdentity => entry !== null);
}

export interface BoundedTerminationResult {
  outcome: 'terminated' | 'already_gone' | 'error';
  detail: string;
}

/**
 * Terminate the exact server process FROM THE HARNESS (#7848): an external,
 * unexpected process death the extension must observe through its own
 * Running→Stopped crash path. This is deliberately NOT the extension's user
 * restart command and not the activation API's stop seam — the issue's
 * negative controls forbid substituting either for the crash.
 */
export async function terminateServerProcess(pid: number): Promise<BoundedTerminationResult> {
  if (!Number.isInteger(pid) || pid <= 0) {
    return { outcome: 'error', detail: `invalid pid ${JSON.stringify(pid)}` };
  }
  if (process.platform === 'win32') {
    const result = await runBoundedProcess('taskkill', ['/PID', String(pid), '/F'], {
      shell: false,
      timeoutMs: 15_000,
      maxOutputBytes: 64 * 1024,
      terminationGraceMs: 2_000,
      terminationWatchdogMs: 10_000,
      windowsHide: true,
    });
    if (result.outcome === 'completed' && result.exitCode === 0) {
      return { outcome: 'terminated', detail: `taskkill /F pid ${pid}` };
    }
    if (
      result.outcome === 'completed' &&
      /not found|no such/i.test(result.stdout + result.stderr)
    ) {
      return { outcome: 'already_gone', detail: `taskkill reported pid ${pid} already gone` };
    }
    return {
      outcome: 'error',
      detail: `taskkill pid ${pid} ended ${result.outcome} exit ${String(result.exitCode)}: ${(
        result.stderr ||
        result.stdout ||
        ''
      ).slice(0, 300)}`,
    };
  }
  try {
    process.kill(pid, 'SIGKILL');
    return { outcome: 'terminated', detail: `SIGKILL pid ${pid}` };
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    if (/ESRCH/i.test(message)) {
      return { outcome: 'already_gone', detail: `pid ${pid} already gone (ESRCH)` };
    }
    return { outcome: 'error', detail: message };
  }
}

/**
 * Whether this host can suspend an external process at all. Windows uses the
 * checked-in owned-child PowerShell helper; if the helper is absent, retain
 * the typed `not_proven` limitation rather than fabricating a hang.
 */
export function canSuspendServerProcesses(): boolean {
  return process.platform !== 'win32' || fs.existsSync(path.resolve(__dirname, '../../../../scripts/tests/windows-owned-suspend.ps1'));
}

export interface SuspendResult {
  outcome: 'suspended' | 'resumed' | 'already_gone' | 'error';
  detail: string;
}

const windowsSuspensionHelpers = new Map<number, { child: ChildProcessWithoutNullStreams; creation: string }>();

async function terminateOwnedWindowsProcess(pid: number, creation: string): Promise<string> {
  const helper = path.resolve(__dirname, '../../../../scripts/tests/windows-owned-suspend.ps1');
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', helper, '-Cleanup', '-ProcessId', String(pid), '-CreationTimeFileTime', creation], { stdio: 'pipe', windowsHide: true });
  return await new Promise<string>((resolve) => {
    let settled = false;
    const finish = (detail: string): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(detail);
    };
    const timer = setTimeout(() => { child.kill(); finish('owned termination helper timed out'); }, 15_000);
    let error = '';
    child.stdout.on('data', () => { /* Drain helper output so cleanup cannot block. */ });
    child.stderr.on('data', (chunk: Buffer) => { error = (error + chunk.toString('utf8')).slice(-64 * 1024); });
    child.on('error', (spawnError) => finish(`owned termination helper error: ${spawnError.message}`));
    child.on('close', (code) => { finish(code === 0 ? 'owned target termination completed' : error || `owned termination helper exited ${String(code)}`); });
  });
}

/** Suspend the exact server process, using a pinned Windows helper when needed. */
export async function suspendServerProcess(
  pid: number,
  creationTimeFileTime?: string,
): Promise<SuspendResult> {
  // process.kill(0, ...) would signal the whole process group — including the
  // extension host. Guard every caller, not just the current scan-filtered one.
  if (!Number.isInteger(pid) || pid <= 0) {
    return { outcome: 'error', detail: `invalid server pid: ${pid}` };
  }
  if (process.platform === 'win32') {
    if (creationTimeFileTime === undefined || windowsSuspensionHelpers.has(pid)) {
      return { outcome: 'error', detail: 'missing or duplicate Windows process identity' };
    }
    const helper = path.resolve(__dirname, '../../../../scripts/tests/windows-owned-suspend.ps1');
    const child = spawn('powershell.exe', [
      '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', helper,
      '-ProcessId', String(pid), '-CreationTimeFileTime', creationTimeFileTime,
      '-TimeoutMilliseconds', '120000',
    ], { stdio: 'pipe', windowsHide: true });
    return await new Promise<SuspendResult>((resolve) => {
      let settled = false;
      let timer: NodeJS.Timeout | undefined;
      const fail = (detail: string): void => {
        if (settled) return;
        settled = true;
        if (timer !== undefined) clearTimeout(timer);
        // The helper may have suspended the target before it emitted its
        // handshake. Never strand that target by killing only the helper.
        child.kill();
        void terminateOwnedWindowsProcess(pid, creationTimeFileTime).then((cleanup) => {
          resolve({ outcome: 'error', detail: `${detail}; ${cleanup}` });
        });
      };
      timer = setTimeout(() => {
        fail('Windows suspension handshake timed out');
      }, 130_000);
      let output = '';
      child.stderr.on('data', (chunk: Buffer) => { output = (output + chunk.toString('utf8')).slice(-64 * 1024); });
      child.stdout.on('data', (chunk: Buffer) => {
        output = (output + chunk.toString('utf8')).slice(-64 * 1024);
        if (output.split(/\r?\n/).some((line) => line.startsWith('SUSPENDED ')) && !settled) {
          settled = true; clearTimeout(timer); windowsSuspensionHelpers.set(pid, { child, creation: creationTimeFileTime }); resolve({ outcome: 'suspended', detail: output.trim() });
        }
      });
      child.on('error', (error) => {
        fail(`Windows suspension helper error: ${error.message}`);
      });
      child.on('exit', (code) => {
        fail(`Windows suspension helper exited ${String(code)}: ${output.trim()}`);
      });
      child.on('close', () => {
        const state = windowsSuspensionHelpers.get(pid);
        if (settled && state?.child === child) {
          windowsSuspensionHelpers.delete(pid);
          void terminateOwnedWindowsProcess(pid, creationTimeFileTime);
        }
      });
    });
  }
  try {
    process.kill(pid, 'SIGSTOP');
    return { outcome: 'suspended', detail: `SIGSTOP pid ${pid}` };
  } catch (error: unknown) {
    return {
      outcome: 'error',
      detail: error instanceof Error ? error.message : String(error),
    };
  }
}

/** Resume a suspended server process, releasing only its helper-owned handles. */
export async function resumeServerProcess(pid: number): Promise<SuspendResult> {
  if (!Number.isInteger(pid) || pid <= 0) {
    return { outcome: 'error', detail: `invalid server pid: ${pid}` };
  }
  if (process.platform === 'win32') {
    const state = windowsSuspensionHelpers.get(pid);
    if (state === undefined) return { outcome: 'error', detail: `no owned Windows suspension for pid ${pid}` };
    const child = state.child;
    windowsSuspensionHelpers.delete(pid);
    return await new Promise<SuspendResult>((resolve) => {
      let error = '';
      let settled = false;
      let timer: NodeJS.Timeout | undefined;
      const finish = (result: SuspendResult): void => {
        if (settled) return;
        settled = true;
        if (timer !== undefined) clearTimeout(timer);
        resolve(result);
      };
      if (child.exitCode !== null) {
        void terminateOwnedWindowsProcess(pid, state.creation).then((detail) => finish({ outcome: 'error', detail: `Windows resume helper already exited ${String(child.exitCode)}; ${detail}` }));
        return;
      }
      timer = setTimeout(() => {
        child.kill();
        void terminateOwnedWindowsProcess(pid, state.creation).then((detail) => finish({ outcome: 'error', detail: `Windows resume handshake timed out; ${detail}` }));
      }, 10_000);
      child.stderr.on('data', (chunk: Buffer) => { error = (error + chunk.toString('utf8')).slice(-64 * 1024); });
      child.stdin.on('error', (inputError) => { error += inputError.message; });
      child.on('close', (code) => {
        if (code === 0) {
          finish({ outcome: 'resumed', detail: `resumed owned Windows suspension pid ${pid}` });
        } else {
          try { process.kill(pid, 0); void terminateOwnedWindowsProcess(pid, state.creation).then((detail) => finish({ outcome: 'error', detail: `${error || `Windows resume helper exited ${String(code)}`}; ${detail}` })); }
          catch (probeError: unknown) {
            const codeText = probeError instanceof Error && 'code' in probeError ? String((probeError as NodeJS.ErrnoException).code) : '';
            finish(codeText === 'ESRCH' ? { outcome: 'already_gone', detail: `owned Windows process pid ${pid} exited before resume` } : { outcome: 'error', detail: error || `Windows resume helper exited ${String(code)}` });
          }
        }
      });
      child.stdin.write('resume\n'); child.stdin.end();
    });
  }
  try {
    process.kill(pid, 'SIGCONT');
    return { outcome: 'resumed', detail: `SIGCONT pid ${pid}` };
  } catch (error: unknown) {
    return {
      outcome: 'error',
      detail: error instanceof Error ? error.message : String(error),
    };
  }
}

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
  // Match the directory boundary, not a bare prefix: `/path/to/dir-other`
  // must not match a scan for `/path/to/dir`.
  const separator = process.platform === 'win32' ? '\\' : '/';
  const bounded = resolved.endsWith(separator) ? resolved : resolved + separator;
  const needle = caseInsensitive ? bounded.toLowerCase() : bounded;
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
