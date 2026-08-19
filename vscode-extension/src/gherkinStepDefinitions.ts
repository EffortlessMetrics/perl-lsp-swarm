import { randomUUID } from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { isPotentiallyExpensiveRegex } from './gherkinRedosGuard';

const CREATE_STEP_DEFINITION_COMMAND = 'perl-lsp.createGherkinStepDefinition';
const GHERKIN_STEP_RE = /^\s*(Given|When|Then|And|But)\b\s*(.*)$/;
const STEP_DEFINITION_RE = /^\s*(Given|When|Then|And|But)\s+qr\//;
const QUOTED_CAPTURE_RE = /"[^"\r\n]*"/y;
const OUTLINE_PLACEHOLDER_RE = /<[^>\r\n]+>/y;
const DEFAULT_STEP_DEFINITION_GLOB = '**/*.pm';
const DEFAULT_EXCLUDE_GLOB = '{**/node_modules/**,**/blib/**}';
const MAX_STEP_DEFINITION_FILES = 500;
const MAX_STEP_DEFINITION_FILE_BYTES = 512 * 1024;
const MAX_STEP_DEFINITION_TOTAL_BYTES = 16 * 1024 * 1024;
const MAX_MATCH_REGEX_LENGTH = 256;
const MAX_MATCH_STEP_TEXT_LENGTH = 512;
// Rejecting ReDoS-shaped patterns bounds the cost of any single match, not the
// number of matches. An accepted 16 MiB workspace can still hold hundreds of
// thousands of individually linear-time step definitions, so the population
// itself gets a budget. Ordinary suites are three orders of magnitude below it.
const MAX_MATCH_ATTEMPTS = 20_000;
// Catastrophic backtracking (ReDoS) requires a *quantified group that itself
// contains a quantifier, a backreference, a lookaround, or alternation. A
// single character class
// followed by one quantifier (`[^"]+`, `[0-9]+`) is linear-time and safe —
// flagging it produced false "ambiguous" classifications for ordinary step
// definitions, including the `"([^"]+)"` patterns this module generates itself
// (see buildGeneratedStepPattern).
// The shared parser and denylist live in gherkinRedosGuard.ts.
export type StepKeyword = 'Given' | 'When' | 'Then' | 'And' | 'But';
export type StepDefinitionStatus = 'defined' | 'undefined' | 'ambiguous';

export interface GherkinStepLine {
  keyword: StepKeyword;
  text: string;
  line: number;
  rawLine: string;
}

export interface ExtractedStepDefinition {
  keyword: StepKeyword;
  pattern: string;
}

export interface StepDefinitionScan {
  definitions: ExtractedStepDefinition[];
  ambiguous: boolean;
}

interface CreateStepDefinitionArgs {
  featureUri: string;
  line: number;
}

interface TargetIdentity {
  dev: number;
  ino: number;
  size: number;
  mtimeMs: number;
}

interface ExistingTarget {
  text: string;
  mode: number;
  identity: TargetIdentity;
}

interface SafeTarget {
  canonicalWorkspaceRoot: string;
  targetPath: string;
  parentPath: string;
}

export function registerGherkinStepDefinitionSupport(): vscode.Disposable[] {
  const selector: vscode.DocumentSelector = [{ language: 'gherkin' }];

  const provider: vscode.CodeActionProvider = {
    async provideCodeActions(document, range) {
      // Generating a step definition writes to the workspace, so it is not
      // offered while the workspace is untrusted.
      if (!vscode.workspace.isTrusted) {
        return [];
      }
      return provideGherkinStepDefinitionActions(document, range);
    },
  };

  return [
    vscode.languages.registerCodeActionsProvider(selector, provider),
    vscode.commands.registerCommand(
      CREATE_STEP_DEFINITION_COMMAND,
      async (args: CreateStepDefinitionArgs) => {
        await createStepDefinitionFromFeature(args);
      },
    ),
  ];
}

export function parseGherkinStepLine(lineText: string, line: number): GherkinStepLine | null {
  const match = lineText.match(GHERKIN_STEP_RE);
  if (!match) {
    return null;
  }

  const text = match[2]?.trim();
  if (!text) {
    return null;
  }

  return {
    keyword: match[1] as StepKeyword,
    text,
    line,
    rawLine: lineText,
  };
}

