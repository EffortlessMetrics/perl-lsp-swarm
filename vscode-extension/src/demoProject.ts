import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

const MAX_TEMPLATE_FILES = 128;
const MAX_TEMPLATE_BYTES = 4 * 1024 * 1024;
const DEMO_ENTRY_POINT = 'main.pl';
const DEMO_METADATA_FILE = '.perl-lsp-demo-template.json';
const DEMO_METADATA_SCHEMA = 'perl-lsp-demo-template.v1';

interface TemplateEntry {
  readonly relativePath: string;
  readonly bytes: Buffer;
  readonly digest: string;
}

interface TemplateSnapshot {
  readonly digest: string;
  readonly entries: readonly TemplateEntry[];
}

interface DemoMetadata {
  readonly schema: typeof DEMO_METADATA_SCHEMA;
  readonly extensionVersion: string;
  readonly templateDigest: string;
}

export type DemoProjectPreparation =
  | {
      readonly kind: 'created' | 'existing';
      readonly destination: string;
      readonly templateDigest: string;
    }
  | {
      readonly kind: 'failed';
      readonly reason: string;
    };

function safeVersion(value: unknown): string {
  const raw = typeof value === 'string' && value.trim() ? value.trim() : 'current';
  return raw.replace(/[^A-Za-z0-9._-]+/g, '-').slice(0, 64) || 'current';
}

function isWithinBase(base: string, candidate: string): boolean {
  const relative = path.relative(base, candidate);
  return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

function metadataFor(snapshot: TemplateSnapshot, extensionVersion: string): DemoMetadata {
  return {
    schema: DEMO_METADATA_SCHEMA,
    extensionVersion,
    templateDigest: snapshot.digest,
  };
}

function parseMetadata(value: string): DemoMetadata {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw new Error('existing demo metadata is not valid JSON');
  }
  if (parsed === null || typeof parsed !== 'object') {
    throw new Error('existing demo metadata has the wrong shape');
  }
  const record = parsed as Record<string, unknown>;
  if (
    record.schema !== DEMO_METADATA_SCHEMA ||
    typeof record.extensionVersion !== 'string' ||
    typeof record.templateDigest !== 'string'
  ) {
    throw new Error('existing demo metadata is incomplete or incompatible');
  }
  return {
    schema: DEMO_METADATA_SCHEMA,
    extensionVersion: record.extensionVersion,
    templateDigest: record.templateDigest,
  };
}

async function collectTemplate(templateRoot: string): Promise<TemplateSnapshot> {
  const rootStat = await fs.promises.lstat(templateRoot);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) {
    throw new Error('packaged demo template is not a regular directory');
  }

  const entries: TemplateEntry[] = [];
  let totalBytes = 0;

  const walk = async (current: string): Promise<void> => {
    const children = await fs.promises.readdir(current, { withFileTypes: true });
    children.sort((left, right) => left.name.localeCompare(right.name));

    for (const child of children) {
      const absolute = path.join(current, child.name);
      const relative = path.relative(templateRoot, absolute);
      if (!relative || !isWithinBase(templateRoot, absolute)) {
        throw new Error('packaged demo member escaped the template root');
      }

      const stat = await fs.promises.lstat(absolute);
      if (stat.isSymbolicLink()) {
        throw new Error(`packaged demo member is a symbolic link: ${relative}`);
      }
      if (stat.isDirectory()) {
        await walk(absolute);
        continue;
      }
      if (!stat.isFile()) {
        throw new Error(`packaged demo member is not a regular file: ${relative}`);
      }
      if (entries.length >= MAX_TEMPLATE_FILES) {
        throw new Error(`packaged demo exceeds ${MAX_TEMPLATE_FILES} files`);
      }

      const bytes = await fs.promises.readFile(absolute);
      totalBytes += bytes.length;
      if (totalBytes > MAX_TEMPLATE_BYTES) {
        throw new Error(`packaged demo exceeds ${MAX_TEMPLATE_BYTES} bytes`);
      }
      entries.push({
        relativePath: relative.split(path.sep).join('/'),
        bytes,
        digest: crypto.createHash('sha256').update(bytes).digest('hex'),
      });
    }
  };

  await walk(templateRoot);
  if (!entries.some((entry) => entry.relativePath === DEMO_ENTRY_POINT)) {
    throw new Error(`packaged demo is missing ${DEMO_ENTRY_POINT}`);
  }

  const digest = crypto
    .createHash('sha256')
    .update(
      entries
        .map((entry) => `${entry.relativePath}\0${entry.bytes.length}\0${entry.digest}\n`)
        .join(''),
    )
    .digest('hex');
  return { digest, entries };
}

async function validateExistingDestination(
  destination: string,
  snapshot: TemplateSnapshot,
  extensionVersion: string,
): Promise<void> {
  const destinationStat = await fs.promises.lstat(destination);
  if (!destinationStat.isDirectory() || destinationStat.isSymbolicLink()) {
    throw new Error('existing demo destination is not a regular directory');
  }

  const entryPoint = path.join(destination, DEMO_ENTRY_POINT);
  const entryStat = await fs.promises.lstat(entryPoint);
  if (!entryStat.isFile() || entryStat.isSymbolicLink()) {
    throw new Error(`existing demo destination has no regular ${DEMO_ENTRY_POINT}`);
  }

  const metadataPath = path.join(destination, DEMO_METADATA_FILE);
  const metadataStat = await fs.promises.lstat(metadataPath);
  if (!metadataStat.isFile() || metadataStat.isSymbolicLink()) {
    throw new Error('existing demo destination has no regular template metadata');
  }
  const metadata = parseMetadata(await fs.promises.readFile(metadataPath, 'utf8'));
  if (
    metadata.schema !== DEMO_METADATA_SCHEMA ||
    metadata.extensionVersion !== extensionVersion ||
    metadata.templateDigest !== snapshot.digest
  ) {
    throw new Error('existing demo destination belongs to another template identity');
  }
}

