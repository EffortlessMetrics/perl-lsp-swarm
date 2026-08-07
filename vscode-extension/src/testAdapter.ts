import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { spawn, type ChildProcess } from 'child_process';

/**
 * Perl Test Explorer integration.
 *
 * Discovers `.t` test files in the workspace, parses `subtest` blocks,
 * and runs them via `prove -v`, mapping TAP output to VS Code test results.
 */

const DEFAULT_PROVE_TIMEOUT_MS = 5 * 60 * 1000;
const MIN_PROVE_TIMEOUT_MS = 1000;
const MAX_PROVE_TIMEOUT_MS = 60 * 60 * 1000;
const PROVE_TERMINATION_GRACE_MS = 2000;
const MAX_STDERR_CHARACTERS = 64 * 1024;
const MAX_TAP_DIAGNOSTIC_CHARACTERS = 32 * 1024;
const MAX_TAP_LINE_CHARACTERS = 64 * 1024;

/** Resolve and clamp the per-file prove timeout. */
export function normalizeProveTimeoutMs(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return DEFAULT_PROVE_TIMEOUT_MS;
  }

  const milliseconds = Math.round(value * 1000);
  return Math.min(MAX_PROVE_TIMEOUT_MS, Math.max(MIN_PROVE_TIMEOUT_MS, milliseconds));
}

/** Keep only a bounded tail of noisy subprocess text. */
export class BoundedTextBuffer {
  private value = '';
  private truncated = false;

  constructor(private readonly maxCharacters: number) {
    if (!Number.isSafeInteger(maxCharacters) || maxCharacters <= 0) {
      throw new Error('BoundedTextBuffer requires a positive integer limit.');
    }
  }

  append(chunk: string | Buffer): void {
    const text = typeof chunk === 'string' ? chunk : chunk.toString('utf8');
    if (text.length >= this.maxCharacters) {
      this.value = text.slice(-this.maxCharacters);
      this.truncated = true;
      return;
    }

    const combined = this.value + text;
    if (combined.length > this.maxCharacters) {
      this.value = combined.slice(-this.maxCharacters);
      this.truncated = true;
    } else {
      this.value = combined;
    }
  }

  clear(): void {
    this.value = '';
    this.truncated = false;
  }

  toString(): string {
    return this.truncated ? `[earlier output truncated]\n${this.value}` : this.value;
  }
}

export interface TapSummary {
  total: number;
  passed: number;
  failed: number;
  skipped: number;
  bailOut: string | null;
}

export interface SubtestResult {
  ok: boolean;
  diagnostic: string;
  duration: number;
}

/**
 * Incrementally parse the TAP fields used by Test Explorer.
 *
 * stdout is never accumulated as one unbounded string. Partial lines and
 * subtest diagnostics have explicit ceilings; top-level counters and subtest
 * verdicts are retained as compact structured state.
 */
export class TapStreamParser {
  private pendingLine = '';
  private total = 0;
  private passed = 0;
  private failed = 0;
  private skipped = 0;
  private bailOut: string | null = null;
  private currentSubtest: string | null = null;
  private readonly currentDiagnostic = new BoundedTextBuffer(
    MAX_TAP_DIAGNOSTIC_CHARACTERS,
  );
  private readonly subtests = new Map<string, SubtestResult>();

  push(chunk: string | Buffer): void {
    const text = typeof chunk === 'string' ? chunk : chunk.toString('utf8');
    let cursor = 0;

    while (cursor < text.length) {
      const newline = text.indexOf('\n', cursor);
      if (newline === -1) {
        this.appendPartial(text.slice(cursor));
        return;
      }

      this.appendPartial(text.slice(cursor, newline));
      this.flushPendingLine();
      cursor = newline + 1;
    }
  }

  finish(): void {
    if (this.pendingLine.length > 0) {
      this.flushPendingLine();
    }
  }

  getSummary(): TapSummary {
    return {
      total: this.total,
      passed: this.passed,
      failed: this.failed,
      skipped: this.skipped,
      bailOut: this.bailOut,
    };
  }

  getSubtestResults(): Map<string, SubtestResult> {
    return new Map(this.subtests);
  }

