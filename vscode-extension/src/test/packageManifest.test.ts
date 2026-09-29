import * as fs from 'fs';
import * as path from 'path';

describe('package manifest Perl language registration', () => {
  test('registers common Perl project files by filename', () => {
    const manifestPath = path.resolve(__dirname, '../../package.json');
    const packageJson = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as {
      contributes?: {
        languages?: Array<{
          id?: string;
          filenames?: string[];
        }>;
      };
    };

    const perlLanguage = packageJson.contributes?.languages?.find(
      (language) => language.id === 'perl',
    );
    expect(perlLanguage).toBeDefined();

    const filenames = perlLanguage?.filenames ?? [];
    expect(filenames).toEqual(
      expect.arrayContaining([
        'Makefile.PL',
        'Build.PL',
        'cpanfile',
        'cpanfile.snapshot',
        'dist.ini',
      ]),
    );
  });
});

describe('package manifest AI egress scope (#4997)', () => {
  const extRoot = path.resolve(__dirname, '../..');

  function manifestConfigurationProperties(): Record<string, { scope?: string }> {
    const packageJson = JSON.parse(fs.readFileSync(path.join(extRoot, 'package.json'), 'utf8')) as {
      contributes?: {
        configuration?:
          | { properties?: Record<string, { scope?: string }> }
          | Array<{ properties?: Record<string, { scope?: string }> }>;
      };
    };
    const configuration = packageJson.contributes?.configuration;
    const blocks = Array.isArray(configuration) ? configuration : [configuration ?? {}];
    return Object.assign({}, ...blocks.map((block) => block.properties ?? {}));
  }

  test('aiCompletion activation toggles are machine-scoped so workspaces cannot set them', () => {
    const properties = manifestConfigurationProperties();
    expect(properties['perl-lsp.aiCompletion.enabled']?.scope).toBe('machine');
    expect(properties['perl-lsp.aiCompletion.streaming.enabled']?.scope).toBe('machine');
  });
});

describe('package manifest demo project command (#1635)', () => {
  const extRoot = path.resolve(__dirname, '../..');

  test('contributes the perl-lsp.openDemoProject command', () => {
    const packageJson = JSON.parse(fs.readFileSync(path.join(extRoot, 'package.json'), 'utf8')) as {
      contributes?: { commands?: Array<{ command?: string; title?: string; category?: string }> };
    };
    const command = packageJson.contributes?.commands?.find(
      (c) => c.command === 'perl-lsp.openDemoProject',
    );
    const catalog = JSON.parse(
      fs.readFileSync(path.join(extRoot, 'package.nls.json'), 'utf8'),
    ) as Record<string, string>;
    expect(command).toBeDefined();
    expect(command?.title).toBe('%command.openDemoProject.title%');
    expect(catalog['command.openDemoProject.title']).toBe('Open Demo Project');
    expect(command?.category).toBe('Perl');
  });

  test('bundles the demo project so the command can open it', () => {
    const demoRoot = path.join(extRoot, 'assets', 'demo-project');
    expect(fs.existsSync(path.join(demoRoot, 'main.pl'))).toBe(true);
    expect(fs.existsSync(path.join(demoRoot, 'lib', 'Utils.pm'))).toBe(true);
    expect(fs.existsSync(path.join(demoRoot, 'lib', 'Database.pm'))).toBe(true);
  });
});

describe('first-run demo content (#16591)', () => {
  const repoRoot = path.resolve(__dirname, '../../..');
  const sourceRoot = path.join(repoRoot, 'demo_workspace');
  const bundledRoot = path.join(repoRoot, 'vscode-extension', 'assets', 'demo-project');

  function filesUnder(root: string, directory = ''): string[] {
    return fs.readdirSync(path.join(root, directory), { withFileTypes: true }).flatMap((entry) => {
      const relativePath = path.join(directory, entry.name);
      return entry.isDirectory() ? filesUnder(root, relativePath) : [relativePath];
    });
  }

  test('bundled demo has exactly the same files and bytes as the source demo', () => {
    const sourceFiles = filesUnder(sourceRoot).sort();
    expect(sourceFiles).toEqual(
      ['.perl-lsp.toml', 'README.md', 'lib/Database.pm', 'lib/Utils.pm', 'main.pl']
        .map((file) => path.normalize(file))
        .sort(),
    );
    expect(filesUnder(bundledRoot).sort()).toEqual(sourceFiles);
    for (const file of sourceFiles) {
      expect(fs.readFileSync(path.join(bundledRoot, file))).toEqual(
        fs.readFileSync(path.join(sourceRoot, file)),
      );
    }
  });

  test('both demo copies pin include_paths to the directory the demo uses (#16592)', () => {
    // A `.perl-lsp.toml` that merely exists would leave the demo exactly as
    // unpinned as before, so assert the *content* names the real module dir.
    for (const root of [sourceRoot, bundledRoot]) {
      const config = fs.readFileSync(path.join(root, '.perl-lsp.toml'), 'utf8');
      expect(config).toMatch(/^\s*\[perl\]\s*$/m);
      const declared = /include_paths\s*=\s*\[([^\]]*)\]/.exec(config);
      expect(declared).not.toBeNull();
      const body = declared?.[1] ?? '';
      const entries = body
        .split(',')
        .map((entry) => entry.trim().replace(/^["']|["']$/g, ''))
        .filter((entry) => entry.length > 0);
      expect(entries).toContain('lib');
      // And that directory is the one holding the demo's modules.
      expect(fs.existsSync(path.join(root, 'lib', 'Utils.pm'))).toBe(true);
    }
  });

  test('default walkthrough calls the project functions and has no deliberate dead routines', () => {
    const main = fs.readFileSync(path.join(sourceRoot, 'main.pl'), 'utf8');
    const utils = fs.readFileSync(path.join(sourceRoot, 'lib', 'Utils.pm'), 'utf8');
    const database = fs.readFileSync(path.join(sourceRoot, 'lib', 'Database.pm'), 'utf8');
    expect(main).toContain('Utils::load_data()');
    expect(main).toContain('Utils::process_data($data)');
    expect(main).toContain('Database::save($summary)');
    expect(utils).not.toMatch(/sub\s+unused_helper\b/);
    expect(database).not.toMatch(/sub\s+(?:connect|unused_query)\b/);
    expect(database).not.toMatch(/require\s+DBI\b/);
  });
});
