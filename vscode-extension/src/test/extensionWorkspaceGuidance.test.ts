import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  isIncludePathCandidateCovered,
  runDiscoveredIncludePathGuidance,
  runIncludePathValidation,
  registerIncludePathGuidanceWorkspaceListener,
  rerunIncludePathGuidance,
  suggestAiCompletionIfSupported,
  validateIncludePaths,
} from '../extensionWorkspaceGuidance';

const workspaceMock = vscode.workspace as unknown as { workspaceFolders: unknown };
const extensionsMock = vscode.extensions as unknown as { all: unknown[] };
const fixtureDirs: string[] = [];

function tempWorkspace(prefix: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  fixtureDirs.push(dir);
  return dir;
}

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

async function waitForCalls(mock: jest.Mock, count: number): Promise<void> {
  for (let attempt = 0; attempt < 50 && mock.mock.calls.length < count; attempt += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
}

afterEach(() => {
  for (const dir of fixtureDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
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
  const workspaceDir = tempWorkspace('perl-lsp-guidance-default-');
  mountWorkspace(workspaceDir, ['lib']);

  await runIncludePathValidation({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
});

test('missing include-path validation no longer offers a filesystem mutation', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-missing-');
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
  const workspaceDir = tempWorkspace('perl-lsp-guidance-cover-');
  fs.mkdirSync(path.join(workspaceDir, 'src', 'lib'), { recursive: true });

  await expect(isIncludePathCandidateCovered(workspaceDir, ['src'], 'src/lib')).resolves.toBe(true);
  await expect(isIncludePathCandidateCovered(workspaceDir, ['src/lib'], 'src')).resolves.toBe(
    false,
  );
  await expect(isIncludePathCandidateCovered(workspaceDir, ['./src'], 'src')).resolves.toBe(true);
  fs.mkdirSync(path.join(workspaceDir, 'src', '..sources'), { recursive: true });
  await expect(isIncludePathCandidateCovered(workspaceDir, ['src'], 'src/..sources')).resolves.toBe(
    true,
  );
});

test('continues coverage after an unreadable configured root', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-cover-unreadable-');
  fs.mkdirSync(path.join(workspaceDir, 'src', 'lib'), { recursive: true });
  const blocked = path.join(workspaceDir, 'blocked');
  const originalRealpath = fs.promises.realpath;
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    if (String(target) === blocked) {
      const error = new Error('permission denied') as NodeJS.ErrnoException;
      error.code = 'EACCES';
      throw error;
    }
    return originalRealpath.call(fs.promises, target);
  });

  await expect(
    isIncludePathCandidateCovered(workspaceDir, ['blocked', 'src'], 'src/lib'),
  ).resolves.toBe(true);

  realpath.mockRestore();
});

test('suggests a mixed root while excluding its configured descendant', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-mixed-layout-');
  fs.mkdirSync(path.join(workspaceDir, 'src', 'lib'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'TopLevel.pm'), 'package TopLevel; 1;\n');
  fs.writeFileSync(path.join(workspaceDir, 'src', 'lib', 'Nested.pm'), 'package Nested; 1;\n');
  mountWorkspace(workspaceDir, ['src/lib']);
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([expect.objectContaining({ discovered: ['src'], complete: true })]),
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    expect.stringContaining('workspace: src'),
    'Add for These Folders',
    'Open Settings',
    'Dismiss',
  );
});

test('keeps a root silent when all modules are under a configured descendant', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-covered-descendant-');
  fs.mkdirSync(path.join(workspaceDir, 'src', 'lib'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'lib', 'Nested.pm'), 'package Nested; 1;\n');
  mountWorkspace(workspaceDir, ['src/lib']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([expect.objectContaining({ discovered: [], complete: true })]),
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
});

