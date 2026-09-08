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

async function waitForFailedHealth(): Promise<HealthCheckResult> {
  return withTimeout(
    'rejecting language server health result',
    vscode.commands.executeCommand('perl-lsp.runHealthCheck') as Promise<HealthCheckResult>,
    90_000,
  );
}

suite('Installed Health Check failure and recovery', function () {
  this.timeout(240_000);

  test('reports blocked replacement after a failed startup and preserves the failure', async function () {
    if (process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE === '1') {
      this.skip();
    }
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

    const metricsBeforeRestart = activation?.getLanguageClientStartupMetrics?.();
    assert.ok(metricsBeforeRestart, 'startup metrics must be exported by the installed extension');
    assert.equal(
      metricsBeforeRestart.server_start_status,
      'ok',
      JSON.stringify(metricsBeforeRestart),
    );
    assert.equal(
      metricsBeforeRestart.initialize_status,
      'error',
      JSON.stringify(metricsBeforeRestart),
    );
    assert.equal(
      typeof metricsBeforeRestart.server_start_ms,
      'number',
      JSON.stringify(metricsBeforeRestart),
    );

    const bundledPath = bundledBinaryPath(extension.extensionPath);
    await config.update('serverPath', bundledPath, vscode.ConfigurationTarget.Global);
    await withTimeout(
      'blocked restart decision',
      vscode.commands.executeCommand('perl-lsp.restart'),
      20_000,
    );
    const afterRestart = (await waitForFailedHealth()) as HealthCheckResult;
    const runtime = check(afterRestart, 'LSP runtime');
    assert.equal(afterRestart.ok, false, JSON.stringify(afterRestart, null, 2));
    assert.equal(runtime.status, 'error', JSON.stringify(afterRestart, null, 2));
    assert.match(runtime.detail, /cleanup|reload|replacement/i);
    const metricsAfterRestart = activation?.getLanguageClientStartupMetrics?.();
    assert.ok(metricsAfterRestart, 'startup metrics must remain available after blocked restart');
    assert.notEqual(
      metricsAfterRestart.lifecycle_state,
      'running',
      JSON.stringify(metricsAfterRestart),
    );
    assert.equal(
      metricsAfterRestart.server_start_status,
      'ok',
      JSON.stringify(metricsAfterRestart),
    );
    assert.equal(
      metricsAfterRestart.initialize_status,
      'error',
      JSON.stringify(metricsAfterRestart),
    );
    const bundledProcesses = await scanProcessesUnderDirectory(path.dirname(bundledPath));
    assert.equal(
      bundledProcesses.length,
      0,
      `blocked restart must not launch the bundled server: ${JSON.stringify(bundledProcesses)}`,
    );
  });

  test('starts the bundled server in a fresh host and reports healthy runtime', async function () {
    if (process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE !== '1') {
      this.skip();
    }

    const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
    assert.ok(extension, 'installed extension must be available');
    const config = vscode.workspace.getConfiguration('perl-lsp');
    const bundledPath = bundledBinaryPath(extension.extensionPath);
    await config.update('autoDownload', false, vscode.ConfigurationTarget.Global);
    await config.update('serverPath', bundledPath, vscode.ConfigurationTarget.Global);
    const activation = (await withTimeout('extension activation', extension.activate(), 90_000)) as
      | { getLanguageClientStartupMetrics?: () => Record<string, unknown> }
      | undefined;
    await waitForCommand('perl-lsp.runHealthCheck');

    const healthy = (await withTimeout(
      'bundled implicit health check',
      vscode.commands.executeCommand('perl-lsp.runHealthCheck'),
      90_000,
    )) as HealthCheckResult;
    assert.equal(healthy.ok, true, JSON.stringify(healthy, null, 2));
    assert.equal(check(healthy, 'LSP binary').status, 'ok', JSON.stringify(healthy, null, 2));
    assert.equal(check(healthy, 'LSP runtime').status, 'ok', JSON.stringify(healthy, null, 2));
    assert.equal(
      activation?.getLanguageClientStartupMetrics?.().lifecycle_state,
      'running',
      JSON.stringify(activation?.getLanguageClientStartupMetrics?.()),
    );

    const processPaths = await scanProcessesUnderDirectory(path.dirname(bundledPath));
    assert.equal(
      processPaths.length,
      1,
      `fresh bundled host should leave exactly one server process: ${JSON.stringify(processPaths)}`,
    );
  });
});
