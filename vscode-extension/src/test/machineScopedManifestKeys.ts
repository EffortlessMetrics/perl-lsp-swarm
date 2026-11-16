import * as fs from 'fs';
import * as path from 'path';

const EXT_ROOT = path.resolve(__dirname, '..', '..');
const EXTENSION_MANIFEST = path.join(EXT_ROOT, 'package.json');

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readRecord(filePath: string): Record<string, unknown> {
  const value = JSON.parse(fs.readFileSync(filePath, 'utf8')) as unknown;
  if (!isRecord(value)) {
    throw new Error(`${filePath} must contain one JSON object`);
  }
  return value;
}

/**
 * Settings declared with `scope: "machine"` in the extension manifest. VS Code
 * reads them from User/Machine settings only.
 */
export function declaredMachineScopedKeys(): string[] {
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
