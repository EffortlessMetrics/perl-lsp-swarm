import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { spawn, type ChildProcess, type SpawnOptions } from 'child_process';
import { StringDecoder } from 'string_decoder';

const WINDOWS_TREE_KILL_TIMEOUT_MS = 5_000;

export type TreeKillResult = { ok: true } | { ok: false; diagnostic: string };

function killWindowsProcessTree(pid: number | undefined): Promise<TreeKillResult> {
  if (process.platform !== 'win32') {
    return Promise.resolve({ ok: true });
  }
  if (pid === undefined) {
    return Promise.resolve({
      ok: false,
      diagnostic: 'Windows process-tree cleanup could not start because the parent had no PID.',
    });
  }

  return new Promise((resolve) => {
    const killer = spawn('taskkill', ['/PID', String(pid), '/T', '/F'], {
      shell: false,
      windowsHide: true,
      stdio: 'ignore',
    });
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) {
        return;
      }
      settled = true;
      try {
        killer.kill();
      } catch {
        // taskkill may already have exited or been torn down with its parent.
      }
      resolve({
        ok: false,
        diagnostic: `Windows process-tree cleanup exceeded ${WINDOWS_TREE_KILL_TIMEOUT_MS} ms.`,
      });
    }, WINDOWS_TREE_KILL_TIMEOUT_MS);
    const finish = (result: TreeKillResult): void => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      resolve(result);
    };
    killer.once('error', (error: Error) =>
      finish({ ok: false, diagnostic: `Windows process-tree cleanup failed: ${error.message}.` }),
    );
    killer.once('close', (code: number | null, signal: NodeJS.Signals | null) => {
      if (code === 0) {
        finish({ ok: true });
        return;
      }
      finish({
        ok: false,
        diagnostic: `Windows process-tree cleanup exited with ${code ?? signal ?? 'unknown'}.`,
      });
    });
  });
}

export interface ProveExecutionLimits {
  timeoutMs: number;
  maxOutputBytes: number;
  terminationGraceMs: number;
}

export const DEFAULT_PROVE_EXECUTION_LIMITS: Readonly<ProveExecutionLimits> = {
  timeoutMs: 120_000,
  maxOutputBytes: 1_048_576,
  terminationGraceMs: 1_000,
};

export type BoundedProcessOutcome =
  | 'completed'
  | 'timed_out'
  | 'output_limit'
  | 'cancelled'
  | 'input_error'
  | 'termination_failed'
  | 'spawn_error';

export interface BoundedProcessResult {
  outcome: BoundedProcessOutcome;
  stdout: string;
  stderr: string;
  exitCode: number | null;
  signal: NodeJS.Signals | null;
  capturedOutputBytes: number;
  diagnostic?: string;
}

export interface BoundedProcessOptions extends Omit<SpawnOptions, 'signal' | 'stdio'> {
  signal?: AbortSignal;
  /** Optional newline-delimited input written to the child before execution. */
  stdin?: string;
  timeoutMs: number;
  maxOutputBytes: number;
  terminationGraceMs: number;
  /**
   * Bound after SIGKILL / tree-kill escalation. If the child never emits
   * `close` within this window, the probe finishes as `termination_failed`
   * instead of hanging the host process.
   */
  terminationWatchdogMs?: number;
  /**
   * Test seam for kill delivery. Return `false` when the signal could not be
   * delivered; production callers leave this unset and use `ChildProcess.kill`.
   */
  killProcess?: (proc: ReturnType<typeof spawn>, signal: NodeJS.Signals) => boolean;
  /** Test seam for Windows process-tree cleanup; production callers leave this unset. */
  killProcessTree?: (pid: number | undefined) => Promise<TreeKillResult>;
  /** Test seam for child lifecycle ordering; production callers leave this unset. */
  spawnProcess?: typeof spawn;
}

/**
 * Run an external process with one bounded output envelope and one termination
 * path. The caller owns the user-facing mapping of the returned outcome.
 */
const DEFAULT_TERMINATION_WATCHDOG_MS = 5_000;