export function buildGeneratedStepPattern(stepText: string): string {
  let pattern = '^';
  let cursor = 0;

  while (cursor < stepText.length) {
    QUOTED_CAPTURE_RE.lastIndex = cursor;
    const quoted = QUOTED_CAPTURE_RE.exec(stepText);
    if (quoted && quoted.index === cursor) {
      pattern += '"([^"]+)"';
      cursor += quoted[0].length;
      continue;
    }

    OUTLINE_PLACEHOLDER_RE.lastIndex = cursor;
    const placeholder = OUTLINE_PLACEHOLDER_RE.exec(stepText);
    if (placeholder && placeholder.index === cursor) {
      pattern += '([^\\r\\n]+)';
      cursor += placeholder[0].length;
      continue;
    }

    const character = stepText[cursor];
    if (character === undefined) {
      break;
    }

    pattern += escapeRegexLiteral(character);
    cursor += 1;
  }

  pattern += '$';
  return pattern;
}

export function buildGeneratedStepStub(step: GherkinStepLine, relativeFeaturePath: string): string {
  const safeFeaturePath = sanitizeGeneratedComment(relativeFeaturePath);
  return [
    `# Auto-generated from ${safeFeaturePath}:${step.line + 1}`,
    `${step.keyword} qr/${buildGeneratedStepPattern(step.text)}/, sub {`,
    '    # TODO: implement step',
    '    return;',
    '};',
  ].join('\n');
}

export function buildStepDefinitionFileContent(
  step: GherkinStepLine,
  relativeFeaturePath: string,
): string {
  return [
    'use Test::BDD::Cucumber::StepFile;',
    'use strict;',
    'use warnings;',
    '',
    buildGeneratedStepStub(step, relativeFeaturePath),
    '',
  ].join('\n');
}

export function suggestStepDefinitionPath(featurePath: string, workspaceRoot: string): string {
  const normalisedFeaturePath = path.normalize(featurePath);
  const featureBasename = path.basename(normalisedFeaturePath, '.feature');
  const safeStem = featureBasename
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase();
  const filename = `${safeStem || 'generated'}_steps.pm`;
  const featureParts = normalisedFeaturePath.split(path.sep);
  const featuresIndex = featureParts.lastIndexOf('features');

  if (featuresIndex !== -1) {
    const featuresRoot = featureParts.slice(0, featuresIndex + 1).join(path.sep) || path.sep;
    return path.join(featuresRoot, 'step_definitions', filename);
  }

  return path.join(workspaceRoot, 'features', 'step_definitions', filename);
}

export function scanStepDefinitions(source: string): StepDefinitionScan {
  const definitions: ExtractedStepDefinition[] = [];
  let ambiguous = false;

  for (const line of source.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed.match(/^(Given|When|Then|And|But)\b/)) {
      continue;
    }

    if (!STEP_DEFINITION_RE.test(trimmed)) {
      ambiguous = true;
      continue;
    }

    const match = trimmed.match(/^(Given|When|Then|And|But)\s+qr\//);
    if (!match) {
      ambiguous = true;
      continue;
    }

    const pattern = extractSlashDelimitedPattern(trimmed, match[0].length - 1);
    if (!pattern) {
      ambiguous = true;
      continue;
    }

    definitions.push({
      keyword: match[1] as StepKeyword,
      pattern,
    });
  }

  return { definitions, ambiguous };
}

export function classifyStepDefinitionStatus(
  step: GherkinStepLine,
  sources: string[],
): StepDefinitionStatus {
  let ambiguous = false;
  let attempts = 0;

  for (const source of sources) {
    const scan = scanStepDefinitions(source);
    ambiguous = ambiguous || scan.ambiguous;

    for (const definition of scan.definitions) {
      if (attempts >= MAX_MATCH_ATTEMPTS) {
        // The population was never fully tested, so "undefined" would be a
        // claim this scan cannot support. Report the uncertainty instead; the
        // ambiguous path declines to generate rather than writing a stub that
        // may duplicate an untested definition.
        return 'ambiguous';
      }
      attempts += 1;

      const matches = testExtractedDefinition(definition, step.text);
      if (matches === true) {
        return 'defined';
      }
      if (matches === null) {
        ambiguous = true;
      }
    }
  }

  return ambiguous ? 'ambiguous' : 'undefined';
}

async function provideGherkinStepDefinitionActions(
  document: vscode.TextDocument,
  range: vscode.Range,
): Promise<vscode.CodeAction[]> {
  const step = parseGherkinStepLine(document.lineAt(range.start.line).text, range.start.line);
  if (!step) {
    return [];
  }

  const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (!workspaceFolder) {
    return [];
  }

  const sources = await collectWorkspaceStepDefinitionSources(workspaceFolder);
  const status = classifyStepDefinitionStatus(step, sources);
  if (status !== 'undefined') {
    return [];
  }

  const targetPath = suggestStepDefinitionPath(document.uri.fsPath, workspaceFolder.uri.fsPath);
  const action = {
    title: `Create step definition stub in ${vscode.workspace.asRelativePath(vscode.Uri.file(targetPath))}`,
    kind: vscode.CodeActionKind.QuickFix,
    command: {
      command: CREATE_STEP_DEFINITION_COMMAND,
      title: 'Create step definition stub',
      arguments: [{ featureUri: document.uri.toString(), line: step.line }],
    },
  } as vscode.CodeAction;

  return [action];
}