  private appendPartial(text: string): void {
    if (text.length === 0) {
      return;
    }

    const remaining = MAX_TAP_LINE_CHARACTERS - this.pendingLine.length;
    if (remaining <= 0) {
      return;
    }
    this.pendingLine += text.slice(0, remaining);
  }

  private flushPendingLine(): void {
    const line = this.pendingLine.endsWith('\r')
      ? this.pendingLine.slice(0, -1)
      : this.pendingLine;
    this.pendingLine = '';
    this.processLine(line);
  }

  private processLine(line: string): void {
    if (/^ok \d+/.test(line)) {
      this.total += 1;
      if (/\s#\s*SKIP\S*/i.test(line)) {
        this.skipped += 1;
      } else {
        this.passed += 1;
      }
    } else if (/^not ok \d+/.test(line)) {
      this.total += 1;
      if (/\s#\s*TODO\S*/i.test(line)) {
        this.skipped += 1;
      } else {
        this.failed += 1;
      }
    } else {
      const bailOut = /^Bail out!\s*(.*)/.exec(line)?.[1];
      if (bailOut !== undefined) {
        this.bailOut = bailOut;
      }

      const plan = /^1\.\.(\d+)/.exec(line)?.[1];
      if (plan !== undefined) {
        this.total = Math.max(this.total, Number.parseInt(plan, 10));
      }
    }

    const subtestName = /^\s*#\s*Subtest:\s*(.+)/.exec(line)?.[1];
    if (subtestName !== undefined) {
      this.currentSubtest = subtestName.trim();
      this.currentDiagnostic.clear();
      return;
    }

    if (this.currentSubtest && /^\s{4,}#/.test(line)) {
      if (this.currentDiagnostic.toString().length > 0) {
        this.currentDiagnostic.append('\n');
      }
      this.currentDiagnostic.append(line.trim());
      return;
    }

    if (!this.currentSubtest) {
      return;
    }

    const okName = /^ok \d+\s*-\s*(.*)/.exec(line)?.[1];
    const notOkName = /^not ok \d+\s*-\s*(.*)/.exec(line)?.[1];
    if (okName?.trim() === this.currentSubtest) {
      this.subtests.set(this.currentSubtest, {
        ok: true,
        diagnostic: this.currentDiagnostic.toString(),
        duration: 0,
      });
      this.resetCurrentSubtest();
    } else if (notOkName?.trim() === this.currentSubtest) {
      const name = this.currentSubtest;
      this.subtests.set(name, {
        ok: false,
        diagnostic: this.currentDiagnostic.toString() || `Subtest "${name}" failed`,
        duration: 0,
      });
      this.resetCurrentSubtest();
    }
  }

  private resetCurrentSubtest(): void {
    this.currentSubtest = null;
    this.currentDiagnostic.clear();
  }
}

/**
 * Resolve the `prove` command for the current platform.
 *
 * On Windows, `prove` is a `.bat` script shim — spawning it without
 * `shell: true` fails with ENOENT. On all platforms, attempt to derive
 * `prove` from the directory of the `perl` binary on PATH so that
 * perlbrew/plenv users get the matching `prove`.
 */
export function resolveProveCommand(extraArgs: string[]): {
  command: string;
  args: string[];
  shell: boolean;
} {
  const isWindows = process.platform === 'win32';

  let provePath: string | null = null;
  try {
    const { execSync } = require('child_process');
    const perlPath = execSync('perl -e "print $^X"', {
      encoding: 'utf8',
      timeout: 3000,
    }).trim();
    const perlDir = path.dirname(perlPath);
    const candidate = path.join(perlDir, isWindows ? 'prove.bat' : 'prove');
    if (fs.existsSync(candidate)) {
      provePath = candidate;
    }
  } catch {
    // perl not on PATH or execSync failed — fall back to bare 'prove'.
  }

  if (provePath) {
    return { command: provePath, args: extraArgs, shell: false };
  }

  return { command: 'prove', args: extraArgs, shell: isWindows };
}

export interface SubtestInfo {
  name: string;
  line: number;
}

