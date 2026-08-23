import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

const findFiles = jest.fn<Promise<{ fsPath: string }[]>, unknown[]>();

jest.mock(
  'vscode',
  () => ({
    workspace: {
      isTrusted: true,
      findFiles: (...args: unknown[]) => findFiles(...args),
    },
    CodeActionKind: { QuickFix: 'quickfix' },
  }),
  { virtual: true },
);

import { isPotentiallyExpensiveRegex } from '../gherkinRedosGuard';
import {
  buildGeneratedStepPattern,
  buildGeneratedStepStub,
  classifyStepDefinitionStatus,
  collectWorkspaceStepDefinitionSources,
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

  it('declines to call a step undefined once the match budget is exhausted', () => {
    const step = {
      keyword: 'Given' as const,
      text: 'a step nothing defines',
      line: 0,
      rawLine: 'Given a step nothing defines',
    };
    // Every pattern here is individually linear-time and policy-safe, so the
    // ReDoS guard alone would let the whole population run.
    const source = Array.from(
      { length: 25_000 },
      (_unused, index) => `Given qr/^step number ${index}$/, sub { return; };`,
    ).join('\n');

    expect(classifyStepDefinitionStatus(step, [source])).toBe('ambiguous');
  });

  it('still matches a real definition inside the match budget', () => {
    const step = {
      keyword: 'Given' as const,
      text: 'step number 9',
      line: 0,
      rawLine: 'Given step number 9',
    };
    const source = Array.from(
      { length: 200 },
      (_unused, index) => `Given qr/^step number ${index}$/, sub { return; };`,
    ).join('\n');

    expect(classifyStepDefinitionStatus(step, [source])).toBe('defined');
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

/**
 * Run `race` immediately before the nth `lstat` of `target`, so the write path
 * observes the workspace as it is after a concurrent change rather than as it
 * was when the content was derived. Call 1 is the initial read; call 2 is the
 * first re-validation inside the replace path.
 */
function raceOnTargetLstat(
  target: string,
  nth: number,
  race: () => Promise<void>,
): jest.SpyInstance {
  const realLstat = fs.promises.lstat.bind(fs.promises);
  const spy = jest.spyOn(fs.promises, 'lstat');
  let seen = 0;
  spy.mockImplementation(async (candidate, ...rest) => {
    if (candidate === target) {
      seen += 1;
      if (seen === nth) {
        await race();
      }
    }
    return realLstat(candidate as fs.PathLike, ...(rest as []));
  });
  return spy;
}

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

  it('refuses to discard bytes written to the target after it was read', async () => {
    const parent = path.join(workspaceRoot, 'features', 'step_definitions');
    await fs.promises.mkdir(parent, { recursive: true });
    const target = path.join(parent, 'example_steps.pm');
    const original = 'Given qr/^first$/, sub { return; };\n';
    await fs.promises.writeFile(target, original, 'utf8');

    // Stand in for an editor save landing after the read and before the
    // rename: the target is only observed again inside the replace path.
    const concurrent = `${original}Then qr/^saved by the user$/, sub { return; };\n`;
    const lstat = raceOnTargetLstat(target, 2, async () => {
      await fs.promises.writeFile(target, concurrent, 'utf8');
    });

    try {
      await expect(
        writeGeneratedStepDefinitionFile(workspaceRoot, target, 'content\n', 'stub'),
      ).rejects.toThrow(/target changed during validation/);
    } finally {
      lstat.mockRestore();
    }

    // The concurrent write survives, and no partial file is left behind.
    expect(await fs.promises.readFile(target, 'utf8')).toBe(concurrent);
    expect((await fs.promises.readdir(parent)).sort()).toEqual(['example_steps.pm']);
  });

  it('refuses to clobber a target that appeared after the create decision', async () => {
    const parent = path.join(workspaceRoot, 'features', 'step_definitions');
    await fs.promises.mkdir(parent, { recursive: true });
    const target = path.join(parent, 'example_steps.pm');

    // The create path saw no file at all; someone else creates one before the
    // rename would have committed.
    const lstat = raceOnTargetLstat(target, 2, async () => {
      await fs.promises.writeFile(target, 'written by someone else\n', 'utf8');
    });

    try {
      await expect(
        writeGeneratedStepDefinitionFile(workspaceRoot, target, 'content\n', 'stub'),
      ).rejects.toThrow(/target appeared during validation/);
    } finally {
      lstat.mockRestore();
    }

    expect(await fs.promises.readFile(target, 'utf8')).toBe('written by someone else\n');
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

describe('bounded workspace step-definition scan', () => {
  const PER_FILE_LIMIT = 512 * 1024;
  let workspaceRoot: string;

  beforeEach(async () => {
    workspaceRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'perl-lsp-gherkin-scan-'));
    findFiles.mockReset();
  });

  afterEach(async () => {
    await fs.promises.rm(workspaceRoot, { recursive: true, force: true });
  });

  async function writeStepFile(name: string, body: string): Promise<string> {
    const parent = path.join(workspaceRoot, 'features', 'step_definitions');
    await fs.promises.mkdir(parent, { recursive: true });
    const filePath = path.join(parent, name);
    await fs.promises.writeFile(filePath, body, 'utf8');
    return filePath;
  }

  function scan(): Promise<string[]> {
    return collectWorkspaceStepDefinitionSources({
      uri: { fsPath: workspaceRoot },
    } as never);
  }

  it('accepts an ordinary step-definition file', async () => {
    const filePath = await writeStepFile('small_steps.pm', 'Given qr/^ok$/, sub { return; };\n');
    findFiles.mockResolvedValue([{ fsPath: filePath }]);

    expect(await scan()).toEqual(['Given qr/^ok$/, sub { return; };\n']);
  });

  it('skips a file already past the per-file limit', async () => {
    const filePath = await writeStepFile(
      'large_steps.pm',
      'Test::BDD::Cucumber::StepFile\n'.padEnd(PER_FILE_LIMIT + 1, 'x'),
    );
    findFiles.mockResolvedValue([{ fsPath: filePath }]);

    expect(await scan()).toEqual([]);
  });

  it('holds the per-file bound when the workspace grows the file mid-scan', async () => {
    const filePath = await writeStepFile(
      'grown_steps.pm',
      'Test::BDD::Cucumber::StepFile\nGiven qr/^ok$/, sub { return; };\n',
    );
    const grown = 'Test::BDD::Cucumber::StepFile\n'.padEnd(PER_FILE_LIMIT * 4, 'x');

    // A hostile workspace process grows the file in the window between a
    // path-based size decision and a path-based read: the size observation
    // returns the small file, and any later read of the path returns the large
    // one. An implementation that reads a bounded window from its own
    // descriptor never opens that window, so nothing here fires and it accepts
    // the small file. One that decides on `lstat`/`stat` and then re-reads the
    // path admits the grown file past its own limit.
    const realLstat = fs.promises.lstat.bind(fs.promises);
    const realStat = fs.promises.stat.bind(fs.promises);
    const realReadFile = fs.promises.readFile.bind(fs.promises);
    const grow = async () => {
      await fs.promises.writeFile(filePath, grown, 'utf8');
    };
    const spies = [
      jest.spyOn(fs.promises, 'lstat').mockImplementation(async (candidate, ...rest) => {
        const stats = await realLstat(candidate as fs.PathLike, ...(rest as []));
        if (candidate === filePath) {
          await grow();
        }
        return stats;
      }),
      jest.spyOn(fs.promises, 'stat').mockImplementation(async (candidate, ...rest) => {
        const stats = await realStat(candidate as fs.PathLike, ...(rest as []));
        if (candidate === filePath) {
          await grow();
        }
        return stats;
      }),
      jest.spyOn(fs.promises, 'readFile').mockImplementation(async (candidate, ...rest) => {
        if (candidate === filePath) {
          await grow();
        }
        return realReadFile(candidate as never, ...(rest as []));
      }),
    ];

    let sources: string[];
    try {
      findFiles.mockResolvedValue([{ fsPath: filePath }]);
      sources = await scan();
    } finally {
      for (const spy of spies) {
        spy.mockRestore();
      }
    }

    expect(sources).toHaveLength(1);
    expect(Buffer.byteLength(sources[0] ?? '', 'utf8')).toBeLessThanOrEqual(PER_FILE_LIMIT);
  });

  it('accepts a file sitting exactly on the per-file limit', async () => {
    const header = 'Test::BDD::Cucumber::StepFile\n';
    const filePath = await writeStepFile(
      'exact_steps.pm',
      header + 'x'.repeat(PER_FILE_LIMIT - header.length),
    );
    findFiles.mockResolvedValue([{ fsPath: filePath }]);

    const sources = await scan();
    expect(sources).toHaveLength(1);
    expect(Buffer.byteLength(sources[0] ?? '', 'utf8')).toBe(PER_FILE_LIMIT);
  });

  it('stops before a file that would straddle the aggregate envelope', async () => {
    const TOTAL_LIMIT = 16 * 1024 * 1024;
    // Deliberately not a divisor of the envelope: with an exact divisor the
    // last accepted file lands on the boundary and a missing envelope check is
    // indistinguishable from a present one.
    const perFile = 300 * 1024;
    const fits = Math.floor(TOTAL_LIMIT / perFile);
    expect(fits * perFile).toBeLessThan(TOTAL_LIMIT);
    expect((fits + 1) * perFile).toBeGreaterThan(TOTAL_LIMIT);

    const chunk = 'Given qr/^ok$/, sub { return; };\n'.padEnd(perFile, ' ');
    const paths: string[] = [];
    for (let index = 0; index < fits + 2; index += 1) {
      paths.push(await writeStepFile(`bulk_${String(index).padStart(3, '0')}_steps.pm`, chunk));
    }
    findFiles.mockResolvedValue(paths.map((fsPath) => ({ fsPath })));

    const sources = await scan();
    const total = sources.reduce((sum, source) => sum + Buffer.byteLength(source, 'utf8'), 0);
    expect(total).toBeLessThanOrEqual(TOTAL_LIMIT);
    expect(sources).toHaveLength(fits);
  });

  it('does not read through a symlinked candidate', async () => {
    if (process.platform === 'win32') {
      return;
    }

    const outside = path.join(os.tmpdir(), `perl-lsp-outside-scan-${process.pid}.pm`);
    await fs.promises.writeFile(outside, 'Given qr/^outside$/, sub { return; };\n', 'utf8');
    const parent = path.join(workspaceRoot, 'features', 'step_definitions');
    await fs.promises.mkdir(parent, { recursive: true });
    const link = path.join(parent, 'linked_steps.pm');
    try {
      await fs.promises.symlink(outside, link);
      findFiles.mockResolvedValue([{ fsPath: link }]);

      expect(await scan()).toEqual([]);
    } finally {
      await fs.promises.rm(outside, { force: true });
    }
  });
});
