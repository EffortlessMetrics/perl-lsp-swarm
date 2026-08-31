import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

const DEFAULT_INCLUDE_PATHS = ['lib', 'local/lib/perl5'] as const;
const DISCOVERY_CANDIDATE_DIRS = ['src', 'local', 'vendor', 'lib', 't/lib', 'blib/lib', 'modules'];
const DISCOVERY_ENTRY_BUDGET = 200;
const DISCOVERY_MAX_DEPTH = 2;

const validationRuns = new WeakMap<vscode.ExtensionContext, Promise<void>>();
const discoveryRuns = new WeakMap<vscode.ExtensionContext, Promise<void>>();

interface DiscoveryScanResult {
  readonly found: boolean;
  readonly complete: boolean;
  readonly visited: number;
}

interface DiscoveryFinding {
  readonly folder: vscode.WorkspaceFolder;
  readonly config: vscode.WorkspaceConfiguration;
  readonly includePaths: string[];
  readonly discovered: string[];
  readonly complete: boolean;
  readonly cacheKey: string;
  readonly signature: string;
}

export interface IncludePathDiscoveryReport {
  readonly folder: string;
  readonly discovered: readonly string[];
  readonly complete: boolean;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function hasErrorCode(error: unknown, code: string): boolean {
  return (
    error !== null &&
    typeof error === 'object' &&
    'code' in error &&
    (error as NodeJS.ErrnoException).code === code
  );
}

function isWithinBasePath(basePath: string, targetPath: string): boolean {
  const relative = path.relative(basePath, targetPath);
  return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

async function pathExists(targetPath: string): Promise<boolean> {
  try {
    await fs.promises.access(targetPath);
    return true;
  } catch {
    return false;
  }
}

async function nearestExistingPath(candidatePath: string): Promise<string | undefined> {
  let current = candidatePath;
  while (true) {
    try {
      await fs.promises.lstat(current);
      return current;
    } catch (error: unknown) {
      if (!hasErrorCode(error, 'ENOENT')) {
        throw error;
      }
    }

    const parent = path.dirname(current);
    if (parent === current) {
      return undefined;
    }
    current = parent;
  }
}

async function isSafeCreatableRelativePath(
  workspacePath: string,
  workspaceRealPath: string,
  includePath: string,
): Promise<boolean> {
  if (path.isAbsolute(includePath)) {
    return false;
  }

  const targetPath = path.resolve(workspacePath, includePath);
  if (!isWithinBasePath(workspacePath, targetPath) || targetPath === workspacePath) {
    return false;
  }

  try {
    const existing = await nearestExistingPath(targetPath);
    if (!existing) {
      return false;
    }
    const existingRealPath = await fs.promises.realpath(existing);
    return isWithinBasePath(workspaceRealPath, existingRealPath);
  } catch {
    return false;
  }
}

async function createSafeRelativeDirectory(
  workspacePath: string,
  workspaceRealPath: string,
  includePath: string,
): Promise<boolean> {
  if (!(await isSafeCreatableRelativePath(workspacePath, workspaceRealPath, includePath))) {
    throw new Error('path is not contained by the current workspace folder');
  }

  const targetPath = path.resolve(workspacePath, includePath);
  const relative = path.relative(workspacePath, targetPath);
  const segments = relative.split(path.sep).filter(Boolean);
  let current = workspacePath;
  let created = false;

  for (const segment of segments) {
    const next = path.join(current, segment);
    try {
      await fs.promises.mkdir(next);
      created = true;
    } catch (error: unknown) {
      if (!hasErrorCode(error, 'EEXIST')) {
        throw error;
      }
    }

    const stat = await fs.promises.lstat(next);
    if (stat.isSymbolicLink() || !stat.isDirectory()) {
      throw new Error('path component is not a regular directory');
    }
    const realPath = await fs.promises.realpath(next);
    if (!isWithinBasePath(workspaceRealPath, realPath)) {
      throw new Error('path escaped the current workspace folder');
    }
    current = next;
  }

  return created;
}

function scheduleGuidance(
  runs: WeakMap<vscode.ExtensionContext, Promise<void>>,
  context: vscode.ExtensionContext,
  label: string,
  task: () => Promise<unknown>,
): Promise<void> {
  if (runs.has(context)) {
    return Promise.resolve();
  }

  let pending: Promise<void>;
  pending = task()
    .then(() => undefined)
    .catch((error: unknown) => {
      void vscode.window.showWarningMessage(`Perl LSP: ${label} failed: ${errorMessage(error)}`);
    })
    .finally(() => {
      if (runs.get(context) === pending) {
        runs.delete(context);
      }
    });
  runs.set(context, pending);
  return Promise.resolve();
}

/**
 * Schedule include-path validation without keeping language-client startup or
 * active-document readiness behind optional filesystem guidance.
 */
export function validateIncludePaths(context: vscode.ExtensionContext): Promise<void> {
  return scheduleGuidance(
    validationRuns,
    context,
    'include-path validation',
    () => runIncludePathValidation(context),
  );
}

/** Execute the validation pass to completion for tests and explicit callers. */
export async function runIncludePathValidation(context: vscode.ExtensionContext): Promise<void> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders || workspaceFolders.length === 0) {
    return;
  }