const SUBTEST_RE = /^\s*subtest\s+(['"])(.*?)\1\s*(?:=>|,)\s*sub\s*\{/;

type ProveTerminationReason = 'cancelled' | 'timed-out';

function terminateProcess(processToStop: ChildProcess, force: boolean): void {
  if (processToStop.exitCode !== null || processToStop.signalCode !== null) {
    return;
  }

  try {
    processToStop.kill(force ? 'SIGKILL' : 'SIGTERM');
  } catch {
    // The process may have exited between the state check and kill request.
  }
}

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

    const watcher = vscode.workspace.createFileSystemWatcher('**/*.t');
    watcher.onDidCreate((uri) => this.discoverFileTests(uri));
    watcher.onDidChange((uri) => this.discoverFileTests(uri));
    watcher.onDidDelete((uri) => this.removeFile(uri));
    this.disposables.push(watcher);

    const saveListener = vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.uri.fsPath.endsWith('.t')) {
        void this.discoverFileTests(doc.uri);
      }
    });
    this.disposables.push(saveListener);

    void this.discoverAllTests();
  }

  private async discoverAllTests(): Promise<void> {
    this.testController.items.replace([]);
    this.fileItems.clear();

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

    const subtests = await this.parseSubtests(uri);
    for (const subtest of subtests) {
      const child = this.testController.createTestItem(
        `${fileId}::${subtest.name}`,
        subtest.name,
        uri,
      );
      child.range = new vscode.Range(subtest.line, 0, subtest.line, 0);
      fileItem.children.add(child);
    }
  }

  private async parseSubtests(uri: vscode.Uri): Promise<SubtestInfo[]> {
    try {
      const doc = await vscode.workspace.openTextDocument(uri);
      const subtests: SubtestInfo[] = [];

      for (let index = 0; index < doc.lineCount; index += 1) {
        const match = SUBTEST_RE.exec(doc.lineAt(index).text);
        const name = match?.[2];
        if (name !== undefined) {
          subtests.push({ name, line: index });
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

  private async runHandler(
    request: vscode.TestRunRequest,
    token: vscode.CancellationToken,
  ): Promise<void> {
    const run = this.testController.createTestRun(request);
    const testsToRun = request.include ?? this.gatherAllItems();
    const byFile = new Map<string, { fileItem: vscode.TestItem; subtests: vscode.TestItem[] }>();

    for (const item of testsToRun) {
      if (token.isCancellationRequested) {
        break;
      }

      if (item.uri && item.children.size > 0) {
        const children: vscode.TestItem[] = [];
        item.children.forEach((child) => children.push(child));
        byFile.set(item.uri.fsPath, { fileItem: item, subtests: children });
      } else if (item.uri) {
        const fsPath = item.uri.fsPath;
        const entry = byFile.get(fsPath);
        if (entry) {
          entry.subtests.push(item);
        } else {
          const fileItem = this.fileItems.get(item.uri.toString());
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
      for (const subtest of subtests) {
        run.started(subtest);
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

  private async runProve(
    filePath: string,
    fileItem: vscode.TestItem,
    subtests: vscode.TestItem[],
    run: vscode.TestRun,
    token: vscode.CancellationToken,
  ): Promise<void> {
    const workspaceFolder = vscode.workspace.getWorkspaceFolder(fileItem.uri!);
    const cwd = workspaceFolder?.uri.fsPath ?? path.dirname(filePath);
    const configuredTimeoutSeconds = vscode.workspace
      .getConfiguration('perl-lsp', fileItem.uri)
      .get<number>('testTimeoutSeconds', DEFAULT_PROVE_TIMEOUT_MS / 1000);
    const timeoutMs = normalizeProveTimeoutMs(configuredTimeoutSeconds);

    return new Promise<void>((resolve) => {
      const startTime = Date.now();
      const { command, args, shell } = resolveProveCommand(['-v', '--nocolor', filePath]);
      const processHandle = spawn(command, args, {
        cwd,
        env: { ...process.env, HARNESS_ACTIVE: '1' },
        shell,
      });
      const tapParser = new TapStreamParser();
      const stderr = new BoundedTextBuffer(MAX_STDERR_CHARACTERS);
      let terminationReason: ProveTerminationReason | undefined;
      let settled = false;
      let forceKillTimer: NodeJS.Timeout | undefined;

      const clearProcessResources = (): void => {
        clearTimeout(timeoutTimer);
        if (forceKillTimer) {
          clearTimeout(forceKillTimer);
        }
        cancellation.dispose();
      };

      const requestTermination = (reason: ProveTerminationReason): void => {
        if (terminationReason || settled) {
          return;
        }
        terminationReason = reason;
        terminateProcess(processHandle, false);
        forceKillTimer = setTimeout(() => {
          terminateProcess(processHandle, true);
        }, PROVE_TERMINATION_GRACE_MS);
        forceKillTimer.unref();
      };

      const timeoutTimer = setTimeout(() => {
        requestTermination('timed-out');
      }, timeoutMs);
      timeoutTimer.unref();

      const cancellation = token.onCancellationRequested(() => {
        requestTermination('cancelled');
      });

      processHandle.stdout?.on('data', (data: Buffer) => {
        tapParser.push(data);
      });
      processHandle.stderr?.on('data', (data: Buffer) => {
        stderr.append(data);
      });

      processHandle.once('close', (code) => {
        if (settled) {
          return;
        }
        settled = true;
        clearProcessResources();
        tapParser.finish();
        const duration = Date.now() - startTime;

        if (terminationReason === 'cancelled') {
          run.skipped(fileItem);
          for (const subtest of subtests) {
            run.skipped(subtest);
          }
          resolve();
          return;
        }

        if (terminationReason === 'timed-out') {
          const timeoutMessage = new vscode.TestMessage(
            `prove timed out after ${Math.round(timeoutMs / 1000)} seconds and was terminated.`,
          );
          if (fileItem.uri) {
            timeoutMessage.location = new vscode.Location(fileItem.uri, new vscode.Position(0, 0));
          }
          run.failed(fileItem, timeoutMessage, duration);
          for (const subtest of subtests) {
            run.failed(subtest, timeoutMessage);
          }
          resolve();
          return;
        }

        const tapResults = tapParser.getSummary();
        const subtestResults = tapParser.getSubtestResults();
        for (const subtest of subtests) {
          const result = subtestResults.get(subtest.label);
          if (result === undefined) {
            run.skipped(subtest);
          } else if (result.ok) {
            run.passed(subtest, result.duration);
          } else {
            run.failed(
              subtest,
              new vscode.TestMessage(
                result.diagnostic || `Subtest "${subtest.label}" failed`,
              ),
              result.duration,
            );
          }
        }

        if (code === 0 && tapResults.failed === 0) {
          run.passed(fileItem, duration);
        } else {
          const stderrText = stderr.toString().trim();
          const fallback =
            `${tapResults.failed} of ${tapResults.total} tests failed` +
            (tapResults.bailOut ? ` (Bail out! ${tapResults.bailOut})` : '');
          const message = new vscode.TestMessage(stderrText || fallback);
          if (fileItem.uri) {
            message.location = new vscode.Location(fileItem.uri, new vscode.Position(0, 0));
          }
          run.failed(fileItem, message, duration);
        }

        resolve();
      });

      processHandle.once('error', (error: Error) => {
        if (settled) {
          return;
        }
        settled = true;
        clearProcessResources();
        run.errored(
          fileItem,
          new vscode.TestMessage(`Failed to run prove: ${error.message}. Is prove installed?`),
        );
        for (const subtest of subtests) {
          run.errored(subtest, new vscode.TestMessage('prove not available'));
        }
        resolve();
      });
    });
  }

  public async runFileTests(uri: vscode.Uri): Promise<void> {
    const fileItem = this.fileItems.get(uri.toString());

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
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
  }
}

/** Parse the top-level TAP summary from prove output. */
export function parseTapOutput(output: string): TapSummary {
  const parser = new TapStreamParser();
  parser.push(output);
  parser.finish();
  return parser.getSummary();
}

/** Parse subtest results from verbose prove TAP output. */
export function parseSubtestResults(output: string): Map<string, SubtestResult> {
  const parser = new TapStreamParser();
  parser.push(output);
  parser.finish();
  return parser.getSubtestResults();
}