export function runBoundedProcess(
  command: string,
  args: readonly string[],
  options: BoundedProcessOptions,
): Promise<BoundedProcessResult> {
  return new Promise((resolve) => {
    const {
      signal,
      stdin,
      timeoutMs,
      maxOutputBytes,
      terminationGraceMs,
      terminationWatchdogMs = DEFAULT_TERMINATION_WATCHDOG_MS,
      killProcess,
      killProcessTree,
      spawnProcess,
      ...spawnOptions
    } = options;
    const proc: ChildProcess = (spawnProcess ?? spawn)(command, [...args], {
      ...spawnOptions,
      stdio: 'pipe',
    });
    const stdoutDecoder = new StringDecoder('utf8');
    const stderrDecoder = new StringDecoder('utf8');
    let stdout = '';
    let stderr = '';
    let capturedOutputBytes = 0;
    let truncatedTarget: 'stdout' | 'stderr' | undefined;
    let decodersFlushed = false;
    let termination:
      | Exclude<BoundedProcessOutcome, 'completed' | 'spawn_error' | 'termination_failed'>
      | undefined;
    let settled = false;
    let closed = false;
    let graceTimer: NodeJS.Timeout | undefined;
    let watchdogTimer: NodeJS.Timeout | undefined;
    let treeKill: Promise<TreeKillResult> | undefined;
    let parentExited = false;
    let stdinError: Error | undefined;
    const timeout = setTimeout(() => requestTermination('timed_out'), timeoutMs);
    const needsTreeKill = process.platform === 'win32';
    const killTree = killProcessTree ?? killWindowsProcessTree;

    const startTreeKill = (): void => {
      if (parentExited || proc.exitCode !== null || proc.signalCode !== null) {
        treeKill = Promise.resolve({
          ok: false,
          diagnostic:
            'Windows process-tree cleanup could not start because the parent had already exited.',
        });
        return;
      }
      const cleanupPromise = killTree(proc.pid);
      treeKill = cleanupPromise;
    };

    const deliverKill = (killSignal: NodeJS.Signals): boolean => {
      if (killProcess) {
        return killProcess(proc, killSignal);
      }
      try {
        return proc.kill(killSignal);
      } catch {
        // The process may have exited between the guard and kill(). The close
        // event remains the single completion path when it fires.
        return false;
      }
    };

    const cleanup = (): void => {
      clearTimeout(timeout);
      if (graceTimer !== undefined) {
        clearTimeout(graceTimer);
      }
      if (watchdogTimer !== undefined) {
        clearTimeout(watchdogTimer);
      }
      signal?.removeEventListener('abort', onAbort);
    };

    const finish = (
      outcome: BoundedProcessOutcome,
      exitCode: number | null,
      signal: NodeJS.Signals | null,
      diagnostic?: string,
    ): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      resolve({
        outcome,
        stdout,
        stderr,
        exitCode,
        signal,
        capturedOutputBytes,
        ...(diagnostic === undefined ? {} : { diagnostic }),
      });
    };

    const armTerminationWatchdog = (): void => {
      if (watchdogTimer !== undefined || closed || settled) {
        return;
      }
      watchdogTimer = setTimeout(() => {
        if (closed || settled) {
          return;
        }
        flushDecoders();
        finish(
          'termination_failed',
          null,
          null,
          `Process did not exit within ${terminationWatchdogMs} ms after forced termination.`,
        );
      }, terminationWatchdogMs);
    };

    const requestTermination = (
      reason: Exclude<BoundedProcessOutcome, 'completed' | 'spawn_error' | 'termination_failed'>,
    ): void => {
      if (termination !== undefined || closed) {
        return;
      }
      termination = reason;
      if (needsTreeKill) {
        startTreeKill();
        armTerminationWatchdog();
        return;
      }
      deliverKill('SIGTERM');
      graceTimer = setTimeout(() => {
        if (closed) {
          return;
        }
        deliverKill('SIGKILL');
        armTerminationWatchdog();
      }, terminationGraceMs);
    };

    const finishAfterTreeKill = (exitCode: number | null, signal: NodeJS.Signals | null): void => {
      const outcome = termination;
      if (outcome === undefined) {
        finish('completed', exitCode, signal);
        return;
      }
      const detail = {
        timed_out: `Process exceeded the ${timeoutMs} ms deadline.`,
        output_limit: `Process output exceeded the ${maxOutputBytes}-byte capture limit.`,
        cancelled: 'Process execution was cancelled.',
        input_error: `Failed to provide process input: ${stdinError?.message ?? 'the input stream closed unexpectedly'}.`,
      }[outcome];
      void (treeKill ?? Promise.resolve({ ok: true as const })).then((cleanupResult) => {
        if (!cleanupResult.ok) {
          finish('termination_failed', exitCode, signal, cleanupResult.diagnostic);
          return;
        }
        finish(outcome, exitCode, signal, detail);
      });
    };

    const appendDecodedOutput = (target: 'stdout' | 'stderr', text: string): void => {
      if (target === 'stdout') {
        stdout += text;
      } else {
        stderr += text;
      }
    };

    const flushDecoders = (): void => {
      if (decodersFlushed) {
        return;
      }
      decodersFlushed = true;
      // StringDecoder.end() emits U+FFFD for an incomplete trailing sequence.
      // When the byte ceiling cuts a character, that partial character is not
      // captured output and must not become a synthetic diagnostic character.
      if (truncatedTarget !== 'stdout') {
        appendDecodedOutput('stdout', stdoutDecoder.end());
      }
      if (truncatedTarget !== 'stderr') {
        appendDecodedOutput('stderr', stderrDecoder.end());
      }
    };

    const onAbort = (): void => requestTermination('cancelled');
    if (signal?.aborted) {
      onAbort();
    } else {
      signal?.addEventListener('abort', onAbort, { once: true });
    }

    const appendOutput = (target: 'stdout' | 'stderr', chunk: Buffer): void => {
      if (termination !== undefined || settled) {
        return;
      }
      const remaining = maxOutputBytes - capturedOutputBytes;
      if (chunk.byteLength > remaining) {
        const bounded = remaining > 0 ? chunk.subarray(0, remaining) : undefined;
        if (bounded !== undefined) {
          appendDecodedOutput(
            target,
            target === 'stdout' ? stdoutDecoder.write(bounded) : stderrDecoder.write(bounded),
          );
        }
        truncatedTarget = target;
        capturedOutputBytes = maxOutputBytes;
        requestTermination('output_limit');
        return;
      }
      appendDecodedOutput(
        target,
        target === 'stdout' ? stdoutDecoder.write(chunk) : stderrDecoder.write(chunk),
      );
      capturedOutputBytes += chunk.byteLength;
    };

    proc.stdout?.on('data', (chunk: Buffer) => appendOutput('stdout', chunk));
    proc.stderr?.on('data', (chunk: Buffer) => appendOutput('stderr', chunk));
    proc.on('error', (error: Error) => {
      finish(
        'spawn_error',
        null,
        null,
        `Failed to run prove: ${error.message}. Is prove installed?`,
      );
    });
    proc.on('exit', () => {
      parentExited = true;
    });
    if (stdin !== undefined && proc.stdin !== null) {
      proc.stdin.once('error', (error: Error) => {
        stdinError = error;
        if (termination === undefined && !closed) {
          requestTermination('input_error');
        }
      });
      proc.stdin.end(stdin);
    }
    proc.on('close', (exitCode, signal) => {
      closed = true;
      flushDecoders();
      if (termination === undefined) {
        finish('completed', exitCode, signal);
        return;
      }
      finishAfterTreeKill(exitCode, signal);
    });
  });
}