async function createStepDefinitionFromFeature(args: CreateStepDefinitionArgs): Promise<void> {
  const featureUri = vscode.Uri.parse(args.featureUri);
  const document = await vscode.workspace.openTextDocument(featureUri);
  const step = parseGherkinStepLine(document.lineAt(args.line).text, args.line);
  if (!step) {
    void vscode.window.showWarningMessage('No Gherkin step found on the selected line.');
    return;
  }

  const workspaceFolder = vscode.workspace.getWorkspaceFolder(featureUri);
  if (!workspaceFolder) {
    void vscode.window.showWarningMessage(
      'Step definition generation requires an open workspace folder.',
    );
    return;
  }

  const sources = await collectWorkspaceStepDefinitionSources(workspaceFolder);
  const status = classifyStepDefinitionStatus(step, sources);
  if (status === 'defined') {
    void vscode.window.showInformationMessage(
      `A matching step definition already exists for "${step.text}".`,
    );
    return;
  }
  if (status === 'ambiguous') {
    void vscode.window.showWarningMessage(
      'Step definition generation is unavailable because existing step definitions could not be matched confidently.',
    );
    return;
  }

  const targetPath = suggestStepDefinitionPath(featureUri.fsPath, workspaceFolder.uri.fsPath);
  const relativeFeaturePath = vscode.workspace.asRelativePath(featureUri);

  try {
    await writeGeneratedStepDefinitionFile(
      workspaceFolder.uri.fsPath,
      targetPath,
      buildStepDefinitionFileContent(step, relativeFeaturePath),
      buildGeneratedStepStub(step, relativeFeaturePath),
    );
  } catch (error) {
    void vscode.window.showErrorMessage(
      `Could not write the generated step definition: ${error instanceof Error ? error.message : String(error)}`,
    );
    return;
  }

  const targetDocument = await vscode.workspace.openTextDocument(vscode.Uri.file(targetPath));
  await vscode.window.showTextDocument(targetDocument);
}

// Exported for the containment proof in gherkinSecurity.test.ts: the scan
// bounds are a security claim and need a direct seam, not one observed through
// the code-action provider.
export async function collectWorkspaceStepDefinitionSources(
  workspaceFolder: vscode.WorkspaceFolder,
): Promise<string[]> {
  const files = await vscode.workspace.findFiles(
    DEFAULT_STEP_DEFINITION_GLOB,
    DEFAULT_EXCLUDE_GLOB,
    MAX_STEP_DEFINITION_FILES,
  );
  const workspacePrefix = ensureTrailingSeparator(workspaceFolder.uri.fsPath);
  const candidateFiles = files.filter((uri) =>
    ensureTrailingSeparator(uri.fsPath).startsWith(workspacePrefix),
  );

  // Read sequentially under a global byte envelope. The previous concurrent
  // read had no per-file or aggregate bound, so a workspace could hold the
  // extension host open on arbitrarily large step-definition candidates.
  const sources: string[] = [];
  let acceptedBytes = 0;

  for (const uri of candidateFiles) {
    if (acceptedBytes >= MAX_STEP_DEFINITION_TOTAL_BYTES) {
      break;
    }

    const read = await readBoundedFile(uri.fsPath, MAX_STEP_DEFINITION_FILE_BYTES);
    if (!read) {
      continue;
    }
    if (acceptedBytes + read.byteLength > MAX_STEP_DEFINITION_TOTAL_BYTES) {
      break;
    }

    acceptedBytes += read.byteLength;

    if (
      !uri.fsPath.includes(`${path.sep}step_definitions${path.sep}`) &&
      !read.text.includes('Test::BDD::Cucumber::StepFile')
    ) {
      continue;
    }

    sources.push(read.text);
  }

  return sources;
}

/**
 * Read at most `limit` bytes from a regular file, allocating no more than
 * `limit + 1` bytes regardless of how the file changes after it is opened.
 *
 * Deciding on `lstat().size` and then calling `readFile` does not bound the
 * read: a workspace process can grow or replace the file in between, and
 * `readFile` allocates whatever is actually there. The size is therefore taken
 * from the already-open descriptor and enforced by the read itself. Returns
 * `null` for anything that is not a readable regular file within the limit,
 * including a symlink, which `O_NOFOLLOW` rejects.
 */
