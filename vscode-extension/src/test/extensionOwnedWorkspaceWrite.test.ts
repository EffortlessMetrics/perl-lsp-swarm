import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

import {
  ExtensionOwnedWorkspaceWriteError,
  ensureWorkspaceDirectoryPath,
  replaceWorkspaceRegularFile,
  workspaceRegularFileExists,
} from '../extensionOwnedWorkspaceWrite';

const extensionRoot = path.resolve(__dirname, '..', '..');
const inventory = JSON.parse(
  fs.readFileSync(
    path.join(extensionRoot, 'scripts', 'extension-owned-workspace-write-inventory.json'),
    'utf8',
  ),
) as {
  schema: number;
  policy_id: string;
  primitive_module: string;
  consumers: { module: string; import: string }[];
};

describe('extension-owned workspace write inventory', () => {
  test('lists every production consumer of the shared primitive', () => {
    expect(inventory.policy_id).toBe('vscode.extension-owned-write');
    expect(fs.existsSync(path.join(extensionRoot, inventory.primitive_module))).toBe(true);
    for (const consumer of inventory.consumers) {
      const source = fs.readFileSync(path.join(extensionRoot, consumer.module), 'utf8');
      expect(source).toContain(consumer.import);
    }
  });
});

describe('extensionOwnedWorkspaceWrite', () => {
  let workspaceRoot: string;
  let outsideDir: string;

  beforeEach(async () => {
    workspaceRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'perl-lsp-workspace-write-'));
    outsideDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'perl-lsp-workspace-outside-'));
  });

  afterEach(async () => {
    await fs.promises.rm(workspaceRoot, { recursive: true, force: true });
    await fs.promises.rm(outsideDir, { recursive: true, force: true });
  });

  test('creates and overwrites a regular in-workspace launch.json', async () => {
    const launchPath = path.join(workspaceRoot, '.vscode', 'launch.json');
    const initial = '{ "version": "0.2.0" }\n';
    await replaceWorkspaceRegularFile(workspaceRoot, launchPath, initial, {
      replace: 'create-only',
    });
    expect(await fs.promises.readFile(launchPath, 'utf8')).toBe(initial);
    expect(workspaceRegularFileExists(workspaceRoot, launchPath)).toBe(true);

    const updated = '{ "version": "0.2.0", "configurations": [] }\n';
    await replaceWorkspaceRegularFile(workspaceRoot, launchPath, updated, {
      replace: 'overwrite',
    });
    expect(await fs.promises.readFile(launchPath, 'utf8')).toBe(updated);
  });

  test('rejects a symlinked .vscode directory before touching an outside sentinel', async () => {
    const sentinel = path.join(outsideDir, 'sentinel.json');
    await fs.promises.writeFile(sentinel, 'unchanged\n', 'utf8');
    const vscodeLink = path.join(workspaceRoot, '.vscode');
    try {
      fs.symlinkSync(outsideDir, vscodeLink, process.platform === 'win32' ? 'junction' : 'dir');
    } catch {
      return;
    }

    const launchPath = path.join(workspaceRoot, '.vscode', 'launch.json');
    await expect(
      replaceWorkspaceRegularFile(workspaceRoot, launchPath, '{}\n', { replace: 'create-only' }),
    ).rejects.toBeInstanceOf(ExtensionOwnedWorkspaceWriteError);
    expect(await fs.promises.readFile(sentinel, 'utf8')).toBe('unchanged\n');
  });

  test('rejects a symlinked launch.json leaf after overwrite intent', async () => {
    if (process.platform === 'win32') {
      return;
    }

    const sentinel = path.join(outsideDir, 'sentinel.json');
    await fs.promises.writeFile(sentinel, 'unchanged\n', 'utf8');
    await fs.promises.mkdir(path.join(workspaceRoot, '.vscode'), { recursive: true });
    const launchPath = path.join(workspaceRoot, '.vscode', 'launch.json');
    await fs.promises.symlink(sentinel, launchPath);

    await expect(
      replaceWorkspaceRegularFile(workspaceRoot, launchPath, '{ "version": "0.2.0" }\n', {
        replace: 'overwrite',
      }),
    ).rejects.toMatchObject({
      workspaceRelativePath: '.vscode/launch.json',
    });
    expect(await fs.promises.readFile(sentinel, 'utf8')).toBe('unchanged\n');
  });

  test('ensureWorkspaceDirectoryPath rejects a symlink parent for directory creation', async () => {
    if (process.platform === 'win32') {
      return;
    }

    const sentinel = path.join(outsideDir, 'created-lib');
    await fs.promises.symlink(outsideDir, path.join(workspaceRoot, 'linked'), 'dir');
    await expect(
      ensureWorkspaceDirectoryPath(workspaceRoot, path.join(workspaceRoot, 'linked', 'lib')),
    ).rejects.toBeInstanceOf(ExtensionOwnedWorkspaceWriteError);
    expect(fs.existsSync(sentinel)).toBe(false);
  });
});