function configuredProveLimits(resource?: vscode.Uri): ProveExecutionLimits {
  const config = vscode.workspace.getConfiguration('perl-lsp', resource);
  const positive = (value: number | undefined, fallback: number): number =>
    Number.isFinite(value) && value !== undefined && value > 0 ? Math.floor(value) : fallback;
  return {
    timeoutMs: positive(
      config.get<number>('testAdapterTimeoutMs'),
      DEFAULT_PROVE_EXECUTION_LIMITS.timeoutMs,
    ),
    maxOutputBytes: positive(
      config.get<number>('testAdapterMaxOutputBytes'),
      DEFAULT_PROVE_EXECUTION_LIMITS.maxOutputBytes,
    ),
    terminationGraceMs: positive(
      config.get<number>('testAdapterTerminationGraceMs'),
      DEFAULT_PROVE_EXECUTION_LIMITS.terminationGraceMs,
    ),
  };
}

/**
 * Perl Test Explorer integration.
 *
 * Discovers `.t` test files in the workspace, parses `subtest` blocks,
 * and runs them via `prove -v`, mapping TAP output to VSCode test results.
 */

/**
 * Resolve the `prove` command for the current platform.
 *
 * On Windows, invoke the matching Perl interpreter directly against its
 * adjacent `prove.bat` with the documented stdin-list form. This preserves
 * the Perl installation selected by PATH and avoids resolving a different
 * `prove` from PATH. On other platforms, derive `prove` from the Perl directory when
 * possible.
 *
 * Returns `{ command, args, shell }` for use with `child_process.spawn`.
 */