  for (const folder of workspaceFolders) {
    const cacheKey = `perl-lsp.includePathsWarning.${encodeURIComponent(folder.uri.toString())}`;
    const config = vscode.workspace.getConfiguration('perl-lsp', folder.uri);
    const includePaths: string[] = config.get('includePaths', [...DEFAULT_INCLUDE_PATHS]);

    const inspected =
      typeof config.inspect === 'function' ? config.inspect<string[]>('includePaths') : undefined;
    const defaultPaths = new Set<string>(['.', ...(inspected?.defaultValue ?? DEFAULT_INCLUDE_PATHS)]);
    const missingPaths: string[] = [];

    for (const includePath of includePaths) {
      if (defaultPaths.has(includePath)) {
        continue;
      }
      if (!(await pathExists(path.resolve(folder.uri.fsPath, includePath)))) {
        missingPaths.push(includePath);
      }
    }

    if (missingPaths.length === 0) {
      await context.globalState.update(cacheKey, undefined);
      continue;
    }

    const missingSignature = missingPaths.slice().sort().join('\n');
    if (context.globalState.get<string | undefined>(cacheKey) === missingSignature) {
      continue;
    }

    let workspaceRealPath: string;
    try {
      workspaceRealPath = await fs.promises.realpath(folder.uri.fsPath);
    } catch {
      continue;
    }

    const creatablePaths: string[] = [];
    for (const includePath of missingPaths) {
      if (
        await isSafeCreatableRelativePath(
          folder.uri.fsPath,
          workspaceRealPath,
          includePath,
        )
      ) {
        creatablePaths.push(includePath);
      }
    }

    const firstMissing = missingPaths[0];
    if (firstMissing === undefined) {
      continue;
    }
    const relativeNote = path.isAbsolute(firstMissing)
      ? 'absolute path'
      : `relative to ${folder.name}`;
    const suffix =
      missingPaths.length > 1 ? ` ${missingPaths.length} include paths are missing.` : '';
    const actions = ['Open Settings'];
    if (creatablePaths.length > 0) {
      actions.push('Create Missing Directories');
    }

    const choice = await vscode.window.showWarningMessage(
      `Perl LSP: configured include path "${firstMissing}" (${relativeNote}) does not exist.${suffix}`,
      ...actions,
    );

    if (choice === 'Open Settings') {
      void vscode.commands.executeCommand(
        'workbench.action.openSettings',
        '@ext:EffortlessMetrics.perl-lsp-rs perl-lsp.includePaths',
      );
      await context.globalState.update(cacheKey, missingSignature);
      continue;
    }

    if (choice === 'Create Missing Directories') {
      const createdPaths: string[] = [];
      let creationFailed = false;
      for (const includePath of creatablePaths) {
        try {
          const created = await createSafeRelativeDirectory(
            folder.uri.fsPath,
            workspaceRealPath,
            includePath,
          );
          if (created || (await pathExists(path.resolve(folder.uri.fsPath, includePath)))) {
            createdPaths.push(includePath);
          }
        } catch (error: unknown) {
          creationFailed = true;
          void vscode.window.showWarningMessage(
            `Perl LSP: failed to create directory "${includePath}": ${errorMessage(error)}`,
          );
        }
      }

      if (createdPaths.length > 0) {
        void vscode.window.showInformationMessage(
          `Created ${createdPaths.length} include director${createdPaths.length === 1 ? 'y' : 'ies'}: ${createdPaths.join(', ')}.`,
        );
      }
      if (!creationFailed && createdPaths.length === creatablePaths.length) {
        await context.globalState.update(cacheKey, undefined);
      }
      continue;
    }

    await context.globalState.update(cacheKey, missingSignature);
  }
}

