import { randomUUID } from 'crypto';
import * as fs from 'fs';
import * as path from 'path';

/**
 * Shared containment for extension-performed writes under a selected workspace
 * folder (#7461). Coordinate with `vscode.extension-owned-write`; tree
 * materialization under global storage is owned by #14488.
 */
export const EXTENSION_OWNED_WORKSPACE_WRITE_POLICY_ID = 'vscode.extension-owned-write';

export class ExtensionOwnedWorkspaceWriteError extends Error {
  readonly workspaceRelativePath: string;

  constructor(workspaceRelativePath: string, message: string) {
    super(message);
    this.name = 'ExtensionOwnedWorkspaceWriteError';
    this.workspaceRelativePath = workspaceRelativePath;
  }
}

export interface SafeWorkspaceRegularFileTarget {
  readonly canonicalWorkspaceRoot: string;
  readonly targetPath: string;
  readonly parentPath: string;
  readonly workspaceRelativePath: string;
}

interface TargetIdentity {
  dev: number;
  ino: number;
  size: number;
  mtimeMs: number;
}

export type WorkspaceRegularFileIdentity = TargetIdentity;

export type WorkspaceRegularFileReplaceMode = 'create-only' | 'overwrite' | 'match-existing';

export function displayWorkspaceRelativePath(workspaceRoot: string, absolutePath: string): string {
  const relative = path.relative(workspaceRoot, absolutePath);
  if (relative === '') {
    return '.';
  }
  return relative.split(path.sep).join('/');
}

/**
 * Returns true when a regular file exists at a verified in-workspace path.
 * Symlinks, special files, and paths that resolve outside the workspace return false.
 */
export function workspaceRegularFileExists(workspaceRoot: string, targetPath: string): boolean {
  try {
    const prepared = prepareWorkspaceRegularFileTargetSync(workspaceRoot, targetPath);
    const stat = fs.lstatSync(prepared.targetPath);
    if (stat.isSymbolicLink() || !stat.isFile()) {
      return false;
    }
    const canonicalTarget = fs.realpathSync(prepared.targetPath);
    return isPathContained(prepared.canonicalWorkspaceRoot, canonicalTarget);
  } catch (error) {
    if (isNodeError(error, 'ENOENT')) {
      return false;
    }
    if (error instanceof ExtensionOwnedWorkspaceWriteError) {
      return false;
    }
    return false;
  }
}

export async function ensureWorkspaceDirectoryPath(
  workspaceRoot: string,
  directoryPath: string,
): Promise<void> {
  const lexicalWorkspaceRoot = path.resolve(workspaceRoot);
  const resolvedDirectory = path.resolve(directoryPath);
  const workspaceRelativePath = displayWorkspaceRelativePath(
    lexicalWorkspaceRoot,
    resolvedDirectory,
  );
  if (
    !isPathContained(lexicalWorkspaceRoot, resolvedDirectory) ||
    resolvedDirectory === lexicalWorkspaceRoot
  ) {
    throw new ExtensionOwnedWorkspaceWriteError(
      workspaceRelativePath,
      `Cannot create directory outside the workspace: ${workspaceRelativePath}`,
    );
  }

  const canonicalWorkspaceRoot = await fs.promises.realpath(lexicalWorkspaceRoot);
  const parentRelative = path.relative(lexicalWorkspaceRoot, resolvedDirectory);
  const segments = parentRelative.split(path.sep).filter((segment) => segment.length > 0);
  let current = lexicalWorkspaceRoot;

  for (const segment of segments) {
    current = path.join(current, segment);
    let stat: fs.Stats;
    try {
      stat = await fs.promises.lstat(current);
    } catch (error) {
      if (!isNodeError(error, 'ENOENT')) {
        throw error;
      }
      await fs.promises.mkdir(current);
      stat = await fs.promises.lstat(current);
    }

    rejectReparsePoint(
      current,
      displayWorkspaceRelativePath(lexicalWorkspaceRoot, current),
      stat,
    );
    if (!stat.isDirectory()) {
      throw new ExtensionOwnedWorkspaceWriteError(
        workspaceRelativePath,
        `Workspace path is not a directory: ${displayWorkspaceRelativePath(lexicalWorkspaceRoot, current)}`,
      );
    }

    const canonicalCurrent = await fs.promises.realpath(current);
    if (!isPathContained(canonicalWorkspaceRoot, canonicalCurrent)) {
      throw new ExtensionOwnedWorkspaceWriteError(
        workspaceRelativePath,
        `Workspace directory escapes the selected folder: ${displayWorkspaceRelativePath(lexicalWorkspaceRoot, current)}`,
      );
    }
  }
}