test('does not exhaust the exact entry budget on a configured descendant', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-covered-budget-');
  const srcDir = path.join(workspaceDir, 'src');
  fs.mkdirSync(path.join(srcDir, 'lib'), { recursive: true });
  for (let index = 0; index < 199; index += 1) {
    fs.writeFileSync(path.join(srcDir, `file-${index}.txt`), 'not perl\n');
  }
  mountWorkspace(workspaceDir, ['src/lib']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([expect.objectContaining({ discovered: [], complete: true })]),
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
});

test('does not exhaust the depth budget on a configured descendant', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-covered-depth-');
  fs.mkdirSync(path.join(workspaceDir, 'src', 'one', 'two', 'three'), { recursive: true });
  mountWorkspace(workspaceDir, ['src/one/two/three']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([expect.objectContaining({ discovered: [], complete: true })]),
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalled();
});

test('a configured symlink alias covers its canonical candidate', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-alias-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  try {
    fs.symlinkSync(path.join(workspaceDir, 'src'), path.join(workspaceDir, 'alias'), 'dir');
  } catch {
    return;
  }

  await expect(isIncludePathCandidateCovered(workspaceDir, ['alias'], 'src')).resolves.toBe(true);
});

test('does not scan or suggest a candidate symlinked outside the workspace', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-escape-');
  const outsideDir = tempWorkspace('perl-lsp-guidance-outside-');
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
  const workspaceDir = tempWorkspace('perl-lsp-guidance-cache-');
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
  const workspaceDir = tempWorkspace('perl-lsp-guidance-config-');
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

  expect(
    (vscode.window.showInformationMessage as jest.Mock).mock.calls.filter(([message]) =>
      String(message).includes('found Perl module roots outside'),
    ),
  ).toHaveLength(2);
});

test('drops a folder snapshot changed while a later root check is held', async () => {
  const firstDir = tempWorkspace('perl-lsp-guidance-held-first-');
  const secondDir = tempWorkspace('perl-lsp-guidance-held-second-');
  fs.mkdirSync(path.join(firstDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(firstDir, 'src', 'First.pm'), 'package First; 1;\n');
  fs.mkdirSync(path.join(secondDir, 'vendor'), { recursive: true });
  fs.writeFileSync(path.join(secondDir, 'vendor', 'Second.pm'), 'package Second; 1;\n');
  const first = folderFor(firstDir, 'first');
  const second = folderFor(secondDir, 'second');
  workspaceMock.workspaceFolders = [first, second];
  let firstIncludePaths = ['lib'];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(
    (_section: string, folder: vscode.Uri) => ({
      get: jest.fn(() => (folder.fsPath === firstDir ? firstIncludePaths : ['lib'])),
      inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
      update: jest.fn(async () => undefined),
    }),
  );
  const originalRealpath = fs.promises.realpath;
  let laterRootChecks = 0;
  let markEntered!: () => void;
  const entered = new Promise<void>((resolve) => {
    markEntered = resolve;
  });
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    const result = await originalRealpath.call(fs.promises, target);
    if (String(target) === secondDir) {
      laterRootChecks += 1;
      if (laterRootChecks === 2) {
        markEntered();
        await gate;
      }
    }
    return result;
  });
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await entered;
  firstIncludePaths = ['lib', 'src'];
  release();
  await run;

  realpath.mockRestore();
  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(1);
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    expect.stringContaining('second: vendor'),
    'Add for These Folders',
    'Open Settings',
    'Dismiss',
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalledWith(
    expect.stringContaining('first: src'),
    expect.anything(),
    expect.anything(),
    expect.anything(),
  );
});

test('workspace-folder removal invalidates dismissal for a replacement with the same URI', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-folder-generation-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const original = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [original];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);
  let onFoldersChanged!: (event: { removed: vscode.WorkspaceFolder[] }) => void;
  (vscode.workspace.onDidChangeWorkspaceFolders as jest.Mock).mockImplementationOnce(
    (callback: (event: { removed: vscode.WorkspaceFolder[] }) => void) => {
      onFoldersChanged = callback;
      return { dispose: jest.fn() };
    },
  );
  const context = { globalState } as unknown as vscode.ExtensionContext;
  const listener = registerIncludePathGuidanceWorkspaceListener(context);
  const replacement = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [replacement];
  onFoldersChanged({ removed: [original] });
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 2);

  listener.dispose();
  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(2);
});

