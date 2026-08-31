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

function mountWorkspace(workspaceDir: string, includePaths: string[]): void {
  workspaceMock.workspaceFolders = [
    {
      name: 'workspace',
      uri: { fsPath: workspaceDir, toString: () => `file://${workspaceDir}` },
    },
  ];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((key: string, defaultValue?: unknown) =>
      key === 'includePaths' ? includePaths : defaultValue,
    ),
    inspect: jest.fn(() => ({ defaultValue: ['lib', 'local/lib/perl5'] })),
    update: jest.fn(async () => undefined),
  }));
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

  await runIncludePathValidation({ globalState: makeState() } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showWarningMessage).not.toHaveBeenCalled();
});

test('creates safe missing include directories inside the workspace', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-create-'));
  mountWorkspace(workspaceDir, ['generated/perl']);
  (vscode.window.showWarningMessage as jest.Mock).mockResolvedValue('Create Missing Directories');

  await runIncludePathValidation({ globalState: makeState() } as unknown as vscode.ExtensionContext);

  expect(fs.existsSync(path.join(workspaceDir, 'generated', 'perl'))).toBe(true);
  expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
    'Created 1 include directory: generated/perl.',
  );
});

test('does not offer or perform directory creation through a workspace symlink', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-link-'));
  const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-outside-'));
  const link = path.join(workspaceDir, 'linked');
  try {
    fs.symlinkSync(outsideDir, link, 'dir');
  } catch {
    return;
  }
  mountWorkspace(workspaceDir, ['linked/escape']);
  (vscode.window.showWarningMessage as jest.Mock).mockResolvedValue('Create Missing Directories');

  await runIncludePathValidation({ globalState: makeState() } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.any(String),
    'Open Settings',
  );
  expect(fs.existsSync(path.join(outsideDir, 'escape'))).toBe(false);
});

test('reports directory creation failures without aborting guidance', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-mkdir-'));
  fs.writeFileSync(path.join(workspaceDir, 'blocked'), 'not a directory');
  mountWorkspace(workspaceDir, ['blocked/child']);
  const state = makeState();
  (vscode.window.showWarningMessage as jest.Mock).mockResolvedValue('Create Missing Directories');

  await runIncludePathValidation({ globalState: state } as unknown as vscode.ExtensionContext);
  await runIncludePathValidation({ globalState: state } as unknown as vscode.ExtensionContext);

  expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
    expect.stringContaining('Perl LSP: failed to create directory "blocked/child":'),
  );
  expect(
    (vscode.window.showWarningMessage as jest.Mock).mock.calls.filter(([message]) =>
      String(message).includes('configured include path "blocked/child"'),
    ),
  ).toHaveLength(2);
});

test('configured ancestors cover candidates but configured descendants do not', () => {
  const workspaceDir = path.join(path.sep, 'workspace');

  expect(isIncludePathCandidateCovered(workspaceDir, ['src'], 'src/lib')).toBe(true);
  expect(isIncludePathCandidateCovered(workspaceDir, ['src/lib'], 'src')).toBe(false);
  expect(isIncludePathCandidateCovered(workspaceDir, ['./src'], 'src')).toBe(true);
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

test('retries a discovered-path suggestion after an update failure', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-guidance-retry-'));
  fs.mkdirSync(path.join(workspaceDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(workspaceDir, 'src', 'Module.pm'), 'package Module; 1;\n');
  const globalState = makeState();
  const update = jest.fn(async () => {
    throw new Error('workspace is read-only');
  });
  workspaceMock.workspaceFolders = [
    {
      name: 'workspace',
      uri: { fsPath: workspaceDir, toString: () => `file://${workspaceDir}` },
    },
  ];
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
  workspaceMock.workspaceFolders = [
    {
      name: 'workspace',
      uri: { fsPath: workspaceDir, toString: () => `file://${workspaceDir}` },
    },
  ];
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(() => ({
    get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    inspect: jest.fn(),
    update,
  }));
  (vscode.window.showInformationMessage as jest.Mock).mockResolvedValue('Add for These Folders');

  await runDiscoveredIncludePathGuidance({ globalState } as unknown as vscode.ExtensionContext);

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

test('reports bounded discovery as incomplete rather than a complete empty scan', async () => {
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
  expect(globalState.update).not.toHaveBeenCalledWith(
    expect.stringContaining('perl-lsp.includePathsSuggestion.'),
    expect.any(String),
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