export function resolveProveCommand(extraArgs: string[]): {
  command: string;
  args: string[];
  shell: boolean;
  error?: string;
} {
  const isWindows = process.platform === 'win32';

  // Try to find `perl` and its matching `prove` on PATH.
  let perlPath: string | null = null;
  let provePath: string | null = null;
  try {
    const { execFileSync } = require('child_process');
    perlPath = execFileSync('perl', ['-e', 'print $^X'], {
      encoding: 'utf8',
      timeout: 3000,
    }).trim();
    if (perlPath) {
      const perlDir = path.dirname(perlPath);
      const candidate = path.join(perlDir, isWindows ? 'prove.bat' : 'prove');
      if (fs.existsSync(candidate)) {
        provePath = candidate;
      }
    }
  } catch {
    // perl not on PATH or execFileSync failed — report an actionable resolution error.
  }

  if (isWindows && perlPath && provePath) {
    return { command: perlPath, args: ['-x', provePath, ...extraArgs], shell: false };
  }

  if (provePath) {
    return { command: provePath, args: extraArgs, shell: false };
  }

  if (isWindows) {
    return {
      command: '',
      args: [],
      shell: false,
      error:
        'A matching Perl/prove installation was not found on PATH; install Perl with prove or configure a supported Perl runtime.',
    };
  }

  return { command: 'prove', args: extraArgs, shell: false };
}

export interface SubtestInfo {
  name: string;
  line: number;
}

