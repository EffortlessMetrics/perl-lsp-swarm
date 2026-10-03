import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import type { ReinstallCommandResult } from '../../commandResults';
import {
  assertProviderSucceeded,
  bundledServerVersion,
  pathsEquivalent,
  providerResult,
  scanServerProcessIdentities,
  withTimeout,
} from './journeySupport';

interface UpdateStage {
  phase: 'reload_requested';
  extensionPath: string;
  oldPath: string;
  oldVersion: string;
  oldPid: number;
  oldCreationIdentity?: string;
  newPath: string;
  newVersion: string;
}

function stagePath(): string {
  const file = process.env.PERL_LSP_UPDATE_CLOSURE_STAGE_FILE;
  assert.ok(file, 'installed update journey requires a persistent stage receipt');
  return file;
}

function selectedBinary(oldPath: string): string | null {
  const baseDir = path.dirname(path.dirname(oldPath));
  const pointer = path.join(baseDir, 'current');
  if (!fs.existsSync(pointer)) {
    return null;
  }
  const name = fs.readFileSync(pointer, 'utf8').trim();
  if (!name || name.includes('/') || name.includes('\\') || name === '.' || name === '..') {
    throw new Error(`invalid managed current pointer: ${JSON.stringify(name)}`);
  }
  return path.join(baseDir, name, path.basename(oldPath));
}

