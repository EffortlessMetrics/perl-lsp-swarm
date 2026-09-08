import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

async function waitForRunningStartup(
  getMetrics: () => { lifecycle_state?: unknown },
  deadline: number,
): Promise<void> {
  let state: unknown = undefined;
  while (Date.now() < deadline) {
    state = getMetrics().lifecycle_state;
    if (state === 'running') return;
    if (state === 'failed' || state === 'stopped') {
      throw new Error(`Language client startup entered terminal state ${String(state)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    `Timed out waiting for language client startup to reach running (state ${String(state)})`,
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
        getLanguageClientStartupMetrics?: () => { lifecycle_state?: unknown };
      };
      const document = await vscode.workspace.openTextDocument(fixture);
      await vscode.window.showTextDocument(document);
      assert.ok(
        extensionApi.waitForActiveDocumentReady,
        'installed Test Explorer journey requires the readiness awaitable',
      );
      await extensionApi.waitForActiveDocumentReady(
        document.uri.toString(),
        remainingBudget(deadline, 'document readiness'),
      );
      assert.ok(
        extensionApi.getLanguageClientStartupMetrics,
        'installed Test Explorer journey requires startup lifecycle metrics',
      );
      await waitForRunningStartup(extensionApi.getLanguageClientStartupMetrics, deadline);
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
    } finally {
      if (previousMarker === undefined) delete process.env.PERL_LSP_TEST_EXPLORER_MARKER;
      else process.env.PERL_LSP_TEST_EXPLORER_MARKER = previousMarker;
      if (previousStartMarker === undefined) delete process.env.PERL_LSP_TEST_EXPLORER_START_MARKER;
      else process.env.PERL_LSP_TEST_EXPLORER_START_MARKER = previousStartMarker;
      fs.rmSync(fixtureDirectory, { recursive: true, force: true });
    }
  });
});