// Matches: subtest 'name' => sub {   or   subtest "name" => sub {
// Also matches: subtest 'name', sub {
const SUBTEST_RE = /^\s*subtest\s+(['"])(.*?)\1\s*(?:=>|,)\s*sub\s*\{/;

export class PerlTestAdapter implements vscode.Disposable {
  private testController: vscode.TestController;
  private disposables: vscode.Disposable[] = [];
  private fileItems = new Map<string, vscode.TestItem>();

  constructor() {
    this.testController = vscode.tests.createTestController('perlTestController', 'Perl Tests');

    this.testController.createRunProfile(
      'Run',
      vscode.TestRunProfileKind.Run,
      (request, token) => this.runHandler(request, token),
      true,
    );

    this.testController.refreshHandler = () => this.discoverAllTests();

    // File system watcher for .t files
    const watcher = vscode.workspace.createFileSystemWatcher('**/*.t');
    watcher.onDidCreate((uri) => this.discoverFileTests(uri));
    watcher.onDidChange((uri) => this.discoverFileTests(uri));
    watcher.onDidDelete((uri) => this.removeFile(uri));
    this.disposables.push(watcher);

    // Re-parse on document save (picks up new subtests)
    const saveListener = vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.uri.fsPath.endsWith('.t')) {
        void this.discoverFileTests(doc.uri);
      }
    });
    this.disposables.push(saveListener);

    // Initial discovery
    void this.discoverAllTests();
  }

  // -- Discovery -------------------------------------------------------

  private async discoverAllTests(): Promise<void> {
    this.testController.items.replace([]);
    this.fileItems.clear();

    // Cap at 500 files to prevent extension host freeze on large workspaces (#5110).
    const files = await vscode.workspace.findFiles(
      '**/*.t',
      '{**/node_modules/**,**/blib/**}',
      500,
    );
    for (const uri of files) {
      await this.discoverFileTests(uri);
    }
  }

  private async discoverFileTests(uri: vscode.Uri): Promise<void> {
    const workspaceFolder = vscode.workspace.getWorkspaceFolder(uri);
    const relativePath = workspaceFolder
      ? path.relative(workspaceFolder.uri.fsPath, uri.fsPath)
      : path.basename(uri.fsPath);

    const fileId = uri.toString();
    let fileItem = this.fileItems.get(fileId);

    if (!fileItem) {
      fileItem = this.testController.createTestItem(fileId, relativePath, uri);
      this.testController.items.add(fileItem);
      this.fileItems.set(fileId, fileItem);
    } else {
      fileItem.children.replace([]);
    }

    // Parse subtests from file content
    const subtests = await this.parseSubtests(uri);
    for (const st of subtests) {
      const child = this.testController.createTestItem(`${fileId}::${st.name}`, st.name, uri);
      child.range = new vscode.Range(st.line, 0, st.line, 0);
      fileItem.children.add(child);
    }
  }

  private async parseSubtests(uri: vscode.Uri): Promise<SubtestInfo[]> {
    try {
      const doc = await vscode.workspace.openTextDocument(uri);
      const subtests: SubtestInfo[] = [];

      for (let i = 0; i < doc.lineCount; i++) {
        const line = doc.lineAt(i).text;
        const match = SUBTEST_RE.exec(line);
        if (match) {
          const name = match[2];
          if (name !== undefined) {
            subtests.push({ name, line: i });
          }
        }
      }

      return subtests;
    } catch {
      return [];
    }
  }

  private removeFile(uri: vscode.Uri): void {
    const fileId = uri.toString();
    this.testController.items.delete(fileId);
    this.fileItems.delete(fileId);
  }

  // -- Run handler -----------------------------------------------------

  private async runHandler(
    request: vscode.TestRunRequest,
    token: vscode.CancellationToken,
  ): Promise<void> {
    const run = this.testController.createTestRun(request);

    // Collect files to run. If no specific tests requested, run all.
    const testsToRun = request.include ?? this.gatherAllItems();

    // Group by file so we run prove once per file
    const byFile = new Map<string, { fileItem: vscode.TestItem; subtests: vscode.TestItem[] }>();

    for (const item of testsToRun) {
      if (token.isCancellationRequested) {
        break;
      }

      if (item.uri && item.children.size > 0) {
        // This is a file-level item
        const children: vscode.TestItem[] = [];
        item.children.forEach((c) => children.push(c));
        byFile.set(item.uri.fsPath, { fileItem: item, subtests: children });
      } else if (item.uri) {
        // This is a subtest -- find parent file
        const fsPath = item.uri.fsPath;
        const entry = byFile.get(fsPath);
        if (entry) {
          entry.subtests.push(item);
        } else {
          // Find the file item for this subtest
          const fileId = item.uri.toString();
          const fileItem = this.fileItems.get(fileId);
          if (fileItem) {
            byFile.set(fsPath, { fileItem, subtests: [item] });
          }
        }
      }
    }

    for (const [filePath, { fileItem, subtests }] of byFile) {
      if (token.isCancellationRequested) {
        break;
      }

      run.started(fileItem);
      for (const st of subtests) {
        run.started(st);
      }

      await this.runProve(filePath, fileItem, subtests, run, token);
    }

    run.end();
  }

  private gatherAllItems(): vscode.TestItem[] {
    const items: vscode.TestItem[] = [];
    this.testController.items.forEach((item) => items.push(item));
    return items;
  }

  // -- prove execution & TAP parsing -----------------------------------

  private async runProve(
    filePath: string,
    fileItem: vscode.TestItem,
    subtests: vscode.TestItem[],
    run: vscode.TestRun,
    token: vscode.CancellationToken,
  ): Promise<void> {
    const workspaceFolder = vscode.workspace.getWorkspaceFolder(fileItem.uri!);
    const cwd = workspaceFolder?.uri.fsPath ?? path.dirname(filePath);
    const startTime = Date.now();
    const isWindows = process.platform === 'win32';
    const resolution = resolveProveCommand(
      isWindows ? ['-v', '--nocolor', '-'] : ['-v', '--nocolor', filePath],
    );
    if (resolution.error !== undefined) {
      const message = new vscode.TestMessage(resolution.error);
      if (fileItem.uri) {
        message.location = new vscode.Location(fileItem.uri, new vscode.Position(0, 0));
      }
      run.errored(fileItem, message, Date.now() - startTime);
      for (const st of subtests) {
        run.errored(st, new vscode.TestMessage(message.message));
      }
      return;
    }
    const { command: proveCmd, args: proveArgs, shell: useShell } = resolution;
    const cancellation = new AbortController();
    const killOnCancel = token.onCancellationRequested(() => cancellation.abort());
    if (token.isCancellationRequested) {
      cancellation.abort();
    }

    const result = await runBoundedProcess(proveCmd, proveArgs, {
      cwd,
      env: { ...process.env, HARNESS_ACTIVE: '1' },
      shell: useShell,
      signal: cancellation.signal,
      ...(isWindows ? { stdin: `${filePath}\n` } : {}),
      ...configuredProveLimits(fileItem.uri),
    });
    killOnCancel.dispose();

    if (result.outcome !== 'completed') {
      const message = new vscode.TestMessage(
        result.diagnostic ?? 'The prove process ended before producing a complete result.',
      );
      if (fileItem.uri) {
        message.location = new vscode.Location(fileItem.uri, new vscode.Position(0, 0));
      }
      run.errored(fileItem, message, Date.now() - startTime);
      for (const st of subtests) {
        run.errored(st, new vscode.TestMessage(message.message));
      }
      return;
    }

    const duration = Date.now() - startTime;
    const tapResults = parseTapOutput(result.stdout);
    const subtestResults = parseSubtestResults(result.stdout);

    // Map subtest results to test items
    for (const st of subtests) {
      const stName = st.label;
      const subtestResult = subtestResults.get(stName);

      if (subtestResult !== undefined) {
        if (subtestResult.ok) {
          run.passed(st, subtestResult.duration);
        } else {
          run.failed(
            st,
            new vscode.TestMessage(subtestResult.diagnostic || `Subtest "${stName}" failed`),
            subtestResult.duration,
          );
        }
      } else {
        // Subtest was not in output -- mark skipped
        run.skipped(st);
      }
    }

    // File-level result
    if (result.exitCode === 0 && tapResults.failed === 0) {
      run.passed(fileItem, duration);
    } else {
      const message = new vscode.TestMessage(
        result.stderr.trim() || describeFileFailure(result.exitCode, tapResults),
      );
      if (fileItem.uri) {
        message.location = new vscode.Location(fileItem.uri, new vscode.Position(0, 0));
      }
      run.failed(fileItem, message, duration);
    }
  }

  // -- Public API -------------------------------------------------------

  public async runFileTests(uri: vscode.Uri): Promise<void> {
    const fileId = uri.toString();
    const fileItem = this.fileItems.get(fileId);

    if (fileItem) {
      const request = new vscode.TestRunRequest([fileItem]);
      const tokenSource = new vscode.CancellationTokenSource();
      try {
        await this.runHandler(request, tokenSource.token);
      } finally {
        tokenSource.dispose();
      }
    } else {
      vscode.window.showWarningMessage(
        'No tests found in this file. Try refreshing the test explorer.',
      );
    }
  }

  dispose(): void {
    this.testController.dispose();
    for (const d of this.disposables) {
      d.dispose();
    }
  }
}