test('a removed folder prompt cannot restore dismissal for its replacement', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-folder-inflight-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const original = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [original];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  const oldPrompt = new Deferred<string>();
  const newPrompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock)
    .mockReturnValueOnce(oldPrompt.promise)
    .mockReturnValueOnce(newPrompt.promise);

  const context = { globalState } as unknown as vscode.ExtensionContext;
  const oldRun = runDiscoveredIncludePathGuidance(context);
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);
  let onFoldersChanged!: (event: { removed: vscode.WorkspaceFolder[] }) => void;
  (vscode.workspace.onDidChangeWorkspaceFolders as jest.Mock).mockImplementationOnce(
    (callback: (event: { removed: vscode.WorkspaceFolder[] }) => void) => {
      onFoldersChanged = callback;
      return { dispose: jest.fn() };
    },
  );
  registerIncludePathGuidanceWorkspaceListener(context);
  const replacement = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [replacement];
  onFoldersChanged({ removed: [original] });
  oldPrompt.resolve('Dismiss');
  await oldRun;
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 2);
  newPrompt.resolve('Dismiss');
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 2);

  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(2);
});

test('added-only workspace topology changes rerun guidance', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-folder-added-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');
  let onFoldersChanged!: (event: {
    removed: vscode.WorkspaceFolder[];
    added: vscode.WorkspaceFolder[];
  }) => void;
  (vscode.workspace.onDidChangeWorkspaceFolders as jest.Mock).mockImplementationOnce(
    (callback: typeof onFoldersChanged) => {
      onFoldersChanged = callback;
      return { dispose: jest.fn() };
    },
  );

  registerIncludePathGuidanceWorkspaceListener({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  onFoldersChanged({ removed: [], added: [folder] });
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);

  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(1);
});

test('reruns added-folder guidance when removed-folder cache cleanup fails', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-cache-topology-fault-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const original = folderFor(workspaceDir);
  const replacement = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [original];
  const globalState = makeState();
  globalState.update.mockImplementation(async (key: string) => {
    if (key.includes(encodeURIComponent(original.uri.toString()))) {
      throw new Error('removed-folder cache unavailable');
    }
  });
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');
  let onFoldersChanged!: (event: {
    removed: vscode.WorkspaceFolder[];
    added: vscode.WorkspaceFolder[];
  }) => void;
  (vscode.workspace.onDidChangeWorkspaceFolders as jest.Mock).mockImplementationOnce(
    (callback: typeof onFoldersChanged) => {
      onFoldersChanged = callback;
      return { dispose: jest.fn() };
    },
  );
  const context = { globalState } as unknown as vscode.ExtensionContext;
  const listener = registerIncludePathGuidanceWorkspaceListener(context);
  workspaceMock.workspaceFolders = [replacement];
  onFoldersChanged({ removed: [original], added: [replacement] });
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);

  listener.dispose();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('could not clear workspace-folder guidance state'),
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    expect.stringContaining('workspace: src'),
    'Add for These Folders',
    'Open Settings',
    'Dismiss',
  );
});

test('removal during discovery invalidates the pre-removal scan', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-folder-scan-generation-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const original = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [original];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Dismiss');
  let onFoldersChanged!: (event: {
    removed: vscode.WorkspaceFolder[];
    added: vscode.WorkspaceFolder[];
  }) => void;
  (vscode.workspace.onDidChangeWorkspaceFolders as jest.Mock).mockImplementationOnce(
    (callback: typeof onFoldersChanged) => {
      onFoldersChanged = callback;
      return { dispose: jest.fn() };
    },
  );
  const context = { globalState: makeState() } as unknown as vscode.ExtensionContext;
  registerIncludePathGuidanceWorkspaceListener(context);

  let rootStarted = false;
  let releaseRoot!: () => void;
  const rootGate = new Promise<void>((resolve) => {
    releaseRoot = resolve;
  });
  const originalRealpath = fs.promises.realpath;
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    const result = await originalRealpath.call(fs.promises, target);
    if (String(target) === workspaceDir && !rootStarted) {
      rootStarted = true;
      await rootGate;
    }
    return result;
  });

  const run = runDiscoveredIncludePathGuidance(context);
  for (let attempt = 0; attempt < 50 && !rootStarted; attempt += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
  const replacement = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [replacement];
  onFoldersChanged({ removed: [original], added: [replacement] });
  releaseRoot();
  await run;
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);

  realpath.mockRestore();
  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(1);
});