async function readBoundedFile(
  filePath: string,
  limit: number,
): Promise<{ text: string; byteLength: number } | null> {
  let handle: fs.promises.FileHandle;
  try {
    handle = await fs.promises.open(filePath, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
  } catch {
    return null;
  }

  try {
    const stat = await handle.stat();
    if (!stat.isFile()) {
      return null;
    }

    // One byte past the limit distinguishes "exactly at the limit" from
    // "larger than the limit" without reading the remainder of the file.
    const buffer = Buffer.allocUnsafe(limit + 1);
    let filled = 0;
    while (filled < buffer.length) {
      const { bytesRead } = await handle.read(buffer, filled, buffer.length - filled, filled);
      if (bytesRead === 0) {
        break;
      }
      filled += bytesRead;
    }

    if (filled > limit) {
      return null;
    }

    return { text: buffer.subarray(0, filled).toString('utf8'), byteLength: filled };
  } catch {
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
}

function ensureTrailingSeparator(value: string): string {
  return value.endsWith(path.sep) ? value : `${value}${path.sep}`;
}

function escapeRegexLiteral(text: string): string {
  return text.replace(/[|\\{}()[\]^$+*?.]/g, '\\$&').replace(/\//g, '\\/');
}

function extractSlashDelimitedPattern(line: string, delimiterIndex: number): string | null {
  let pattern = '';
  let escaped = false;

  for (let index = delimiterIndex + 1; index < line.length; index += 1) {
    const char = line[index];
    if (escaped) {
      pattern += char;
      escaped = false;
      continue;
    }

    if (char === '\\') {
      pattern += char;
      escaped = true;
      continue;
    }

    if (char === '/') {
      return pattern;
    }

    pattern += char;
  }

  return null;
}

function testExtractedDefinition(
  definition: ExtractedStepDefinition,
  stepText: string,
): boolean | null {
  if (!isSafeRegexForStepMatching(definition.pattern, stepText)) {
    return null;
  }

  try {
    return new RegExp(definition.pattern).test(stepText);
  } catch {
    return null;
  }
}

function isSafeRegexForStepMatching(source: string, stepText: string): boolean {
  if (source.length > MAX_MATCH_REGEX_LENGTH || stepText.length > MAX_MATCH_STEP_TEXT_LENGTH) {
    return false;
  }

  return !isPotentiallyExpensiveRegex(source);
}

/**
 * Create or append a generated step definition without following a workspace
 * symlink outside the trusted workspace root.
 */
export async function writeGeneratedStepDefinitionFile(
  workspaceRoot: string,
  targetPath: string,
  createContent: string,
  appendStub: string,
): Promise<void> {
  const safeTarget = await prepareSafeTarget(workspaceRoot, targetPath);
  const existing = await readExistingTarget(safeTarget);
  const nextContent = existing
    ? `${existing.text.replace(/\s*$/, '')}${existing.text.trimEnd().length === 0 ? '' : '\n\n'}${appendStub}\n`
    : createContent;

  if (Buffer.byteLength(nextContent, 'utf8') > MAX_STEP_DEFINITION_FILE_BYTES) {
    throw new Error('generated step definition exceeds the file-size limit');
  }

  await atomicReplaceTarget(
    safeTarget,
    nextContent,
    existing?.mode ?? 0o600,
    existing?.identity ?? null,
  );
}

async function prepareSafeTarget(workspaceRoot: string, targetPath: string): Promise<SafeTarget> {
  const lexicalWorkspaceRoot = path.resolve(workspaceRoot);
  const resolvedTarget = path.resolve(targetPath);
  if (
    !isPathContained(lexicalWorkspaceRoot, resolvedTarget) ||
    resolvedTarget === lexicalWorkspaceRoot
  ) {
    throw new Error('generated step-definition path escapes the workspace');
  }

  const canonicalWorkspaceRoot = await fs.promises.realpath(lexicalWorkspaceRoot);
  const parentPath = path.dirname(resolvedTarget);
  const parentRelative = path.relative(lexicalWorkspaceRoot, parentPath);
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

    if (stat.isSymbolicLink()) {
      throw new Error(`generated step-definition parent is a symlink: ${current}`);
    }
    if (!stat.isDirectory()) {
      throw new Error(`generated step-definition parent is not a directory: ${current}`);
    }

    const canonicalCurrent = await fs.promises.realpath(current);
    if (!isPathContained(canonicalWorkspaceRoot, canonicalCurrent)) {
      throw new Error(`generated step-definition parent escapes the workspace: ${current}`);
    }
  }

  return { canonicalWorkspaceRoot, targetPath: resolvedTarget, parentPath };
}

async function readExistingTarget(target: SafeTarget): Promise<ExistingTarget | null> {
  let before: fs.Stats;
  try {
    before = await fs.promises.lstat(target.targetPath);
  } catch (error) {
    if (isNodeError(error, 'ENOENT')) {
      return null;
    }
    throw error;
  }

  if (before.isSymbolicLink()) {
    throw new Error('generated step-definition target is a symlink');
  }
  if (!before.isFile()) {
    throw new Error('generated step-definition target is not a regular file');
  }
  if (before.size > MAX_STEP_DEFINITION_FILE_BYTES) {
    throw new Error('existing step-definition file exceeds the file-size limit');
  }

  const canonicalTarget = await fs.promises.realpath(target.targetPath);
  if (!isPathContained(target.canonicalWorkspaceRoot, canonicalTarget)) {
    throw new Error('generated step-definition target escapes the workspace');
  }

  const handle = await fs.promises.open(
    target.targetPath,
    fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW,
  );
  try {
    const after = await handle.stat();
    if (!after.isFile() || !sameFileIdentity(before, after)) {
      throw new Error('generated step-definition target changed during validation');
    }
    const text = await handle.readFile({ encoding: 'utf8' });
    if (Buffer.byteLength(text, 'utf8') > MAX_STEP_DEFINITION_FILE_BYTES) {
      throw new Error('existing step-definition file exceeds the file-size limit');
    }
    return { text, mode: before.mode & 0o777, identity: toTargetIdentity(after) };
  } finally {
    await handle.close();
  }
}

async function atomicReplaceTarget(
  target: SafeTarget,
  content: string,
  mode: number,
  expected: TargetIdentity | null,
): Promise<void> {
  await revalidateSafeParent(target);
  await assertTargetUnchanged(target.targetPath, expected);

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

    await revalidateSafeParent(target);
    await assertTargetUnchanged(target.targetPath, expected);
    await fs.promises.rename(temporaryPath, target.targetPath);
  } finally {
    if (handle) {
      await handle.close().catch(() => undefined);
    }
    await fs.promises.rm(temporaryPath, { force: true }).catch(() => undefined);
  }
}

async function revalidateSafeParent(target: SafeTarget): Promise<void> {
  const stat = await fs.promises.lstat(target.parentPath);
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    throw new Error('generated step-definition parent changed during validation');
  }
  const canonicalParent = await fs.promises.realpath(target.parentPath);
  if (!isPathContained(target.canonicalWorkspaceRoot, canonicalParent)) {
    throw new Error('generated step-definition parent escapes the workspace');
  }
}