/**
 * Describe why a test file failed when the process produced no stderr.
 *
 * A non-zero exit with no failing assertion is a real outcome — a bail out, a
 * plan the run never reached, or a crash after the last `ok` line. Reporting it
 * as "0 of N tests failed" contradicts the failure the user is looking at, so
 * each shape gets its own explanation.
 */
export function describeFileFailure(
  exitCode: number | null,
  tapResults: { total: number; failed: number; bailOut: string | null },
): string {
  const bailSuffix =
    tapResults.bailOut !== null
      ? ` (Bail out!${tapResults.bailOut ? ` ${tapResults.bailOut}` : ''})`
      : '';

  if (tapResults.failed > 0) {
    const tests = tapResults.total === 1 ? 'test' : 'tests';
    return `${tapResults.failed} of ${tapResults.total} ${tests} failed${bailSuffix}`;
  }

  if (tapResults.bailOut !== null) {
    return `Test run bailed out${tapResults.bailOut ? `: ${tapResults.bailOut}` : ''}`;
  }

  const exitDescription =
    exitCode === null ? 'was terminated by a signal' : `exited with ${exitCode}`;
  if (tapResults.total === 0) {
    return `No test results were reported; the test process ${exitDescription}.`;
  }
  return `No assertion failed, but the test process ${exitDescription}.`;
}