test('retries a discovered-path suggestion after an update failure', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-retry-');
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
  const workspaceDir = tempWorkspace('perl-lsp-guidance-add-');
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
    undefined,
  );
});

test('removing an accepted discovered path allows the suggestion again', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-removed-accepted-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  let includePaths = ['lib'];
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => includePaths ?? defaultValue),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock)
    .mockResolvedValueOnce('Add for These Folders')
    .mockResolvedValueOnce('Dismiss');

  const context = { globalState } as unknown as vscode.ExtensionContext;
  await runDiscoveredIncludePathGuidance(context);
  includePaths = ['lib', 'src'];
  await runDiscoveredIncludePathGuidance(context);
  includePaths = ['lib'];
  await runDiscoveredIncludePathGuidance(context);

  expect(
    (vscode.window.showInformationMessage as jest.Mock).mock.calls.filter(([message]) =>
      String(message).includes('found Perl module roots outside'),
    ),
  ).toHaveLength(2);
  expect(update).toHaveBeenCalledWith(
    'includePaths',
    expect.arrayContaining(['src']),
    vscode.ConfigurationTarget.WorkspaceFolder,
  );
});

test('reports entry-budget exhaustion as incomplete', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-budget-');
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
  const workspaceDir = tempWorkspace('perl-lsp-guidance-depth-');
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

test('continues with independent candidates when one candidate realpath fails', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-realpath-error-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  fs.mkdirSync(path.join(workspaceDir, 'vendor'), { recursive: true });
  const originalRealpath = fs.promises.realpath;
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    if (String(target).endsWith(path.join('vendor'))) {
      const error = new Error('permission denied') as NodeJS.ErrnoException;
      error.code = 'EACCES';
      throw error;
    }
    return originalRealpath.call(fs.promises, target);
  });
  mountWorkspace(workspaceDir, ['lib']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  realpath.mockRestore();
  expect(reports).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ folder: 'workspace', discovered: ['src'], complete: false }),
    ]),
  );
});

test('does not overwrite include paths changed while the suggestion is open', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-stale-config-');
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
  expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
});

test('does not apply a finding after the folder is removed and re-added', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-stale-root-');
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
  expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
});

test('does not mark an exactly full directory budget incomplete', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-budget-exact-');
  const srcDir = path.join(workspaceDir, 'src');
  fs.mkdirSync(srcDir, { recursive: true });
  for (let index = 0; index < 200; index += 1) {
    fs.writeFileSync(path.join(srcDir, `file-${index}.txt`), 'not perl\n');
  }
  mountWorkspace(workspaceDir, ['lib']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ folder: 'workspace', discovered: [], complete: true }),
    ]),
  );
});

test('marks a directory with an entry beyond the budget incomplete', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-budget-over-');
  const srcDir = path.join(workspaceDir, 'src');
  fs.mkdirSync(srcDir, { recursive: true });
  for (let index = 0; index < 201; index += 1) {
    fs.writeFileSync(path.join(srcDir, `file-${index}.txt`), 'not perl\n');
  }
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

test('continues discovery when a configured descendant is unreadable', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-error-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  // The embedded NUL makes realpath reject this configured descendant with a
  // non-ENOENT error while the independent src candidate remains discoverable.
  mountWorkspace(workspaceDir, ['blocked\0child']);

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);

  expect(reports).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ folder: 'workspace', discovered: ['src'], complete: false }),
    ]),
  );
});

