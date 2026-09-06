import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import { openUserOwnedDemoProject, prepareUserOwnedDemoProject } from '../demoProject';

const METADATA_FILE = '.perl-lsp-demo-template.json';

function makeContext(
  extensionRoot: string,
  storageRoot: string,
): {
  context: vscode.ExtensionContext;
  update: jest.Mock;
} {
  const update = jest.fn(async () => undefined);
  return {
    context: {
      extensionPath: extensionRoot,
      globalStorageUri: { fsPath: storageRoot },
      extension: { packageJSON: { version: '0.18.0' } },
      globalState: { update },
    } as unknown as vscode.ExtensionContext,
    update,
  };
}

function makeTemplate(): { extensionRoot: string; storageRoot: string; templateRoot: string } {
  const extensionRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-demo-extension-'));
  const storageRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-demo-storage-'));
  const templateRoot = path.join(extensionRoot, 'assets', 'demo-project');
  fs.mkdirSync(path.join(templateRoot, 'lib'), { recursive: true });
  fs.writeFileSync(path.join(templateRoot, 'main.pl'), 'use lib "lib";\nprint "demo\\n";\n');
  fs.writeFileSync(path.join(templateRoot, 'lib', 'Demo.pm'), 'package Demo;\n1;\n');
  return { extensionRoot, storageRoot, templateRoot };
}

afterEach(() => {
  jest.clearAllMocks();
});

test('creates a verified persistent copy outside the extension directory', async () => {
  const roots = makeTemplate();
  const { context } = makeContext(roots.extensionRoot, roots.storageRoot);

  const result = await prepareUserOwnedDemoProject(context);

  expect(result.kind).toBe('created');
  if (result.kind === 'failed') return;
  expect(path.relative(roots.extensionRoot, result.destination).startsWith('..')).toBe(true);
  expect(path.relative(roots.storageRoot, result.destination).startsWith('..')).toBe(false);
  expect(result.destination.endsWith(result.templateDigest)).toBe(true);
  expect(fs.readFileSync(path.join(result.destination, 'main.pl'), 'utf8')).toContain('demo');
  expect(fs.readFileSync(path.join(result.destination, 'lib', 'Demo.pm'), 'utf8')).toContain(
    'package Demo',
  );
  const metadata = JSON.parse(
    fs.readFileSync(path.join(result.destination, METADATA_FILE), 'utf8'),
  ) as Record<string, unknown>;
  expect(metadata).toEqual({
    schema: 'perl-lsp-demo-template.v1',
    extensionVersion: '0.18.0',
    templateDigest: result.templateDigest,
  });
});

test('reopens an existing copy without overwriting user edits', async () => {
  const roots = makeTemplate();
  const { context } = makeContext(roots.extensionRoot, roots.storageRoot);
  const first = await prepareUserOwnedDemoProject(context);
  expect(first.kind).toBe('created');
  if (first.kind === 'failed') return;

  const edited = '# user work\n';
  fs.writeFileSync(path.join(first.destination, 'main.pl'), edited);
  const second = await prepareUserOwnedDemoProject(context);

  expect(second).toEqual({
    kind: 'existing',
    destination: first.destination,
    templateDigest: first.templateDigest,
  });
  expect(fs.readFileSync(path.join(first.destination, 'main.pl'), 'utf8')).toBe(edited);
});

test('rejects a predictable existing directory without the immutable template marker', async () => {
  const roots = makeTemplate();
  const { context } = makeContext(roots.extensionRoot, roots.storageRoot);
  const first = await prepareUserOwnedDemoProject(context);
  expect(first.kind).toBe('created');
  if (first.kind === 'failed') return;

  fs.rmSync(first.destination, { recursive: true, force: true });
  fs.mkdirSync(first.destination, { recursive: true });
  fs.writeFileSync(path.join(first.destination, 'main.pl'), 'foreign content\n');

  const second = await prepareUserOwnedDemoProject(context);

  expect(second.kind).toBe('failed');
  if (second.kind === 'failed') {
    expect(second.reason).toContain('template metadata');
  }
});

test('rejects an existing copy whose full template identity was changed', async () => {
  const roots = makeTemplate();
  const { context } = makeContext(roots.extensionRoot, roots.storageRoot);
  const first = await prepareUserOwnedDemoProject(context);
  expect(first.kind).toBe('created');
  if (first.kind === 'failed') return;

  fs.writeFileSync(
    path.join(first.destination, METADATA_FILE),
    `${JSON.stringify({
      schema: 'perl-lsp-demo-template.v1',
      extensionVersion: '0.18.0',
      templateDigest: `${first.templateDigest.slice(0, 12)}${'0'.repeat(52)}`,
    })}\n`,
  );

  const second = await prepareUserOwnedDemoProject(context);

  expect(second.kind).toBe('failed');
  if (second.kind === 'failed') {
    expect(second.reason).toContain('another template identity');
  }
});

test('rejects symbolic-link members in the packaged template', async () => {
  const roots = makeTemplate();
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-demo-outside-'));
  fs.writeFileSync(path.join(outside, 'secret.txt'), 'secret');
  try {
    fs.symlinkSync(
      path.join(outside, 'secret.txt'),
      path.join(roots.templateRoot, 'linked.txt'),
      'file',
    );
  } catch {
    return;
  }
  const { context } = makeContext(roots.extensionRoot, roots.storageRoot);

  const result = await prepareUserOwnedDemoProject(context);

  expect(result.kind).toBe('failed');
  if (result.kind === 'failed') {
    expect(result.reason).toContain('symbolic link');
  }
});

test('records success only after the user-owned folder open is accepted', async () => {
  const roots = makeTemplate();
  const { context, update } = makeContext(roots.extensionRoot, roots.storageRoot);
  const events: string[] = [];
  update.mockImplementation(async () => {
    events.push('state');
  });
  (vscode.commands.executeCommand as jest.Mock).mockImplementation(async (_command, target) => {
    events.push('open');
    expect(target.fsPath.startsWith(roots.storageRoot)).toBe(true);
    expect(target.fsPath.startsWith(roots.extensionRoot)).toBe(false);
  });

  await openUserOwnedDemoProject(context);

  expect(events).toEqual(['open', 'state']);
  expect(update).toHaveBeenCalledWith(
    expect.stringMatching(/^perl-lsp\.demoProjectOpened\.[a-f0-9]{64}$/),
    true,
  );
});

test('an open failure leaves the demo-opened state unset', async () => {
  const roots = makeTemplate();
  const { context, update } = makeContext(roots.extensionRoot, roots.storageRoot);
  (vscode.commands.executeCommand as jest.Mock).mockRejectedValue(new Error('window rejected'));

  await openUserOwnedDemoProject(context);

  expect(update).not.toHaveBeenCalled();
  expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
    expect.stringContaining('could not be opened: window rejected'),
  );
});

test('copying the demo never mutates packaged template bytes', async () => {
  const roots = makeTemplate();
  const { context } = makeContext(roots.extensionRoot, roots.storageRoot);
  const before = fs.readFileSync(path.join(roots.templateRoot, 'main.pl'));

  const result = await prepareUserOwnedDemoProject(context);
  expect(result.kind).not.toBe('failed');
  if (result.kind === 'failed') return;
  fs.writeFileSync(path.join(result.destination, 'main.pl'), '# changed copy\n');

  expect(fs.readFileSync(path.join(roots.templateRoot, 'main.pl'))).toEqual(before);
});
