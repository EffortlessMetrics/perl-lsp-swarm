import * as fs from 'fs';
import * as path from 'path';

const EXT_ROOT = path.resolve(__dirname, '..', '..');
const REPO_ROOT = path.resolve(EXT_ROOT, '..');
const SHARED_SETTINGS = path.join(REPO_ROOT, '.vscode', 'settings.json');
const LOCAL_CONTRACT = path.join(
  REPO_ROOT,
  'docs',
  'contributing',
  'VS_CODE_LOCAL_SERVER.md',
);
const GITIGNORE = path.join(REPO_ROOT, '.gitignore');
const LOCAL_WORKSPACE = '.tmp/perl-lsp-swarm.local.code-workspace';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readText(filePath: string): string {
  return fs.readFileSync(filePath, 'utf8');
}

function readRecord(filePath: string): Record<string, unknown> {
  const value = JSON.parse(readText(filePath)) as unknown;
  if (!isRecord(value)) {
    throw new Error(`${filePath} must contain one JSON object`);
  }
  return value;
}

function collectStrings(value: unknown): string[] {
  if (typeof value === 'string') {
    return [value];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item) => collectStrings(item));
  }
  if (isRecord(value)) {
    return Object.values(value).flatMap((item) => collectStrings(item));
  }
  return [];
}

function localWorkspaceExample(markdown: string): Record<string, unknown> {
  const match = /<!-- local-workspace-example:start -->\s*```json\s*([\s\S]*?)\s*```\s*<!-- local-workspace-example:end -->/.exec(
    markdown,
  );
  if (match?.[1] === undefined) {
    throw new Error('local workspace example markers are missing');
  }

  const value = JSON.parse(match[1]) as unknown;
  if (!isRecord(value)) {
    throw new Error('local workspace example must contain one JSON object');
  }
  return value;
}

describe('checked-in repository workspace settings', () => {
  test('remain portable and product-neutral', () => {
    const settings = readRecord(SHARED_SETTINGS);
    const forbiddenLifecycleKeys = [
      'perl-lsp.serverPath',
      'perl-lsp.autoDownload',
      'perl-lsp.downloadBaseUrl',
      'perl-lsp.versionTag',
      'perl-lsp.includePaths',
      'perl-lsp.externalIncludePaths',
    ];

    for (const key of forbiddenLifecycleKeys) {
      expect(Object.prototype.hasOwnProperty.call(settings, key)).toBe(false);
    }

    for (const value of collectStrings(settings)) {
      expect(path.posix.isAbsolute(value)).toBe(false);
      expect(path.win32.isAbsolute(value)).toBe(false);
      expect(value).not.toMatch(/\/path\/to|__REPLACE_WITH_|https?:\/\//i);
    }
  });

  test('does not contain comment-shaped duplicate keys', () => {
    const settings = readRecord(SHARED_SETTINGS);
    expect(Object.keys(settings).some((key) => key.startsWith('//'))).toBe(false);
  });
});

describe('explicit local server override contract', () => {
  const contract = readText(LOCAL_CONTRACT);

  test('uses an ignored opt-in workspace file with a valid example', () => {
    const ignoreLines = readText(GITIGNORE)
      .split(/\r?\n/)
      .map((line) => line.trim());
    expect(ignoreLines).toContain('/.tmp/');
    expect(contract).toContain(LOCAL_WORKSPACE);

    const example = localWorkspaceExample(contract);
    const folders = example.folders;
    expect(Array.isArray(folders)).toBe(true);
    const firstFolder = Array.isArray(folders) ? folders[0] : undefined;
    expect(isRecord(firstFolder) ? firstFolder.path : undefined).toBe('..');

    const settings = example.settings;
    expect(isRecord(settings)).toBe(true);
    expect(isRecord(settings) ? settings['perl-lsp.serverPath'] : undefined).toBe(
      '__REPLACE_WITH_ABSOLUTE_PERLLSP_PATH__',
    );
    expect(isRecord(settings) ? settings['perl-lsp.autoDownload'] : undefined).toBe(false);
  });

  test('states candidate identity and reset boundaries', () => {
    expect(contract).toContain('perllsp --version');
    expect(contract).toContain('checkout SHA');
    expect(contract).toContain('do not make it a public release');
    expect(contract).toContain('reopen the repository folder');
    expect(contract).toContain('normal installed-product lifecycle');
  });
});
