import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

type SourcePolicy = {
  types: string[];
  noUncheckedSideEffectImports: boolean;
  libReplacement: boolean;
};

type Inventory = {
  policy: {
    source: SourcePolicy;
  };
};

type EffectiveConfig = {
  compilerOptions?: Partial<SourcePolicy>;
};

const extensionRoot = path.resolve(__dirname, '..', '..');
const tscEntry = path.join(extensionRoot, 'node_modules', 'typescript', 'bin', 'tsc');
const inventory = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, 'scripts', 'typescript-config-inventory.json'), 'utf8'),
) as Inventory;

function readEffectiveSourcePolicy(): Partial<SourcePolicy> {
  const result = spawnSync(process.execPath, [tscEntry, '--showConfig', '-p', 'tsconfig.json'], {
    cwd: extensionRoot,
    encoding: 'utf8',
  });
  expect(result.status).toBe(0);
  const parsed = JSON.parse(result.stdout) as EffectiveConfig;
  return parsed.compilerOptions ?? {};
}

function policyFailures(expected: SourcePolicy, actual: Partial<SourcePolicy>): string[] {
  const failures: string[] = [];
  if (JSON.stringify(actual.types) !== JSON.stringify(expected.types)) {
    failures.push(
      `types expected ${JSON.stringify(expected.types)}, received ${JSON.stringify(actual.types)}`,
    );
  }
  if (actual.noUncheckedSideEffectImports !== expected.noUncheckedSideEffectImports) {
    failures.push(
      `noUncheckedSideEffectImports expected ${String(expected.noUncheckedSideEffectImports)}, received ${String(actual.noUncheckedSideEffectImports)}`,
    );
  }
  if (actual.libReplacement !== expected.libReplacement) {
    failures.push(
      `libReplacement expected ${String(expected.libReplacement)}, received ${String(actual.libReplacement)}`,
    );
  }
  return failures;
}

function runFixture(source: string): { status: number | null; output: string } {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-ts-policy-'));
  const root = path.join(scratch, 'fixture with spaces');
  fs.mkdirSync(root, { recursive: true });
  try {
    fs.symlinkSync(
      path.join(extensionRoot, 'node_modules'),
      path.join(root, 'node_modules'),
      process.platform === 'win32' ? 'junction' : 'dir',
    );
    fs.writeFileSync(path.join(root, 'index.ts'), source);
    fs.writeFileSync(
      path.join(root, 'tsconfig.json'),
      JSON.stringify({
        extends: path.join(extensionRoot, 'tsconfig.json'),
        compilerOptions: {
          rootDir: '.',
          noEmit: true,
        },
        include: ['index.ts'],
      }),
    );
    const result = spawnSync(
      process.execPath,
      [tscEntry, '-p', 'tsconfig.json', '--pretty', 'false'],
      {
        cwd: root,
        encoding: 'utf8',
      },
    );
    return {
      status: result.status,
      output: `${result.stdout ?? ''}\n${result.stderr ?? ''}`,
    };
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

describe('explicit TypeScript 7 source policy', () => {
  test('effective source policy matches the canonical inventory', () => {
    expect(policyFailures(inventory.policy.source, readEffectiveSourcePolicy())).toEqual([]);
  });

  test.each([
    ['types', { types: ['node'] }],
    ['noUncheckedSideEffectImports', { noUncheckedSideEffectImports: false }],
    ['libReplacement', { libReplacement: true }],
  ])('%s drift is named', (name, mutation) => {
    const actual = { ...inventory.policy.source, ...mutation };
    expect(policyFailures(inventory.policy.source, actual).join('\n')).toContain(name);
  });

  test('source does not inherit ambient Node globals from installed @types packages', () => {
    const result = runFixture('const env: NodeJS.ProcessEnv = {};\nvoid env;\n');
    expect(result.status).not.toBe(0);
    expect(result.output).toMatch(/NodeJS/);
  });

  test('missing side-effect imports remain blocking diagnostics', () => {
    const result = runFixture("import './missing-side-effect';\nexport const ok = true;\n");
    expect(result.status).not.toBe(0);
    expect(result.output).toMatch(/missing-side-effect/);
  });
});