export async function prepareWorkspaceRegularFileTarget(
  workspaceRoot: string,
  targetPath: string,
): Promise<SafeWorkspaceRegularFileTarget> {
  return prepareWorkspaceRegularFileTargetAsync(workspaceRoot, targetPath);
}

export async function replaceWorkspaceRegularFile(
  workspaceRoot: string,
  targetPath: string,
  content: string,
  options?: {
    mode?: number;
    replace?: WorkspaceRegularFileReplaceMode;
    observedIdentity?: WorkspaceRegularFileIdentity | null;
  },
): Promise<void> {
  const target = await prepareWorkspaceRegularFileTargetAsync(workspaceRoot, targetPath);
  const mode = options?.mode ?? 0o644;
  const replace = options?.replace ?? 'overwrite';
  const observedIdentity = options?.observedIdentity;
  const existing =
    replace === 'match-existing' && observedIdentity === undefined
      ? await readExistingRegularFile(target)
      : await peekExistingRegularFile(target);
  if (replace === 'create-only' && existing) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace file already exists: ${target.workspaceRelativePath}`,
    );
  }
  const expectedIdentity =
    replace === 'match-existing'
      ? (observedIdentity !== undefined ? observedIdentity : (existing?.identity ?? null))
      : null;
  await atomicReplacePreparedRegularFile(
    target,
    content,
    existing?.mode ?? mode,
    replace,
    expectedIdentity,
  );
}

function prepareWorkspaceRegularFileTargetSync(
  workspaceRoot: string,
  targetPath: string,
): SafeWorkspaceRegularFileTarget {
  const lexicalWorkspaceRoot = path.resolve(workspaceRoot);
  const resolvedTarget = path.resolve(targetPath);
  const workspaceRelativePath = displayWorkspaceRelativePath(lexicalWorkspaceRoot, resolvedTarget);
  if (
    !isPathContained(lexicalWorkspaceRoot, resolvedTarget) ||
    resolvedTarget === lexicalWorkspaceRoot
  ) {
    throw new ExtensionOwnedWorkspaceWriteError(
      workspaceRelativePath,
      `Cannot write outside the workspace: ${workspaceRelativePath}`,
    );
  }

  const canonicalWorkspaceRoot = fs.realpathSync(lexicalWorkspaceRoot);
  const parentPath = path.dirname(resolvedTarget);
  const parentRelative = path.relative(lexicalWorkspaceRoot, parentPath);
  const segments = parentRelative.split(path.sep).filter((segment) => segment.length > 0);
  let current = lexicalWorkspaceRoot;

  for (const segment of segments) {
    current = path.join(current, segment);
    let stat: fs.Stats;
    try {
      stat = fs.lstatSync(current);
    } catch (error) {
      if (!isNodeError(error, 'ENOENT')) {
        throw error;
      }
      throw new ExtensionOwnedWorkspaceWriteError(
        workspaceRelativePath,
        `Missing parent directory for workspace write: ${displayWorkspaceRelativePath(lexicalWorkspaceRoot, current)}`,
      );
    }

    rejectReparsePoint(
      current,
      displayWorkspaceRelativePath(lexicalWorkspaceRoot, current),
      stat,
    );
    if (!stat.isDirectory()) {
      throw new ExtensionOwnedWorkspaceWriteError(
        workspaceRelativePath,
        `Workspace parent is not a directory: ${displayWorkspaceRelativePath(lexicalWorkspaceRoot, current)}`,
      );
    }

    const canonicalCurrent = fs.realpathSync(current);
    if (!isPathContained(canonicalWorkspaceRoot, canonicalCurrent)) {
      throw new ExtensionOwnedWorkspaceWriteError(
        workspaceRelativePath,
        `Workspace parent escapes the selected folder: ${displayWorkspaceRelativePath(lexicalWorkspaceRoot, current)}`,
      );
    }
  }

  return {
    canonicalWorkspaceRoot,
    targetPath: resolvedTarget,
    parentPath,
    workspaceRelativePath,
  };
}

async function prepareWorkspaceRegularFileTargetAsync(
  workspaceRoot: string,
  targetPath: string,
): Promise<SafeWorkspaceRegularFileTarget> {
  const lexicalWorkspaceRoot = path.resolve(workspaceRoot);
  const resolvedTarget = path.resolve(targetPath);
  const workspaceRelativePath = displayWorkspaceRelativePath(lexicalWorkspaceRoot, resolvedTarget);
  if (
    !isPathContained(lexicalWorkspaceRoot, resolvedTarget) ||
    resolvedTarget === lexicalWorkspaceRoot
  ) {
    throw new ExtensionOwnedWorkspaceWriteError(
      workspaceRelativePath,
      `Cannot write outside the workspace: ${workspaceRelativePath}`,
    );
  }

  const canonicalWorkspaceRoot = await fs.promises.realpath(lexicalWorkspaceRoot);
  const parentPath = path.dirname(resolvedTarget);
  await ensureWorkspaceDirectoryPath(lexicalWorkspaceRoot, parentPath);

  return {
    canonicalWorkspaceRoot,
    targetPath: resolvedTarget,
    parentPath,
    workspaceRelativePath,
  };
}

interface ExistingRegularFile {
  mode: number;
  identity: TargetIdentity;
}

async function peekExistingRegularFile(
  target: SafeWorkspaceRegularFileTarget,
): Promise<ExistingRegularFile | null> {
  let before: fs.Stats;
  try {
    before = await fs.promises.lstat(target.targetPath);
  } catch (error) {
    if (isNodeError(error, 'ENOENT')) {
      return null;
    }
    throw error;
  }

  rejectReparsePoint(target.targetPath, target.workspaceRelativePath, before);
  if (!before.isFile()) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace destination is not a regular file: ${target.workspaceRelativePath}`,
    );
  }

  const canonicalTarget = await fs.promises.realpath(target.targetPath);
  if (!isPathContained(target.canonicalWorkspaceRoot, canonicalTarget)) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace file escapes the selected folder: ${target.workspaceRelativePath}`,
    );
  }

  return { mode: before.mode & 0o777, identity: toTargetIdentity(before) };
}

async function readExistingRegularFile(
  target: SafeWorkspaceRegularFileTarget,
): Promise<ExistingRegularFile | null> {
  const existing = await peekExistingRegularFile(target);
  if (!existing) {
    return null;
  }

  const handle = await fs.promises.open(
    target.targetPath,
    fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW,
  );
  try {
    const after = await handle.stat();
    if (!after.isFile() || !sameFileIdentity(await fs.promises.lstat(target.targetPath), after)) {
      throw new ExtensionOwnedWorkspaceWriteError(
        target.workspaceRelativePath,
        `Workspace file changed during validation: ${target.workspaceRelativePath}`,
      );
    }
    return existing;
  } finally {
    await handle.close();
  }
}

async function atomicReplacePreparedRegularFile(
  target: SafeWorkspaceRegularFileTarget,
  content: string,
  mode: number,
  replace: WorkspaceRegularFileReplaceMode,
  expectedIdentity: TargetIdentity | null,
): Promise<void> {
  await revalidatePreparedParent(target);
  await assertPreparedTargetUnchanged(target, replace, expectedIdentity);

  const temporaryPath = path.join(
    target.parentPath,
    `.${path.basename(target.targetPath)}.${process.pid}.${randomUUID()}.tmp`,
  );
  let handle: fs.promises.FileHandle | undefined;
  try {
    handle = await fs.promises.open(
      temporaryPath,
      fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_NOFOLLOW,
      mode,
    );
    await handle.writeFile(content, { encoding: 'utf8' });
    await handle.sync();
    await handle.close();
    handle = undefined;

    await revalidatePreparedParent(target);
    await assertPreparedTargetUnchanged(target, replace, expectedIdentity);
    await fs.promises.rename(temporaryPath, target.targetPath);
  } finally {
    if (handle) {
      await handle.close().catch(() => undefined);
    }
    await fs.promises.rm(temporaryPath, { force: true }).catch(() => undefined);
  }
}

async function revalidatePreparedParent(target: SafeWorkspaceRegularFileTarget): Promise<void> {
  const stat = await fs.promises.lstat(target.parentPath);
  rejectReparsePoint(target.parentPath, target.workspaceRelativePath, stat);
  if (!stat.isDirectory()) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace parent changed during validation: ${displayWorkspaceRelativePath(path.dirname(target.canonicalWorkspaceRoot), target.parentPath)}`,
    );
  }
  const canonicalParent = await fs.promises.realpath(target.parentPath);
  if (!isPathContained(target.canonicalWorkspaceRoot, canonicalParent)) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace parent escapes the selected folder during validation: ${target.workspaceRelativePath}`,
    );
  }
}

async function assertPreparedTargetUnchanged(
  target: SafeWorkspaceRegularFileTarget,
  replace: WorkspaceRegularFileReplaceMode,
  expected: TargetIdentity | null,
): Promise<void> {
  let stat: fs.Stats;
  try {
    stat = await fs.promises.lstat(target.targetPath);
  } catch (error) {
    if (isNodeError(error, 'ENOENT')) {
      if (expected) {
        throw new ExtensionOwnedWorkspaceWriteError(
          target.workspaceRelativePath,
          `Workspace file was removed during validation: ${target.workspaceRelativePath}`,
        );
      }
      return;
    }
    throw error;
  }

  rejectReparsePoint(target.targetPath, target.workspaceRelativePath, stat);
  if (!stat.isFile()) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace destination is not a regular file: ${target.workspaceRelativePath}`,
    );
  }

  const canonicalTarget = await fs.promises.realpath(target.targetPath);
  if (!isPathContained(target.canonicalWorkspaceRoot, canonicalTarget)) {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace file escapes the selected folder: ${target.workspaceRelativePath}`,
    );
  }

  if (replace === 'create-only') {
    throw new ExtensionOwnedWorkspaceWriteError(
      target.workspaceRelativePath,
      `Workspace file appeared during validation: ${target.workspaceRelativePath}`,
    );
  }

  if (replace === 'match-existing') {
    if (!expected) {
      throw new ExtensionOwnedWorkspaceWriteError(
        target.workspaceRelativePath,
        `Workspace file appeared during validation: ${target.workspaceRelativePath}`,
      );
    }
    if (!sameTargetIdentity(expected, toTargetIdentity(stat))) {
      throw new ExtensionOwnedWorkspaceWriteError(
        target.workspaceRelativePath,
        `Workspace file changed during validation: ${target.workspaceRelativePath}`,
      );
    }
  }
}

function rejectReparsePoint(
  _absolutePath: string,
  workspaceRelativePath: string,
  stat: fs.Stats,
): void {
  if (stat.isSymbolicLink()) {
    throw new ExtensionOwnedWorkspaceWriteError(
      workspaceRelativePath,
      `Workspace path is a symlink or reparse point: ${workspaceRelativePath}`,
    );
  }
  if (!stat.isFile() && !stat.isDirectory()) {
    throw new ExtensionOwnedWorkspaceWriteError(
      workspaceRelativePath,
      `Workspace path is not a regular file or directory: ${workspaceRelativePath}`,
    );
  }
}

function toTargetIdentity(stat: fs.Stats): TargetIdentity {
  return { dev: stat.dev, ino: stat.ino, size: stat.size, mtimeMs: stat.mtimeMs };
}

function sameTargetIdentity(left: TargetIdentity, right: TargetIdentity): boolean {
  return (
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.size === right.size &&
    left.mtimeMs === right.mtimeMs
  );
}

function sameFileIdentity(left: fs.Stats, right: fs.Stats): boolean {
  return left.dev === right.dev && left.ino === right.ino;
}

function isPathContained(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return (
    relative === '' ||
    (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))
  );
}

function isNodeError(error: unknown, code: string): error is NodeJS.ErrnoException {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    (error as { code?: unknown }).code === code
  );
}
