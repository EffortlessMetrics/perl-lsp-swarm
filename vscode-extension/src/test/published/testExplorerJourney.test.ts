import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

async function waitForFile(filePath: string, timeoutMs: number): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (fs.existsSync(filePath)) {
      return fs.readFileSync(filePath, 'utf8');
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out waiting for Test Explorer marker ${filePath}`);
}

suite('Installed Test Explorer prove journey', function () {
  this.timeout(120_000);

  test('runs a special-character test through the installed Test Explorer profile', async function () {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspaceFolder, 'installed Test Explorer journey requires a workspace folder');

    const fixtureDirectory = path.join(workspaceFolder.uri.fsPath, 'test explorer & [1] {a,b}');
    const fixture = path.join(fixtureDirectory, 'selected test.t');
    const marker = path.join(fixtureDirectory, 'selected-test-marker.json');
    fs.mkdirSync(fixtureDirectory, { recursive: true });
    fs.writeFileSync(
      fixture,
      [
        'use strict;',
        'use warnings;',
        'select undef, undef, undef, 0.2;',
        'open my $marker, ">", $ENV{PERL_LSP_TEST_EXPLORER_MARKER} or die $!;',
        'print {$marker} "completed\\n$0\\n";',
        'close $marker or die $!;',
        'print "1..1\\n";',
        'print "ok 1 - installed special path\\n";',
        '',
      ].join('\n'),
      'utf8',
    );

    const previousMarker = process.env.PERL_LSP_TEST_EXPLORER_MARKER;
    process.env.PERL_LSP_TEST_EXPLORER_MARKER = marker;
    try {
      const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
      assert.ok(extension, 'installed Test Explorer journey requires the extension');
      await extension.activate();
      const document = await vscode.workspace.openTextDocument(fixture);
      await vscode.window.showTextDocument(document);
      await vscode.commands.executeCommand('testing.refreshTests');
      await new Promise((resolve) => setTimeout(resolve, 3_000));
      await vscode.commands.executeCommand('testing.runAll');
      const raw = await waitForFile(marker, 90_000);
      const [phase, test0] = raw.trim().split(/\r?\n/);
      assert.equal(phase, 'completed');
      assert.equal(path.normalize(test0 ?? ''), path.normalize(fixture));
    } finally {
      if (previousMarker === undefined) delete process.env.PERL_LSP_TEST_EXPLORER_MARKER;
      else process.env.PERL_LSP_TEST_EXPLORER_MARKER = previousMarker;
      fs.rmSync(fixtureDirectory, { recursive: true, force: true });
    }
  });
});