test('allows a partial positive while disclosing an incomplete scan', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-partial-positive-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  mountWorkspace(workspaceDir, ['blocked\0child']);
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => ['blocked\0child']),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update,
  }));
  const originalRealpath = fs.promises.realpath;
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    if (String(target).includes('\0')) {
      throw new Error('permission denied');
    }
    return originalRealpath.call(fs.promises, target);
  });
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  const reports = await runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  realpath.mockRestore();

  expect(reports).toEqual(
    expect.arrayContaining([expect.objectContaining({ discovered: ['src'], complete: false })]),
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    expect.stringContaining('additional paths may exist'),
    'Add for These Folders',
    'Open Settings',
    'Dismiss',
  );
  expect(update).toHaveBeenCalledWith(
    'includePaths',
    expect.arrayContaining(['src']),
    vscode.ConfigurationTarget.WorkspaceFolder,
  );
});

test('queues one validation rerun for a change during an active prompt', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-rerun-');
  mountWorkspace(workspaceDir, ['first/missing']);
  let includePaths = ['first/missing'];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => includePaths),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  const firstPrompt = new Deferred<string | undefined>();
  (vscode.window.showWarningMessage as jest.Mock).mockReturnValue(firstPrompt.promise);

  const context = { globalState: makeState() } as unknown as vscode.ExtensionContext;
  void validateIncludePaths(context);
  await waitForCalls(vscode.window.showWarningMessage as jest.Mock, 1);
  includePaths = ['second/missing'];
  void validateIncludePaths(context);
  firstPrompt.resolve(undefined);
  await waitForCalls(vscode.window.showWarningMessage as jest.Mock, 2);

  expect(vscode.window.showWarningMessage).toHaveBeenCalledTimes(2);
  expect(vscode.window.showWarningMessage).toHaveBeenLastCalledWith(
    expect.stringContaining('second/missing'),
    'Open Settings',
  );
});

test('does not cache a warning after configuration changes during its prompt', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-validation-stale-');
  let includePaths = ['first/missing'];
  const globalState = makeState();
  mountWorkspace(workspaceDir, includePaths);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => includePaths),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  (vscode.window.showWarningMessage as jest.Mock).mockImplementationOnce(async () => {
    includePaths = ['changed/missing'];
    return undefined;
  });

  const context = { globalState } as unknown as vscode.ExtensionContext;
  await runIncludePathValidation(context);

  expect(globalState.update).not.toHaveBeenCalled();
});

test('does not warn or cache when configuration changes during path access', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-validation-access-');
  let includePaths = ['first/missing'];
  const globalState = makeState();
  mountWorkspace(workspaceDir, includePaths);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => includePaths),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  let rejectAccess!: () => void;
  const accessGate = new Promise<void>((_resolve, reject) => {
    rejectAccess = () => {
      const error = new Error('missing') as NodeJS.ErrnoException;
      error.code = 'ENOENT';
      reject(error);
    };
  });
  const access = jest.spyOn(fs.promises, 'access').mockReturnValueOnce(accessGate);

  const run = runIncludePathValidation({ globalState } as unknown as vscode.ExtensionContext);
  await waitForCalls(access as unknown as jest.Mock, 1);
  includePaths = ['changed/missing'];
  rejectAccess();
  await run;

  access.mockRestore();
  expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
  expect(globalState.update).not.toHaveBeenCalled();
});

test('live guidance rerun coalesces a configuration event during discovery', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-live-event-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  let includePaths = ['lib'];
  mountWorkspace(workspaceDir, includePaths);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => includePaths),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
  const firstPrompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock)
    .mockReturnValueOnce(firstPrompt.promise)
    .mockResolvedValue('Dismiss');
  const context = { globalState: makeState() } as unknown as vscode.ExtensionContext;

  const firstEvent = rerunIncludePathGuidance(context);
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);
  includePaths = ['lib', 'changed-during-discovery'];
  void rerunIncludePathGuidance(context);
  firstPrompt.resolve('Dismiss');
  await firstEvent;
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 2);

  expect(vscode.window.showInformationMessage).toHaveBeenCalledTimes(2);
});

