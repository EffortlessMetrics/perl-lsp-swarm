import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  isIncludePathCandidateCovered,
  runDiscoveredIncludePathGuidance,
  runIncludePathValidation,
  suggestAiCompletionIfSupported,
} from '../extensionWorkspaceGuidance';

const workspaceMock = vscode.workspace as unknown as { workspaceFolders: unknown };
const extensionsMock = vscode.extensions as unknown as { all: unknown[] };

class Deferred<T> {
  readonly promise: Promise<T>;
  private resolvePromise!: (value: T) => void;

  constructor() {
    this.promise = new Promise<T>((resolve) => {
      this.resolvePromise = resolve;
    });
  }

  resolve(value: T): void {
    this.resolvePromise(value);
  }
}

function makeState(): { get: jest.Mock; update: jest.Mock } {
  const values = new Map<string, unknown>();
  return {
    get: jest.fn((key: string, defaultValue?: unknown) => values.get(key) ?? defaultValue),
    update: jest.fn(async (key: string, value: unknown) => {
      if (value === undefined) {
        values.delete(key);
      } else {
        values.set(key, value);
      }
    }),
  };
}

function folderFor(workspaceDir: string, name = 'workspace'): vscode.WorkspaceFolder {
  return {
    name,
    uri: { fsPath: workspaceDir, toString: () => `file://${workspaceDir}` },
  } as unknown as vscode.WorkspaceFolder;
}

function mountWorkspace(workspaceDir: string, includePaths: string[]): vscode.WorkspaceFolder {
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((key: string, defaultValue?: unknown) =>
      key === 'includePaths' ? includePaths : defaultValue,
    ),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  return folder;
}

async function settleAsyncWork(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise<void>((resolve) => setImmediate(resolve));
}

afterEach(() => {
  jest.clearAllMocks();
  workspaceMock.workspaceFolders = undefined;
  extensionsMock.all = [];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(),
    update: jest.fn(),
  }));
});

test('does not prompt for absent built-in include paths', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-default-'));
  mountWorkspace(workspaceDir, ['lib']);

  await runIncludePathValidation({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
});

test('missing include-path validation no longer offers a filesystem mutation', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-missing-'));
  mountWorkspace(workspaceDir, ['generated/perl']);

  await runIncludePathValidation({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('configured include path "generated/perl"'),
    'Open Settings',
  );
  expect(fs.existsSync(path.join(workspaceDir, 'generated', 'perl'))).toBe(false);
});

test('configured canonical ancestors cover candidates but descendants do not', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-cover-'));
  fs.mkdirSync(path.join(workspaceDir, 'src', 'lib'), { recursive: true });

  await expect(isIncludePathCandidateCovered(workspaceDir, ['src'], 'src/lib')).resolves.toBe(true);
  await expect(isIncludePathCandidateCovered(workspaceDir, ['src/lib'], 'src')).resolves.toBe(
    false,
  );
  await expect(isIncludePathCandidateCovered(workspaceDir, ['./src'], 'src')).resolves.toBe(true);
});

test('a configured symlink alias covers its canonical candidate', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-alias-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  try {
    fs.symlinkSync(path.join(workspaceDir, 'src'), path.join(workspaceDir, 'alias'), 'dir');
  } catch {
    return;
  }

  await expect(isIncludePathCandidateCovered(workspaceDir, ['alias'], 'src')).resolves.toBe(true);
});

test('does not scan or suggest a candidate symlinked outside the workspace', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-escape-'));
  const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-outside-'));
  fs.writeFileSync(path.join(outsideDir, 'Escaped.pm'), 'package Escaped; 1;\n');
  try {
    fs.symlinkSync(outsideDir, path.join(workspaceDir, 'vendor'), 'dir');
  } catch {
    return;
  }

  mountWorkspace(workspaceDir, ['lib']);
  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([expect.objectContaining({ folder: 'workspace', discovered: [] })]),
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
});

test('dismissal is sticky for an unchanged discovered module layout', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-cache-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  mountWorkspace(workspaceDir, ['lib']);
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);
  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(1);
});

test('configuration changes invalidate a prior discovery dismissal', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-config-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  let includePaths = ['lib'];
  mountWorkspace(workspaceDir, includePaths);
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);
  includePaths = ['lib', 'manual'];
  mountWorkspace(workspaceDir, includePaths);
  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(2);
});

