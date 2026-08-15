import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

jest.mock(
  'vscode',
  () => ({
    workspace: { isTrusted: true },
    CodeActionKind: { QuickFix: 'quickfix' },
  }),
  { virtual: true },
);

import { isPotentiallyExpensiveRegex } from '../gherkinRedosGuard';
import {
  buildGeneratedStepPattern,
  buildGeneratedStepStub,
  classifyStepDefinitionStatus,
  writeGeneratedStepDefinitionFile,
} from '../gherkinStepDefinitions';

// The regex policy itself is owned by gherkinRedosGuard (#6158). These cases
// pin the acceptance criteria named in #5997 so the guard cannot regress back
// past them, without re-asserting the stricter policy #6158 deliberately
// rejected to avoid the ordinary-pattern false negatives in #859.
describe('Gherkin regex safety', () => {
  it.each(['^(a|aa)+$', '^(a|a?)+$', '^(a+)+$', '^(a)\\1$', '^(?=a)a$'])(
    'rejects catastrophic workspace regex %s without executing it',
    (source) => {
      expect(isPotentiallyExpensiveRegex(source)).toBe(true);
    },
  );

  it.each(['^I have "([^"]+)"$', '^I have ([0-9]{2}) items$', '^status: (pass|fail)$', '^a+b$'])(
    'keeps ordinary anchored capture regex %s available',
    (source) => {
      expect(isPotentiallyExpensiveRegex(source)).toBe(false);
    },
  );

  it('classifies an over-budget definition as ambiguous rather than matching it', () => {
    const step = {
      keyword: 'Given' as const,
      text: 'a step',
      line: 0,
      rawLine: 'Given a step',
    };
    const overBudget = `^${'a'.repeat(300)}$`;
    expect(classifyStepDefinitionStatus(step, [`Given qr/${overBudget}/, sub { return; };`])).toBe(
      'ambiguous',
    );
  });

  it('generates outline captures without matching newlines', () => {
    expect(buildGeneratedStepPattern('I have <count> items')).toBe('^I have ([^\\r\\n]+) items$');
  });

  it('keeps generated provenance comments on one line', () => {
    const stub = buildGeneratedStepStub(
      { keyword: 'Given', text: 'a safe step', line: 2, rawLine: 'Given a safe step' },
      'features/example.feature\nInjected qr/.+/',
    );
    expect(stub.split('\n')[0]).toBe(
      '# Auto-generated from features/example.feature Injected qr/.+/:3',
    );
  });
});

describe('generated step-definition writes', () => {
  let workspaceRoot: string;

  beforeEach(async () => {
    workspaceRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'perl-lsp-gherkin-security-'));
  });

  afterEach(async () => {
    await fs.promises.rm(workspaceRoot, { recursive: true, force: true });
  });

  it('creates and atomically appends a normal generated definition', async () => {
    const target = path.join(workspaceRoot, 'features', 'step_definitions', 'example_steps.pm');
    const createContent = [
      'use Test::BDD::Cucumber::StepFile;',
      '',
      'Given qr/^first$/, sub { return; };',
      '',
    ].join('\n');

    await writeGeneratedStepDefinitionFile(
      workspaceRoot,
      target,
      createContent,
      'Given qr/^first$/, sub { return; };',
    );
    expect(await fs.promises.readFile(target, 'utf8')).toBe(createContent);

    await writeGeneratedStepDefinitionFile(
      workspaceRoot,
      target,
      createContent,
      'Then qr/^second$/, sub { return; };',
    );
    expect(await fs.promises.readFile(target, 'utf8')).toBe(
      `${createContent.trimEnd()}\n\nThen qr/^second$/, sub { return; };\n`,
    );
  });

  it('rejects lexical workspace escape', async () => {
    const target = path.resolve(workspaceRoot, '..', 'outside_steps.pm');
    await expect(
      writeGeneratedStepDefinitionFile(workspaceRoot, target, 'content\n', 'stub'),
    ).rejects.toThrow(/escapes the workspace/);
  });

  it('rejects a symlinked parent directory', async () => {
    if (process.platform === 'win32') {
      return;
    }

    const outside = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'perl-lsp-outside-'));
    try {
      await fs.promises.symlink(outside, path.join(workspaceRoot, 'features'));
      const target = path.join(workspaceRoot, 'features', 'step_definitions', 'example_steps.pm');
      await expect(
        writeGeneratedStepDefinitionFile(workspaceRoot, target, 'content\n', 'stub'),
      ).rejects.toThrow(/parent is a symlink/);
      await expect(fs.promises.stat(path.join(outside, 'step_definitions'))).rejects.toMatchObject({
        code: 'ENOENT',
      });
    } finally {
      await fs.promises.rm(outside, { recursive: true, force: true });
    }
  });

  it('rejects a symlinked target file', async () => {
    if (process.platform === 'win32') {
      return;
    }

    const parent = path.join(workspaceRoot, 'features', 'step_definitions');
    await fs.promises.mkdir(parent, { recursive: true });
    const outside = path.join(os.tmpdir(), `perl-lsp-outside-${Date.now()}.pm`);
    await fs.promises.writeFile(outside, 'outside\n', 'utf8');
    const target = path.join(parent, 'example_steps.pm');
    try {
      await fs.promises.symlink(outside, target);
      await expect(
        writeGeneratedStepDefinitionFile(workspaceRoot, target, 'content\n', 'stub'),
      ).rejects.toThrow(/target is a symlink/);
      expect(await fs.promises.readFile(outside, 'utf8')).toBe('outside\n');
    } finally {
      await fs.promises.rm(outside, { force: true });
    }
  });
});
