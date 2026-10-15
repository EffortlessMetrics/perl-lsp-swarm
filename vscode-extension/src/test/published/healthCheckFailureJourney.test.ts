import * as assert from 'assert';
import { spawnSync } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  bundledBinaryPath,
  receiptsDir,
  scanProcessesUnderDirectory,
  sha256,
  withTimeout,
} from './journeySupport';

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
  if (result.error) {
    throw result.error;
  }
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

  test('rejects a configured directory in the installed health command', async function () {
    if (process.env.PERL_LSP_HEALTH_CHECK_DIRECTORY_SMOKE !== '1') {
      this.skip();
    }
    const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
    assert.ok(extension, 'installed extension must be available');
    assert.equal(extension.isActive, false, 'directory leg requires a fresh extension host');
    const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-directory-health-'));
    const directory = path.join(fixture, 'not-a-binary');
    const regularFile = path.join(fixture, 'regular-file');
    fs.mkdirSync(directory);
    fs.writeFileSync(regularFile, 'regular file control\n', { mode: 0o600 });
    const receipt: Record<string, unknown> = {
      schema_version: 1,
      journey: 'directory-health',
      outcome: 'failed',
      platform: process.platform,
      arch: process.arch,
      vscode_version: vscode.version,
      extension_path: extension.extensionPath,
      extension_version: extension.packageJSON.version,
      declared_source_sha: process.env.PERL_LSP_CURRENT_SOURCE_SHA ?? null,
      declared_source_tree: process.env.PERL_LSP_CURRENT_SOURCE_TREE ?? null,
      vsix: null,
      fixture: { directory, regular_file: regularFile },
      // The startup OS probe can take priority over onboarding diagnostics.
      // This journey observes the registered command, not the startup dialog.
      startup_diagnosis: 'not exposed by the activation API',
    };
    try {
      receipt.installed_bundle_sha256 = sha256(
        path.join(extension.extensionPath, 'out/extension.js'),
      );
      const vsixPath = process.env.PERL_LSP_PUBLISHED_VSIX_PATH;
      if (vsixPath) {
        const digest = sha256(vsixPath);
        receipt.vsix = { path: vsixPath, sha256: digest };
        assert.equal(
          digest,
          process.env.PERL_LSP_VSIX_SHA256,
          'installed archive identity changed',
        );
      }
      assert.ok(fs.statSync(directory).isDirectory());
      assert.ok(fs.statSync(regularFile).isFile());
      const config = vscode.workspace.getConfiguration('perl-lsp');
      await config.update('autoDownload', false, vscode.ConfigurationTarget.Global);
      await config.update('serverPath', directory, vscode.ConfigurationTarget.Global);
      const activation = (await withTimeout(
        'extension activation',
        extension.activate(),
        90_000,
      )) as { getLanguageClientStartupMetrics?: () => Record<string, unknown> } | undefined;
      await waitForCommand('perl-lsp.runHealthCheck');

      // Run the opposite-direction control first, so an exists-only mutant
      // cannot fail before demonstrating that a regular file remains eligible.
      const control = (await withTimeout(
        'regular-file setup control',
        vscode.commands.executeCommand('perl-lsp.runHealthCheck', regularFile),
        90_000,
      )) as HealthCheckResult;
      receipt.regular_file = control;
      assert.equal(check(control, 'LSP binary').status, 'ok', JSON.stringify(control));
      assert.ok(check(control, 'LSP binary').detail.includes(regularFile));

      // No argument: exercise the installed configured/lifecycle projection.
      const implicit = await waitForFailedHealth();
      receipt.implicit = implicit;
      const explicit = (await withTimeout(
        'explicit directory setup check',
        vscode.commands.executeCommand('perl-lsp.runHealthCheck', directory),
        90_000,
      )) as HealthCheckResult;
      receipt.explicit = explicit;
      receipt.startup_metrics = activation?.getLanguageClientStartupMetrics?.() ?? null;
      for (const [subject, result] of [
        ['implicit', implicit],
        ['explicit', explicit],
      ] as const) {
        assert.equal(result.ok, false, `${subject}: ${JSON.stringify(result)}`);
        const binary = check(result, 'LSP binary');
        assert.equal(
          binary.status,
          'error',
          `${subject} directory binary row: ${JSON.stringify(result)}`,
        );
        assert.equal(binary.detail, `perl-lsp binary path is not a regular file: ${directory}`);
        assert.doesNotMatch(binary.detail, /not found/i);
      }
      receipt.outcome = 'passed';
    } catch (error: unknown) {
      receipt.error = error instanceof Error ? error.message : String(error);
      throw error;
    } finally {
      try {
        fs.writeFileSync(
          path.join(receiptsDir(), 'directory_health_receipt.json'),
          `${JSON.stringify(receipt, null, 2)}\n`,
        );
      } finally {
        fs.rmSync(fixture, { recursive: true, force: true });
      }
    }
  });

  test('reports blocked replacement after a failed startup and preserves the failure', async function () {
    if (
      process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE === '1' ||
      process.env.PERL_LSP_HEALTH_CHECK_DIRECTORY_SMOKE === '1'
    ) {
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
    if (
      process.env.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE !== '1' ||
      process.env.PERL_LSP_HEALTH_CHECK_DIRECTORY_SMOKE === '1'
    ) {
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
