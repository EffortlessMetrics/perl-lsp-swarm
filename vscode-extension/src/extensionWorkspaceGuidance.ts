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

interface CoverageResult {
  readonly covered: boolean;
  readonly complete: boolean;
}

interface DiscoveryFinding {
  readonly folder: vscode.WorkspaceFolder;
  readonly folderUri: string;
  readonly rootRealPath: string;
  readonly includePaths: string[];
  readonly includePathsFingerprint: string;
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

function errorCode(error: unknown): string | undefined {
  return error !== null && typeof error === 'object' && 'code' in error
    ? String((error as NodeJS.ErrnoException).code)
    : undefined;
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

async function realpathIfExists(targetPath: string): Promise<string | undefined> {
  try {
    return await fs.promises.realpath(targetPath);
  } catch (error: unknown) {
    if (errorCode(error) === 'ENOENT') {
      return undefined;
    }
    throw error;
  }
}

function includePathsFingerprint(includePaths: readonly string[]): string {
  return crypto.createHash('sha256').update(JSON.stringify(includePaths)).digest('hex');
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
  return scheduleGuidance(validationRuns, context, 'include-path validation', () =>
    runIncludePathValidation(context),
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
    const defaultPaths = new Set<string>([
      '.',
      ...(inspected?.defaultValue ?? DEFAULT_INCLUDE_PATHS),
    ]);
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

    const firstMissing = missingPaths[0];
    if (firstMissing === undefined) {
      continue;
    }
    const relativeNote = path.isAbsolute(firstMissing)
      ? 'absolute path'
      : `relative to ${folder.name}`;
    const suffix =
      missingPaths.length > 1 ? ` ${missingPaths.length} include paths are missing.` : '';

    const choice = await vscode.window.showWarningMessage(
      `Perl LSP: configured include path "${firstMissing}" (${relativeNote}) does not exist.${suffix}`,
      'Open Settings',
    );

    if (choice === 'Open Settings') {
      void vscode.commands.executeCommand(
        'workbench.action.openSettings',
        '@ext:EffortlessMetrics.perl-lsp-rs perl-lsp.includePaths',
      );
    }
    await context.globalState.update(cacheKey, missingSignature);
  }
}

async function canonicalCoverage(
  workspaceRoot: string,
  configuredPaths: readonly string[],
  candidate: string,
): Promise<CoverageResult> {
  try {
    const candidateRealPath = await realpathIfExists(path.resolve(workspaceRoot, candidate));
    if (!candidateRealPath) {
      return { covered: false, complete: false };
    }

    for (const configured of configuredPaths) {
      const configuredRealPath = await realpathIfExists(path.resolve(workspaceRoot, configured));
      if (!configuredRealPath) {
        continue;
      }
      if (isWithinBasePath(configuredRealPath, candidateRealPath)) {
        return { covered: true, complete: true };
      }
    }
    return { covered: false, complete: true };
  } catch {
    return { covered: false, complete: false };
  }
}

async function hasConfiguredDescendant(
  workspaceRoot: string,
  configuredPaths: readonly string[],
  candidateRealPath: string,
): Promise<boolean> {
  for (const configured of configuredPaths) {
    const configuredRealPath = await realpathIfExists(path.resolve(workspaceRoot, configured));
    if (configuredRealPath && isWithinBasePath(candidateRealPath, configuredRealPath)) {
      return true;
    }
  }
  return false;
}

/** True when a current canonical configured root is equal to or an ancestor of the candidate. */
export async function isIncludePathCandidateCovered(
  workspaceRoot: string,
  configuredPaths: readonly string[],
  candidate: string,
): Promise<boolean> {
  return (await canonicalCoverage(workspaceRoot, configuredPaths, candidate)).covered;
}

async function directoryContainsPerlModule(
  dir: string,
  maxDepth = DISCOVERY_MAX_DEPTH,
  entryBudget = DISCOVERY_ENTRY_BUDGET,
): Promise<DiscoveryScanResult> {
  const state = { remaining: entryBudget, visited: 0, complete: true };

  const walk = async (current: string, depth: number): Promise<boolean> => {
    if (state.remaining <= 0) {
      state.complete = false;
      return false;
    }

    const entries: fs.Dirent[] = [];
    try {
      const directory = await fs.promises.opendir(current);
      try {
        while (state.remaining > 0) {
          const entry = await directory.read();
          if (entry === null) {
            break;
          }
          state.remaining -= 1;
          state.visited += 1;
          entries.push(entry);
        }
      } finally {
        if (state.remaining <= 0) {
          state.complete = false;
        }
        await directory.close();
      }
    } catch {
      state.complete = false;
      return false;
    }

    for (const entry of entries) {
      if (entry.isFile() && entry.name.endsWith('.pm')) {
        return true;
      }
    }

    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name.startsWith('.')) {
        continue;
      }
      if (depth >= maxDepth) {
        state.complete = false;
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
export function suggestDiscoveredIncludePaths(context: vscode.ExtensionContext): Promise<void> {
  return scheduleGuidance(discoveryRuns, context, 'include-path discovery', async () => {
    await validationRuns.get(context);
    await runDiscoveredIncludePathGuidance(context);
  });
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
    let rootRealPath: string;
    try {
      rootRealPath = await fs.promises.realpath(folder.uri.fsPath);
    } catch {
      reports.push({ folder: folder.name, discovered: [], complete: false });
      continue;
    }

    for (const candidate of DISCOVERY_CANDIDATE_DIRS) {
      const resolved = path.resolve(folder.uri.fsPath, candidate);
      const candidateRealPath = await realpathIfExists(resolved);
      if (!candidateRealPath || !isWithinBasePath(rootRealPath, candidateRealPath)) {
        continue;
      }
      try {
        const stat = await fs.promises.stat(candidateRealPath);
        if (!stat.isDirectory()) {
          continue;
        }
      } catch (error: unknown) {
        if (errorCode(error) !== 'ENOENT') {
          complete = false;
        }
        continue;
      }

      const coverage = await canonicalCoverage(folder.uri.fsPath, includePaths, candidate);
      complete = complete && coverage.complete;
      if (coverage.covered) {
        continue;
      }
      if (await hasConfiguredDescendant(folder.uri.fsPath, includePaths, candidateRealPath)) {
        continue;
      }

      const scan = await directoryContainsPerlModule(candidateRealPath);
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
      .update(
        JSON.stringify({
          folderUri: folder.uri.toString(),
          rootRealPath,
          includePathsFingerprint: includePathsFingerprint(includePaths),
          complete,
          discovered: discovered.slice().sort(),
        }),
      )
      .digest('hex');
    const cacheKey = `perl-lsp.includePathsSuggestion.${encodeURIComponent(folder.uri.toString())}`;
    if (context.globalState.get<string | undefined>(cacheKey) === signature) {
      continue;
    }

    findings.push({
      folder,
      folderUri: folder.uri.toString(),
      rootRealPath,
      includePaths: [...includePaths],
      includePathsFingerprint: includePathsFingerprint(includePaths),
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
    const stale: string[] = [];
    for (const finding of findings) {
      const currentFolder = vscode.workspace.workspaceFolders?.find(
        (folder) => folder === finding.folder && folder.uri.toString() === finding.folderUri,
      );
      if (!currentFolder) {
        stale.push(finding.folder.name);
        continue;
      }

      let currentRootRealPath: string;
      try {
        currentRootRealPath = await fs.promises.realpath(currentFolder.uri.fsPath);
      } catch {
        stale.push(finding.folder.name);
        continue;
      }
      const currentConfig = vscode.workspace.getConfiguration('perl-lsp', currentFolder.uri);
      const currentIncludePaths: string[] = currentConfig.get('includePaths', [
        ...DEFAULT_INCLUDE_PATHS,
      ]);
      if (
        currentRootRealPath !== finding.rootRealPath ||
        includePathsFingerprint(currentIncludePaths) !== finding.includePathsFingerprint
      ) {
        stale.push(finding.folder.name);
        continue;
      }

      const currentDiscovered: string[] = [];
      for (const candidate of finding.discovered) {
        const candidateRealPath = await realpathIfExists(
          path.resolve(currentFolder.uri.fsPath, candidate),
        );
        if (!candidateRealPath || !isWithinBasePath(currentRootRealPath, candidateRealPath)) {
          stale.push(finding.folder.name);
          continue;
        }
        const stat = await fs.promises.stat(candidateRealPath).catch(() => undefined);
        if (!stat?.isDirectory()) {
          stale.push(finding.folder.name);
          continue;
        }
        currentDiscovered.push(candidate);
      }
      if (currentDiscovered.length === 0) {
        continue;
      }
      const next = Array.from(new Set([...currentIncludePaths, ...currentDiscovered]));
      try {
        await currentConfig.update(
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
    if (stale.length > 0) {
      void vscode.window.showWarningMessage(
        `Perl LSP: include-path suggestions for ${stale.join(', ')} changed before they could be applied. Run the guidance again.`,
      );
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
