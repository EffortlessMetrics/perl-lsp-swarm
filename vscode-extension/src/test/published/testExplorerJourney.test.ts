import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { bundledBinaryPath, pathsEquivalent, sha256 } from './journeySupport';

/**
 * Structural view of the extension's startup metrics snapshot. Declared here
 * (instead of importing the production type) to match the published-journey
 * convention of typing the activation API inline.
 */
interface StartupMetrics {
  lifecycle_state?: unknown;
  binary_resolution_source?: unknown;
  binary_resolution_status?: unknown;
  binary_resolution_path?: unknown;
  binary_resolution_ms?: unknown;
  server_start_status?: unknown;
  initialize_status?: unknown;
  server_version?: unknown;
  startup_error?: unknown;
  milestones?: unknown;
}

/**
 * Render the extension's startup metrics into a bounded, single-line diagnosis
 * appended to startup failures. A terminal failed state alone ("state failed")
 * cannot distinguish binary resolution, server spawn, handshake, or teardown
 * races (#15592); the phase statuses and recorded startup error can.
 */
function describeStartupDiagnostics(metrics: StartupMetrics | undefined): string {
  if (!metrics) {
    return 'startup diagnostics unavailable: getLanguageClientStartupMetrics returned no snapshot';
  }
  const phases = [
    `binary_resolution=${String(metrics.binary_resolution_status)}/${String(metrics.binary_resolution_source)}`,
    `server_start=${String(metrics.server_start_status)}`,
    `initialize=${String(metrics.initialize_status)}`,
  ];
  const extras = [
    metrics.binary_resolution_ms === undefined || metrics.binary_resolution_ms === null
      ? undefined
      : `resolution_ms=${String(metrics.binary_resolution_ms)}`,
    metrics.binary_resolution_path === undefined || metrics.binary_resolution_path === null
      ? undefined
      : `resolved_path=${String(metrics.binary_resolution_path)}`,
    metrics.server_version === undefined || metrics.server_version === null
      ? undefined
      : `server_version=${String(metrics.server_version)}`,
    metrics.startup_error === undefined || metrics.startup_error === null
      ? undefined
      : `startup_error=${JSON.stringify(String(metrics.startup_error))}`,
  ].filter((entry): entry is string => entry !== undefined);
  const milestones = Object.entries((metrics.milestones ?? {}) as Record<string, number>)
    .map(([name, at]) => `${name}@${at}ms`)
    .join(',');
  return [
    `state=${String(metrics.lifecycle_state)}`,
    ...phases,
    ...extras,
    `milestones=[${milestones}]`,
  ].join(' ');
}

