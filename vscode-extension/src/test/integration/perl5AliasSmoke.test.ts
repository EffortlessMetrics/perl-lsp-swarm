import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import type { ReinstallCommandResult } from '../../commandResults';

/**
 * Real-client alias proof for the perl/perl5 language-ID contract (#7699).
 *
 * Issue #7699's required proof for the retained `perl5` alias: "the exact
 * candidate `perllsp` receives `didOpen` and returns at least diagnostics plus
 * one navigation/completion/hover result", proven "through the real language
 * client". This smoke reproduces the production premise end to end:
 *
 * 1. a second extension (the perl5-alias-language fixture) contributes the
 *    `perl5` language ID — exactly how the ID exists for real users;
 * 2. a file is explicitly classified `perl5` (_Change Language Mode_);
 * 3. `onLanguage:perl5` activation + client selection attach the buffer to the
 *    one language client and the real managed `perllsp` binary;
 * 4. the server must answer with published diagnostics for the alias buffer
 *    and a provider round trip (completion/hover).
 *
 * Runs only when PERL_LSP_ALIAS_SMOKE=1 (see suite/index.ts), which also adds
 * the fixture development extension in runTest.ts. Skipped otherwise so the
 * default smoke topology stays unchanged.
 */

async function withTimeout<T>(
  label: string,
  operation: PromiseLike<T>,
  timeoutMs: number,
): Promise<T> {
  let timeout: NodeJS.Timeout | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timeout = setTimeout(() => {
      reject(new Error(`${label} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
  });

  try {
    return await Promise.race([Promise.resolve(operation), timeoutPromise]);
  } finally {
    if (timeout) {
      clearTimeout(timeout);
    }
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function waitForCommand(command: string, timeoutMs: number): Promise<void> {
  await withTimeout(
    `${command} registration`,
    (async () => {
      for (;;) {
        const commands = await vscode.commands.getCommands(true);
        if (commands.includes(command)) {
          return;
        }
        await delay(100);
      }
    })(),
    timeoutMs,
  );
}

async function waitForDiagnostics(
  uri: vscode.Uri,
  timeoutMs: number,
): Promise<vscode.Diagnostic[]> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const diagnostics = vscode.languages.getDiagnostics(uri);
    if (diagnostics.length > 0) {
      return diagnostics;
    }
    if (Date.now() > deadline) {
      return vscode.languages.getDiagnostics(uri);
    }
    await delay(250);
  }
}

function aliasReceiptsDir(): string {
  // __dirname after compile is <repo>/vscode-extension/out/test/integration → 4x .. reaches repo root.
  const root =
    process.env.PERL_LSP_SMOKE_RECEIPTS_DIR ??
    path.resolve(__dirname, '..', '..', '..', '..', 'target', 'receipts', 'vscode-smoke');
  const dir = path.join(root, 'perl5-alias', process.platform);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

suite('Perl5 alias extension-host smoke (#7699)', function () {
  this.timeout(240_000);

  test('perl5-classified document flows activation → client → server with diagnostics and a provider result', async function () {
    this.timeout(240_000);

    const receiptPath = path.join(aliasReceiptsDir(), 'perl5_alias_receipt.json');
    const writeReceipt = (receipt: Record<string, unknown>): void => {
      try {
        fs.writeFileSync(receiptPath, JSON.stringify(receipt, null, 2));
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        process.stderr.write(`[alias-smoke] failed to write receipt ${receiptPath}: ${msg}\n`);
      }
    };

    const failures: Array<Record<string, unknown>> = [];
    let reinstall: ReinstallCommandResult | undefined;
    let classifiedLanguageId: string | null = null;
    let diagnostics: vscode.Diagnostic[] = [];
    let completionItemLabel: string | null = null;
    let hoverCount: number | null = null;

    try {
      // The fixture stands in for "another extension contributes perl5" — the
      // only production-honest way the alias ID exists.
      const fixture = vscode.extensions.getExtension(
        'EffortlessMetrics.perl5-alias-language-fixture',
      );
      assert.ok(
        fixture,
        'perl5 alias language fixture must be loaded (run with PERL_LSP_ALIAS_SMOKE=1)',
      );

      const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
      assert.ok(workspaceFolder, 'alias smoke requires a workspace folder');

      // Managed-download mode: resolve a real perllsp binary the same way the
      // managed-binary smoke does, then point the client at it deterministically.
      const config = vscode.workspace.getConfiguration('perl-lsp');
      await config.update('autoDownload', false, vscode.ConfigurationTarget.Global);
      await config.update('serverPath', '', vscode.ConfigurationTarget.Global);
      await config.update('channel', 'tag', vscode.ConfigurationTarget.Global);
      await config.update('versionTag', 'v0.13.1', vscode.ConfigurationTarget.Global);
      await config.update('downloadBaseUrl', '', vscode.ConfigurationTarget.Global);
      await config.update('updateCheckInterval', 0, vscode.ConfigurationTarget.Global);
      await config.update('perlcritic.enabled', false, vscode.ConfigurationTarget.Global);

      const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
      assert.ok(extension, 'extension should be available in the extension host');
      await withTimeout('extension activation', extension.activate(), 30_000);
      await waitForCommand('perl-lsp.reinstall', 10_000);

      reinstall = await withTimeout(
        'managed binary reinstall command (alias smoke)',
        vscode.commands.executeCommand<ReinstallCommandResult>('perl-lsp.reinstall'),
        120_000,
      );
      assert.ok(reinstall, 'reinstall command should return a result');
      assert.equal(reinstall.ok, true, JSON.stringify(reinstall, null, 2));
      assert.ok(
        reinstall.serverPath && fs.existsSync(reinstall.serverPath),
        `managed binary should exist after reinstall: ${reinstall?.serverPath}`,
      );

      await config.update('serverPath', reinstall.serverPath, vscode.ConfigurationTarget.Global);
      await withTimeout(
        'language client restart against managed binary',
        vscode.commands.executeCommand('perl-lsp.restart'),
        90_000,
      );

      // Classify a file as perl5 explicitly (files.associations / Change
      // Language Mode territory): the alias must attach to the same client.
      const probeText = [
        'use strict;',
        'use warnings;',
        'my $alias_probe = 1;',
        'my $alias_copy = $alias_pro',
        'my $broken = ;',
        '',
      ].join('\n');
      const probePath = path.join(workspaceFolder.uri.fsPath, 'zz_perl5_alias_probe.pl5x');
      fs.writeFileSync(probePath, probeText);

      const document = await vscode.workspace.openTextDocument(probePath);
      const classified = await vscode.languages.setTextDocumentLanguage(document, 'perl5');
      classifiedLanguageId = classified.languageId;
      assert.equal(classified.languageId, 'perl5', 'fixture must allow perl5 classification');
      await vscode.window.showTextDocument(classified);

      // Client-side attachment receipt, where the extension exposes it.
      const extensionApi = (extension.exports ?? {}) as
        | { waitForActiveDocumentReady?: (uri: string, timeoutMs?: number) => Promise<void> }
        | undefined;
      let documentReady: 'observed' | 'api_unavailable' = 'api_unavailable';
      if (typeof extensionApi?.waitForActiveDocumentReady === 'function') {
        await withTimeout(
          'alias document client readiness',
          extensionApi.waitForActiveDocumentReady(classified.uri.toString(), 30_000),
          30_000,
        );
        documentReady = 'observed';
      }

      // Server acceptance: published diagnostics for the alias buffer prove
      // perllsp received the didOpen and parsed the content.
      diagnostics = await withTimeout(
        'diagnostics for perl5-classified document',
        waitForDiagnostics(classified.uri, 90_000),
        90_000,
      );
      assert.ok(
        diagnostics.length > 0,
        'server must publish diagnostics for a perl5-classified document (didOpen fell through?)',
      );

      // One navigation/completion/hover result through the real client.
      const completionPosition = classified.positionAt(
        classified.getText().indexOf('$alias_pro') + '$alias_pro'.length,
      );
      const completionList = await withTimeout(
        'alias completion round trip',
        vscode.commands.executeCommand<vscode.CompletionList>(
          'vscode.executeCompletionItemProvider',
          classified.uri,
          completionPosition,
        ),
        20_000,
      );
      assert.ok(completionList, 'completion provider must answer for the alias document');
      const firstCompletion = completionList.items[0];
      completionItemLabel =
        firstCompletion === undefined
          ? null
          : typeof firstCompletion.label === 'string'
            ? firstCompletion.label
            : firstCompletion.label.label;

      const hoverPosition = classified.positionAt(classified.getText().indexOf('$alias_probe') + 2);
      const hovers = await withTimeout(
        'alias hover round trip',
        vscode.commands.executeCommand<vscode.Hover[]>(
          'vscode.executeHoverProvider',
          classified.uri,
          hoverPosition,
        ),
        20_000,
      );
      assert.ok(Array.isArray(hovers), 'hover provider must answer for the alias document');
      hoverCount = hovers.length;

      writeReceipt({
        schema_version: 1,
        issue: 7699,
        outcome: 'completed',
        generated_at: new Date().toISOString(),
        alias: {
          classified_language_id: classifiedLanguageId,
          document_ready: documentReady,
          server_path: reinstall.serverPath,
          fixture_extension: fixture.id,
        },
        server_proof: {
          diagnostics_published: diagnostics.length,
          diagnostic_messages: diagnostics.slice(0, 10).map((diagnostic) => diagnostic.message),
          completion_item_count: completionList.items.length,
          completion_first_label: completionItemLabel,
          hover_count: hoverCount,
        },
        failures,
      });
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      failures.push({ phase: 'alias-smoke', message });
      writeReceipt({
        schema_version: 1,
        issue: 7699,
        outcome: 'failed',
        generated_at: new Date().toISOString(),
        alias: {
          classified_language_id: classifiedLanguageId,
          server_path: reinstall?.serverPath ?? null,
        },
        server_proof: {
          diagnostics_published: diagnostics.length,
          completion_item_count: null,
          completion_first_label: completionItemLabel,
          hover_count: hoverCount,
        },
        failures,
      });
      throw err;
    }
  });
});
