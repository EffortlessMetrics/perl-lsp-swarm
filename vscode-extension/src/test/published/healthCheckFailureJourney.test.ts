import * as assert from 'assert';
import { spawnSync } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { bundledBinaryPath, scanProcessesUnderDirectory, withTimeout } from './journeySupport';

interface HealthCheckResult {
  ok: boolean;
  checks: Array<{ label: string; status: string; detail: string }>;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForCommand(command: string): Promise<void> {
  await withTimeout(
    `${command} registration`,
    (async () => {
      for (;;) {
        if ((await vscode.commands.getCommands(true)).includes(command)) {
          return;
        }
        await delay(100);
      }
    })(),
    20_000,
  );
}

function nativeGitPath(): string {
  const lookup = process.platform === 'win32' ? 'where.exe' : 'which';
  const result = spawnSync(lookup, ['git'], { encoding: 'utf8', windowsHide: true });
  assert.equal(
    result.status,
    0,
    `native git lookup failed: ${result.stderr || result.error || ''}`,
  );
  const candidate = (result.stdout || '')
    .split(/\r?\n/)
    .map((entry) => entry.trim())
    .find((entry) => entry.length > 0 && fs.existsSync(entry) && fs.statSync(entry).isFile());
  assert.ok(candidate, `native git lookup returned no regular file: ${result.stdout || ''}`);
  return path.resolve(candidate);
}

function check(result: HealthCheckResult, label: string): { status: string; detail: string } {
  const found = result.checks.find((entry) => entry.label === label);
  assert.ok(found, `health result is missing ${label}: ${JSON.stringify(result)}`);
  return found;
}

async function waitForLifecycleRunning(
  activation: { getLanguageClientStartupMetrics?: () => Record<string, unknown> } | undefined,
): Promise<void> {
  await withTimeout(
    'language server lifecycle recovery',
    (async () => {
      for (;;) {
        if (activation?.getLanguageClientStartupMetrics?.().lifecycle_state === 'running') {
          return;
        }
        await delay(100);
      }
    })(),
    45_000,
  );
}

async function waitForFailedHealth(): Promise<HealthCheckResult> {
  return withTimeout(
    'rejecting language server health result',
    vscode.commands.executeCommand('perl-lsp.runHealthCheck') as Promise<HealthCheckResult>,
    90_000,
  );
}

suite('Installed Health Check failure and recovery', function () {
  this.timeout(240_000);

  test('reports startup failure and recovers through the real restart command', async function () {
    const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
    assert.ok(extension, 'installed extension must be available');
    const config = vscode.workspace.getConfiguration('perl-lsp');
    const gitPath = nativeGitPath();

    await config.update('autoDownload', false, vscode.ConfigurationTarget.Global);
    await config.update('serverPath', gitPath, vscode.ConfigurationTarget.Global);
    const activation = (await withTimeout('extension activation', extension.activate(), 90_000)) as
      | { getLanguageClientStartupMetrics?: () => Record<string, unknown> }
      | undefined;
    await waitForCommand('perl-lsp.runHealthCheck');

    const settledFailure = await waitForFailedHealth();
    assert.equal(settledFailure.ok, false, JSON.stringify(settledFailure, null, 2));
    assert.equal(
      check(settledFailure, 'LSP binary').status,
      'ok',
      JSON.stringify(settledFailure, null, 2),
    );
    assert.equal(
      check(settledFailure, 'LSP runtime').status,
      'error',
      JSON.stringify(settledFailure, null, 2),
    );

    const bundledPath = bundledBinaryPath(extension.extensionPath);
    await config.update('serverPath', bundledPath, vscode.ConfigurationTarget.Global);
    await withTimeout(
      'language server restart',
      vscode.commands.executeCommand('perl-lsp.restart'),
      90_000,
    );
    await waitForLifecycleRunning(activation);
    const recovered = (await withTimeout(
      'implicit health check after recovery',
      vscode.commands.executeCommand('perl-lsp.runHealthCheck'),
      45_000,
    )) as HealthCheckResult;
    assert.equal(recovered.ok, true, JSON.stringify(recovered, null, 2));
    assert.equal(check(recovered, 'LSP binary').status, 'ok', JSON.stringify(recovered, null, 2));
    assert.equal(check(recovered, 'LSP runtime').status, 'ok', JSON.stringify(recovered, null, 2));

    const processPaths = await scanProcessesUnderDirectory(path.dirname(bundledPath));
    assert.equal(
      processPaths.length,
      1,
      `recovery should leave exactly one bundled server process: ${JSON.stringify(processPaths)}`,
    );
    assert.equal(
      activation?.getLanguageClientStartupMetrics?.().lifecycle_state,
      'running',
      JSON.stringify(activation?.getLanguageClientStartupMetrics?.()),
    );
  });
});