async function writeTemplateSnapshot(
  staging: string,
  snapshot: TemplateSnapshot,
  extensionVersion: string,
): Promise<void> {
  for (const entry of snapshot.entries) {
    const relativeParts = entry.relativePath.split('/');
    const destination = path.join(staging, ...relativeParts);
    if (!isWithinBase(staging, destination)) {
      throw new Error(`demo member escaped the staging directory: ${entry.relativePath}`);
    }

    await fs.promises.mkdir(path.dirname(destination), { recursive: true });
    await fs.promises.writeFile(destination, entry.bytes, { flag: 'wx' });

    const copied = await fs.promises.readFile(destination);
    const copiedDigest = crypto.createHash('sha256').update(copied).digest('hex');
    if (copiedDigest !== entry.digest) {
      throw new Error(`demo copy verification failed for ${entry.relativePath}`);
    }
  }

  const metadataPath = path.join(staging, DEMO_METADATA_FILE);
  const metadataBytes = `${JSON.stringify(metadataFor(snapshot, extensionVersion), null, 2)}\n`;
  await fs.promises.writeFile(metadataPath, metadataBytes, { flag: 'wx' });
}

async function destinationExists(destination: string): Promise<boolean> {
  try {
    await fs.promises.lstat(destination);
    return true;
  } catch (error: unknown) {
    if (
      error !== null &&
      typeof error === 'object' &&
      'code' in error &&
      (error as NodeJS.ErrnoException).code === 'ENOENT'
    ) {
      return false;
    }
    throw error;
  }
}

/**
 * Materialize one persistent user-owned demo project without mutating packaged
 * extension assets or overwriting an existing edited copy.
 *
 * The current implementation still relies on path-based Node filesystem calls
 * while populating nested staging directories. The PR remains draft until the
 * exact no-follow/reparse-safe destination writer required by #14456 is wired.
 */
export async function prepareUserOwnedDemoProject(
  context: vscode.ExtensionContext,
): Promise<DemoProjectPreparation> {
  const templateRoot = path.join(context.extensionPath, 'assets', 'demo-project');
  try {
    const snapshot = await collectTemplate(templateRoot);
    const storageRoot = context.globalStorageUri.fsPath;
    await fs.promises.mkdir(storageRoot, { recursive: true });
    const storageRealPath = await fs.promises.realpath(storageRoot);

    const demosRoot = path.join(storageRealPath, 'demo-projects');
    await fs.promises.mkdir(demosRoot, { recursive: true });
    const demosRealPath = await fs.promises.realpath(demosRoot);
    if (!isWithinBase(storageRealPath, demosRealPath)) {
      throw new Error('demo storage root escaped extension-owned user storage');
    }

    const version = safeVersion(context.extension.packageJSON.version);
    const destination = path.join(demosRealPath, `perl-lsp-demo-${version}-${snapshot.digest}`);
    if (!isWithinBase(demosRealPath, destination)) {
      throw new Error('demo destination escaped the user-owned demo root');
    }

    if (await destinationExists(destination)) {
      await validateExistingDestination(destination, snapshot, version);
      return { kind: 'existing', destination, templateDigest: snapshot.digest };
    }

    const staging = await fs.promises.mkdtemp(path.join(demosRealPath, '.demo-stage-'));
    try {
      await writeTemplateSnapshot(staging, snapshot, version);
      await validateExistingDestination(staging, snapshot, version);
      try {
        await fs.promises.rename(staging, destination);
      } catch (error: unknown) {
        const code =
          error !== null && typeof error === 'object' && 'code' in error
            ? (error as NodeJS.ErrnoException).code
            : undefined;
        if (code !== 'EEXIST' && code !== 'ENOTEMPTY') {
          throw error;
        }
        await fs.promises.rm(staging, { recursive: true, force: true });
        await validateExistingDestination(destination, snapshot, version);
        return { kind: 'existing', destination, templateDigest: snapshot.digest };
      }
      return { kind: 'created', destination, templateDigest: snapshot.digest };
    } catch (error: unknown) {
      await fs.promises.rm(staging, { recursive: true, force: true });
      throw error;
    }
  } catch (error: unknown) {
    return {
      kind: 'failed',
      reason: error instanceof Error ? error.message : String(error),
    };
  }
}

/** Open the verified user-owned copy and record success only after VS Code accepts the command. */
export async function openUserOwnedDemoProject(context: vscode.ExtensionContext): Promise<void> {
  const prepared = await prepareUserOwnedDemoProject(context);
  if (prepared.kind === 'failed') {
    void vscode.window.showErrorMessage(
      `Perl LSP: demo project could not be prepared: ${prepared.reason}`,
    );
    return;
  }

  void vscode.window.showInformationMessage(
    prepared.kind === 'created'
      ? 'Opening a user-owned copy of the Perl demo project.'
      : 'Opening your existing Perl demo project copy.',
  );

  try {
    await vscode.commands.executeCommand(
      'vscode.openFolder',
      vscode.Uri.file(prepared.destination),
      { forceNewWindow: true },
    );
  } catch (error: unknown) {
    void vscode.window.showErrorMessage(
      `Perl LSP: the demo project was prepared but could not be opened: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
    return;
  }

  await context.globalState.update(`perl-lsp.demoProjectOpened.${prepared.templateDigest}`, true);
}