/**
 * Refuse the rename unless the target is still exactly what the append content
 * was derived from.
 *
 * `expected` is the identity observed while reading the existing file, or
 * `null` when the write is a create. Proving only that the current path holds
 * some regular file is not enough: an editor save or another workspace process
 * between the read and the rename would have its bytes silently discarded, and
 * a create would clobber a file that appeared in the meantime. This narrows
 * that window to the interval between this check and `rename`, which POSIX
 * gives no way to close; any change observed here aborts the write instead.
 */
async function assertTargetUnchanged(
  targetPath: string,
  expected: TargetIdentity | null,
): Promise<void> {
  let stat: fs.Stats;
  try {
    stat = await fs.promises.lstat(targetPath);
  } catch (error) {
    if (isNodeError(error, 'ENOENT')) {
      if (expected) {
        throw new Error('generated step-definition target was removed during validation');
      }
      return;
    }
    throw error;
  }

  if (stat.isSymbolicLink()) {
    throw new Error('generated step-definition target is a symlink');
  }
  if (!stat.isFile()) {
    throw new Error('generated step-definition target is not a regular file');
  }
  if (!expected) {
    throw new Error('generated step-definition target appeared during validation');
  }
  if (!sameTargetIdentity(expected, toTargetIdentity(stat))) {
    throw new Error('generated step-definition target changed during validation');
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

function sanitizeGeneratedComment(value: string): string {
  return value.replace(/[\r\n\u2028\u2029]+/g, ' ').trim();
}

// Deliberately structural rather than `instanceof Error`: fs rejections are
// constructed in Node's realm and cross a module boundary before reaching
// here, so an identity check on the Error constructor can be false for a
// genuine ENOENT and turn "create the missing directory" into a hard failure.
function isNodeError(error: unknown, code: string): error is NodeJS.ErrnoException {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    (error as { code?: unknown }).code === code
  );
}