/** True when an existing configured root is equal to or an ancestor of the candidate. */
export function isIncludePathCandidateCovered(
  workspaceRoot: string,
  configuredPaths: readonly string[],
  candidate: string,
): boolean {
  const candidatePath = path.resolve(workspaceRoot, candidate);
  return configuredPaths.some((configured) => {
    const configuredPath = path.resolve(workspaceRoot, configured);
    return isWithinBasePath(configuredPath, candidatePath);
  });
}

async function directoryContainsPerlModule(
  dir: string,
  maxDepth = DISCOVERY_MAX_DEPTH,
  entryBudget = DISCOVERY_ENTRY_BUDGET,
): Promise<DiscoveryScanResult> {
  const state = { remaining: entryBudget, visited: 0, complete: true };

  const walk = async (current: string, depth: number): Promise<boolean> => {
    if (depth > maxDepth) {
      return false;
    }
    if (state.remaining <= 0) {
      state.complete = false;
      return false;
    }

    let entries: fs.Dirent[];
    try {
      entries = await fs.promises.readdir(current, { withFileTypes: true });
    } catch {
      state.complete = false;
      return false;
    }

    for (const entry of entries) {
      if (state.remaining <= 0) {
        state.complete = false;
        return false;
      }
      state.remaining -= 1;
      state.visited += 1;
      if (entry.isFile() && entry.name.endsWith('.pm')) {
        return true;
      }
    }

    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name.startsWith('.')) {
        continue;
      }
      if (await walk(path.join(current, entry.name), depth + 1)) {
        return true;
      }
      if (!state.complete && state.remaining <= 0) {
        return false;
      }
    }
    return false;
  };

  return {
    found: await walk(dir, 0),
    complete: state.complete,
    visited: state.visited,
  };
}

/** Schedule discovery as advisory background work. */
export function suggestDiscoveredIncludePaths(
  context: vscode.ExtensionContext,
): Promise<void> {
  return scheduleGuidance(
    discoveryRuns,
    context,
    'include-path discovery',
    () => runDiscoveredIncludePathGuidance(context),
  );
}