test('does not apply a module root removed while the prompt is open', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-removed-module-');
  const modulePath = path.join(workspaceDir, 'src', 'Module.pm');
  fs.mkdirSync(path.dirname(modulePath), { recursive: true });
  fs.writeFileSync(modulePath, 'package Module; 1;\n');
  const update = jest.fn(async () => undefined);
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    update,
  }));
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);
  fs.rmSync(modulePath);
  prompt.resolve('Add for These Folders');
  await run;

  expect(update).not.toHaveBeenCalled();
});

test('does not apply a mixed root after its uncovered top-level module is removed', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-mixed-stale-');
  const topLevel = path.join(workspaceDir, 'src', 'TopLevel.pm');
  fs.mkdirSync(path.join(workspaceDir, 'src', 'lib'), { recursive: true });
  fs.writeFileSync(topLevel, 'package TopLevel; 1;\n');
  fs.writeFileSync(path.join(workspaceDir, 'src', 'lib', 'Nested.pm'), 'package Nested; 1;\n');
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => ['src/lib']),
    update,
  }));
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);
  fs.rmSync(topLevel);
  prompt.resolve('Add for These Folders');
  await run;

  expect(update).not.toHaveBeenCalled();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('changed before they could be applied'),
  );
});

test('rejects a configuration change during candidate revalidation', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-final-recheck-');
  const modulePath = path.join(workspaceDir, 'src', 'Module.pm');
  fs.mkdirSync(path.dirname(modulePath), { recursive: true });
  fs.writeFileSync(modulePath, 'package Module; 1;\n');
  let includePaths = ['lib'];
  const update = jest.fn(async () => undefined);
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => includePaths ?? defaultValue),
    update,
  }));
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);
  let revalidation = false;
  const originalRealpath = fs.promises.realpath;
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    const result = await originalRealpath.call(fs.promises, target);
    if (revalidation && String(target).endsWith(path.join('src'))) {
      includePaths = ['lib', 'changed-during-rescan'];
      revalidation = false;
    }
    return result;
  });

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);
  revalidation = true;
  prompt.resolve('Add for These Folders');
  await run;

  realpath.mockRestore();
  expect(update).not.toHaveBeenCalled();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('changed before they could be applied'),
  );
});

test('rejects a workspace-folder replacement during final root realpath', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-final-root-replacement-');
  const modulePath = path.join(workspaceDir, 'src', 'Module.pm');
  fs.mkdirSync(path.dirname(modulePath), { recursive: true });
  fs.writeFileSync(modulePath, 'package Module; 1;\n');
  const update = jest.fn(async () => undefined);
  const folder = folderFor(workspaceDir);
  workspaceMock.workspaceFolders = [folder];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    update,
  }));
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);
  let rootRealpathCalls = 0;
  let finalRootStarted = false;
  let releaseFinalRoot!: () => void;
  const finalRootGate = new Promise<void>((resolve) => {
    releaseFinalRoot = resolve;
  });
  const originalRealpath = fs.promises.realpath;
  const realpath = jest.spyOn(fs.promises, 'realpath').mockImplementation(async (target) => {
    const result = await originalRealpath.call(fs.promises, target);
    if (String(target) === workspaceDir) {
      rootRealpathCalls += 1;
      if (rootRealpathCalls === 3) {
        finalRootStarted = true;
        await finalRootGate;
      }
    }
    return result;
  });

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await waitForCalls(vscode.window.showInformationMessage as jest.Mock, 1);
  prompt.resolve('Add for These Folders');
  for (let attempt = 0; attempt < 50 && !finalRootStarted; attempt += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
  workspaceMock.workspaceFolders = [folderFor(tempWorkspace('perl-lsp-guidance-replaced-'))];
  releaseFinalRoot();
  await run;

  realpath.mockRestore();
  expect(update).not.toHaveBeenCalled();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('changed before they could be applied'),
  );
});