async function waitForRunningStartup(
  getMetrics: () => StartupMetrics,
  deadline: number,
): Promise<void> {
  let state: unknown = undefined;
  let lastMetrics: StartupMetrics | undefined;
  while (Date.now() < deadline) {
    lastMetrics = getMetrics();
    state = lastMetrics.lifecycle_state;
    if (state === 'running') return;
    if (state === 'failed' || state === 'stopped') {
      throw new Error(
        `Language client startup entered terminal state ${String(state)}; ${describeStartupDiagnostics(lastMetrics)}`,
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    `Timed out waiting for language client startup to reach running (state ${String(state)}); ${describeStartupDiagnostics(lastMetrics)}`,
  );
}

function remainingBudget(deadline: number, label: string): number {
  const remaining = deadline - Date.now();
  if (remaining <= 0)
    throw new Error(
      `Installed Test Explorer journey exceeded its 90-second budget before ${label}`,
    );
  return remaining;
}

suite('Installed Test Explorer runAll journey', function () {
  this.timeout(120_000);

  test('executes a generated special-character fixture through testing.runAll', async function () {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspaceFolder, 'installed Test Explorer journey requires a workspace folder');

    const token = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const fixtureDirectory = path.join(
      workspaceFolder.uri.fsPath,
      `test explorer & [1] {a,b} ${token}`,
    );
    const fixture = path.join(fixtureDirectory, 'generated test.t');
    const startMarker = path.join(fixtureDirectory, 'selected-test-start.json');
    const marker = path.join(fixtureDirectory, 'selected-test-marker.json');
    const previousMarker = process.env.PERL_LSP_TEST_EXPLORER_MARKER;
    const previousStartMarker = process.env.PERL_LSP_TEST_EXPLORER_START_MARKER;
    const deadline = Date.now() + 90_000;
    try {
      fs.mkdirSync(fixtureDirectory, { recursive: true });
      fs.writeFileSync(
        fixture,
        [
          'use strict;',
          'use warnings;',
          `open my $start, ">>", $ENV{PERL_LSP_TEST_EXPLORER_START_MARKER} or die $!; print {$start} "START:${token}\\n"; close $start or die $!;`,
          'print "1..1\\n";',
          'print "ok 1 - installed special path\\n";',
          `my $tmp = $ENV{PERL_LSP_TEST_EXPLORER_MARKER} . '.tmp'; open my $end, ">", $tmp or die $!; print {$end} "END:${token}\\n$0\\n"; close $end or die $!; rename $tmp, $ENV{PERL_LSP_TEST_EXPLORER_MARKER} or die $!;`,
          '',
        ].join('\n'),
        'utf8',
      );
      process.env.PERL_LSP_TEST_EXPLORER_MARKER = marker;
      process.env.PERL_LSP_TEST_EXPLORER_START_MARKER = startMarker;
      const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
      assert.ok(extension, 'installed Test Explorer journey requires the extension');
      const extensionApi = (await extension.activate()) as {
        waitForActiveDocumentReady?: (uri: string, timeoutMs?: number) => Promise<void>;
        getLanguageClientStartupMetrics?: () => {
          lifecycle_state?: unknown;
          binary_resolution_source?: unknown;
          binary_resolution_status?: unknown;
          binary_resolution_path?: unknown;
        };
      };
      const document = await vscode.workspace.openTextDocument(fixture);
      await vscode.window.showTextDocument(document);
      assert.ok(
        extensionApi.waitForActiveDocumentReady,
        'installed Test Explorer journey requires the readiness awaitable',
      );
      assert.ok(
        extensionApi.getLanguageClientStartupMetrics,
        'installed Test Explorer journey requires startup lifecycle metrics',
      );
      // Readiness waiters belong to a specific client generation and are
      // rejected when startup demand replaces that generation. Establish the
      // running generation before waiting for document/index readiness so the
      // test does not turn a normal first-demand restart into a false failure.
      await waitForRunningStartup(extensionApi.getLanguageClientStartupMetrics, deadline);
      const receiptPath = process.env.PERL_LSP_TEST_EXPLORER_RECEIPT;
      let startup:
        | ReturnType<NonNullable<typeof extensionApi.getLanguageClientStartupMetrics>>
        | undefined;
      let bundledServerSha256: string | undefined;
      if (receiptPath) {
        startup = extensionApi.getLanguageClientStartupMetrics();
        const bundledServerPath = bundledBinaryPath(extension.extensionPath);
        bundledServerSha256 = sha256(bundledServerPath);
        assert.equal(startup.binary_resolution_source, 'bundled');
        assert.equal(startup.binary_resolution_status, 'ok');
        assert.ok(pathsEquivalent(startup.binary_resolution_path, bundledServerPath));
        assert.equal(
          bundledServerSha256,
          process.env.PERL_LSP_SERVER_ARTIFACT_SHA256,
          'installed bundled server must match the staged server artifact',
        );
      }
      await extensionApi.waitForActiveDocumentReady(
        document.uri.toString(),
        remainingBudget(deadline, 'document readiness'),
      );
      await vscode.commands.executeCommand('testing.refreshTests');
      const runDeadline = Math.min(deadline, Date.now() + 10_000);
      let runAttempts = 0;
      let ranFixture = false;
      // Refresh awaits discovery, but VS Code publishes TestItemCollection diffs on a debounce;
      // each runAll is awaited before the next bounded attempt, and the marker guards repeats.
      while (Date.now() < runDeadline) {
        runAttempts += 1;
        await vscode.commands.executeCommand('testing.runAll');
        ranFixture = fs.existsSync(startMarker);
        if (ranFixture) break;
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      assert.ok(
        ranFixture,
        `testing.runAll did not execute the generated fixture after ${runAttempts} attempts`,
      );
      process.stdout.write(`[installed-test-explorer] runAll attempts: ${runAttempts}\n`);
      assert.ok(
        fs.existsSync(marker),
        'testing.runAll returned before the generated fixture final marker was written',
      );
      const starts = fs
        .readFileSync(startMarker, 'utf8')
        .trim()
        .split(/\r?\n/)
        .map((line) => line.trimEnd());
      assert.deepEqual(starts, [`START:${token}`]);
      const raw = fs.readFileSync(marker, 'utf8');
      const [phase, test0] = raw.trim().split(/\r?\n/);
      assert.equal(phase, `END:${token}`);
      assert.equal(path.normalize(test0 ?? ''), path.normalize(fixture));
      if (receiptPath) {
        const receiptTemp = `${receiptPath}.tmp-${process.pid}-${token}`;
        fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
        fs.writeFileSync(
          receiptTemp,
          JSON.stringify(
            {
              schema_version: 'test_explorer_journey.v1',
              outcome: 'completed',
              source_revision: process.env.PERL_LSP_CURRENT_SOURCE_SHA ?? null,
              server_source_revision: process.env.PERL_LSP_SERVER_SOURCE_SHA ?? null,
              server_artifact_sha256: process.env.PERL_LSP_SERVER_ARTIFACT_SHA256 ?? null,
              binary_resolution_source: startup?.binary_resolution_source ?? null,
              binary_resolution_status: startup?.binary_resolution_status ?? null,
              binary_resolution_path: startup?.binary_resolution_path ?? null,
              binary_resolution_sha256: bundledServerSha256 ?? null,
              vsix_sha256: process.env.PERL_LSP_VSIX_SHA256 ?? null,
              fixture,
              test_zero: test0,
            },
            null,
            2,
          ),
          'utf8',
        );
        fs.renameSync(receiptTemp, receiptPath);
      }
    } finally {
      if (previousMarker === undefined) delete process.env.PERL_LSP_TEST_EXPLORER_MARKER;
      else process.env.PERL_LSP_TEST_EXPLORER_MARKER = previousMarker;
      if (previousStartMarker === undefined) delete process.env.PERL_LSP_TEST_EXPLORER_START_MARKER;
      else process.env.PERL_LSP_TEST_EXPLORER_START_MARKER = previousStartMarker;
      fs.rmSync(fixtureDirectory, { recursive: true, force: true });
    }
  });
});