async function waitForProcess(binary: string, timeoutMs: number) {
  return withTimeout(
    `server process ${binary}`,
    (async () => {
      for (;;) {
        const processes = await scanServerProcessIdentities(path.dirname(path.dirname(binary)));
        const matched = processes.find((entry) => pathsEquivalent(entry.path, binary));
        if (matched) {
          return matched;
        }
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
    })(),
    timeoutMs,
  );
}

suite('Installed update closure', function () {
  this.timeout(240_000);

  test('managed A remains active until reload and the fresh host launches selected B', async function () {
    this.timeout(240_000);
    const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
    assert.ok(extension, 'candidate VSIX must be installed in the isolated profile');
    const file = stagePath();
    if (fs.existsSync(file)) {
      const stage = JSON.parse(fs.readFileSync(file, 'utf8')) as UpdateStage;
      assert.equal(stage.phase, 'reload_requested');
      assert.ok(
        fs.existsSync(path.join(path.dirname(file), 'update-closure-reload-dispatched')),
        'first host must dispatch Reload Window before the second host is accepted',
      );
      assert.ok(pathsEquivalent(extension.extensionPath, stage.extensionPath));
      const document = await vscode.workspace.openTextDocument(
        vscode.Uri.file(
          path.join(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? '', 'smoke.pl'),
        ),
      );
      await vscode.window.showTextDocument(document);
      const activation = (await withTimeout(
        'extension activation after reload',
        extension.activate(),
        30_000,
      )) as {
        getLanguageClientStartupMetrics?: () => Record<string, unknown>;
      };
      const active = await waitForProcess(stage.newPath, 60_000);
      assert.notEqual(active.pid, stage.oldPid, 'reload must start a new server process');
      assert.ok(pathsEquivalent(fs.realpathSync(active.path), fs.realpathSync(stage.newPath)));
      const version = await bundledServerVersion(active.path);
      assert.equal(version.status, 'ok', JSON.stringify(version));
      if (version.status === 'ok') {
        assert.equal(version.version, stage.newVersion);
      }
      assert.ok(pathsEquivalent(selectedBinary(stage.oldPath), stage.newPath));
      assert.ok(
        activation.getLanguageClientStartupMetrics,
        'installed extension must expose startup metrics',
      );
      const metrics = await withTimeout(
        'post-reload LanguageClient startup',
        (async () => {
          for (;;) {
            const current = activation.getLanguageClientStartupMetrics?.() ?? {};
            if (current.initialize_status === 'ok') {
              return current;
            }
            await new Promise((resolve) => setTimeout(resolve, 100));
          }
        })(),
        30_000,
      );
      assert.equal(metrics.binary_resolution_status, 'ok');
      assert.equal(metrics.server_start_status, 'ok');
      assert.ok(pathsEquivalent(metrics.binary_resolution_path, stage.newPath));
      assert.equal(metrics.server_version, stage.newVersion);
      const hover = await providerResult(
        'post-reload hover',
        'vscode.executeHoverProvider',
        document.uri,
        new vscode.Position(2, 2),
      );
      assertProviderSucceeded('post-reload hover', hover);
      fs.writeFileSync(
        path.join(path.dirname(file), 'update-closure-result.json'),
        JSON.stringify(
          {
            ...stage,
            newPid: active.pid,
            newCreationIdentity: active.creationIdentity,
            metrics,
            hover,
          },
          null,
          2,
        ),
      );
      return;
    }

    const config = vscode.workspace.getConfiguration('perl-lsp');
    await config.update('serverPath', '', vscode.ConfigurationTarget.Global);
    await config.update('autoDownload', false, vscode.ConfigurationTarget.Global);
    await config.update('channel', 'tag', vscode.ConfigurationTarget.Global);
    await config.update('versionTag', 'v0.16.0', vscode.ConfigurationTarget.Global);
    await config.update('downloadBaseUrl', '', vscode.ConfigurationTarget.Global);
    await config.update('updateCheckInterval', 0, vscode.ConfigurationTarget.Global);
    await config.update('autoUpdate', false, vscode.ConfigurationTarget.Global);
    await config.update('perlcritic.enabled', false, vscode.ConfigurationTarget.Global);
    await withTimeout('extension activation', extension.activate(), 30_000);
    const reinstall = await withTimeout(
      'install managed A',
      vscode.commands.executeCommand<ReinstallCommandResult>('perl-lsp.reinstall'),
      120_000,
    );
    assert.ok(reinstall?.ok, JSON.stringify(reinstall));
    const oldPath = fs.realpathSync(reinstall.serverPath);
    const oldVersion = await bundledServerVersion(oldPath);
    assert.equal(oldVersion.status, 'ok', JSON.stringify(oldVersion));
    if (oldVersion.status !== 'ok') {
      return;
    }
    assert.equal(oldVersion.version, '0.16.0');
    const document = await vscode.workspace.openTextDocument(
      vscode.Uri.file(
        path.join(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? '', 'smoke.pl'),
      ),
    );
    await vscode.window.showTextDocument(document);
    const oldProcess = await waitForProcess(oldPath, 60_000);

    await config.update('channel', 'latest', vscode.ConfigurationTarget.Global);
    await config.update('versionTag', '', vscode.ConfigurationTarget.Global);
    await config.update('updateCheckInterval', 24, vscode.ConfigurationTarget.Global);
    await config.update('autoUpdate', true, vscode.ConfigurationTarget.Global);
    await withTimeout(
      'accept automatic update',
      vscode.commands.executeCommand('perl-lsp.checkForUpdate'),
      120_000,
    );
    const newPath = selectedBinary(oldPath);
    assert.ok(
      newPath && fs.existsSync(newPath),
      'successful update must select a new managed binary',
    );
    assert.ok(!pathsEquivalent(newPath, oldPath), 'selected update must differ from managed A');
    const newVersion = await bundledServerVersion(newPath);
    assert.equal(newVersion.status, 'ok', JSON.stringify(newVersion));
    if (newVersion.status !== 'ok') {
      return;
    }
    assert.notEqual(newVersion.version, oldVersion.version);
    const stillRunning = await scanServerProcessIdentities(path.dirname(path.dirname(oldPath)));
    assert.ok(
      stillRunning.some(
        (entry) => entry.pid === oldProcess.pid && pathsEquivalent(entry.path, oldPath),
      ),
      'A must remain the active LanguageClient process while reload is pending',
    );
    assert.ok(
      !stillRunning.some((entry) => pathsEquivalent(entry.path, newPath)),
      'B must not launch before reload',
    );
    await config.update('autoDownload', true, vscode.ConfigurationTarget.Global);
    const stage: UpdateStage = {
      phase: 'reload_requested',
      extensionPath: extension.extensionPath,
      oldPath,
      oldVersion: oldVersion.version,
      oldPid: oldProcess.pid,
      ...(oldProcess.creationIdentity === undefined
        ? {}
        : { oldCreationIdentity: oldProcess.creationIdentity }),
      newPath: fs.realpathSync(newPath),
      newVersion: newVersion.version,
    };
    fs.writeFileSync(file, JSON.stringify(stage, null, 2));
    const reload = vscode.commands.executeCommand('workbench.action.reloadWindow');
    fs.writeFileSync(path.join(path.dirname(file), 'update-closure-reload-dispatched'), '1\n');
    await reload;
  });
});