test('reports only the module roots applied after partial revalidation', async () => {
  const firstDir = tempWorkspace('perl-lsp-guidance-partial-first-');
  const secondDir = tempWorkspace('perl-lsp-guidance-partial-second-');
  const firstModule = path.join(firstDir, 'src', 'First.pm');
  fs.mkdirSync(path.dirname(firstModule), { recursive: true });
  fs.writeFileSync(firstModule, 'package First; 1;\n');
  fs.mkdirSync(path.join(secondDir, 'vendor'), { recursive: true });
  fs.writeFileSync(path.join(secondDir, 'vendor', 'Second.pm'), 'package Second; 1;\n');
  const first = folderFor(firstDir, 'first');
  const second = folderFor(secondDir, 'second');
  workspaceMock.workspaceFolders = [first, second];
  const firstUpdate = jest.fn(async () => undefined);
  const secondUpdate = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(
    (_section: string, folder: vscode.Uri) => ({
      get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
      update: folder.fsPath === firstDir ? firstUpdate : secondUpdate,
    }),
  );
  const prompt = new Deferred<string>();
  (vscode.window.showInformationMessage as jest.Mock).mockReturnValue(prompt.promise);

  const run = runDiscoveredIncludePathGuidance({
    globalState: makeState(),
  } as unknown as vscode.ExtensionContext);
  await settleAsyncWork();
  fs.rmSync(firstModule);
  prompt.resolve('Add for These Folders');
  await run;

  expect(firstUpdate).not.toHaveBeenCalled();
  expect(secondUpdate).toHaveBeenCalledWith(
    'includePaths',
    expect.arrayContaining(['vendor']),
    vscode.ConfigurationTarget.WorkspaceFolder,
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    'Added include paths for second: vendor.',
  );
});

test('reports committed settings when guidance-state cleanup fails', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-cache-reject-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const folder = mountWorkspace(workspaceDir, ['lib']);
  const globalState = makeState();
  globalState.update.mockImplementation(async (key: string) => {
    if (key.includes('includePathsSuggestion')) {
      throw new Error('state store unavailable');
    }
  });
  const update = jest.fn(async () => undefined);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => ['lib']),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  expect(update).toHaveBeenCalledWith(
    'includePaths',
    expect.arrayContaining(['src']),
    vscode.ConfigurationTarget.WorkspaceFolder,
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    'Added include paths for workspace: src.',
  );
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('guidance state cleanup failed'),
  );
  void folder;
});

test('reports a setting failure without claiming the guidance was applied', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-setting-reject-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const update = jest.fn(async () => {
    throw new Error('settings store unavailable');
  });
  mountWorkspace(workspaceDir, ['lib']);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => ['lib']),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  expect(update).toHaveBeenCalled();
  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('could not update include paths'),
  );
  expect(vscode.window.showInformationMessage).not.toHaveBeenCalledWith(
    expect.stringContaining('Added include paths'),
  );
  expect(globalState.update).not.toHaveBeenCalledWith(
    expect.stringContaining('includePathsSuggestion'),
    undefined,
  );
});

test('keeps committed settings when the folder is removed after update', async () => {
  const workspaceDir = tempWorkspace('perl-lsp-guidance-remove-after-update-');
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const update = jest.fn(async () => {
    workspaceMock.workspaceFolders = undefined;
  });
  mountWorkspace(workspaceDir, ['lib']);
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn(() => ['lib']),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

  expect(update).toHaveBeenCalled();
  expect(globalState.update).not.toHaveBeenCalledWith(
    expect.stringContaining('includePathsSuggestion'),
    undefined,
  );
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    'Added include paths for workspace: src.',
  );
  expect(vscode.window.showWarningMessage).not.toHaveBeenCalledWith(
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