test('retries a discovered-path suggestion after an update failure', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-retry-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const update = jest.fn(async () => {
    throw new Error('workspace is read-only');
  });
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);
  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  expect(update).toHaveBeenCalledTimes(2);
  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(2);
  expect(globalState.update).not.toHaveBeenCalledWith(
    expect.stringContaining('perl-lsp.includePathsSuggestion.'),
    expect.any(String),
  );
});

test('adds discovered module directories for the owning workspace folder', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-add-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.mkdirSync(path.join(workspaceDir, 'vendor'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  fs.writeFileSync(path.join(workspaceDir, 'vendor', 'Other.pm'), 'package Other; 1;\n');
  const globalState = makeState();
  const update = jest.fn(async () => undefined);
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  // `includePaths` is resource-scoped and these directories were discovered
  // under this folder's own root, so the write belongs to the folder. Writing
  // ConfigurationTarget.Workspace published one folder's include paths to every
  // other folder in a multi-root workspace (#14447).
  expect(update).toHaveBeenCalledWith(
    'includePaths',
    expect.arrayContaining(['src', 'vendor']),
    vscode.ConfigurationTarget.WorkspaceFolder,
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    'Added include paths for workspace: src, vendor.',
  );
  expect(globalState.update).toHaveBeenCalledWith(
    expect.stringContaining('perl-lsp.includePathsSuggestion.'),
    expect.any(String),
  );
});

test('reports entry-budget exhaustion as incomplete', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-budget-'));
  const srcDir = path.join(workspaceDir, 'src');
  fs.mkdirSync(srcDir, { recursive: true });
  for (let index = 0; index < 220; index += 1) {
    fs.writeFileSync(path.join(srcDir, `file-${index}.txt`), 'not perl\n');
  }
  const globalState = makeState();
  mountWorkspace(workspaceDir, ['lib']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState,
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ folder: 'workspace', discovered: [], complete: false }),
    ]),
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
});

test('reports depth-budget exhaustion as incomplete', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-depth-'));
  const deep = path.join(workspaceDir, 'src', 'one', 'two', 'three');
  fs.mkdirSync(deep, { recursive: true });
  fs.writeFileSync(path.join(deep, 'Deep.pm'), 'package Deep; 1;\n');
  mountWorkspace(workspaceDir, ['lib']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ folder: 'workspace', discovered: [], complete: false }),
    ]),
  );
});

test('does not overwrite include paths changed while the suggestion is open', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-stale-config-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  let includePaths = ['lib'];
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => includePaths),
    update,
  }));
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await settleAsyncWork();
  includePaths = ['lib', 'manual'];
  prompt.resolve('Add for These Folders');
  await run;

  expect(update).not.toHaveBeenCalled();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('changed before they could be applied'),
  );
});

test('does not apply a finding after the folder is removed and re-added', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-stale-root-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const original = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [original];
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    update,
  }));
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await settleAsyncWork();
  workspaceMock.workspaceFolders = [folderFor(workspaceDir)];
  prompt.resolve('Add for These Folders');
  await run;

  expect(update).not.toHaveBeenCalled();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('changed before they could be applied'),
  );
});

test('does not prompt for AI completion without a real server capability', async () => {
  (vscode.workspace.getConfiguration as jest.Mock).mockReturnValue({
    get: jest.fn(() => false),
    update: jest.fn(),
  });
  const workspaceState = makeState();

  await suggestAiCompletionIfSupported({ workspaceState } as unknown as vscode.ExtensionContext, {
    initializeResult: { capabilities: { hoverProvider: true } },
  });

  expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
});

test('offers AI completion when the server advertises inline completions', async () => {
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockReturnValue({
    get: jest.fn(() => false),
    update,
  });
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Enable');
  const workspaceState = makeState();

  await suggestAiCompletionIfSupported({ workspaceState } as unknown as vscode.ExtensionContext, {
    initializeResult: { capabilities: { inlineCompletionProvider: {} } },
  });

  expect(update).toHaveBeenCalledWith(
    'aiCompletion.enabled',
    true,
    vscode.ConfigurationTarget.Global,
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    'AI-powered inline completions enabled.',
  );
  expect(workspaceState.update).toHaveBeenCalledWith(
    'perl-lsp.aiCompletion.firstRunNotificationShown',
    true,
  );
});
