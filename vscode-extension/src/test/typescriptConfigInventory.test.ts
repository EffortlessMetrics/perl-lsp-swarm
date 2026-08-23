import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

type InventoryConfig = {
  path: string;
  role: string;
  blocking: boolean;
  authority: boolean;
  command: string;
  emit: boolean;
  outDir: string;
  rootDir: string;
};

type Inventory = {
  schema: number;
  configs: InventoryConfig[];
};

type EffectiveConfig = {
  noEmit: boolean;
  outDir: string | undefined;
  rootDir: string | undefined;
  error?: string;
};

type EvaluationInput = {
  inventory: Inventory;
  discoveredFiles: string[];
  authorityFiles: string[];
  typecheckAll: string;
  effectiveConfigs: Record<string, EffectiveConfig>;
};

type EvaluationResult = {
  ok: boolean;
  failures: string[];
  facts: string[];
};

type InventoryModule = {
  evaluateTypeScriptConfigInventory(input: EvaluationInput): EvaluationResult;
  checkTypeScriptConfigInventory(extensionRoot: string): EvaluationResult;
};

const extensionRoot = path.resolve(__dirname, '..', '..');
const inventoryPath = path.join(extensionRoot, 'scripts', 'typescript-config-inventory.json');
const inventoryModule = require(
  path.join(extensionRoot, 'scripts', 'check-typescript-config-inventory'),
) as InventoryModule;
const inventory = JSON.parse(fs.readFileSync(inventoryPath, 'utf8')) as Inventory;

function healthyInput(): EvaluationInput {
  const configs = inventory.configs.map((config) => config.path);
  return {
    inventory,
    discoveredFiles: [...configs].sort(),
    authorityFiles: [...configs].sort(),
    typecheckAll: inventory.configs
      .filter((config) => config.blocking)
      .map((config) => `npm run ${config.command}`)
      .join(' && '),
    effectiveConfigs: Object.fromEntries(
      inventory.configs.map((config) => [
        config.path,
        {
          noEmit: !config.emit,
          outDir: config.outDir,
          rootDir: config.rootDir,
        },
      ]),
    ),
  };
}

function expectFailure(result: EvaluationResult, pattern: RegExp): void {
  expect(result.ok).toBe(false);
  expect(result.failures.some((failure) => pattern.test(failure))).toBe(true);
}

describe('TypeScript configuration authority inventory', () => {
  test('the real extension tree reconciles discovery, authority, commands, and effective options', () => {
    const result = inventoryModule.checkTypeScriptConfigInventory(extensionRoot);
    expect(result.ok).toBe(true);
    expect(result.failures).toEqual([]);
  });

  test('an unclassified sixth config is red', () => {
    const input = healthyInput();
    input.discoveredFiles.push('tsconfig.extra.json');
    expectFailure(
      inventoryModule.evaluateTypeScriptConfigInventory(input),
      /tsconfig\.extra\.json is unclassified/,
    );
  });

  test('a blocking config missing from typecheck:all is red', () => {
    const input = healthyInput();
    input.typecheckAll = input.typecheckAll.replace('npm run typecheck:scripts', '');
    expectFailure(
      inventoryModule.evaluateTypeScriptConfigInventory(input),
      /tsconfig\.scripts\.json is blocking.*typecheck:all does not invoke/,
    );
  });

  test('authority and inventory cannot silently disagree', () => {
    const input = healthyInput();
    input.authorityFiles = input.authorityFiles.filter(
      (file) => file !== 'tsconfig.published-smoke.json',
    );
    expectFailure(
      inventoryModule.evaluateTypeScriptConfigInventory(input),
      /tsconfig\.published-smoke\.json authority=true.*reports false/,
    );
  });

  test('effective emit and output drift are named', () => {
    const input = healthyInput();
    input.effectiveConfigs['tsconfig.test.json'] = {
      noEmit: true,
      outDir: 'out',
      rootDir: 'src',
    };
    const result = inventoryModule.evaluateTypeScriptConfigInventory(input);
    expectFailure(result, /tsconfig\.test\.json inventory emit=true.*noEmit=true/);
    expectFailure(result, /tsconfig\.test\.json inventory outDir=out-test.*outDir=out/);
  });

  test('the real I/O path detects an added config in a temporary tree', () => {
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-ts-inventory-'));
    const root = path.join(scratch, 'extension root with spaces');
    fs.mkdirSync(path.join(root, 'scripts'), { recursive: true });
    fs.mkdirSync(path.join(root, 'src', 'test', 'integration'), { recursive: true });
    fs.mkdirSync(path.join(root, 'src', 'test', 'published'), { recursive: true });

    try {
      for (const file of [
        'package.json',
        'tsconfig.json',
        'tsconfig.test.json',
        'tsconfig.integration.json',
        'tsconfig.published-smoke.json',
        'tsconfig.scripts.json',
      ]) {
        fs.copyFileSync(path.join(extensionRoot, file), path.join(root, file));
      }
      fs.copyFileSync(
        inventoryPath,
        path.join(root, 'scripts', 'typescript-config-inventory.json'),
      );
      fs.writeFileSync(path.join(root, 'src', 'index.ts'), 'export const ok = 1;\n');
      fs.writeFileSync(
        path.join(root, 'src', 'test', 'integration', 'smoke.ts'),
        'export const integration = true;\n',
      );
      fs.writeFileSync(
        path.join(root, 'src', 'test', 'integration', 'firstHourReceipt.test.ts'),
        'export const receipt = true;\n',
      );
      fs.writeFileSync(
        path.join(root, 'src', 'test', 'published', 'smoke.ts'),
        'export const published = true;\n',
      );
      fs.writeFileSync(path.join(root, 'scripts', 'sample.js'), "'use strict';\n");
      fs.writeFileSync(
        path.join(root, 'tsconfig.extra.json'),
        JSON.stringify({ extends: './tsconfig.json', include: ['src/index.ts'] }),
      );
      fs.symlinkSync(
        path.join(extensionRoot, 'node_modules'),
        path.join(root, 'node_modules'),
        process.platform === 'win32' ? 'junction' : 'dir',
      );

      const result = inventoryModule.checkTypeScriptConfigInventory(root);
      expectFailure(result, /tsconfig\.extra\.json is unclassified/);
    } finally {
      fs.rmSync(scratch, { recursive: true, force: true });
    }
  });
});