/** Parse the top-level TAP summary from prove output. */
export function parseTapOutput(output: string): {
  total: number;
  passed: number;
  failed: number;
  skipped: number;
  bailOut: string | null;
} {
  const lines = output.split('\n');
  let total = 0;
  let passed = 0;
  let failed = 0;
  let skipped = 0;
  let bailOut: string | null = null;

  for (const line of lines) {
    if (/^ok \d+/.test(line)) {
      total++;
      // `ok N - desc # SKIP reason` → test was intentionally skipped.
      if (/\s#\s*SKIP\S*/i.test(line)) {
        skipped++;
      } else {
        passed++;
      }
    } else if (/^not ok \d+/.test(line)) {
      // `not ok N - desc # TODO reason` → a failing TODO test is expected
      // and must NOT count as a failure per TAP semantics.
      if (/\s#\s*TODO\S*/i.test(line)) {
        total++;
        skipped++;
      } else {
        total++;
        failed++;
      }
    } else if (/^Bail out!\s*(.*)/.test(line)) {
      bailOut = /^Bail out!\s*(.*)/.exec(line)?.[1] ?? '';
    } else if (/^1\.\.(\d+)/.test(line)) {
      const count = /^1\.\.(\d+)/.exec(line)?.[1];
      if (count !== undefined) {
        total = Math.max(total, parseInt(count, 10));
      }
    }
  }

  return { total, passed, failed, skipped, bailOut };
}

/** Parse subtest results from verbose prove TAP output. */
export function parseSubtestResults(
  output: string,
): Map<string, { ok: boolean; diagnostic: string; duration: number }> {
  const results = new Map<string, { ok: boolean; diagnostic: string; duration: number }>();
  const lines = output.split('\n');

  let currentSubtest: string | null = null;
  let diagnosticLines: string[] = [];

  for (const line of lines) {
    const subtestName = /^\s*#\s*Subtest:\s*(.+)/.exec(line)?.[1];
    if (subtestName !== undefined) {
      currentSubtest = subtestName.trim();
      diagnosticLines = [];
      continue;
    }

    if (currentSubtest && /^\s{4,}#/.test(line)) {
      diagnosticLines.push(line.trim());
      continue;
    }

    if (currentSubtest) {
      const okName = /^ok \d+\s*-\s*(.*)/.exec(line)?.[1];
      const notOkName = /^not ok \d+\s*-\s*(.*)/.exec(line)?.[1];

      if (okName?.trim() === currentSubtest) {
        results.set(currentSubtest, {
          ok: true,
          diagnostic: diagnosticLines.join('\n'),
          duration: 0,
        });
        currentSubtest = null;
        diagnosticLines = [];
      } else if (notOkName?.trim() === currentSubtest) {
        results.set(currentSubtest, {
          ok: false,
          diagnostic: diagnosticLines.join('\n') || `Subtest "${currentSubtest}" failed`,
          duration: 0,
        });
        currentSubtest = null;
        diagnosticLines = [];
      }
    }
  }

  return results;
}
