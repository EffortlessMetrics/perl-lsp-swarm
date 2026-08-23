import * as fs from 'fs';
import * as path from 'path';

const EXT_ROOT = path.resolve(__dirname, '..', '..');
const REPO_ROOT = path.resolve(EXT_ROOT, '..');
const EXTENSION_MANIFEST = path.join(EXT_ROOT, 'package.json');
const SHARED_SETTINGS = path.join(REPO_ROOT, '.vscode', 'settings.json');
const LOCAL_CONTRACT = path.join(REPO_ROOT, 'docs', 'contributing', 'VS_CODE_LOCAL_SERVER.md');

/**
 * Resource-scoped keys that are legal in workspace settings but must still stay
 * out of the shared file: they encode a particular checkout's layout rather than
 * portable editor behavior.
 */
const FORBIDDEN_RESOURCE_KEYS = ['perl-lsp.includePaths', 'perl-lsp.externalIncludePaths'];

/**
 * The machine-scoped settings that decide which binary runs and where it comes
 * from. The local-override contract exists to explain exactly these, so it must
 * keep naming them even as unrelated settings gain or lose machine scope.
 */
const REQUIRED_LIFECYCLE_KEYS = [
  'perl-lsp.autoDownload',
  'perl-lsp.downloadBaseUrl',
  'perl-lsp.serverPath',
  'perl-lsp.versionTag',
];

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

/**
 * The settings the extension declares as `scope: "machine"`. VS Code reads these
 * from User/Machine settings only, so no committed file can set them.
 */
function declaredMachineScopedKeys(): string[] {
  const manifest = readRecord(EXTENSION_MANIFEST);
  const contributes = manifest.contributes;
  const configuration = isRecord(contributes) ? contributes.configuration : undefined;
  const sections = Array.isArray(configuration) ? configuration : [configuration];

  const keys: string[] = [];
  for (const section of sections) {
    const properties = isRecord(section) ? section.properties : undefined;
    if (!isRecord(properties)) {
      continue;
    }
    for (const [key, declaration] of Object.entries(properties)) {
      if (isRecord(declaration) && declaration.scope === 'machine') {
        keys.push(key);
      }
    }
  }
  return keys.sort();
}

function fencedJson(markdown: string, marker: string): Record<string, unknown> {
  const pattern = new RegExp(
    `<!-- ${marker}:start -->\\s*\`\`\`json\\s*([\\s\\S]*?)\\s*\`\`\`\\s*<!-- ${marker}:end -->`,
  );
  const match = pattern.exec(markdown);
  if (match?.[1] === undefined) {
    throw new Error(`${marker} example markers are missing`);
  }

  const value = JSON.parse(match[1]) as unknown;
  if (!isRecord(value)) {
    throw new Error(`${marker} example must contain one JSON object`);
  }
  return value;
}

function bulletedKeys(markdown: string, marker: string): string[] {
  const pattern = new RegExp(`<!-- ${marker}:start -->([\\s\\S]*?)<!-- ${marker}:end -->`);
  const match = pattern.exec(markdown);
  if (match?.[1] === undefined) {
    throw new Error(`${marker} list markers are missing`);
  }

  return [...match[1].matchAll(/^-\s+`([^`]+)`\s*$/gm)]
    .flatMap((entry) => (entry[1] === undefined ? [] : [entry[1]]))
    .sort();
}

describe('checked-in repository workspace settings', () => {
  test('remain portable and product-neutral', () => {
    const settings = readRecord(SHARED_SETTINGS);
    const forbidden = [...declaredMachineScopedKeys(), ...FORBIDDEN_RESOURCE_KEYS];

    // Guards against the declaration set going empty and vacuously passing.
    expect(forbidden.length).toBeGreaterThan(FORBIDDEN_RESOURCE_KEYS.length);

    for (const key of forbidden) {
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

  test('names only real machine-scoped keys, including the lifecycle core', () => {
    // If one of these loses machine scope, the contract is describing the wrong
    // mechanism and must be rewritten rather than silently misleading a reader.
    const declared = declaredMachineScopedKeys();
    const listed = bulletedKeys(contract, 'machine-scoped-keys');

    expect(listed.length).toBeGreaterThan(0);
    for (const key of listed) {
      expect(declared).toContain(key);
    }
    for (const key of REQUIRED_LIFECYCLE_KEYS) {
      expect(listed).toContain(key);
    }
  });

  test('routes the override to user settings, not to any committed file', () => {
    const example = fencedJson(contract, 'user-settings-example');

    expect(example['perl-lsp.serverPath']).toBe('__REPLACE_WITH_ABSOLUTE_PERLLSP_PATH__');
    expect(example['perl-lsp.autoDownload']).toBe(false);

    // A workspace-file shape here would reintroduce the ignored-override bug.
    expect(Object.prototype.hasOwnProperty.call(example, 'folders')).toBe(false);
    expect(Object.prototype.hasOwnProperty.call(example, 'settings')).toBe(false);

    expect(contract).toContain('Preferences: Open User Settings (JSON)');
  });

  test('states that committed placement is ignored, and why', () => {
    expect(contract).toContain('.code-workspace');
    expect(contract).toMatch(/ignored|ignores/);
    expect(contract).toContain('Workspace Configuration RCE');
  });

  test('states candidate identity and reset boundaries', () => {
    expect(contract).toContain('perllsp --version');
    expect(contract).toContain('checkout SHA');
    expect(contract).toContain('do not make it a public release');
    expect(contract).toContain('reload the window');
    expect(contract).toContain('normal installed-product lifecycle');
  });
});