/** Execute the discovery pass to completion for tests and explicit callers. */
export async function runDiscoveredIncludePathGuidance(
  context: vscode.ExtensionContext,
): Promise<readonly IncludePathDiscoveryReport[]> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders || workspaceFolders.length === 0) {
    return [];
  }

  const findings: DiscoveryFinding[] = [];
  const reports: IncludePathDiscoveryReport[] = [];

  for (const folder of workspaceFolders) {
    const config = vscode.workspace.getConfiguration('perl-lsp', folder.uri);
    const includePaths: string[] = config.get('includePaths', [...DEFAULT_INCLUDE_PATHS]);
    const discovered: string[] = [];
    let complete = true;

    for (const candidate of DISCOVERY_CANDIDATE_DIRS) {
      if (isIncludePathCandidateCovered(folder.uri.fsPath, includePaths, candidate)) {
        continue;
      }

      const resolved = path.resolve(folder.uri.fsPath, candidate);
      try {
        const stat = await fs.promises.stat(resolved);
        if (!stat.isDirectory()) {
          continue;
        }
      } catch {
        continue;
      }

      const scan = await directoryContainsPerlModule(resolved);
      complete = complete && scan.complete;
      if (scan.found) {
        discovered.push(candidate);
      }
    }

    reports.push({ folder: folder.name, discovered: [...discovered], complete });
    if (discovered.length === 0) {
      continue;
    }

    const signature = crypto
      .createHash('sha256')
      .update(`${complete ? 'complete' : 'incomplete'}\n${discovered.slice().sort().join('\n')}`)
      .digest('hex');
    const cacheKey = `perl-lsp.includePathsSuggestion.${encodeURIComponent(folder.uri.toString())}`;
    if (context.globalState.get<string | undefined>(cacheKey) === signature) {
      continue;
    }

    findings.push({
      folder,
      config,
      includePaths,
      discovered,
      complete,
      cacheKey,
      signature,
    });
  }

  if (findings.length === 0) {
    return reports;
  }

  const summary = findings
    .map((finding) => `${finding.folder.name}: ${finding.discovered.join(', ')}`)
    .join('; ');
  const incomplete = findings.some((finding) => !finding.complete)
    ? ' The bounded scan was incomplete, so additional paths may exist.'
    : '';
  const choice = await vscode.window.showInformationMessage(
    `Perl LSP: found Perl module roots outside the configured include paths — ${summary}.${incomplete}`,
    'Add for These Folders',
    'Open Settings',
    'Dismiss',
  );

  if (choice === 'Add for These Folders') {
    const applied: string[] = [];
    for (const finding of findings) {
      const next = Array.from(new Set([...finding.includePaths, ...finding.discovered]));
      try {
        await finding.config.update(
          'includePaths',
          next,
          vscode.ConfigurationTarget.WorkspaceFolder,
        );
        await context.globalState.update(finding.cacheKey, finding.signature);
        applied.push(`${finding.folder.name}: ${finding.discovered.join(', ')}`);
      } catch (error: unknown) {
        void vscode.window.showWarningMessage(
          `Perl LSP: could not update include paths for ${finding.folder.name}: ${errorMessage(error)}`,
        );
      }
    }
    if (applied.length > 0) {
      void vscode.window.showInformationMessage(`Added include paths for ${applied.join('; ')}.`);
    }
    return reports;
  }

  if (choice === 'Open Settings') {
    void vscode.commands.executeCommand(
      'workbench.action.openSettings',
      '@ext:EffortlessMetrics.perl-lsp-rs perl-lsp.includePaths',
    );
  }

  for (const finding of findings) {
    await context.globalState.update(finding.cacheKey, finding.signature);
  }
  return reports;
}

export async function suggestAiCompletionIfSupported(
  context: vscode.ExtensionContext,
  client: { initializeResult?: { capabilities?: unknown | undefined } | undefined } | undefined,
): Promise<void> {
  if (!client) {
    return;
  }

  const config = vscode.workspace.getConfiguration('perl-lsp');
  if (config.get<boolean>('aiCompletion.enabled', false)) {
    return;
  }

  const capabilities = client.initializeResult?.capabilities;
  const inlineProvider =
    !!capabilities && typeof capabilities === 'object'
      ? (capabilities as Record<string, unknown>).inlineCompletionProvider
      : undefined;
  if (inlineProvider === undefined || inlineProvider === false || inlineProvider === null) {
    return;
  }

  const stateKey = 'perl-lsp.aiCompletion.firstRunNotificationShown';
  if (context.workspaceState.get<boolean>(stateKey, false)) {
    return;
  }
  await context.workspaceState.update(stateKey, true);

  const choice = await vscode.window.showInformationMessage(
    'Perl LSP: your language server supports AI-powered inline completions. They are off by default — enable them now?',
    'Enable',
    'Learn More',
    'Dismiss',
  );

  if (choice === 'Enable') {
    try {
      await config.update('aiCompletion.enabled', true, vscode.ConfigurationTarget.Global);
      void vscode.window.showInformationMessage('AI-powered inline completions enabled.');
    } catch (error: unknown) {
      void vscode.window.showWarningMessage(
        `Perl LSP: could not enable AI completions: ${errorMessage(error)}`,
      );
    }
  } else if (choice === 'Learn More') {
    void vscode.commands.executeCommand(
      'workbench.action.openSettings',
      '@ext:EffortlessMetrics.perl-lsp-rs perl-lsp.aiCompletion',
    );
  }
}

export { openUserOwnedDemoProject as openDemoProjectCommand } from './demoProject';
