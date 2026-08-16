import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

type CompilerOptions = {
  noEmit?: boolean;
  outDir?: string;
  rootDir?: string;
};

type EffectiveConfig = {
  compilerOptions?: CompilerOptions;
};

const extensionRoot = path.resolve(__dirname, '..', '..');
const tscEntry = path.join(extensionRoot, 'node_modules', 'typescript', 'bin', 'tsc');

function effectiveCompilerOptions(configFile: string): CompilerOptions {
  const output = execFileSync(process.execPath, [tscEntry, '--showConfig', '-p', configFile], {
    cwd: extensionRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const config = JSON.parse(output) as EffectiveConfig;
  return config.compilerOptions ?? {};
}

function resolvedOption(value: string | undefined): string | undefined {
  return value === undefined ? undefined : path.resolve(extensionRoot, value);
}

describe('TypeScript emit ownership', () => {
  const contracts = [
    { file: 'tsconfig.json', noEmit: true, outDir: 'out', rootDir: 'src' },
    { file: 'tsconfig.test.json', noEmit: false, outDir: 'out-test', rootDir: 'src' },
    { file: 'tsconfig.integration.json', noEmit: false, outDir: 'out', rootDir: 'src' },
    { file: 'tsconfig.published-smoke.json', noEmit: false, outDir: 'out', rootDir: 'src' },
    { file: 'tsconfig.scripts.json', noEmit: true, outDir: 'out', rootDir: '.' },
  ] as const;

  test.each(contracts)('$file owns its effective emit boundary', (contract) => {
    const options = effectiveCompilerOptions(contract.file);
    expect(options.noEmit).toBe(contract.noEmit);
    expect(resolvedOption(options.outDir)).toBe(path.resolve(extensionRoot, contract.outDir));
    expect(resolvedOption(options.rootDir)).toBe(path.resolve(extensionRoot, contract.rootDir));
  });

  test('direct base-config tsc cannot emit even when an output directory is supplied', () => {
    const redirectedOutput = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-ts-no-emit-'));
    try {
      execFileSync(process.execPath, [tscEntry, '-p', 'tsconfig.json', '--outDir', redirectedOutput], {
        cwd: extensionRoot,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      expect(fs.readdirSync(redirectedOutput)).toEqual([]);
    } finally {
      fs.rmSync(redirectedOutput, { recursive: true, force: true });
    }
  });
});
